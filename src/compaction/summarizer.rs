//! LLM-based structured summarization for context compaction.
//!
//! The summarizer produces compressed summaries organized into categories:
//! user identity/preferences, key decisions, critical tool results,
//! workflow state, and open questions.

use crate::compaction::counter::TokenCounter;
use crate::llm::provider::{LlmProvider, Message};
use crate::store::models::ConversationTurn;
use anyhow::Result;

/// Result of a summarization attempt.
#[derive(Debug, Clone)]
pub struct SummarizationResult {
    pub summary: String,
    pub summary_token_count: usize,
    pub was_retried: bool,
    pub was_truncated: bool,
}

pub struct Summarizer;

impl Summarizer {
    /// Build the structured summarization prompt (T012).
    ///
    /// Instructs the LLM to produce a summary organized into:
    /// 1. User identity and preferences
    /// 2. Key decisions and constraints
    /// 3. Critical tool results
    /// 4. Workflow state (T028, T030)
    /// 5. Open questions
    pub fn build_summarization_prompt(
        system_prompt: &str,
        turns_to_compact: &[ConversationTurn],
        max_summary_tokens: usize,
        workflow_state: Option<&str>,
    ) -> Vec<Message> {
        let mut conversation_text = String::new();
        for turn in turns_to_compact {
            conversation_text.push_str(&format!("[{}]: {}\n\n", turn.role, turn.content));
        }

        let workflow_section = if let Some(ws) = workflow_state {
            format!(
                "\n\n## WORKFLOW STATE (CRITICAL - PRESERVE EXACTLY)\n\
                 The conversation is part of a multi-step workflow. The current workflow state is:\n\
                 {}\n\
                 You MUST include a dedicated '## Workflow State' section in your summary that \
                 preserves the current step, completed steps, pending steps, and any intermediate \
                 results verbatim. This section is essential for workflow resumption.",
                ws
            )
        } else {
            String::new()
        };

        // T031: Handle large tool results by instructing extraction of key data points
        let tool_result_guidance = if turns_to_compact.iter().any(|t| t.role == "tool_result") {
            "\n\nFor tool results: extract and preserve key data points, values, and identifiers \
             referenced by the user or needed for pending operations. Do NOT include raw JSON \
             payloads — summarize them by their meaningful content."
        } else {
            ""
        };

        let prompt = format!(
            "You are a context compaction assistant. Your task is to compress the following \
             conversation into a structured summary that preserves all critical information.\n\n\
             The summary MUST be no longer than {max_summary_tokens} tokens.\n\n\
             Organize the summary into these sections (omit empty sections):\n\n\
             ## User Identity & Preferences\n\
             Key facts about who the user is, what they prefer, and how they communicate.\n\n\
             ## Key Decisions & Constraints\n\
             Important decisions made, constraints stated, and agreements reached.\n\n\
             ## Critical Tool Results\n\
             Essential data from tool calls: IDs, values, status codes, and outcomes that \
             may be referenced later.{tool_result_guidance}\n\n\
             ## Workflow State\n\
             Current step in any multi-step process, what has been completed, what remains.\n\n\
             ## Open Questions\n\
             Unresolved questions, pending actions, or threads that need follow-up.\n\n\
             IMPORTANT: Be concise but complete. Do not lose facts that could be referenced \
             later in the conversation. Prioritize preserving specific values (IDs, names, \
             numbers) over general descriptions.{workflow_section}\n\n\
             ---\n\n\
             System context (do NOT include in summary, this is for your reference):\n\
             {system_prompt}\n\n\
             ---\n\n\
             Conversation to summarize:\n\
             {conversation_text}"
        );

        vec![
            Message::text(
                "system",
                "You are a precise context compaction assistant. Produce structured \
                 summaries that preserve all critical information in minimal tokens.",
            ),
            Message::text("user", prompt),
        ]
    }

    /// Run LLM summarization (T013) with size enforcement (T014).
    ///
    /// 1. Send structured prompt to the agent's configured model.
    /// 2. Measure summary token count.
    /// 3. If too large, re-prompt once with tighter constraint.
    /// 4. If still too large, hard-truncate as final backstop.
    pub async fn summarize(
        provider: &dyn LlmProvider,
        counter: &TokenCounter,
        system_prompt: &str,
        turns_to_compact: &[ConversationTurn],
        max_summary_tokens: usize,
        workflow_state: Option<&str>,
    ) -> Result<SummarizationResult> {
        // First attempt
        let messages = Self::build_summarization_prompt(
            system_prompt,
            turns_to_compact,
            max_summary_tokens,
            workflow_state,
        );

        let response = provider.chat_complete(&messages, &[]).await?;
        let summary = response
            .content
            .ok_or_else(|| anyhow::anyhow!("LLM returned no content for summarization"))?;

        let token_count = counter.count_text(&summary);

        if token_count <= max_summary_tokens {
            return Ok(SummarizationResult {
                summary,
                summary_token_count: token_count,
                was_retried: false,
                was_truncated: false,
            });
        }

        // T014: Re-prompt once with tighter constraint
        let retry_messages = vec![
            Message::text(
                "system",
                "You are a precise context compaction assistant.",
            ),
            Message::text(
                "user",
                format!(
                    "The following summary is too long ({} tokens). Compress it further \
                     to at most {} tokens while preserving all critical facts, especially \
                     specific values (IDs, names, numbers) and workflow state.\n\n{}",
                    token_count, max_summary_tokens, summary
                ),
            ),
        ];

        let retry_response = provider.chat_complete(&retry_messages, &[]).await?;
        if let Some(retry_summary) = retry_response.content {
            let retry_token_count = counter.count_text(&retry_summary);
            if retry_token_count <= max_summary_tokens {
                return Ok(SummarizationResult {
                    summary: retry_summary,
                    summary_token_count: retry_token_count,
                    was_retried: true,
                    was_truncated: false,
                });
            }

            // Hard-truncate as final backstop (R-005)
            let truncated = hard_truncate_summary(&retry_summary, counter, max_summary_tokens);
            let truncated_count = counter.count_text(&truncated);
            return Ok(SummarizationResult {
                summary: truncated,
                summary_token_count: truncated_count,
                was_retried: true,
                was_truncated: true,
            });
        }

        // Retry produced no content — hard-truncate original
        let truncated = hard_truncate_summary(&summary, counter, max_summary_tokens);
        let truncated_count = counter.count_text(&truncated);
        Ok(SummarizationResult {
            summary: truncated,
            summary_token_count: truncated_count,
            was_retried: true,
            was_truncated: true,
        })
    }
}

/// Hard-truncate a summary by removing characters from the end until it fits.
/// Preserves complete lines where possible.
fn hard_truncate_summary(summary: &str, counter: &TokenCounter, max_tokens: usize) -> String {
    // Binary search for the right truncation point by lines
    let lines: Vec<&str> = summary.lines().collect();
    let mut lo = 0usize;
    let mut hi = lines.len();

    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = lines[..mid].join("\n");
        if counter.count_text(&candidate) <= max_tokens {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }

    if lo == 0 {
        // Even one line is too long — truncate by characters
        let chars: Vec<char> = summary.chars().collect();
        let mut end = chars.len();
        loop {
            let candidate: String = chars[..end].iter().collect();
            if counter.count_text(&candidate) <= max_tokens || end <= 1 {
                return format!("{}\n[summary truncated]", candidate);
            }
            end = end * 3 / 4; // Shrink by 25%
        }
    }

    let mut result: String = lines[..lo].join("\n");
    if lo < lines.len() {
        result.push_str("\n[summary truncated]");
    }
    result
}
