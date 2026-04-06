//! Integration tests for the Dashboard API — mirrors Python test_routers.py behavior.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dashboard_api::state::AppState;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use tower::ServiceExt;

const TEST_API_KEY: &str = "test-secret-key-12345";

fn test_state() -> AppState {
    AppState::new(HashMap::new(), Vec::new(), Vec::new(), TEST_API_KEY.to_string())
}

fn app() -> axum::Router {
    dashboard_api::build_router(test_state())
}

fn auth_header() -> String {
    format!("Bearer {TEST_API_KEY}")
}

async fn get(uri: &str) -> (StatusCode, Value) {
    let app = app();
    let req = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let val: Value = serde_json::from_slice(&body).unwrap_or(json!(null));
    (status, val)
}

async fn get_auth(uri: &str) -> (StatusCode, Value) {
    let app = app();
    let req = Request::builder()
        .uri(uri)
        .header("authorization", auth_header())
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let val: Value = serde_json::from_slice(&body).unwrap_or(json!(null));
    (status, val)
}

async fn post_auth(uri: &str, body: Value) -> (StatusCode, Value) {
    let app = app();
    let req = Request::builder()
        .uri(uri)
        .method("POST")
        .header("authorization", auth_header())
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let val: Value = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, val)
}

// ---------------------------------------------------------------------------
// Health & public endpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_health_returns_ok() {
    let (status, data) = get("/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["status"], "ok");
    assert!(data["timestamp"].is_string());
}

#[tokio::test]
async fn test_gpu_history_public() {
    let (status, data) = get("/api/gpu/history").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data["history"].is_array());
}

#[tokio::test]
async fn test_preflight_required_ports_public() {
    let (status, data) = get("/api/preflight/required-ports").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data["ports"].is_array());
}

// ---------------------------------------------------------------------------
// Auth enforcement — no Bearer token → 401
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_api_status_requires_auth() {
    let (status, data) = get("/api/status").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(data["detail"].is_string());
}

#[tokio::test]
async fn test_services_requires_auth() {
    let (status, _) = get("/services").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_features_requires_auth() {
    let (status, _) = get("/api/features").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_workflows_requires_auth() {
    let (status, _) = get("/api/workflows").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_agents_metrics_requires_auth() {
    let (status, _) = get("/api/agents/metrics").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_agents_cluster_requires_auth() {
    let (status, _) = get("/api/agents/cluster").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_agents_throughput_requires_auth() {
    let (status, _) = get("/api/agents/throughput").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_privacy_shield_requires_auth() {
    let (status, _) = get("/api/privacy-shield/status").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_setup_status_requires_auth() {
    let (status, _) = get("/api/setup/status").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_version_requires_auth() {
    let (status, _) = get("/api/version").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_extensions_catalog_requires_auth() {
    let (status, _) = get("/api/extensions/catalog").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Auth enforcement — wrong key → 403
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wrong_key_returns_403() {
    let app = app();
    let req = Request::builder()
        .uri("/api/status")
        .header("authorization", "Bearer wrong-key")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Authenticated endpoints — happy paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_api_status_authenticated() {
    let (status, data) = get_auth("/api/status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("gpu").is_some());
    assert!(data["services"].is_array());
    assert!(data.get("version").is_some());
    assert!(data.get("tier").is_some());
    assert!(data.get("cpu").is_some());
    assert!(data.get("ram").is_some());
    assert!(data.get("inference").is_some());
}

#[tokio::test]
async fn test_services_returns_list() {
    let (status, data) = get_auth("/services").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.is_array());
}

#[tokio::test]
async fn test_disk_endpoint() {
    let (status, data) = get_auth("/disk").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("path").is_some() || data.get("used_gb").is_some());
}

#[tokio::test]
async fn test_bootstrap_endpoint() {
    let (status, data) = get_auth("/bootstrap").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["active"], false);
}

#[tokio::test]
async fn test_features_list() {
    let (status, data) = get_auth("/api/features").await;
    assert_eq!(status, StatusCode::OK);
    // With no services, features is empty array
    assert!(data.is_array());
}

#[tokio::test]
async fn test_agents_metrics_authenticated() {
    let (status, data) = get_auth("/api/agents/metrics").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("agent").is_some());
    assert!(data.get("cluster").is_some());
    assert!(data.get("throughput").is_some());
}

#[tokio::test]
async fn test_agents_cluster_authenticated() {
    let (status, data) = get_auth("/api/agents/cluster").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("nodes").is_some());
}

#[tokio::test]
async fn test_agents_throughput_authenticated() {
    let (status, data) = get_auth("/api/agents/throughput").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("current").is_some());
}

// ---------------------------------------------------------------------------
// Setup router
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_setup_status_authenticated() {
    let (status, data) = get_auth("/api/setup/status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("setup_complete").is_some() || data.get("first_run").is_some());
}

#[tokio::test]
async fn test_list_personas() {
    let (status, data) = get_auth("/api/setup/personas").await;
    assert_eq!(status, StatusCode::OK);
    let personas = data["personas"].as_array().unwrap();
    let ids: Vec<&str> = personas.iter().filter_map(|p| p["id"].as_str()).collect();
    assert!(ids.contains(&"general"));
    assert!(ids.contains(&"coding"));
    assert!(ids.contains(&"creative"));
}

#[tokio::test]
async fn test_get_persona_existing() {
    let (status, data) = get_auth("/api/setup/persona/general").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["id"], "general");
    assert!(data.get("name").is_some());
    assert!(data.get("system_prompt").is_some());
}

#[tokio::test]
async fn test_get_persona_nonexistent() {
    let (status, data) = get_auth("/api/setup/persona/nonexistent").await;
    assert_eq!(status, StatusCode::OK); // returns error JSON, not HTTP 404
    assert!(data.get("error").is_some());
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_service_tokens() {
    let (status, data) = get_auth("/api/service-tokens").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.is_object());
}

#[tokio::test]
async fn test_external_links() {
    let (status, data) = get_auth("/api/external-links").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.is_array());
}

#[tokio::test]
async fn test_api_storage() {
    let (status, data) = get_auth("/api/storage").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("models").is_some());
    assert!(data.get("vector_db").is_some());
    assert!(data.get("total_data").is_some());
    assert!(data.get("disk").is_some());
}

// ---------------------------------------------------------------------------
// Preflight
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_preflight_docker() {
    let (status, data) = get_auth("/api/preflight/docker").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("available").is_some());
}

#[tokio::test]
async fn test_preflight_gpu() {
    let (status, data) = get_auth("/api/preflight/gpu").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("available").is_some());
}

#[tokio::test]
async fn test_preflight_disk() {
    let (status, data) = get_auth("/api/preflight/disk").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("free").is_some());
    assert!(data.get("total").is_some());
}

#[tokio::test]
async fn test_preflight_ports_empty() {
    let (status, data) = post_auth("/api/preflight/ports", json!({"ports": []})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["conflicts"], json!([]));
    assert_eq!(data["available"], true);
}

#[tokio::test]
async fn test_preflight_ports_conflict() {
    // Bind a port to make it in-use
    let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let (status, data) = post_auth("/api/preflight/ports", json!({"ports": [port]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["available"], false);
    assert_eq!(data["conflicts"].as_array().unwrap().len(), 1);

    drop(listener);
}

// ---------------------------------------------------------------------------
// Privacy Shield
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_privacy_shield_status() {
    let (status, data) = get_auth("/api/privacy-shield/status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("enabled").is_some());
    assert!(data.get("container_running").is_some());
    assert!(data.get("port").is_some());
}

#[tokio::test]
async fn test_privacy_shield_toggle() {
    let (status, data) = post_auth("/api/privacy-shield/toggle", json!({"enable": true})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["enable"], true);
}

// ---------------------------------------------------------------------------
// Workflows
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_workflows_list() {
    let (status, data) = get_auth("/api/workflows").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("workflows").is_some() || data.is_object());
}

#[tokio::test]
async fn test_workflow_categories() {
    let (status, data) = get_auth("/api/workflows/categories").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("categories").is_some());
}

// ---------------------------------------------------------------------------
// Updates
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_version_info() {
    let (status, data) = get_auth("/api/version").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("current").is_some());
}

#[tokio::test]
async fn test_update_dry_run() {
    let (status, data) = get_auth("/api/update/dry-run").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["dry_run"], true);
    assert!(data.get("current_version").is_some());
    assert!(data.get("images").is_some());
}

// ---------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_extensions_catalog() {
    let (status, data) = get_auth("/api/extensions/catalog").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.is_array());
}

#[tokio::test]
async fn test_extension_not_found() {
    let (status, data) = get_auth("/api/extensions/nonexistent-ext").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data.get("error").is_some());
}
