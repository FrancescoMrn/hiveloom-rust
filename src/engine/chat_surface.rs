#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SurfaceType {
    Slack,
    Mcp,
}

impl std::fmt::Display for SurfaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SurfaceType::Slack => write!(f, "slack"),
            SurfaceType::Mcp => write!(f, "mcp"),
        }
    }
}

impl std::str::FromStr for SurfaceType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "slack" => Ok(SurfaceType::Slack),
            "mcp" => Ok(SurfaceType::Mcp),
            _ => anyhow::bail!("Unknown surface type: {}", s),
        }
    }
}

#[async_trait::async_trait]
pub trait ChatSurface: Send + Sync {
    async fn send_message(
        &self,
        surface_ref: &str,
        thread_ref: Option<&str>,
        content: &str,
    ) -> anyhow::Result<()>;

    fn surface_type(&self) -> SurfaceType;
}
