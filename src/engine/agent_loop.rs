use crate::compaction::engine::{CompactionEngine, CompactionOutcome};
use crate::compaction::indicator::CompactionIndicator;
use crate::llm::provider::{LlmProvider, Message, ToolDefinition};
use crate::store::models::{Agent, Capability, ConversationTurn, MemoryEntry};
use crate::store::Vault;

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
    let system_context = build_system_context(&invocation.agent, &memories);
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
    let mut messages = vec![];
    messages.push(Message {
        role: "system".to_string(),
        content: system_context.clone(),
    });

    // Include compacted summary if available
    if let CompactionOutcome::Compacted { ref summary, .. } = compaction_outcome {
        if let Some(ref s) = summary {
            messages.push(Message {
                role: "system".to_string(),
                content: format!("[Previous context summary]\n{}", s),
            });
        }
    }

    // History (includes the user message we just appended)
    for turn in &history {
        messages.push(Message {
            role: turn.role.clone(),
            content: turn.content.clone(),
        });
    }

    // 6. Build tool definitions from capabilities
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

    // 7. LLM loop with max 10 tool-call iterations to prevent infinite loops
    for _ in 0..10 {
        let response = provider.chat_complete(&messages, &tools).await?;

        if !response.tool_calls.is_empty() {
            // Process tool calls
            for tc in &response.tool_calls {
                tool_calls_made.push(tc.name.clone());

                // Find the matching capability and execute it
                let result = if let Some(cap) = invocation
                    .capabilities
                    .iter()
                    .find(|c| c.name == tc.name)
                {
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

                messages.push(Message {
                    role: "assistant".to_string(),
                    content: format!("tool_use: {}", tc.name),
                });
                messages.push(Message {
                    role: "user".to_string(),
                    content: result,
                });
            }
            continue;
        }

        if let Some(content) = &response.content {
            // T032: Inject compaction indicator if applicable
            let final_response = CompactionIndicator::inject_indicator(content, &compaction_outcome);
            ConversationTurn::append(
                conn,
                invocation.conversation_id,
                invocation.tenant_id,
                "assistant",
                &final_response,
                0,
            )?;
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
    let system_context = build_system_context(&invocation.agent, &memories);
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
    let mut messages = vec![];
    messages.push(Message {
        role: "system".to_string(),
        content: system_context.clone(),
    });

    // Include compacted summary if available
    if let CompactionOutcome::Compacted { ref summary, .. } = compaction_outcome {
        if let Some(ref s) = summary {
            messages.push(Message {
                role: "system".to_string(),
                content: format!("[Previous context summary]\n{}", s),
            });
        }
    }

    for turn in &history {
        messages.push(Message {
            role: turn.role.clone(),
            content: turn.content.clone(),
        });
    }

    // 6. Build tool definitions from capabilities
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

    // 7. LLM loop with max 10 tool-call iterations
    for _ in 0..10 {
        let response = provider.chat_complete(&messages, &tools).await?;

        if !response.tool_calls.is_empty() {
            for tc in &response.tool_calls {
                tool_calls_made.push(tc.name.clone());

                let result = if let Some(cap) = invocation
                    .capabilities
                    .iter()
                    .find(|c| c.name == tc.name)
                {
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

                messages.push(Message {
                    role: "assistant".to_string(),
                    content: format!("tool_use: {}", tc.name),
                });
                messages.push(Message {
                    role: "user".to_string(),
                    content: result,
                });
            }
            continue;
        }

        if let Some(content) = &response.content {
            // T032: Inject compaction indicator if applicable
            let final_response = CompactionIndicator::inject_indicator(content, &compaction_outcome);
            ConversationTurn::append(
                conn,
                invocation.conversation_id,
                invocation.tenant_id,
                "assistant",
                &final_response,
                0,
            )?;
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
