//! Privacy router — /api/privacy/* endpoints. Mirrors routers/privacy.py.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;

/// GET /api/privacy/status — privacy shield status
pub async fn privacy_status(State(state): State<AppState>) -> Json<Value> {
    let svc = state.services.get("privacy-shield");
    let (port, enabled) = match svc {
        Some(cfg) => (cfg.port as i64, true),
        None => (0, false),
    };

    if !enabled {
        return Json(json!({
            "enabled": false,
            "container_running": false,
            "port": 0,
            "target_api": "",
            "pii_cache_enabled": false,
            "message": "Privacy Shield is not configured",
        }));
    }

    // Check if container is actually running
    let svc_cfg = svc.unwrap();
    let health_url = format!("http://{}:{}{}", svc_cfg.host, svc_cfg.port, svc_cfg.health);
    let running = state
        .http
        .get(&health_url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    let target_api = std::env::var("PRIVACY_SHIELD_TARGET_API")
        .unwrap_or_else(|_| "http://llama-server:8080".to_string());
    let pii_cache = std::env::var("PRIVACY_SHIELD_PII_CACHE")
        .unwrap_or_else(|_| "true".to_string())
        .parse::<bool>()
        .unwrap_or(true);

    Json(json!({
        "enabled": true,
        "container_running": running,
        "port": port,
        "target_api": target_api,
        "pii_cache_enabled": pii_cache,
        "message": if running { "Privacy Shield is active" } else { "Privacy Shield container is not running" },
    }))
}

/// GET /api/privacy-shield/stats — privacy shield usage stats
pub async fn privacy_stats(State(state): State<AppState>) -> Json<Value> {
    let svc = match state.services.get("privacy-shield") {
        Some(cfg) => cfg,
        None => return Json(json!({"error": "Privacy Shield not configured"})),
    };
    let url = format!("http://{}:{}/stats", svc.host, svc.port);
    match state.http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            Json(resp.json().await.unwrap_or(json!({})))
        }
        _ => Json(json!({"error": "Could not fetch Privacy Shield stats"})),
    }
}

/// POST /api/privacy-shield/toggle — enable/disable privacy shield
pub async fn privacy_toggle(Json(body): Json<Value>) -> Json<Value> {
    let enable = body["enable"].as_bool().unwrap_or(false);
    // Toggle is done via docker compose — not directly controllable from the API
    // This endpoint signals intent; the actual toggle requires docker compose restart
    Json(json!({
        "status": "ok",
        "enable": enable,
        "message": if enable {
            "Privacy Shield enable requested. Restart the stack to apply."
        } else {
            "Privacy Shield disable requested. Restart the stack to apply."
        },
    }))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use std::collections::HashMap;
    use tower::ServiceExt;

    use crate::state::AppState;

    fn app() -> axum::Router {
        // No privacy-shield service configured => privacy_status returns disabled
        crate::build_router(AppState::new(HashMap::new(), vec![], vec![], "test-key".into()))
    }

    fn auth_header() -> &'static str {
        "Bearer test-key"
    }

    #[tokio::test]
    async fn privacy_status_requires_auth() {
        let req = Request::builder()
            .uri("/api/privacy-shield/status")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn privacy_status_disabled_when_no_service() {
        let req = Request::builder()
            .uri("/api/privacy-shield/status")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(val["enabled"], false);
        assert_eq!(val["container_running"], false);
        assert_eq!(val["port"], 0);
        assert_eq!(val["pii_cache_enabled"], false);
        assert_eq!(val["message"], "Privacy Shield is not configured");
    }

    #[tokio::test]
    async fn privacy_stats_requires_auth() {
        let req = Request::builder()
            .uri("/api/privacy-shield/stats")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn privacy_stats_without_service_returns_error() {
        let req = Request::builder()
            .uri("/api/privacy-shield/stats")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("error").is_some(), "Expected error when service not configured");
    }

    #[tokio::test]
    async fn privacy_toggle_requires_auth() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/privacy-shield/toggle")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"enable":true}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn privacy_toggle_enable_returns_ok() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/privacy-shield/toggle")
            .header("authorization", auth_header())
            .header("content-type", "application/json")
            .body(Body::from(r#"{"enable":true}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["status"], "ok");
        assert_eq!(val["enable"], true);
    }

    #[tokio::test]
    async fn privacy_toggle_disable_returns_ok() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/privacy-shield/toggle")
            .header("authorization", auth_header())
            .header("content-type", "application/json")
            .body(Body::from(r#"{"enable":false}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["status"], "ok");
        assert_eq!(val["enable"], false);
    }
}
