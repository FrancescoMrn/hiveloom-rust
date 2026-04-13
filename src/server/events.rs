use axum::{
    extract::{Json, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::engine::event_router;

#[derive(Deserialize)]
pub struct InboundEventPayload {
    pub event_type: String,
    pub payload: serde_json::Value,
    /// Optional dedup ID to prevent duplicate processing.
    #[serde(default)]
    pub delivery_id: Option<String>,
}

/// POST /events/:tid/inbound
///
/// Inbound event webhook endpoint. Verifies auth token from Authorization header,
/// optionally deduplicates by delivery_id, and dispatches to the event router.
pub async fn handle_inbound_event(
    State(state): State<Arc<super::AppState>>,
    Path(tid): Path<uuid::Uuid>,
    headers: HeaderMap,
    Json(body): Json<InboundEventPayload>,
) -> impl IntoResponse {
    // Extract auth token from Authorization header
    let auth_token = match headers.get("authorization").and_then(|v| v.to_str().ok()) {
        Some(h) if h.starts_with("Bearer ") => h[7..].to_string(),
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
            );
        }
    };

    // Optional dedup check: open tenant store to use the full dedup mechanism
    if let Some(ref delivery_id) = body.delivery_id {
        if let Ok(tenant_store) = state.open_tenant_store(&tid) {
            let conn = tenant_store.conn();
            match state
                .dedup
                .check_and_record(conn, delivery_id, &tid, "event")
            {
                Ok(false) => {
                    // Duplicate
                    return (
                        StatusCode::OK,
                        Json(
                            serde_json::json!({ "status": "duplicate", "delivery_id": delivery_id }),
                        ),
                    );
                }
                Ok(true) => { /* new delivery, proceed */ }
                Err(_) => { /* dedup check failed, proceed anyway */ }
            }
        }
    }

    // Route the event
    match event_router::route_event(
        &state.data_dir,
        &tid,
        &body.event_type,
        &body.payload,
        &auth_token,
    )
    .await
    {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "event_type": body.event_type
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}
