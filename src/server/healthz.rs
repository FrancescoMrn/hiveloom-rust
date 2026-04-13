use axum::extract::State;
use std::sync::Arc;

pub async fn handler(State(_state): State<Arc<super::AppState>>) -> impl axum::response::IntoResponse {
    axum::http::StatusCode::OK
}
