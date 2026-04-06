//! Features router — /api/features/* endpoints. Mirrors routers/features.py.

use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;

/// GET /api/features — list all loaded feature definitions
pub async fn list_features(State(state): State<AppState>) -> Json<Value> {
    Json(json!(*state.features))
}

/// GET /api/features/:feature_id/enable — instructions to enable a specific feature
pub async fn feature_enable(
    State(state): State<AppState>,
    Path(feature_id): Path<String>,
) -> Json<Value> {
    let feature = state
        .features
        .iter()
        .find(|f| f["id"].as_str() == Some(feature_id.as_str()));

    let feature = match feature {
        Some(f) => f,
        None => {
            return Json(json!({"error": format!("Feature not found: {feature_id}")}));
        }
    };

    fn svc_url(services: &std::collections::HashMap<String, dream_common::manifest::ServiceConfig>, id: &str) -> String {
        if let Some(cfg) = services.get(id) {
            let port = cfg.external_port;
            if port > 0 { return format!("http://localhost:{port}"); }
        }
        String::new()
    }

    fn svc_port(services: &std::collections::HashMap<String, dream_common::manifest::ServiceConfig>, id: &str) -> u16 {
        services.get(id).map(|c| c.external_port).unwrap_or(0)
    }

    let webui_url = svc_url(&state.services, "open-webui");
    let dashboard_url = svc_url(&state.services, "dashboard");
    let n8n_url_val = svc_url(&state.services, "n8n");

    let instructions = match feature_id.as_str() {
        "chat" => json!({"steps": ["Chat is already enabled if llama-server is running", "Open the Dashboard and click 'Chat' to start"], "links": [{"label": "Open Chat", "url": webui_url}]}),
        "voice" => json!({"steps": [format!("Ensure Whisper (STT) is running on port {}", svc_port(&state.services, "whisper")), format!("Ensure Kokoro (TTS) is running on port {}", svc_port(&state.services, "tts")), "Start LiveKit for WebRTC".to_string(), "Open the Voice page in the Dashboard".to_string()], "links": [{"label": "Voice Dashboard", "url": format!("{dashboard_url}/voice")}]}),
        "documents" => json!({"steps": ["Ensure Qdrant vector database is running", "Enable the 'Document Q&A' workflow", "Upload documents via the workflow endpoint"], "links": [{"label": "Workflows", "url": format!("{dashboard_url}/workflows")}]}),
        "workflows" => json!({"steps": [format!("Ensure n8n is running on port {}", svc_port(&state.services, "n8n")), "Open the Workflows page to see available automations".to_string(), "Click 'Enable' on any workflow to import it".to_string()], "links": [{"label": "n8n Dashboard", "url": n8n_url_val}, {"label": "Workflows", "url": format!("{dashboard_url}/workflows")}]}),
        "images" => json!({"steps": ["Image generation requires additional setup", "Coming soon in a future update"], "links": []}),
        "coding" => json!({"steps": ["Switch to the Qwen2.5-Coder model for best results", "Use the model manager to download and load it", "Chat will now be optimized for code"], "links": [{"label": "Model Manager", "url": format!("{dashboard_url}/models")}]}),
        "observability" => json!({"steps": [format!("Langfuse is running on port {}", svc_port(&state.services, "langfuse")), "Open Langfuse to view LLM traces and evaluations".to_string(), "LiteLLM automatically sends traces — no additional configuration needed".to_string()], "links": [{"label": "Open Langfuse", "url": svc_url(&state.services, "langfuse")}]}),
        _ => json!({"steps": [], "links": []}),
    };

    Json(json!({
        "featureId": feature_id,
        "name": feature["name"],
        "instructions": instructions,
    }))
}

/// GET /api/features/status — feature definitions with current health status.
pub async fn features_status(State(state): State<AppState>) -> Json<Value> {
    let cached = state.services_cache.read().await;
    let health_map: std::collections::HashMap<String, String> = cached
        .as_ref()
        .map(|statuses| {
            statuses
                .iter()
                .map(|s| (s.id.clone(), s.status.clone()))
                .collect()
        })
        .unwrap_or_default();

    let features: Vec<Value> = state
        .features
        .iter()
        .map(|f| {
            let mut feat = f.clone();
            let id = feat["id"].as_str().unwrap_or("");
            feat["health"] = json!(health_map.get(id).cloned().unwrap_or_else(|| "unknown".to_string()));
            feat
        })
        .collect();

    Json(json!(features))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use http::Request;
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use tower::ServiceExt;

    use crate::state::AppState;
    use dream_common::manifest::ServiceConfig;

    fn test_service_config(name: &str, port: u16) -> ServiceConfig {
        ServiceConfig {
            host: name.into(),
            port,
            external_port: port,
            health: "/health".into(),
            name: name.into(),
            ui_path: "/".into(),
            service_type: None,
            health_port: None,
        }
    }

    fn test_state_with_features(features: Vec<Value>) -> AppState {
        let mut services = HashMap::new();
        services.insert("open-webui".into(), test_service_config("open-webui", 3000));
        services.insert("dashboard".into(), test_service_config("dashboard", 3001));
        AppState::new(services, features, vec![], "test-key".into())
    }

    fn auth_header() -> (&'static str, &'static str) {
        ("Authorization", "Bearer test-key")
    }

    #[tokio::test]
    async fn test_list_features_returns_features_from_state() {
        let features = vec![
            json!({"id": "chat", "name": "Chat"}),
            json!({"id": "voice", "name": "Voice"}),
        ];
        let app = crate::build_router(test_state_with_features(features));

        let req = Request::builder()
            .uri("/api/features")
            .header(auth_header().0, auth_header().1)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "chat");
        assert_eq!(arr[1]["id"], "voice");
    }

    #[tokio::test]
    async fn test_features_status_empty_cache_returns_unknown_health() {
        let features = vec![
            json!({"id": "chat", "name": "Chat"}),
            json!({"id": "voice", "name": "Voice"}),
        ];
        let app = crate::build_router(test_state_with_features(features));

        let req = Request::builder()
            .uri("/api/features/status")
            .header(auth_header().0, auth_header().1)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // With no cached service statuses, every feature should have "unknown" health
        assert_eq!(arr[0]["health"], "unknown");
        assert_eq!(arr[1]["health"], "unknown");
    }

    #[tokio::test]
    async fn test_feature_enable_chat_returns_steps() {
        let features = vec![json!({"id": "chat", "name": "Chat"})];
        let app = crate::build_router(test_state_with_features(features));

        let req = Request::builder()
            .uri("/api/features/chat/enable")
            .header(auth_header().0, auth_header().1)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["featureId"], "chat");
        assert_eq!(val["name"], "Chat");

        let steps = val["instructions"]["steps"].as_array().unwrap();
        assert!(!steps.is_empty());
        // Verify the chat-specific instruction content
        assert!(steps[0].as_str().unwrap().contains("llama-server"));
    }

    #[tokio::test]
    async fn test_features_requires_auth() {
        let features = vec![json!({"id": "chat", "name": "Chat"})];
        let app = crate::build_router(test_state_with_features(features));
        let req = Request::builder()
            .uri("/api/features")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn test_features_status_requires_auth() {
        let features = vec![json!({"id": "chat", "name": "Chat"})];
        let app = crate::build_router(test_state_with_features(features));
        let req = Request::builder()
            .uri("/api/features/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn test_feature_enable_requires_auth() {
        let features = vec![json!({"id": "chat", "name": "Chat"})];
        let app = crate::build_router(test_state_with_features(features));
        let req = Request::builder()
            .uri("/api/features/chat/enable")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn test_feature_enable_voice_returns_steps() {
        let features = vec![json!({"id": "voice", "name": "Voice"})];
        let app = crate::build_router(test_state_with_features(features));

        let req = Request::builder()
            .uri("/api/features/voice/enable")
            .header(auth_header().0, auth_header().1)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["featureId"], "voice");
        let steps = val["instructions"]["steps"].as_array().unwrap();
        assert!(!steps.is_empty());
        assert!(steps[0].as_str().unwrap().contains("Whisper"));
    }

    #[tokio::test]
    async fn test_feature_enable_workflows_includes_n8n() {
        let features = vec![json!({"id": "workflows", "name": "Workflows"})];
        let app = crate::build_router(test_state_with_features(features));

        let req = Request::builder()
            .uri("/api/features/workflows/enable")
            .header(auth_header().0, auth_header().1)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["featureId"], "workflows");
        let steps = val["instructions"]["steps"].as_array().unwrap();
        assert!(steps[0].as_str().unwrap().contains("n8n"));
    }

    #[tokio::test]
    async fn test_feature_enable_unknown_returns_error() {
        let features = vec![json!({"id": "chat", "name": "Chat"})];
        let app = crate::build_router(test_state_with_features(features));

        let req = Request::builder()
            .uri("/api/features/nonexistent/enable")
            .header(auth_header().0, auth_header().1)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        let err = val["error"].as_str().unwrap();
        assert!(err.contains("Feature not found"));
        assert!(err.contains("nonexistent"));
    }
}
