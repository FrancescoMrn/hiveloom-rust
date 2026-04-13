use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use std::sync::Arc;

pub mod agents;
pub mod auth_tokens;
pub mod backups;
pub mod capabilities;
pub mod credentials;
pub mod event_subscriptions;
pub mod mcp_identities;
pub mod reflections;
pub mod scheduled_jobs;
pub mod tenants;

/// Auth context stored in request extensions after the auth middleware runs.
#[derive(Clone, Debug)]
pub struct AuthContext {
    pub is_platform_admin: bool,
    pub tenant_id: Option<uuid::Uuid>,
    pub scope: String,
}

pub fn router(state: Arc<super::AppState>) -> Router<Arc<super::AppState>> {
    Router::new()
        // ── Tenant routes (T044) ────────────────────────────────────────
        .route("/tenants", post(tenants::create_tenant))
        .route("/tenants", get(tenants::list_tenants))
        .route("/tenants/{tid}", get(tenants::get_tenant))
        .route("/tenants/{tid}", put(tenants::update_tenant))
        .route("/tenants/{tid}", delete(tenants::delete_tenant))
        // ── Agent routes (T041) ─────────────────────────────────────────
        .route("/tenants/{tid}/agents", post(agents::create_agent))
        .route("/tenants/{tid}/agents", get(agents::list_agents))
        .route("/tenants/{tid}/agents/{aid}", get(agents::get_agent))
        .route("/tenants/{tid}/agents/{aid}", put(agents::update_agent))
        .route("/tenants/{tid}/agents/{aid}", delete(agents::delete_agent))
        .route(
            "/tenants/{tid}/agents/{aid}/versions",
            get(agents::list_versions),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/rollback",
            post(agents::rollback_agent),
        )
        // ── Capability routes (T042) ────────────────────────────────────
        .route(
            "/tenants/{tid}/agents/{aid}/capabilities",
            post(capabilities::create_capability),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/capabilities",
            get(capabilities::list_capabilities),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/capabilities/{cid}",
            get(capabilities::get_capability),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/capabilities/{cid}",
            put(capabilities::update_capability),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/capabilities/{cid}",
            delete(capabilities::delete_capability),
        )
        // ── Credential routes (T043) ────────────────────────────────────
        .route("/tenants/{tid}/credentials", post(credentials::set_credential))
        .route("/tenants/{tid}/credentials", get(credentials::list_credentials))
        .route(
            "/tenants/{tid}/credentials/{name}",
            delete(credentials::delete_credential),
        )
        .route(
            "/tenants/{tid}/credentials/{name}/rotate",
            post(credentials::rotate_credential),
        )
        // ── ChatSurfaceBinding routes (T045) ────────────────────────────
        .route(
            "/tenants/{tid}/agents/{aid}/bindings",
            post(agents::create_binding),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/bindings",
            get(agents::list_bindings),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/bindings/{bid}",
            delete(agents::delete_binding),
        )
        // ── Scheduled Job routes (T059) ────────────────────────────────
        .route(
            "/tenants/{tid}/agents/{aid}/scheduled-jobs",
            post(scheduled_jobs::create_scheduled_job),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/scheduled-jobs",
            get(scheduled_jobs::list_scheduled_jobs),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/scheduled-jobs/{jid}",
            get(scheduled_jobs::get_scheduled_job),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/scheduled-jobs/{jid}",
            put(scheduled_jobs::update_scheduled_job),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/scheduled-jobs/{jid}",
            delete(scheduled_jobs::delete_scheduled_job),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/scheduled-jobs/{jid}/pause",
            post(scheduled_jobs::pause_scheduled_job),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/scheduled-jobs/{jid}/resume",
            post(scheduled_jobs::resume_scheduled_job),
        )
        // ── Event Subscription routes (T066) ───────────────────────────
        .route(
            "/tenants/{tid}/agents/{aid}/event-subscriptions",
            post(event_subscriptions::create_event_subscription),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/event-subscriptions",
            get(event_subscriptions::list_event_subscriptions),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/event-subscriptions/{sid}",
            get(event_subscriptions::get_event_subscription),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/event-subscriptions/{sid}",
            delete(event_subscriptions::delete_event_subscription),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/event-subscriptions/{sid}/disable",
            post(event_subscriptions::disable_event_subscription),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/event-subscriptions/{sid}/enable",
            post(event_subscriptions::enable_event_subscription),
        )
        // ── MCP Identity routes (T089) ────────────────────────────────
        .route(
            "/tenants/{tid}/mcp-identities",
            post(mcp_identities::create_mcp_identity),
        )
        .route(
            "/tenants/{tid}/mcp-identities",
            get(mcp_identities::list_mcp_identities),
        )
        .route(
            "/tenants/{tid}/mcp-identities/{mid}",
            get(mcp_identities::get_mcp_identity),
        )
        .route(
            "/tenants/{tid}/mcp-identities/{mid}/map",
            post(mcp_identities::map_person),
        )
        .route(
            "/tenants/{tid}/mcp-identities/{mid}/unmap",
            post(mcp_identities::unmap_person),
        )
        .route(
            "/tenants/{tid}/mcp-identities/{mid}/revoke",
            post(mcp_identities::revoke_mcp_identity),
        )
        .route(
            "/tenants/{tid}/mcp-identities/{mid}/reissue-setup-code",
            post(mcp_identities::reissue_setup_code),
        )
        // ── Auth token routes (T101) ──────────────────────────────────
        .route("/auth/tokens", post(auth_tokens::create_token))
        .route("/auth/tokens", get(auth_tokens::list_tokens))
        .route("/auth/tokens/{id}", delete(auth_tokens::revoke_token))
        // ── Backup routes (T095) ──────────────────────────────────────
        .route("/backups", post(backups::create_backup))
        .route("/backups", get(backups::list_backups))
        .route("/backups/restore", post(backups::restore_backup))
        // ── Reflection routes (T097-T100) ─────────────────────────────
        .route(
            "/tenants/{tid}/agents/{aid}/reflect",
            post(reflections::trigger_reflection),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/reflections",
            get(reflections::list_reflections),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/reflections/{rid}",
            get(reflections::get_reflection),
        )
        .route(
            "/tenants/{tid}/agents/{aid}/reflections/{rid}",
            delete(reflections::delete_reflection),
        )
        // ── Memory promotion (T108) ───────────────────────────────────
        .route(
            "/tenants/{tid}/agents/{aid}/memory/promote",
            post(reflections::promote_memory),
        )
        // ── User offboarding (T109) ───────────────────────────────────
        .route(
            "/tenants/{tid}/agents/{aid}/users/{uid}/offboard",
            post(reflections::offboard_user),
        )
        // ── Auth middleware ─────────────────────────────────────────────
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
