//! Fallback truncation strategy (FR-014).
//!
//! When LLM summarization fails, drop oldest turns beyond the protected window,
//! keeping system prompt + most recent N turns + 10% response buffer.

use crate::store::models::ConversationTurn;
use crate::compaction::counter::TokenCounter;

/// Result of a truncation operation.
#[derive(Debug, Clone)]
pub struct TruncationResult {
    /// Indices of turns that were kept (not dropped).
    pub kept_turn_indices: Vec<i64>,
    /// Indices of turns that were dropped.
    pub dropped_turn_indices: Vec<i64>,
    /// Token count after truncation.
    pub tokens_after: usize,
}

pub struct Truncator;

impl Truncator {
    /// Apply fallback truncation (T015).
    ///
    /// Strategy:
    /// 1. Always keep the system prompt (turn_index 0 if role=system).
    /// 2. Keep the most recent `protected_turn_count` turns.
    /// 3. Keep in-flight tool_call/tool_result pairs in the protected window (T020).
    /// 4. Drop oldest turns until total fits within context_window_size * threshold_fraction.
    /// 5. Reserve 10% of window for response buffer.
    pub fn truncate(
        turns: &[ConversationTurn],
        counter: &TokenCounter,
        context_window_size: usize,
        protected_turn_count: usize,
        existing_summary: Option<&str>,
    ) -> TruncationResult {
        if turns.is_empty() {
            return TruncationResult {
                kept_turn_indices: vec![],
                dropped_turn_indices: vec![],
                tokens_after: 0,
            };
        }

        // Target: leave room for response (10% buffer)
        let target_tokens = (context_window_size as f64 * 0.9) as usize;

        // Identify system turn(s) — always kept
        let mut protected_indices: Vec<usize> = Vec::new();
        for (i, turn) in turns.iter().enumerate() {
            if turn.role == "system" {
                protected_indices.push(i);
            }
        }

        // Protect the most recent N turns
        let total = turns.len();
        let protected_start = if total > protected_turn_count {
            total - protected_turn_count
        } else {
            0
        };

        for i in protected_start..total {
            if !protected_indices.contains(&i) {
                protected_indices.push(i);
            }
        }

        // Extend protection to cover in-flight tool_call/tool_result pairs (T020)
        // If a tool_result is in the protected window, also protect the preceding assistant turn
        let mut extra_protected = Vec::new();
        for &idx in &protected_indices {
            if turns[idx].role == "tool_result" && idx > 0 {
                if !protected_indices.contains(&(idx - 1)) {
                    extra_protected.push(idx - 1);
                }
            }
        }
        protected_indices.extend(extra_protected);
        protected_indices.sort();
        protected_indices.dedup();

        // Count tokens for summary if present
        let summary_tokens = existing_summary
            .map(|s| counter.count_text(s) + 10) // overhead for summary injection
            .unwrap_or(0);

        // Drop oldest non-protected turns until we fit
        let mut kept_indices: Vec<usize> = protected_indices.clone();
        let mut droppable: Vec<usize> = (0..total)
            .filter(|i| !protected_indices.contains(i))
            .collect();
        // Sort droppable newest-first so we drop oldest first
        droppable.reverse();

        // Start with all droppable turns included, then remove from oldest
        kept_indices.extend(droppable.iter());
        kept_indices.sort();

        loop {
            let token_count = count_kept_tokens(turns, &kept_indices, counter) + summary_tokens;
            if token_count <= target_tokens || kept_indices.len() <= protected_indices.len() {
                break;
            }
            // Remove the oldest non-protected turn
            if let Some(pos) = kept_indices
                .iter()
                .position(|i| !protected_indices.contains(i))
            {
                kept_indices.remove(pos);
            } else {
                break;
            }
        }

        let kept_turn_indices: Vec<i64> = kept_indices.iter().map(|&i| turns[i].turn_index).collect();
        let dropped_turn_indices: Vec<i64> = turns
            .iter()
            .filter(|t| !kept_turn_indices.contains(&t.turn_index))
            .map(|t| t.turn_index)
            .collect();

        let tokens_after = count_kept_tokens(turns, &kept_indices, counter) + summary_tokens;

        TruncationResult {
            kept_turn_indices,
            dropped_turn_indices,
            tokens_after,
        }
    }
}

fn count_kept_tokens(
    turns: &[ConversationTurn],
    kept_indices: &[usize],
    counter: &TokenCounter,
) -> usize {
    let mut total = 0;
    for &idx in kept_indices {
        total += 4; // per-message overhead
        total += counter.count_text(&turns[idx].role);
        total += counter.count_text(&turns[idx].content);
    }
    total += 3; // final priming
    total
}
