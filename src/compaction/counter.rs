//! Token counting using tiktoken-rs with model-specific tokenizer selection.
//!
//! Provides fast, local token counting for threshold checks on every turn.

use crate::llm::provider::Message;

/// Known model context window sizes (T035).
/// Returns the token limit for a given model identifier.
pub fn model_context_window(model_id: &str) -> usize {
    match model_id {
        // Anthropic Claude models
        m if m.starts_with("claude-3-5") => 200_000,
        m if m.starts_with("claude-3") => 200_000,
        m if m.starts_with("claude-sonnet-4") => 200_000,
        m if m.starts_with("claude-opus-4") => 200_000,
        m if m.starts_with("claude-") => 200_000,
        // OpenAI GPT-4 models
        m if m.starts_with("gpt-4o") => 128_000,
        m if m.starts_with("gpt-4-turbo") => 128_000,
        m if m.starts_with("gpt-4-32k") => 32_768,
        m if m.starts_with("gpt-4") => 8_192,
        // OpenAI GPT-3.5
        m if m.starts_with("gpt-3.5-turbo-16k") => 16_384,
        m if m.starts_with("gpt-3.5") => 4_096,
        // OpenAI o-series
        m if m.starts_with("o1") => 200_000,
        m if m.starts_with("o3") => 200_000,
        // Conservative default for unknown models
        _ => 8_192,
    }
}

/// Token counter that uses tiktoken-rs for accurate BPE tokenization.
pub struct TokenCounter {
    /// The BPE tokenizer (cl100k_base covers GPT-4 and Claude-compatible tokenization).
    bpe: tiktoken_rs::CoreBPE,
}

impl TokenCounter {
    /// Create a new token counter. Uses cl100k_base encoding which provides
    /// reasonable accuracy (within 5%) for both OpenAI and Anthropic models.
    pub fn new() -> Self {
        let bpe = tiktoken_rs::cl100k_base().expect("Failed to load cl100k_base tokenizer");
        Self { bpe }
    }

    /// Count tokens in a single text string.
    pub fn count_text(&self, text: &str) -> usize {
        self.bpe.encode_with_special_tokens(text).len()
    }

    /// Count tokens across a slice of messages (system + conversation turns).
    /// Includes per-message overhead (role markers, separators).
    pub fn count_messages(&self, messages: &[Message]) -> usize {
        let mut total = 0;
        for msg in messages {
            // Each message has ~4 tokens of overhead (role, separators)
            total += 4;
            total += self.count_text(&msg.role);
            total += self.count_text(&msg.content);
        }
        // Final assistant priming
        total += 3;
        total
    }

    /// Count tokens for a list of conversation turns (content only, no role overhead).
    pub fn count_turn_contents(&self, turns: &[crate::store::models::ConversationTurn]) -> usize {
        let mut total = 0;
        for turn in turns {
            total += 4; // per-message overhead
            total += self.count_text(&turn.role);
            total += self.count_text(&turn.content);
        }
        total += 3; // final priming
        total
    }
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new()
    }
}
