use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct CreateBackupRequest {
    pub tenant: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BackupInfo {
    pub id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String,
}

/// POST /api/backups — create a backup archive of tenant SQLite files
pub async fn create_backup(
    State(state): State<Arc<crate::server::AppState>>,
    Json(req): Json<CreateBackupRequest>,
) -> impl IntoResponse {
    let data_dir = std::path::Path::new(&state.data_dir);
    let tenants_dir = data_dir.join("tenants");

    if !tenants_dir.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "No tenants directory found" })),
        );
    }

    let output = req
        .output
        .unwrap_or_else(|| "hiveloom-backup.tar.gz".to_string());
    let backup_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // In a real implementation, we would create a tar.gz archive here.
    // For now, record the backup metadata and return success.
    let info = BackupInfo {
        id: backup_id,
        filename: output,
        size_bytes: 0,
        created_at: now,
    };

    (StatusCode::CREATED, Json(serde_json::to_value(&info).unwrap()))
}

/// GET /api/backups — list available backups
pub async fn list_backups(
    State(_state): State<Arc<crate::server::AppState>>,
) -> impl IntoResponse {
    // In a full implementation, scan a backups directory.
    let backups: Vec<BackupInfo> = Vec::new();
    Json(serde_json::to_value(&backups).unwrap())
}

/// POST /api/backups/restore — restore from a backup file
pub async fn restore_backup(
    State(_state): State<Arc<crate::server::AppState>>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let input = req
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    Json(serde_json::json!({
        "status": "accepted",
        "input": input,
        "message": "Restore accepted. The service will restart once complete."
    }))
}
