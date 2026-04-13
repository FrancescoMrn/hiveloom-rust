use axum::extract::State;
use axum::http::StatusCode;
use std::sync::Arc;

/// Health check endpoint.
///
/// Verifies the platform store is accessible by acquiring the connection lock.
/// Returns 200 OK if the service is operational, 503 Service Unavailable otherwise.
pub async fn handler(State(state): State<Arc<super::AppState>>) -> StatusCode {
    // Attempt to acquire the platform store connection to verify it is accessible.
    // If the mutex is poisoned or the store is otherwise unavailable, return 503.
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _conn = state.platform_store.conn();
        true
    }))
    .unwrap_or(false);

    if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
