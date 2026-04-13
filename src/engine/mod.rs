pub mod agent_loop;
pub mod capability_exec;
pub mod chat_surface;
pub mod conversation;
pub mod dedup;
pub mod event_router;
pub mod memory;
pub mod reflection;
pub mod scheduler;
pub mod workflow;

pub use agent_loop::{AgentInvocation, InvocationResult};
pub use chat_surface::{ChatSurface, SurfaceType};
pub use dedup::DedupTable;
