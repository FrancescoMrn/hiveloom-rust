pub mod auth;
pub mod session;
pub mod transport;

use crate::engine::chat_surface::{ChatSurface, SurfaceType};

/// MCP ChatSurface implementation (T084).
///
/// For MCP, "sending a message" means the response is returned inline as the
/// JSON-RPC response. This surface stores the pending response so the
/// transport layer can retrieve it.
pub struct McpSurface {
    /// Collected messages to return in the JSON-RPC response.
    messages: std::sync::Mutex<Vec<String>>,
}

impl McpSurface {
    pub fn new() -> Self {
        Self {
            messages: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Retrieve all collected messages (drains the buffer).
    pub fn take_messages(&self) -> Vec<String> {
        let mut msgs = self.messages.lock().unwrap();
        std::mem::take(&mut *msgs)
    }
}

#[async_trait::async_trait]
impl ChatSurface for McpSurface {
    async fn send_message(
        &self,
        _surface_ref: &str,
        _thread_ref: Option<&str>,
        content: &str,
    ) -> anyhow::Result<()> {
        let mut msgs = self.messages.lock().unwrap();
        msgs.push(content.to_string());
        Ok(())
    }

    fn surface_type(&self) -> SurfaceType {
        SurfaceType::Mcp
    }
}
