use crate::llm::provider::{LlmProvider, Message, ToolDefinition};
use crate::store::models::{Agent, Capability, ConversationTurn, MemoryEntry};

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

    // 2. Load conversation history
    let history = ConversationTurn::list_by_conversation(conn, invocation.conversation_id)?;

    // 3. Load relevant memory
    let memories = MemoryEntry::read_for_user(
        conn,
        invocation.tenant_id,
        invocation.agent.id,
        &invocation.user_identity,
    )?;

    // 4. Build messages: system prompt + memory context + conversation history
    let mut messages = vec![];
    // System prompt with memory context
    messages.push(Message {
        role: "system".to_string(),
        content: build_system_context(&invocation.agent, &memories),
    });
    // History (includes the user message we just appended)
    for turn in &history {
        messages.push(Message {
            role: turn.role.clone(),
            content: turn.content.clone(),
        });
    }

    // 5. Build tool definitions from capabilities
    let tools: Vec<ToolDefinition> = invocation
        .capabilities
        .iter()
        .map(|c| ToolDefinition {
            name: c.name.clone(),
            description: c.description.clone(),
            input_schema: c
                .input_schema
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::json!({"type": "object"})),
        })
        .collect();

    let mut tool_calls_made = vec![];

    // 6. LLM loop with max 10 tool-call iterations to prevent infinite loops
    for _ in 0..10 {
        let response = provider.chat_complete(&messages, &tools).await?;

        if !response.tool_calls.is_empty() {
            // Process tool calls
            for tc in &response.tool_calls {
                tool_calls_made.push(tc.name.clone());
                // Execute capability (placeholder -- capability_exec handles this)
                let result = format!("{{\"result\": \"Tool {} called\"}}", tc.name);

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

                messages.push(Message {
                    role: "assistant".to_string(),
                    content: format!("tool_use: {}", tc.name),
                });
                // Tool results sent as user role for simplicity
                messages.push(Message {
                    role: "user".to_string(),
                    content: result,
                });
            }
            continue;
        }

        if let Some(content) = &response.content {
            ConversationTurn::append(
                conn,
                invocation.conversation_id,
                invocation.tenant_id,
                "assistant",
                content,
                0,
            )?;
            return Ok(InvocationResult {
                response: content.clone(),
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

fn build_system_context(agent: &Agent, memories: &[MemoryEntry]) -> String {
    let mut ctx = agent.system_prompt.clone();
    if !memories.is_empty() {
        ctx.push_str("\n\n## Relevant Knowledge\n");
        for mem in memories {
            ctx.push_str(&format!("- {}: {}\n", mem.key, mem.value));
        }
    }
    ctx
}
