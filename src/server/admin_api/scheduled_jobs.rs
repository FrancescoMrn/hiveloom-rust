use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::engine::scheduler::compute_next_fire;
use crate::store::models::ScheduledJob;

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

#[derive(Deserialize)]
pub struct CreateScheduledJobRequest {
    pub cron_expression: Option<String>,
    pub one_time_at: Option<String>,
    #[serde(default = "default_tz")]
    pub timezone: String,
    #[serde(default)]
    pub initial_context: String,
}

fn default_tz() -> String {
    "UTC".to_string()
}

#[derive(Deserialize)]
pub struct UpdateScheduledJobRequest {
    pub cron_expression: Option<String>,
    pub one_time_at: Option<String>,
    pub timezone: Option<String>,
    pub initial_context: Option<String>,
}

pub async fn create_scheduled_job(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, aid)): Path<(uuid::Uuid, uuid::Uuid)>,
    Json(body): Json<CreateScheduledJobRequest>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();

    // Compute initial next_fire_at
    let next_fire_at = if let Some(ref cron_expr) = body.cron_expression {
        match compute_next_fire(cron_expr, &body.timezone, chrono::Utc::now()) {
            Ok(next) => Some(next.to_rfc3339()),
            Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
        }
    } else {
        body.one_time_at.clone()
    };

    match ScheduledJob::create(
        conn,
        tid,
        aid,
        body.cron_expression.as_deref(),
        body.one_time_at.as_deref(),
        &body.timezone,
        &body.initial_context,
        next_fire_at.as_deref(),
    ) {
        Ok(job) => (StatusCode::CREATED, Json(serde_json::to_value(job).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn list_scheduled_jobs(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, aid)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match ScheduledJob::list_by_agent(conn, tid, aid) {
        Ok(jobs) => (StatusCode::OK, Json(serde_json::to_value(jobs).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn get_scheduled_job(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, jid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match ScheduledJob::get(conn, jid) {
        Ok(Some(job)) => (StatusCode::OK, Json(serde_json::to_value(job).unwrap())),
        Ok(None) => err_json(StatusCode::NOT_FOUND, "Scheduled job not found"),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn update_scheduled_job(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, jid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
    Json(body): Json<UpdateScheduledJobRequest>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();

    // Get existing job
    let existing = match ScheduledJob::get(conn, jid) {
        Ok(Some(j)) => j,
        Ok(None) => return err_json(StatusCode::NOT_FOUND, "Scheduled job not found"),
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    let cron_expr = body.cron_expression.as_deref().or(existing.cron_expression.as_deref());
    let tz = body.timezone.as_deref().unwrap_or(&existing.timezone);

    // Recompute next_fire_at if cron changed
    if let Some(cron) = cron_expr {
        match compute_next_fire(cron, tz, chrono::Utc::now()) {
            Ok(next) => {
                let next_str = next.to_rfc3339();
                let _ = ScheduledJob::update_next_fire(conn, jid, Some(&next_str));
            }
            Err(e) => return err_json(StatusCode::BAD_REQUEST, &e.to_string()),
        }
    }

    // Update fields via direct SQL for simplicity
    let now = chrono::Utc::now().to_rfc3339();
    let new_cron = body.cron_expression.or(existing.cron_expression);
    let new_one_time = body.one_time_at.or(existing.one_time_at);
    let new_tz = body.timezone.unwrap_or(existing.timezone);
    let new_ctx = body.initial_context.unwrap_or(existing.initial_context);

    if let Err(e) = conn.execute(
        "UPDATE scheduled_jobs SET cron_expression = ?1, one_time_at = ?2, timezone = ?3, initial_context = ?4 WHERE id = ?5",
        rusqlite::params![new_cron, new_one_time, new_tz, new_ctx, jid.to_string()],
    ) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    let _ = now; // suppress unused

    match ScheduledJob::get(conn, jid) {
        Ok(Some(job)) => (StatusCode::OK, Json(serde_json::to_value(job).unwrap())),
        _ => err_json(StatusCode::NOT_FOUND, "Scheduled job not found after update"),
    }
}

pub async fn delete_scheduled_job(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, jid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match ScheduledJob::delete(conn, jid) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn pause_scheduled_job(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, jid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match ScheduledJob::pause(conn, jid) {
        Ok(()) => {
            match ScheduledJob::get(conn, jid) {
                Ok(Some(job)) => (StatusCode::OK, Json(serde_json::to_value(job).unwrap())),
                _ => err_json(StatusCode::NOT_FOUND, "Scheduled job not found"),
            }
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn resume_scheduled_job(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid, _aid, jid)): Path<(uuid::Uuid, uuid::Uuid, uuid::Uuid)>,
) -> impl IntoResponse {
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match ScheduledJob::resume(conn, jid) {
        Ok(()) => {
            // Recompute next_fire_at
            if let Ok(Some(job)) = ScheduledJob::get(conn, jid) {
                if let Some(ref cron_expr) = job.cron_expression {
                    if let Ok(next) = compute_next_fire(cron_expr, &job.timezone, chrono::Utc::now()) {
                        let next_str = next.to_rfc3339();
                        let _ = ScheduledJob::update_next_fire(conn, jid, Some(&next_str));
                    }
                }
            }
            match ScheduledJob::get(conn, jid) {
                Ok(Some(job)) => (StatusCode::OK, Json(serde_json::to_value(job).unwrap())),
                _ => err_json(StatusCode::NOT_FOUND, "Scheduled job not found"),
            }
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
