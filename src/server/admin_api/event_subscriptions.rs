use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::store::models::EventSubscription;

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

#[derive(Deserialize)]
pub struct CreateEventSubscriptionRequest {
    pub event_type: String,
    pub source_filter: Option<String>,
    /// Plain-text auth token; server hashes it before storing.
    pub auth_token: String,
}

#[derive(Deserialize)]
pub struct UpdateEventSubscriptionRequest {
    pub event_type: Option<String>,
    pub source_filter: Option<String>,
    pub auth_token: Option<String>,
}

pub async fn create_event_subscription(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, aid)): Path<(uuid::Uuid, uuid::Uuid)>,
    Json(body): Json<CreateEventSubscriptionRequest>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();

    let token_hash = hex::encode(Sha256::digest(body.auth_token.as_bytes()));

    match EventSubscription::create(
        conn,
        tid,
        aid,
        &body.event_type,
        body.source_filter.as_deref(),
        &token_hash,
    ) {
        Ok(sub) => (StatusCode::CREATED, Json(serde_json::to_value(sub).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn list_event_subscriptions(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, aid)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match EventSubscription::list_by_agent(conn, tid, aid) {
        Ok(subs) => (StatusCode::OK, Json(serde_json::to_value(subs).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn get_event_subscription(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, sid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match EventSubscription::get(conn, sid) {
        Ok(Some(sub)) => (StatusCode::OK, Json(serde_json::to_value(sub).unwrap())),
        Ok(None) => err_json(StatusCode::NOT_FOUND, "Event subscription not found"),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn delete_event_subscription(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, sid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match EventSubscription::delete(conn, sid) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn disable_event_subscription(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, sid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match EventSubscription::disable(conn, sid) {
        Ok(()) => {
            match EventSubscription::get(conn, sid) {
                Ok(Some(sub)) => (StatusCode::OK, Json(serde_json::to_value(sub).unwrap())),
                _ => err_json(StatusCode::NOT_FOUND, "Subscription not found"),
            }
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn enable_event_subscription(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, sid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match EventSubscription::enable(conn, sid) {
        Ok(()) => {
            match EventSubscription::get(conn, sid) {
                Ok(Some(sub)) => (StatusCode::OK, Json(serde_json::to_value(sub).unwrap())),
                _ => err_json(StatusCode::NOT_FOUND, "Subscription not found"),
            }
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
