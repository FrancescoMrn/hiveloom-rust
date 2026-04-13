use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::store::models::Tenant;

#[derive(Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
    pub slug: String,
    #[serde(default = "default_tz")]
    pub timezone: String,
}

fn default_tz() -> String {
    "UTC".to_string()
}

#[derive(Deserialize)]
pub struct UpdateTenantRequest {
    pub name: String,
    pub slug: String,
    pub timezone: String,
}

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

pub async fn create_tenant(
    State(state): State<Arc<super::super::AppState>>,
    Json(body): Json<CreateTenantRequest>,
) -> impl IntoResponse {
    let conn = state.platform_store.conn();
    match Tenant::create(&conn, &body.name, &body.slug, &body.timezone) {
        Ok(tenant) => (StatusCode::CREATED, Json(serde_json::to_value(tenant).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn list_tenants(
    State(state): State<Arc<super::super::AppState>>,
) -> impl IntoResponse {
    let conn = state.platform_store.conn();
    match Tenant::list(&conn) {
        Ok(tenants) => (StatusCode::OK, Json(serde_json::to_value(tenants).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn get_tenant(
    State(state): State<Arc<super::super::AppState>>,
    Path(tid): Path<uuid::Uuid>,
) -> impl IntoResponse {
    let conn = state.platform_store.conn();
    match Tenant::get_by_id(&conn, tid) {
        Ok(Some(tenant)) => (StatusCode::OK, Json(serde_json::to_value(tenant).unwrap())),
        Ok(None) => err_json(StatusCode::NOT_FOUND, "Tenant not found"),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn update_tenant(
    State(state): State<Arc<super::super::AppState>>,
    Path(tid): Path<uuid::Uuid>,
    Json(body): Json<UpdateTenantRequest>,
) -> impl IntoResponse {
    let conn = state.platform_store.conn();
    match Tenant::update(&conn, tid, &body.name, &body.slug, &body.timezone) {
        Ok(()) => match Tenant::get_by_id(&conn, tid) {
            Ok(Some(t)) => (StatusCode::OK, Json(serde_json::to_value(t).unwrap())),
            _ => err_json(StatusCode::NOT_FOUND, "Tenant not found"),
        },
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn delete_tenant(
    State(state): State<Arc<super::super::AppState>>,
    Path(tid): Path<uuid::Uuid>,
) -> impl IntoResponse {
    let conn = state.platform_store.conn();
    match Tenant::delete(&conn, tid) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
