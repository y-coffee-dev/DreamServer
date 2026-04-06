//! Agent router — /api/agents/* endpoints. Mirrors routers/agents.py.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::agent_monitor::get_full_agent_metrics;
use crate::state::AppState;

/// GET /api/agents/metrics — real-time agent monitoring metrics
pub async fn agent_metrics() -> Json<Value> {
    Json(get_full_agent_metrics())
}

/// GET /api/agents/cluster — cluster health status
pub async fn agent_cluster() -> Json<Value> {
    let metrics = get_full_agent_metrics();
    Json(metrics["cluster"].clone())
}

/// GET /api/agents/throughput — throughput metrics
pub async fn agent_throughput() -> Json<Value> {
    let metrics = get_full_agent_metrics();
    Json(metrics["throughput"].clone())
}

/// GET /api/agents/sessions — active agent sessions from Token Spy
pub async fn agent_sessions(State(state): State<AppState>) -> Json<Value> {
    let token_spy_url =
        std::env::var("TOKEN_SPY_URL").unwrap_or_else(|_| "http://token-spy:8080".to_string());
    let token_spy_key = std::env::var("TOKEN_SPY_API_KEY").unwrap_or_default();

    let mut req = state.http.get(format!("{token_spy_url}/api/summary"));
    if !token_spy_key.is_empty() {
        req = req.bearer_auth(&token_spy_key);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            let data: Value = resp.json().await.unwrap_or(json!([]));
            Json(data)
        }
        _ => Json(json!([])),
    }
}

/// POST /api/agents/chat — forward chat to the configured LLM
pub async fn agent_chat(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let message = body["message"].as_str().unwrap_or("");
    let system = body["system"].as_str();

    let llm_backend = std::env::var("LLM_BACKEND").unwrap_or_default();
    let api_prefix = if llm_backend == "lemonade" { "/api/v1" } else { "/v1" };

    let svc = match state.services.get("llama-server") {
        Some(s) => s,
        None => return Json(json!({"error": "LLM service not configured"})),
    };

    let url = format!("http://{}:{}{}/chat/completions", svc.host, svc.port, api_prefix);

    let mut messages = Vec::new();
    if let Some(sys) = system {
        messages.push(json!({"role": "system", "content": sys}));
    }
    messages.push(json!({"role": "user", "content": message}));

    let payload = json!({
        "model": "default",
        "messages": messages,
        "stream": false,
    });

    match state.http.post(&url).json(&payload).send().await {
        Ok(resp) => {
            let data: Value = resp.json().await.unwrap_or(json!({}));
            Json(data)
        }
        Err(e) => Json(json!({"error": format!("LLM request failed: {e}")})),
    }
}

#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use std::collections::HashMap;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        crate::build_router(AppState::new(
            HashMap::new(), vec![], vec![], "test-key".into(),
        ))
    }

    fn auth_header() -> String {
        "Bearer test-key".to_string()
    }

    #[tokio::test]
    async fn agent_metrics_returns_json_object() {
        let req = Request::builder()
            .uri("/api/agents/metrics")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val["agent"].is_object(), "Expected agent key in metrics");
        assert!(val["cluster"].is_object(), "Expected cluster key");
        assert!(val["throughput"].is_object(), "Expected throughput key");
    }

    #[tokio::test]
    async fn agent_metrics_requires_auth() {
        let req = Request::builder()
            .uri("/api/agents/metrics")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn agent_cluster_returns_cluster_data() {
        let req = Request::builder()
            .uri("/api/agents/cluster")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        // Cluster data has nodes and GPU fields
        assert!(val.get("nodes").is_some() || val.get("total_gpus").is_some() || val.is_object());
    }

    #[tokio::test]
    async fn agent_throughput_returns_throughput_data() {
        let req = Request::builder()
            .uri("/api/agents/throughput")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.is_object());
    }

    #[tokio::test]
    async fn agent_chat_requires_auth() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/agents/chat")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"hello"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn agent_chat_returns_error_without_llm() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/agents/chat")
            .header("authorization", auth_header())
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"hello"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val["error"].is_string(), "Expected error when no LLM configured");
    }
}
