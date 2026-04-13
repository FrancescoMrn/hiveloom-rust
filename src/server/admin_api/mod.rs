use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

/// Auth context stored in request extensions after the auth middleware runs.
#[derive(Clone, Debug)]
pub struct AuthContext {
    pub is_platform_admin: bool,
    pub tenant_id: Option<uuid::Uuid>,
    pub scope: String,
}

pub fn router(state: Arc<super::AppState>) -> Router<Arc<super::AppState>> {
    Router::new()
        // Tenant routes, agent routes, etc. will be added per story
        // For now, just the skeleton with auth middleware
        .layer(middleware::from_fn(
            move |request: Request, next: Next| {
                let state = state.clone();
                async move { auth_middleware_inner(state, request, next).await }
            },
        ))
}

/// Auth middleware that:
/// - Grants implicit trust for local requests (127.0.0.1 / ::1) per FR-037b
/// - For remote requests, validates `Authorization: Bearer <token>` against PlatformAdminToken store
async fn auth_middleware_inner(
    state: Arc<super::AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Check if the request originates from a loopback address.
    let is_local = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().is_loopback())
        .unwrap_or(false);

    if is_local {
        // Local requests get implicit platform-admin trust (FR-037b)
        request.extensions_mut().insert(AuthContext {
            is_platform_admin: true,
            tenant_id: None,
            scope: "platform:admin".to_string(),
        });
        return next.run(request).await;
    }

    // Remote requests require a valid bearer token
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let token = match auth_header {
        Some(ref h) if h.starts_with("Bearer ") => h[7..].to_string(),
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };

    // Hash the token and validate against the platform store
    use sha2::{Sha256, Digest};
    let token_hash = hex::encode(Sha256::digest(token.as_bytes()));

    // Scope the MutexGuard so it is dropped before any .await
    let validated = {
        let conn = state.platform_store.conn();
        crate::store::models::PlatformAdminToken::validate(&conn, &token_hash)
    };

    match validated {
        Ok(Some(admin_token)) => {
            request.extensions_mut().insert(AuthContext {
                is_platform_admin: true,
                tenant_id: None,
                scope: admin_token.scope.clone(),
            });
            next.run(request).await
        }
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}
