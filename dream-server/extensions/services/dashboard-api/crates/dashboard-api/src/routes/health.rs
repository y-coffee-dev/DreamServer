//! GET /health — API health check (no auth).

use axum::Json;
use serde_json::{json, Value};

pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_returns_ok() {
        let app = crate::build_router(AppState::new(
            HashMap::new(), vec![], vec![], "k".into(),
        ));
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["status"], "ok");
        assert!(val["timestamp"].is_string());
    }

    #[tokio::test]
    async fn health_requires_no_auth() {
        let app = crate::build_router(AppState::new(
            HashMap::new(), vec![], vec![], "secret".into(),
        ));
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Health is public — should succeed without auth header
        assert_eq!(resp.status(), 200);
    }
}
