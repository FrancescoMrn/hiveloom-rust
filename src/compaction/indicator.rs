//! End-user compaction indicator (FR-017, T032).
//!
//! When show_indicator is true on the agent's compaction config,
//! injects a status note into the agent's response on turns where
//! compaction occurred.

use crate::compaction::engine::CompactionOutcome;

pub struct CompactionIndicator;

impl CompactionIndicator {
    /// Generate an indicator message if compaction occurred and show_indicator is enabled.
    ///
    /// Returns None if no indicator should be shown (either no compaction or indicator disabled).
    pub fn maybe_indicator(outcome: &CompactionOutcome) -> Option<String> {
        match outcome {
            CompactionOutcome::NotNeeded => None,
            CompactionOutcome::Compacted {
                tokens_before,
                tokens_after,
                strategy,
                fallback_used,
                show_indicator,
                ..
            } => {
                if !show_indicator {
                    return None;
                }

                let reduction_pct = if *tokens_before > 0 {
                    (((*tokens_before - *tokens_after) as f64 / *tokens_before as f64) * 100.0)
                        as u64
                } else {
                    0
                };

                let fallback_note = if *fallback_used {
                    " (using fallback strategy)"
                } else {
                    ""
                };

                Some(format!(
                    "[Context compacted: {} strategy{}, reduced by {}% ({} -> {} tokens)]",
                    strategy, fallback_note, reduction_pct, tokens_before, tokens_after
                ))
            }
        }
    }

    /// Inject indicator into an assistant response if applicable.
    pub fn inject_indicator(response: &str, outcome: &CompactionOutcome) -> String {
        match Self::maybe_indicator(outcome) {
            Some(indicator) => format!("{}\n\n{}", indicator, response),
            None => response.to_string(),
        }
    }
}
