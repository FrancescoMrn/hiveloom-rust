use crate::compaction::engine::{CompactionEngine, CompactionOutcome};
use crate::compaction::indicator::CompactionIndicator;
use crate::llm::provider::{LlmProvider, Message, ToolCall, ToolDefinition};
use crate::store::models::{Agent, Capability, ConversationTurn, MemoryEntry};
use crate::store::Vault;

const LOAD_SKILL_TOOL: &str = "hiveloom_load_skill";
const MEMORY_WRITE_TOOL: &str = "hiveloom_memory_write";
const DEFAULT_MEMORY_CURATION_INTERVAL: usize = 8;
const RECENT_TURNS_FOR_MEMORY_CURATION: usize = 12;
const MAX_MEMORY_WRITES_PER_CURATION: usize = 3;
const MAX_MEMORY_KEY_CHARS: usize = 120;
const MAX_MEMORY_VALUE_CHARS: usize = 1000;

pub struct AgentInvocation {
    pub agent: Agent,
    pub capabilities: Vec<Capability>,
    pub conversation_id: uuid::Uuid,
    pub tenant_id: uuid::Uuid,
    pub user_identity: String,
}

pub struct InvocationResult {
    pub response: String,
    pub tool_calls_made: Vec<String>,
}

/// Run the agent loop: prompt -> LLM -> tool_call -> execute -> loop until text response
pub async fn run_agent_loop(
    invocation: &AgentInvocation,
    provider: &dyn LlmProvider,
    conn: &rusqlite::Connection,
    user_message: &str,
) -> anyhow::Result<InvocationResult> {
    // 1. Append user message as turn
    ConversationTurn::append(
        conn,
        invocation.conversation_id,
        invocation.tenant_id,
        "user",
        user_message,
        0,
    )?;

    // 2. Load relevant memory
    let memories = MemoryEntry::read_for_user(
        conn,
        invocation.tenant_id,
        invocation.agent.id,
        &invocation.user_identity,
    )?;

    // 3. T019: Pre-LLM-call compaction check
    let system_context =
        build_system_context(&invocation.agent, &memories, &invocation.capabilities);
    let compaction_outcome = CompactionEngine::check_and_compact(
        conn,
        provider,
        invocation.tenant_id,
        invocation.agent.id,
        invocation.conversation_id,
        &system_context,
        &invocation.agent.model_id,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Compaction check failed, proceeding without compaction");
        CompactionOutcome::NotNeeded
    });

    // 4. Load conversation history (after potential compaction)
    let history = ConversationTurn::list_by_conversation(conn, invocation.conversation_id)?;

    // 5. Build messages: system prompt + compacted summary + conversation history
    let mut messages = vec![Message::text("system", system_context.clone())];

    // Include compacted summary if available
    if let CompactionOutcome::Compacted {
        summary: Some(ref s),
        ..
    } = compaction_outcome
    {
        messages.push(Message::text(
            "system",
            format!("[Previous context summary]\n{}", s),
        ));
    }

    // History (includes the user message we just appended)
    for turn in &history {
        messages.push(message_from_turn(turn));
    }

    // 6. Build tool definitions, including internal progressive-skill loading.
    let tools = build_tool_definitions(&invocation.capabilities);

    let mut tool_calls_made = vec![];

    // 7. LLM loop with max 10 tool-call iterations to prevent infinite loops
    for _ in 0..10 {
        let response = provider.chat_complete(&messages, &tools).await?;

        if !response.tool_calls.is_empty() {
            // Single assistant message threading all tool_use ids the model emitted.
            let assistant_text = response.content.clone().unwrap_or_default();
            if !assistant_text.is_empty() {
                ConversationTurn::append(
                    conn,
                    invocation.conversation_id,
                    invocation.tenant_id,
                    "assistant",
                    &assistant_text,
                    0,
                )?;
            }
            messages.push(Message::assistant_with_tools(
                assistant_text,
                response.tool_calls.clone(),
            ));

            for tc in &response.tool_calls {
                let result = if let Some(result) =
                    execute_internal_tool(&invocation.capabilities, tc)
                {
                    result
                } else if let Some(cap) = invocation.capabilities.iter().find(|c| c.name == tc.name)
                {
                    tool_calls_made.push(tc.name.clone());
                    serde_json::json!({
                        "result": format!("Tool {} called (capability execution requires vault context)", tc.name),
                        "arguments": tc.arguments,
                        "capability_id": cap.id.to_string()
                    })
                    .to_string()
                } else {
                    format!("{{\"error\": \"unknown tool: {}\"}}", tc.name)
                };

                ConversationTurn::append(
                    conn,
                    invocation.conversation_id,
                    invocation.tenant_id,
                    "assistant",
                    &format!("tool_use: {} {}", tc.name, tc.arguments),
                    0,
                )?;
                ConversationTurn::append(
                    conn,
                    invocation.conversation_id,
                    invocation.tenant_id,
                    "tool_result",
                    &result,
                    0,
                )?;

                messages.push(Message::tool_result(&tc.id, result));
            }
            continue;
        }

        if let Some(content) = &response.content {
            // T032: Inject compaction indicator if applicable
            let final_response =
                CompactionIndicator::inject_indicator(content, &compaction_outcome);
            ConversationTurn::append(
                conn,
                invocation.conversation_id,
                invocation.tenant_id,
                "assistant",
                &final_response,
                0,
            )?;
            if let Err(e) =
                maybe_run_memory_curation(conn, provider, invocation, user_message).await
            {
                tracing::warn!(error = %e, "Memory curation failed, continuing turn");
            }
            return Ok(InvocationResult {
                response: final_response,
                tool_calls_made,
            });
        }

        break;
    }

    Ok(InvocationResult {
        response: "I wasn't able to generate a response.".to_string(),
        tool_calls_made,
    })
}

/// Extended agent loop that has access to the Vault for real capability execution.
pub async fn run_agent_loop_with_vault(
    invocation: &AgentInvocation,
    provider: &dyn LlmProvider,
    conn: &rusqlite::Connection,
    user_message: &str,
    vault: &Vault,
) -> anyhow::Result<InvocationResult> {
    // 1. Append user message as turn
    ConversationTurn::append(
        conn,
        invocation.conversation_id,
        invocation.tenant_id,
        "user",
        user_message,
        0,
    )?;

    // 2. Load relevant memory
    let memories = MemoryEntry::read_for_user(
        conn,
        invocation.tenant_id,
        invocation.agent.id,
        &invocation.user_identity,
    )?;

    // 3. T019: Pre-LLM-call compaction check (vault-enabled loop)
    let system_context =
        build_system_context(&invocation.agent, &memories, &invocation.capabilities);
    let compaction_outcome = CompactionEngine::check_and_compact(
        conn,
        provider,
        invocation.tenant_id,
        invocation.agent.id,
        invocation.conversation_id,
        &system_context,
        &invocation.agent.model_id,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Compaction check failed, proceeding without compaction");
        CompactionOutcome::NotNeeded
    });

    // 4. Load conversation history (after potential compaction)
    let history = ConversationTurn::list_by_conversation(conn, invocation.conversation_id)?;

    // 5. Build messages
    let mut messages = vec![Message::text("system", system_context.clone())];

    // Include compacted summary if available
    if let CompactionOutcome::Compacted {
        summary: Some(ref s),
        ..
    } = compaction_outcome
    {
        messages.push(Message::text(
            "system",
            format!("[Previous context summary]\n{}", s),
        ));
    }

    for turn in &history {
        messages.push(message_from_turn(turn));
    }

    // 6. Build tool definitions, including internal progressive-skill loading.
    let tools = build_tool_definitions(&invocation.capabilities);

    let mut tool_calls_made = vec![];

    // 7. LLM loop with max 10 tool-call iterations
    for _ in 0..10 {
        let response = provider.chat_complete(&messages, &tools).await?;

        if !response.tool_calls.is_empty() {
            // Single assistant message threading all tool_use ids the model emitted.
            let assistant_text = response.content.clone().unwrap_or_default();
            if !assistant_text.is_empty() {
                ConversationTurn::append(
                    conn,
                    invocation.conversation_id,
                    invocation.tenant_id,
                    "assistant",
                    &assistant_text,
                    0,
                )?;
            }
            messages.push(Message::assistant_with_tools(
                assistant_text,
                response.tool_calls.clone(),
            ));

            for tc in &response.tool_calls {
                let result = if let Some(result) =
                    execute_internal_tool(&invocation.capabilities, tc)
                {
                    result
                } else if let Some(cap) = invocation.capabilities.iter().find(|c| c.name == tc.name)
                {
                    tool_calls_made.push(tc.name.clone());
                    let exec_result = crate::engine::capability_exec::execute_capability(
                        conn,
                        cap,
                        &tc.arguments,
                        &invocation.tenant_id,
                        &invocation.agent.id,
                        &invocation.conversation_id,
                        vault,
                    )
                    .await?;
                    serde_json::to_string(&exec_result)?
                } else {
                    format!("{{\"error\": \"unknown tool: {}\"}}", tc.name)
                };

                ConversationTurn::append(
                    conn,
                    invocation.conversation_id,
                    invocation.tenant_id,
                    "assistant",
                    &format!("tool_use: {} {}", tc.name, tc.arguments),
                    0,
                )?;
                ConversationTurn::append(
                    conn,
                    invocation.conversation_id,
                    invocation.tenant_id,
                    "tool_result",
                    &result,
                    0,
                )?;

                messages.push(Message::tool_result(&tc.id, result));
            }
            continue;
        }

        if let Some(content) = &response.content {
            // T032: Inject compaction indicator if applicable
            let final_response =
                CompactionIndicator::inject_indicator(content, &compaction_outcome);
            ConversationTurn::append(
                conn,
                invocation.conversation_id,
                invocation.tenant_id,
                "assistant",
                &final_response,
                0,
            )?;
            if let Err(e) =
                maybe_run_memory_curation(conn, provider, invocation, user_message).await
            {
                tracing::warn!(error = %e, "Memory curation failed, continuing turn");
            }
            return Ok(InvocationResult {
                response: final_response,
                tool_calls_made,
            });
        }

        break;
    }

    Ok(InvocationResult {
        response: "I wasn't able to generate a response.".to_string(),
        tool_calls_made,
    })
}

fn build_system_context(
    agent: &Agent,
    memories: &[MemoryEntry],
    capabilities: &[Capability],
) -> String {
    let mut ctx = agent.system_prompt.clone();

    let markdown_skills: Vec<&Capability> = capabilities
        .iter()
        .filter(|cap| cap.auth_type == "markdown")
        .collect();
    if !markdown_skills.is_empty() {
        ctx.push_str("\n\n## Available Skills\n");
        ctx.push_str(
            "The following reusable procedures are available. Their full bodies are loaded on demand with the `hiveloom_load_skill` tool; do not guess procedural details from summaries alone.\n",
        );
        for cap in markdown_skills {
            ctx.push_str(&format!(
                "- {}: {}\n",
                cap.name,
                skill_summary(cap).unwrap_or_else(|| "No summary provided.".to_string())
            ));
        }
    }

    if !memories.is_empty() {
        ctx.push_str("\n\n## Relevant Knowledge\n");
        for mem in memories {
            ctx.push_str(&format!("- {}: {}\n", mem.key, mem.value));
        }
    }
    ctx
}

fn build_tool_definitions(capabilities: &[Capability]) -> Vec<ToolDefinition> {
    let mut tools = Vec::new();

    if capabilities.iter().any(|c| c.auth_type == "markdown") {
        tools.push(ToolDefinition {
            name: LOAD_SKILL_TOOL.to_string(),
            description:
                "Load the full body of a named markdown skill before using its detailed procedure."
                    .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Exact name of the skill to load"
                    }
                },
                "required": ["skill_name"],
                "additionalProperties": false
            }),
        });
    }

    tools.extend(
        capabilities
            .iter()
            .filter(|c| c.auth_type != "markdown")
            .map(|c| ToolDefinition {
                name: c.name.clone(),
                description: c.description.clone(),
                input_schema: c
                    .input_schema
                    .as_ref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(serde_json::json!({"type": "object"})),
            }),
    );

    tools
}

fn execute_internal_tool(capabilities: &[Capability], tool_call: &ToolCall) -> Option<String> {
    match tool_call.name.as_str() {
        LOAD_SKILL_TOOL => Some(load_skill_tool_result(capabilities, &tool_call.arguments)),
        _ => None,
    }
}

fn load_skill_tool_result(capabilities: &[Capability], arguments: &serde_json::Value) -> String {
    let requested_name = match arguments.get("skill_name").and_then(|v| v.as_str()) {
        Some(name) if !name.trim().is_empty() => name.trim(),
        _ => {
            return serde_json::json!({
                "error": true,
                "message": "skill_name is required"
            })
            .to_string();
        }
    };

    match capabilities
        .iter()
        .find(|cap| cap.auth_type == "markdown" && cap.name.eq_ignore_ascii_case(requested_name))
    {
        Some(cap) => serde_json::json!({
            "skill_name": cap.name,
            "description": cap.description,
            "content": cap.instruction_content.as_deref().unwrap_or("")
        })
        .to_string(),
        None => serde_json::json!({
            "error": true,
            "message": format!("unknown skill: {}", requested_name)
        })
        .to_string(),
    }
}

fn skill_summary(capability: &Capability) -> Option<String> {
    let description = capability.description.trim();
    if !description.is_empty() {
        return Some(truncate_chars(description, 240));
    }

    capability
        .instruction_content
        .as_deref()
        .and_then(|content| content.lines().find(|line| !line.trim().is_empty()))
        .map(|line| truncate_chars(line.trim().trim_start_matches('#').trim(), 240))
}

async fn maybe_run_memory_curation(
    conn: &rusqlite::Connection,
    provider: &dyn LlmProvider,
    invocation: &AgentInvocation,
    user_message: &str,
) -> anyhow::Result<()> {
    let interval = memory_curation_interval();
    let force = looks_like_memory_request(user_message);
    if interval == 0 && !force {
        return Ok(());
    }

    let turn_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM conversation_turns WHERE conversation_id = ?1",
        rusqlite::params![invocation.conversation_id.to_string()],
        |row| row.get(0),
    )?;
    if turn_count <= 0 {
        return Ok(());
    }
    if !force && !((turn_count as usize).is_multiple_of(interval)) {
        return Ok(());
    }

    let turns = ConversationTurn::list_by_conversation(conn, invocation.conversation_id)?;
    let recent_start = turns.len().saturating_sub(RECENT_TURNS_FOR_MEMORY_CURATION);
    let mut recent_text = String::new();
    for turn in &turns[recent_start..] {
        recent_text.push_str(&format!("[{}]: {}\n\n", turn.role, turn.content));
    }

    let memories = MemoryEntry::read_for_user(
        conn,
        invocation.tenant_id,
        invocation.agent.id,
        &invocation.user_identity,
    )?;
    let mut memory_text = String::new();
    for memory in &memories {
        memory_text.push_str(&format!("- {}: {}\n", memory.key, memory.value));
    }
    if memory_text.is_empty() {
        memory_text.push_str("(none)\n");
    }

    let messages = vec![
        Message::text(
            "system",
            "You are a private memory curator for this agent. Decide whether the recent conversation contains stable, reusable facts worth saving. Use the memory tool only for durable user preferences, durable user facts, or explicit remember requests. Do not save transient task details, tool payloads, credentials, secrets, or skill bodies. If nothing is worth saving, respond exactly NO_MEMORY.",
        ),
        Message::text(
            "user",
            format!(
                "Existing memories:\n{}\nRecent conversation:\n{}",
                memory_text, recent_text
            ),
        ),
    ];

    let response = provider
        .chat_complete(&messages, &[memory_write_tool_definition()])
        .await?;
    for tool_call in response
        .tool_calls
        .iter()
        .filter(|tc| tc.name == MEMORY_WRITE_TOOL)
        .take(MAX_MEMORY_WRITES_PER_CURATION)
    {
        write_memory_from_tool_call(conn, invocation, tool_call)?;
    }

    Ok(())
}

fn memory_write_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: MEMORY_WRITE_TOOL.to_string(),
        description: "Persist one durable memory for future sessions.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Short stable key for the memory"
                },
                "value": {
                    "type": "string",
                    "description": "Concise durable fact or preference to remember"
                }
            },
            "required": ["key", "value"],
            "additionalProperties": false
        }),
    }
}

fn write_memory_from_tool_call(
    conn: &rusqlite::Connection,
    invocation: &AgentInvocation,
    tool_call: &ToolCall,
) -> anyhow::Result<()> {
    let key = tool_call
        .arguments
        .get("key")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    let value = tool_call
        .arguments
        .get("value")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");

    if key.is_empty() || value.is_empty() {
        return Ok(());
    }
    if key.chars().count() > MAX_MEMORY_KEY_CHARS || value.chars().count() > MAX_MEMORY_VALUE_CHARS
    {
        tracing::warn!(
            key_len = key.chars().count(),
            value_len = value.chars().count(),
            "Skipping oversized memory curation write"
        );
        return Ok(());
    }

    crate::engine::memory::write_memory(
        conn,
        &invocation.agent,
        &invocation.user_identity,
        key,
        value,
        Some(&invocation.conversation_id),
    )
}

fn memory_curation_interval() -> usize {
    std::env::var("HIVELOOM_MEMORY_CURATION_INTERVAL_TURNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MEMORY_CURATION_INTERVAL)
}

fn looks_like_memory_request(user_message: &str) -> bool {
    let normalized = user_message.to_ascii_lowercase();
    normalized.contains("remember that")
        || normalized.contains("remember this")
        || normalized.contains("memorize")
        || normalized.contains("save this")
        || normalized.contains("note that")
        || normalized.contains("keep in mind")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    value.chars().take(max_chars).collect()
}

fn message_from_turn(turn: &ConversationTurn) -> Message {
    match turn.role.as_str() {
        "user" | "assistant" | "system" => Message::text(turn.role.clone(), turn.content.clone()),
        other => Message::text("user", format!("[{}]\n{}", other, turn.content)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_agent(tenant_id: uuid::Uuid) -> Agent {
        Agent {
            id: uuid::Uuid::new_v4(),
            tenant_id,
            name: "test-agent".to_string(),
            system_prompt: "You are helpful.".to_string(),
            model_id: "claude-test".to_string(),
            scope_mode: "dual".to_string(),
            default_scope_policy: "user".to_string(),
            scope_coerce_policy: "coerce".to_string(),
            reflection_enabled: false,
            reflection_cron: None,
            status: "active".to_string(),
            version: 1,
            is_current: true,
            parent_version_id: None,
            created_at: "2026-04-27T00:00:00Z".to_string(),
        }
    }

    fn markdown_skill(name: &str, description: &str, content: &str) -> Capability {
        Capability {
            id: uuid::Uuid::new_v4(),
            tenant_id: uuid::Uuid::new_v4(),
            agent_id: uuid::Uuid::new_v4(),
            name: name.to_string(),
            description: description.to_string(),
            endpoint_url: String::new(),
            auth_type: "markdown".to_string(),
            credential_ref: None,
            input_schema: None,
            output_schema: None,
            instruction_content: Some(content.to_string()),
            created_at: "2026-04-27T00:00:00Z".to_string(),
            updated_at: "2026-04-27T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn tool_result_turns_replay_as_user_context() {
        let turn = ConversationTurn {
            id: uuid::Uuid::new_v4(),
            tenant_id: uuid::Uuid::new_v4(),
            conversation_id: uuid::Uuid::new_v4(),
            turn_index: 3,
            role: "tool_result".to_string(),
            content: "{\"answer\":42}".to_string(),
            token_count: 0,
            created_at: "2026-04-27T00:00:00Z".to_string(),
        };

        let message = message_from_turn(&turn);
        assert_eq!(message.role, "user");
        assert_eq!(message.content, "[tool_result]\n{\"answer\":42}");
    }

    #[test]
    fn explicit_memory_requests_trigger_curation() {
        assert!(looks_like_memory_request(
            "Please remember that I prefer concise answers."
        ));
        assert!(looks_like_memory_request(
            "Keep in mind that my timezone is UTC."
        ));
        assert!(!looks_like_memory_request("Can you summarize this file?"));
    }

    #[test]
    fn markdown_skill_context_lists_summary_without_body() {
        let tenant_id = uuid::Uuid::new_v4();
        let agent = fake_agent(tenant_id);
        let skill = markdown_skill(
            "deploy-checklist",
            "Deployment checklist for release work",
            "Summary line\nSECRET BODY DETAIL",
        );

        let context = build_system_context(&agent, &[], &[skill]);

        assert!(context.contains("## Available Skills"));
        assert!(context.contains("deploy-checklist"));
        assert!(context.contains("Deployment checklist for release work"));
        assert!(!context.contains("SECRET BODY DETAIL"));
    }

    #[test]
    fn load_skill_tool_is_available_and_returns_markdown_body() {
        let skill = markdown_skill(
            "deploy-checklist",
            "Deployment checklist",
            "Summary line\nSECRET BODY DETAIL",
        );
        let tools = build_tool_definitions(std::slice::from_ref(&skill));

        assert!(tools.iter().any(|tool| tool.name == LOAD_SKILL_TOOL));

        let tool_call = ToolCall {
            id: "call_1".to_string(),
            name: LOAD_SKILL_TOOL.to_string(),
            arguments: serde_json::json!({ "skill_name": "DEPLOY-CHECKLIST" }),
        };
        let result = execute_internal_tool(&[skill], &tool_call).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["skill_name"], "deploy-checklist");
        assert!(parsed["content"]
            .as_str()
            .unwrap()
            .contains("SECRET BODY DETAIL"));
    }

    #[test]
    fn memory_write_tool_persists_guarded_memory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let tenant_id = uuid::Uuid::new_v4();
        let store = crate::store::TenantStore::open(temp_dir.path(), &tenant_id).unwrap();
        let conn = store.conn();
        let agent = fake_agent(tenant_id);
        let invocation = AgentInvocation {
            agent,
            capabilities: Vec::new(),
            conversation_id: uuid::Uuid::new_v4(),
            tenant_id,
            user_identity: "alice".to_string(),
        };
        let tool_call = ToolCall {
            id: "call_1".to_string(),
            name: MEMORY_WRITE_TOOL.to_string(),
            arguments: serde_json::json!({
                "key": "prefers_short_answers",
                "value": "Alice prefers short, direct answers."
            }),
        };

        write_memory_from_tool_call(conn, &invocation, &tool_call).unwrap();

        let memories =
            MemoryEntry::read_for_user(conn, tenant_id, invocation.agent.id, "alice").unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].key, "prefers_short_answers");
        assert_eq!(memories[0].value, "Alice prefers short, direct answers.");
    }
}
