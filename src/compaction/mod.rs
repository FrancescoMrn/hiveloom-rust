//! Context compaction module.
//!
//! Provides LLM-generated summarization to compress older conversation turns
//! when the token count approaches the model's context window limit.
//! Falls back to truncation on summarization failure.

pub mod config;
pub mod counter;
pub mod engine;
pub mod event;
pub mod indicator;
pub mod summarizer;
pub mod truncator;

pub use config::{CompactionConfig, ResolvedCompactionConfig};
pub use counter::TokenCounter;
pub use engine::CompactionEngine;
pub use event::CompactionEvent;
pub use indicator::CompactionIndicator;
pub use summarizer::Summarizer;
pub use truncator::Truncator;
