use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_SYSTEM_DATA_DIR: &str = "/var/lib/hiveloom";
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:3000";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalConfig {
    #[serde(default)]
    pub data_dir: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_tenant_value")]
    pub default_tenant: String,
    #[serde(default = "default_host_value")]
    pub host: String,
    #[serde(default = "default_port_value")]
    pub port: u16,
}

fn default_tenant_value() -> String {
    "default".to_string()
}

fn default_host_value() -> String {
    "127.0.0.1".to_string()
}

fn default_port_value() -> u16 {
    3000
}

pub fn workspace_local_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let dir = cwd.join(".hiveloom");
    if dir.exists() {
        Some(dir)
    } else {
        None
    }
}

pub fn home_local_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".hiveloom");
    if dir.exists() {
        Some(dir)
    } else {
        None
    }
}

pub fn default_data_dir() -> String {
    if let Ok(dir) = std::env::var("HIVELOOM_DATA_DIR") {
        return dir;
    }

    if let Some(dir) = workspace_local_dir() {
        return dir.to_string_lossy().to_string();
    }

    if let Some(dir) = home_local_dir() {
        return dir.to_string_lossy().to_string();
    }

    DEFAULT_SYSTEM_DATA_DIR.to_string()
}

pub fn default_endpoint() -> String {
    if let Ok(endpoint) = std::env::var("HIVELOOM_ENDPOINT") {
        return endpoint;
    }

    let data_dir = PathBuf::from(default_data_dir());
    let endpoint_path = data_dir.join("run").join("endpoint");
    if let Ok(endpoint) = std::fs::read_to_string(&endpoint_path) {
        let endpoint = endpoint.trim();
        if !endpoint.is_empty() {
            return endpoint.to_string();
        }
    }

    if let Some(cfg) = load_local_config(&data_dir) {
        if !cfg.endpoint.is_empty() {
            return cfg.endpoint;
        }
    }

    DEFAULT_ENDPOINT.to_string()
}

pub fn default_tenant() -> String {
    if let Ok(tenant) = std::env::var("HIVELOOM_TENANT") {
        return tenant;
    }

    let data_dir = PathBuf::from(default_data_dir());
    if let Some(cfg) = load_local_config(&data_dir) {
        if !cfg.default_tenant.is_empty() {
            return cfg.default_tenant;
        }
    }

    default_tenant_value()
}

pub fn write_local_config(data_dir: &Path, host: &str, port: u16) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    std::fs::create_dir_all(data_dir.join("run"))?;
    std::fs::create_dir_all(data_dir.join("logs"))?;
    std::fs::create_dir_all(data_dir.join("backups"))?;
    std::fs::create_dir_all(data_dir.join("manifests"))?;

    let endpoint = format!("http://{}:{}", host, port);
    let cfg = LocalConfig {
        data_dir: data_dir.to_string_lossy().to_string(),
        endpoint: endpoint.clone(),
        default_tenant: default_tenant_value(),
        host: host.to_string(),
        port,
    };

    let config_path = data_dir.join("config.json");
    let json = serde_json::to_vec_pretty(&cfg)?;
    std::fs::write(config_path, json)?;
    std::fs::write(
        data_dir.join("run").join("endpoint"),
        format!("{endpoint}\n"),
    )?;
    std::fs::write(
        data_dir.join("run").join("service.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "pid": std::process::id(),
            "endpoint": endpoint,
            "host": host,
            "port": port,
            "data_dir": data_dir.to_string_lossy().to_string(),
        }))?,
    )?;
    std::fs::write(
        data_dir.join("run").join("service.pid"),
        format!("{}\n", std::process::id()),
    )?;

    Ok(())
}

fn load_local_config(data_dir: &Path) -> Option<LocalConfig> {
    let config_path = data_dir.join("config.json");
    let bytes = std::fs::read(config_path).ok()?;
    serde_json::from_slice(&bytes).ok()
}
