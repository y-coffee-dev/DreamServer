//! Dashboard API library — re-exports for integration tests.

pub mod agent_monitor;
pub mod config;
pub mod gpu;
pub mod helpers;
pub mod middleware;
pub mod routes;
pub mod state;

// Re-export the router builder for tests
use axum::routing::{get, post};
use axum::{middleware as axum_mw, Router};

use crate::middleware::require_api_key;
use crate::state::AppState;

/// Build the complete Axum router. Used by both main() and tests.
pub fn build_router(app_state: AppState) -> Router {
    let public_routes = Router::new()
        .route("/health", get(routes::health::health))
        .route(
            "/api/preflight/required-ports",
            get(routes::preflight::preflight_required_ports),
        )
        .route("/api/gpu/detailed", get(routes::gpu::gpu_detailed))
        .route("/api/gpu/topology", get(routes::gpu::gpu_topology))
        .route("/api/gpu/history", get(routes::gpu::gpu_history));

    let protected_routes = Router::new()
        .route("/gpu", get(routes::services::gpu_endpoint))
        .route("/services", get(routes::services::services_endpoint))
        .route("/disk", get(routes::services::disk_endpoint))
        .route("/model", get(routes::services::model_endpoint))
        .route("/bootstrap", get(routes::services::bootstrap_endpoint))
        .route("/status", get(routes::services::full_status_endpoint))
        .route("/api/status", get(routes::status::api_status))
        .route("/api/preflight/docker", get(routes::preflight::preflight_docker))
        .route("/api/preflight/gpu", get(routes::preflight::preflight_gpu))
        .route("/api/preflight/ports", post(routes::preflight::preflight_ports))
        .route("/api/preflight/disk", get(routes::preflight::preflight_disk))
        .route("/api/service-tokens", get(routes::settings::service_tokens))
        .route("/api/external-links", get(routes::settings::external_links))
        .route("/api/storage", get(routes::settings::api_storage))
        .route("/api/features", get(routes::features::list_features))
        .route("/api/features/status", get(routes::features::features_status))
        .route("/api/features/{feature_id}/enable", get(routes::features::feature_enable))
        .route("/api/workflows", get(routes::workflows::list_workflows))
        .route("/api/workflows/categories", get(routes::workflows::workflow_categories))
        .route("/api/workflows/n8n/status", get(routes::workflows::n8n_status))
        .route("/api/workflows/{id}/enable", post(routes::workflows::enable_workflow))
        .route("/api/workflows/{id}/disable", post(routes::workflows::disable_workflow))
        .route("/api/workflows/{id}/executions", get(routes::workflows::workflow_executions))
        .route("/api/setup/status", get(routes::setup::setup_status))
        .route("/api/setup/persona", post(routes::setup::set_persona))
        .route("/api/setup/personas", get(routes::setup::list_personas))
        .route("/api/setup/persona/{persona_id}", get(routes::setup::get_persona))
        .route("/api/setup/complete", post(routes::setup::complete_setup))
        .route("/api/setup/test", post(routes::setup::setup_test))
        .route("/api/chat", post(routes::setup::setup_chat))
        .route("/api/version", get(routes::updates::version_info))
        .route("/api/releases/manifest", get(routes::updates::releases_manifest))
        .route("/api/update/dry-run", get(routes::updates::update_dry_run))
        .route("/api/update", post(routes::updates::update_action))
        .route("/api/agents/metrics", get(routes::agents::agent_metrics))
        .route("/api/agents/cluster", get(routes::agents::agent_cluster))
        .route("/api/agents/throughput", get(routes::agents::agent_throughput))
        .route("/api/agents/sessions", get(routes::agents::agent_sessions))
        .route("/api/agents/chat", post(routes::agents::agent_chat))
        .route("/api/privacy-shield/status", get(routes::privacy::privacy_status))
        .route("/api/privacy-shield/toggle", post(routes::privacy::privacy_toggle))
        .route("/api/privacy-shield/stats", get(routes::privacy::privacy_stats))
        .route("/api/extensions/catalog", get(routes::extensions::extensions_catalog))
        .route("/api/extensions/{id}", get(routes::extensions::get_extension).delete(routes::extensions::uninstall_extension))
        .route("/api/extensions/{id}/install", post(routes::extensions::install_extension))
        .route("/api/extensions/{id}/enable", post(routes::extensions::enable_extension))
        .route("/api/extensions/{id}/disable", post(routes::extensions::disable_extension))
        .route("/api/extensions/{id}/logs", post(routes::extensions::extension_logs))
        .route("/api/workflows/{id}", get(routes::workflows::get_workflow).delete(routes::workflows::disable_workflow))
        .layer(axum_mw::from_fn_with_state(app_state.clone(), require_api_key));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .with_state(app_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::new(HashMap::new(), Vec::new(), Vec::new(), "test-key".into())
    }

    #[test]
    fn test_build_router_returns_router() {
        // Verify that router construction succeeds without panicking.
        let _router = build_router(test_state());
    }

    #[tokio::test]
    async fn test_public_routes_accessible_without_auth() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["status"], "ok");
    }

    #[tokio::test]
    async fn test_protected_routes_require_auth() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }
}
