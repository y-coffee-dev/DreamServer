//! Workflow router — /api/workflows/* endpoints. Mirrors routers/workflows.py.

use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::state::AppState;

fn workflow_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("WORKFLOW_DIR") {
        return PathBuf::from(dir);
    }
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let canonical = PathBuf::from(&install_dir).join("config").join("n8n");
    if canonical.exists() {
        canonical
    } else {
        PathBuf::from(&install_dir).join("workflows")
    }
}

fn n8n_url(services: &std::collections::HashMap<String, dream_common::manifest::ServiceConfig>) -> String {
    if let Ok(url) = std::env::var("N8N_URL") {
        return url;
    }
    if let Some(cfg) = services.get("n8n") {
        return format!("http://{}:{}", cfg.host, cfg.port);
    }
    "http://n8n:5678".to_string()
}

/// GET /api/workflows — list available workflow templates
pub async fn list_workflows() -> Json<Value> {
    let catalog_file = workflow_dir().join("catalog.json");
    if !catalog_file.exists() {
        return Json(json!({"workflows": [], "categories": {}}));
    }
    match std::fs::read_to_string(&catalog_file) {
        Ok(text) => {
            let data: Value = serde_json::from_str(&text).unwrap_or(json!({"workflows": [], "categories": {}}));
            Json(data)
        }
        Err(_) => Json(json!({"workflows": [], "categories": {}})),
    }
}

/// GET /api/workflows/:id — get a specific workflow template
pub async fn get_workflow(Path(id): Path<String>) -> Json<Value> {
    let wf_dir = workflow_dir();
    // Look in templates subdirectory
    let template_path = wf_dir.join("templates").join(format!("{id}.json"));
    if template_path.exists() {
        if let Ok(text) = std::fs::read_to_string(&template_path) {
            if let Ok(data) = serde_json::from_str::<Value>(&text) {
                return Json(data);
            }
        }
    }
    Json(json!({"error": "Workflow not found"}))
}

/// GET /api/workflows/categories — workflow categories from catalog
pub async fn workflow_categories() -> Json<Value> {
    let catalog_file = workflow_dir().join("catalog.json");
    if !catalog_file.exists() {
        return Json(json!({"categories": {}}));
    }
    match std::fs::read_to_string(&catalog_file) {
        Ok(text) => {
            let data: Value = serde_json::from_str(&text).unwrap_or(json!({}));
            Json(json!({"categories": data["categories"]}))
        }
        Err(_) => Json(json!({"categories": {}})),
    }
}

/// GET /api/workflows/n8n/status — n8n availability check
pub async fn n8n_status(State(state): State<AppState>) -> Json<Value> {
    let url = n8n_url(&state.services);
    let available = state
        .http
        .get(format!("{url}/healthz"))
        .send()
        .await
        .map(|r| r.status().as_u16() < 500)
        .unwrap_or(false);
    Json(json!({"available": available, "url": url}))
}

/// POST /api/workflows/:id/enable — import a workflow template into n8n
pub async fn enable_workflow(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let url = n8n_url(&state.services);
    let n8n_key = std::env::var("N8N_API_KEY").unwrap_or_default();

    // Read the workflow template
    let catalog_file = workflow_dir().join("catalog.json");
    let catalog: Value = std::fs::read_to_string(&catalog_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({"workflows": []}));

    let wf_info = catalog["workflows"]
        .as_array()
        .and_then(|wfs| wfs.iter().find(|w| w["id"].as_str() == Some(id.as_str())));

    let wf_info = match wf_info {
        Some(w) => w.clone(),
        None => return Json(json!({"error": format!("Workflow not found: {id}")})),
    };

    let wf_file = wf_info["file"].as_str().unwrap_or("");
    let template_path = workflow_dir().join(wf_file);
    let template: Value = match std::fs::read_to_string(&template_path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or(json!({})),
        Err(_) => return Json(json!({"error": format!("Workflow file not found: {wf_file}")})),
    };

    let mut headers = reqwest::header::HeaderMap::new();
    if !n8n_key.is_empty() {
        if let Ok(val) = n8n_key.parse() {
            headers.insert("X-N8N-API-KEY", val);
        }
    }

    match state
        .http
        .post(format!("{url}/api/v1/workflows"))
        .headers(headers.clone())
        .json(&template)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let data: Value = resp.json().await.unwrap_or(json!({}));
            let n8n_id = data["data"]["id"].as_str().map(|s| s.to_string());
            let mut activated = false;
            if let Some(ref nid) = n8n_id {
                if let Ok(r) = state
                    .http
                    .patch(format!("{url}/api/v1/workflows/{nid}"))
                    .headers(headers)
                    .json(&json!({"active": true}))
                    .send()
                    .await
                {
                    activated = r.status().is_success();
                }
            }
            Json(json!({
                "status": "success",
                "workflowId": id,
                "n8nId": n8n_id,
                "activated": activated,
                "message": format!("{} is now active!", wf_info["name"].as_str().unwrap_or(&id)),
            }))
        }
        Ok(resp) => {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            Json(json!({"error": format!("n8n API error ({status}): {text}")}))
        }
        Err(e) => Json(json!({"error": format!("Cannot reach n8n: {e}")})),
    }
}

/// POST /api/workflows/:id/disable — remove a workflow from n8n
pub async fn disable_workflow(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let url = n8n_url(&state.services);
    let n8n_key = std::env::var("N8N_API_KEY").unwrap_or_default();

    // Find the n8n workflow ID by matching name from catalog
    let catalog_file = workflow_dir().join("catalog.json");
    let catalog: Value = std::fs::read_to_string(&catalog_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({"workflows": []}));

    let wf_info = catalog["workflows"]
        .as_array()
        .and_then(|wfs| wfs.iter().find(|w| w["id"].as_str() == Some(id.as_str())));

    let wf_info = match wf_info {
        Some(w) => w.clone(),
        None => return Json(json!({"error": format!("Workflow not found: {id}")})),
    };

    let mut headers = reqwest::header::HeaderMap::new();
    if !n8n_key.is_empty() {
        if let Ok(val) = n8n_key.parse() {
            headers.insert("X-N8N-API-KEY", val);
        }
    }

    // Fetch n8n workflows to find the matching ID
    let n8n_workflows: Vec<Value> = match state
        .http
        .get(format!("{url}/api/v1/workflows"))
        .headers(headers.clone())
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let data: Value = resp.json().await.unwrap_or(json!({}));
            data["data"].as_array().cloned().unwrap_or_default()
        }
        _ => return Json(json!({"error": "Cannot reach n8n"})),
    };

    let wf_name = wf_info["name"].as_str().unwrap_or("").to_lowercase();
    let n8n_wf = n8n_workflows
        .iter()
        .find(|w| {
            let name = w["name"].as_str().unwrap_or("").to_lowercase();
            wf_name.contains(&name) || name.contains(&wf_name)
        });

    let n8n_wf = match n8n_wf {
        Some(w) => w,
        None => return Json(json!({"error": "Workflow not installed in n8n"})),
    };

    let n8n_id = n8n_wf["id"].as_str().unwrap_or("");
    match state
        .http
        .delete(format!("{url}/api/v1/workflows/{n8n_id}"))
        .headers(headers)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            Json(json!({
                "status": "success",
                "workflowId": id,
                "message": format!("{} has been removed", wf_info["name"].as_str().unwrap_or(&id)),
            }))
        }
        Ok(resp) => {
            let text = resp.text().await.unwrap_or_default();
            Json(json!({"error": format!("n8n API error: {text}")}))
        }
        Err(e) => Json(json!({"error": format!("Cannot reach n8n: {e}")})),
    }
}

/// GET /api/workflows/:id/executions — recent executions for a workflow
pub async fn workflow_executions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let url = n8n_url(&state.services);
    let n8n_key = std::env::var("N8N_API_KEY").unwrap_or_default();

    let catalog_file = workflow_dir().join("catalog.json");
    let catalog: Value = std::fs::read_to_string(&catalog_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({"workflows": []}));

    let wf_info = catalog["workflows"]
        .as_array()
        .and_then(|wfs| wfs.iter().find(|w| w["id"].as_str() == Some(id.as_str())));

    let wf_info = match wf_info {
        Some(w) => w.clone(),
        None => return Json(json!({"error": format!("Workflow not found: {id}")})),
    };

    let mut headers = reqwest::header::HeaderMap::new();
    if !n8n_key.is_empty() {
        if let Ok(val) = n8n_key.parse() {
            headers.insert("X-N8N-API-KEY", val);
        }
    }

    // Fetch n8n workflows to find the matching ID
    let n8n_workflows: Vec<Value> = match state
        .http
        .get(format!("{url}/api/v1/workflows"))
        .headers(headers.clone())
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let data: Value = resp.json().await.unwrap_or(json!({}));
            data["data"].as_array().cloned().unwrap_or_default()
        }
        _ => return Json(json!({"executions": [], "error": "Cannot reach n8n"})),
    };

    let wf_name = wf_info["name"].as_str().unwrap_or("").to_lowercase();
    let n8n_wf = n8n_workflows
        .iter()
        .find(|w| {
            let name = w["name"].as_str().unwrap_or("").to_lowercase();
            wf_name.contains(&name) || name.contains(&wf_name)
        });

    let n8n_wf = match n8n_wf {
        Some(w) => w,
        None => return Json(json!({"executions": [], "message": "Workflow not installed"})),
    };

    let n8n_id = n8n_wf["id"].as_str().unwrap_or("");
    match state
        .http
        .get(format!("{url}/api/v1/executions"))
        .headers(headers)
        .query(&[("workflowId", n8n_id), ("limit", "20")])
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let data: Value = resp.json().await.unwrap_or(json!({}));
            Json(json!({
                "workflowId": id,
                "n8nId": n8n_id,
                "executions": data["data"],
            }))
        }
        _ => Json(json!({"executions": [], "error": "Failed to fetch executions"})),
    }
}

#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use axum::body::Body;
    use dream_common::manifest::ServiceConfig;
    use http::Request;
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use tower::ServiceExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_API_KEY: &str = "test-key-123";

    fn test_state_with_services(
        services: HashMap<String, ServiceConfig>,
    ) -> AppState {
        AppState::new(services, Vec::new(), Vec::new(), TEST_API_KEY.to_string())
    }

    fn test_state() -> AppState {
        test_state_with_services(HashMap::new())
    }

    fn app() -> axum::Router {
        crate::build_router(test_state())
    }

    fn app_with_services(services: HashMap<String, ServiceConfig>) -> axum::Router {
        crate::build_router(test_state_with_services(services))
    }

    fn auth_header() -> String {
        format!("Bearer {TEST_API_KEY}")
    }

    async fn get_auth(uri: &str) -> (http::StatusCode, Value) {
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

    async fn get_auth_with_app(
        router: axum::Router,
        uri: &str,
    ) -> (http::StatusCode, Value) {
        let req = Request::builder()
            .uri(uri)
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap_or(json!(null));
        (status, val)
    }

    // -----------------------------------------------------------------------
    // GET /api/workflows — no catalog file → empty response
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_workflows_returns_empty_when_no_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        // Point WORKFLOW_DIR at an empty temp directory so no catalog.json exists.
        std::env::set_var("WORKFLOW_DIR", tmp.path().as_os_str());
        let (status, data) = get_auth("/api/workflows").await;
        std::env::remove_var("WORKFLOW_DIR");

        assert_eq!(status, http::StatusCode::OK);
        assert!(
            data.get("workflows").is_some(),
            "Expected 'workflows' key in response, got: {data}"
        );
        assert!(
            data["workflows"].as_array().unwrap().is_empty(),
            "Expected empty workflows array when catalog is missing, got: {data}"
        );
    }

    #[tokio::test]
    async fn list_workflows_requires_auth() {
        let app = app();
        let req = Request::builder()
            .uri("/api/workflows")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // GET /api/workflows/categories — no catalog → empty categories
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn workflow_categories_returns_categories_key() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("WORKFLOW_DIR", tmp.path().as_os_str());
        let (status, data) = get_auth("/api/workflows/categories").await;
        std::env::remove_var("WORKFLOW_DIR");

        assert_eq!(status, http::StatusCode::OK);
        assert!(
            data.get("categories").is_some(),
            "Expected 'categories' key in response, got: {data}"
        );
    }

    // -----------------------------------------------------------------------
    // GET /api/workflows/categories — with catalog file
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn workflow_categories_reads_from_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = json!({
            "workflows": [],
            "categories": {
                "automation": "Automation",
                "ai": "AI & ML"
            }
        });
        std::fs::write(
            tmp.path().join("catalog.json"),
            serde_json::to_string(&catalog).unwrap(),
        )
        .unwrap();

        // Point WORKFLOW_DIR at the temp directory
        std::env::set_var("WORKFLOW_DIR", tmp.path().as_os_str());
        let (status, data) = get_auth("/api/workflows/categories").await;
        std::env::remove_var("WORKFLOW_DIR");

        assert_eq!(status, http::StatusCode::OK);
        let cats = &data["categories"];
        assert_eq!(cats["automation"], "Automation");
        assert_eq!(cats["ai"], "AI & ML");
    }

    // -----------------------------------------------------------------------
    // GET /api/workflows/{id} — workflow not found
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_workflow_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("WORKFLOW_DIR", tmp.path().as_os_str());
        let (status, data) = get_auth("/api/workflows/does-not-exist").await;
        std::env::remove_var("WORKFLOW_DIR");

        assert_eq!(status, http::StatusCode::OK);
        assert_eq!(data["error"], "Workflow not found");
    }

    // -----------------------------------------------------------------------
    // GET /api/workflows/n8n/status — n8n unreachable
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn n8n_status_returns_unavailable_when_unreachable() {
        // No n8n service in state and no N8N_URL env → defaults to
        // http://n8n:5678 which is not reachable in tests.
        let (status, data) = get_auth("/api/workflows/n8n/status").await;
        assert_eq!(status, http::StatusCode::OK);
        assert_eq!(data["available"], false);
    }

    // -----------------------------------------------------------------------
    // GET /api/workflows/n8n/status — with mock n8n
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // GET /api/workflows/{id} — with catalog + template
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_workflow_with_template_returns_data() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("templates")).unwrap();
        let template = json!({"id": "doc-qa", "name": "Document Q&A", "nodes": []});
        std::fs::write(
            tmp.path().join("templates/doc-qa.json"),
            serde_json::to_string(&template).unwrap(),
        )
        .unwrap();

        std::env::set_var("WORKFLOW_DIR", tmp.path().as_os_str());
        let (status, data) = get_auth("/api/workflows/doc-qa").await;
        std::env::remove_var("WORKFLOW_DIR");

        assert_eq!(status, http::StatusCode::OK);
        assert_eq!(data["id"], "doc-qa");
        assert_eq!(data["name"], "Document Q&A");
    }

    // -----------------------------------------------------------------------
    // Auth tests for enable/disable/executions
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn enable_workflow_requires_auth() {
        let app = app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/workflows/test-wf/enable")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn disable_workflow_requires_auth() {
        let app = app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/workflows/test-wf/disable")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn workflow_executions_requires_auth() {
        let app = app();
        let req = Request::builder()
            .uri("/api/workflows/test-wf/executions")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // GET /api/workflows — with catalog file returns workflows
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_workflows_with_catalog_returns_data() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = json!({
            "workflows": [{"id": "wf1", "name": "Workflow 1"}],
            "categories": {"auto": "Automation"}
        });
        std::fs::write(
            tmp.path().join("catalog.json"),
            serde_json::to_string(&catalog).unwrap(),
        )
        .unwrap();

        std::env::set_var("WORKFLOW_DIR", tmp.path().as_os_str());
        let (status, data) = get_auth("/api/workflows").await;
        std::env::remove_var("WORKFLOW_DIR");

        assert_eq!(status, http::StatusCode::OK);
        let wfs = data["workflows"].as_array().unwrap();
        assert_eq!(wfs.len(), 1);
        assert_eq!(wfs[0]["id"], "wf1");
    }

    // -----------------------------------------------------------------------
    // GET /api/workflows/n8n/status — with mock n8n
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn n8n_status_returns_available_with_mock() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/healthz"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
            .mount(&mock_server)
            .await;

        let mut services = HashMap::new();
        services.insert(
            "n8n".to_string(),
            ServiceConfig {
                host: mock_server.address().ip().to_string(),
                port: mock_server.address().port(),
                external_port: 5678,
                health: "/healthz".into(),
                name: "n8n".into(),
                ui_path: "/".into(),
                service_type: None,
                health_port: None,
            },
        );

        let router = app_with_services(services);
        let (status, data) = get_auth_with_app(router, "/api/workflows/n8n/status").await;
        assert_eq!(status, http::StatusCode::OK);
        assert_eq!(data["available"], true);
    }
}
