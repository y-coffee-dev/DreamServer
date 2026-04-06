//! Extensions router — /api/extensions/* endpoints. Mirrors routers/extensions.py.
//! This is the largest router (~794 LOC in Python), handling the extensions portal.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::state::AppState;

/// Reject extension IDs that could escape the extensions directory.
fn validate_extension_id(id: &str) -> Result<(), &'static str> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.starts_with('.')
    {
        return Err("Invalid extension id");
    }
    Ok(())
}

fn extensions_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("DREAM_EXTENSIONS_DIR").unwrap_or_else(|_| {
            let install = std::env::var("DREAM_INSTALL_DIR")
                .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
            format!("{install}/extensions/services")
        }),
    )
}

fn catalog_path() -> PathBuf {
    PathBuf::from(
        std::env::var("DREAM_EXTENSIONS_CATALOG").unwrap_or_else(|_| {
            let install = std::env::var("DREAM_INSTALL_DIR")
                .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
            format!("{install}/config/extensions-catalog.json")
        }),
    )
}

fn load_catalog() -> Vec<Value> {
    let path = catalog_path();
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v["extensions"].as_array().cloned())
        .unwrap_or_default()
}

fn user_extensions_dir() -> PathBuf {
    let install = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    PathBuf::from(
        std::env::var("DREAM_USER_EXTENSIONS_DIR")
            .unwrap_or_else(|_| format!("{install}/extensions/user")),
    )
}

fn core_service_ids() -> std::collections::HashSet<String> {
    let install = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let ids_path = PathBuf::from(&install).join("config").join("core-service-ids.json");
    if let Ok(text) = std::fs::read_to_string(&ids_path) {
        if let Ok(ids) = serde_json::from_str::<Vec<String>>(&text) {
            return ids.into_iter().collect();
        }
    }
    // Hardcoded fallback
    [
        "dashboard-api", "dashboard", "llama-server", "open-webui",
        "litellm", "langfuse", "n8n", "openclaw", "opencode",
        "perplexica", "searxng", "qdrant", "tts", "whisper",
        "embeddings", "token-spy", "comfyui", "ape", "privacy-shield",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Deserialize)]
pub struct ExtensionQuery {
    pub category: Option<String>,
    pub search: Option<String>,
}

/// GET /api/extensions — list all extensions with status
pub async fn list_extensions(
    State(state): State<AppState>,
    Query(query): Query<ExtensionQuery>,
) -> Json<Value> {
    let ext_dir = extensions_dir();
    let catalog = load_catalog();
    let core_ids = core_service_ids();

    let cached = state.services_cache.read().await;
    let health_map: HashMap<String, String> = cached
        .as_ref()
        .map(|statuses| {
            statuses
                .iter()
                .map(|s| (s.id.clone(), s.status.clone()))
                .collect()
        })
        .unwrap_or_default();

    let mut extensions: Vec<Value> = Vec::new();

    // Build from loaded services
    for (sid, cfg) in state.services.iter() {
        let is_core = core_ids.contains(sid);
        let status = health_map.get(sid).cloned().unwrap_or_else(|| "unknown".to_string());
        let disabled = ext_dir.join(sid).join("compose.yaml.disabled").exists()
            || ext_dir.join(sid).join("compose.yml.disabled").exists();

        let mut ext = json!({
            "id": sid,
            "name": cfg.name,
            "port": cfg.external_port,
            "status": status,
            "is_core": is_core,
            "enabled": !disabled,
            "ui_path": cfg.ui_path,
        });

        // Enrich from catalog if available
        if let Some(cat_entry) = catalog.iter().find(|c| c["id"].as_str() == Some(sid)) {
            if let Some(desc) = cat_entry["description"].as_str() {
                ext["description"] = json!(desc);
            }
            if let Some(cat) = cat_entry["category"].as_str() {
                ext["category"] = json!(cat);
            }
            if let Some(icon) = cat_entry["icon"].as_str() {
                ext["icon"] = json!(icon);
            }
        }

        extensions.push(ext);
    }

    // Filter by category
    if let Some(ref cat) = query.category {
        extensions.retain(|e| e["category"].as_str() == Some(cat.as_str()));
    }

    // Filter by search
    if let Some(ref search) = query.search {
        let lower = search.to_lowercase();
        extensions.retain(|e| {
            e["name"].as_str().map_or(false, |n| n.to_lowercase().contains(&lower))
                || e["id"].as_str().map_or(false, |id| id.to_lowercase().contains(&lower))
                || e["description"].as_str().map_or(false, |d| d.to_lowercase().contains(&lower))
        });
    }

    Json(json!(extensions))
}

/// GET /api/extensions/:id — get details for a specific extension
pub async fn get_extension(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    if let Err(msg) = validate_extension_id(&id) {
        return Json(json!({"error": msg}));
    }
    let cfg = match state.services.get(&id) {
        Some(c) => c,
        None => return Json(json!({"error": "Extension not found"})),
    };

    let catalog = load_catalog();
    let cat_entry = catalog.iter().find(|c| c["id"].as_str() == Some(&id));

    let ext_dir = extensions_dir().join(&id);
    let manifest_path = ext_dir.join("manifest.yaml");
    let manifest: Value = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|t| serde_yaml::from_str(&t).ok())
            .unwrap_or(json!({}))
    } else {
        json!({})
    };

    let cached = state.services_cache.read().await;
    let status = cached
        .as_ref()
        .and_then(|statuses| statuses.iter().find(|s| s.id == id))
        .map(|s| s.status.clone())
        .unwrap_or_else(|| "unknown".to_string());

    Json(json!({
        "id": id,
        "name": cfg.name,
        "port": cfg.external_port,
        "status": status,
        "health": cfg.health,
        "ui_path": cfg.ui_path,
        "manifest": manifest,
        "catalog": cat_entry,
    }))
}

/// POST /api/extensions/:id/toggle — enable/disable an extension
pub async fn toggle_extension(
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    if let Err(msg) = validate_extension_id(&id) {
        return Json(json!({"error": msg}));
    }
    let enable = body["enable"].as_bool().unwrap_or(true);
    let ext_dir = extensions_dir().join(&id);

    if !ext_dir.exists() {
        return Json(json!({"error": "Extension directory not found"}));
    }

    let compose_file = ext_dir.join("compose.yaml");
    let disabled_file = ext_dir.join("compose.yaml.disabled");

    if enable {
        // Rename .disabled back to .yaml
        if disabled_file.exists() {
            if let Err(e) = std::fs::rename(&disabled_file, &compose_file) {
                return Json(json!({"error": format!("Failed to enable: {e}")}));
            }
        }
    } else {
        // Rename .yaml to .disabled
        if compose_file.exists() {
            if let Err(e) = std::fs::rename(&compose_file, &disabled_file) {
                return Json(json!({"error": format!("Failed to disable: {e}")}));
            }
        }
    }

    Json(json!({
        "status": "ok",
        "id": id,
        "enabled": enable,
        "message": format!("Extension {} {}. Restart the stack to apply.", id, if enable { "enabled" } else { "disabled" }),
    }))
}

/// GET /api/extensions/catalog — full extensions catalog
pub async fn extensions_catalog() -> Json<Value> {
    Json(json!(load_catalog()))
}

/// POST /api/extensions/:id/install — install an extension from the library
pub async fn install_extension(
    Path(id): Path<String>,
) -> Json<Value> {
    if let Err(msg) = validate_extension_id(&id) {
        return Json(json!({"error": msg}));
    }
    let ext_dir = extensions_dir().join(&id);
    if ext_dir.exists() {
        return Json(json!({"error": format!("Extension already installed: {id}")}));
    }

    // Check if extension exists in the library
    let install = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let library_dir = std::path::PathBuf::from(&install)
        .join("extensions")
        .join("library");
    let source = library_dir.join(&id);

    if !source.is_dir() {
        return Json(json!({"error": format!("Extension not found in library: {id}")}));
    }

    // Copy from library to user extensions dir
    let user_ext_dir = user_extensions_dir();
    let _ = std::fs::create_dir_all(&user_ext_dir);
    let dest = user_ext_dir.join(&id);

    fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let dest_path = dst.join(entry.file_name());
            if ty.is_dir() {
                copy_dir_recursive(&entry.path(), &dest_path)?;
            } else if ty.is_file() {
                std::fs::copy(entry.path(), dest_path)?;
            }
        }
        Ok(())
    }

    match copy_dir_recursive(&source, &dest) {
        Ok(_) => Json(json!({
            "id": id,
            "action": "installed",
            "restart_required": true,
            "message": "Extension installed. Run 'dream restart' to start.",
        })),
        Err(e) => Json(json!({"error": format!("Install failed: {e}")})),
    }
}

/// POST /api/extensions/:id/enable — enable an installed extension
pub async fn enable_extension(
    Path(id): Path<String>,
) -> Json<Value> {
    if let Err(msg) = validate_extension_id(&id) {
        return Json(json!({"error": msg}));
    }
    let ext_dir = extensions_dir().join(&id);
    if !ext_dir.exists() {
        return Json(json!({"error": format!("Extension not installed: {id}")}));
    }

    let disabled_file = ext_dir.join("compose.yaml.disabled");
    let enabled_file = ext_dir.join("compose.yaml");

    if enabled_file.exists() {
        return Json(json!({"error": format!("Extension already enabled: {id}")}));
    }
    if !disabled_file.exists() {
        return Json(json!({"error": format!("Extension has no compose file: {id}")}));
    }

    match std::fs::rename(&disabled_file, &enabled_file) {
        Ok(_) => Json(json!({
            "id": id,
            "action": "enabled",
            "restart_required": true,
            "message": "Extension enabled. Run 'dream restart' to start.",
        })),
        Err(e) => Json(json!({"error": format!("Failed to enable: {e}")})),
    }
}

/// POST /api/extensions/:id/disable — disable an enabled extension
pub async fn disable_extension(
    Path(id): Path<String>,
) -> Json<Value> {
    if let Err(msg) = validate_extension_id(&id) {
        return Json(json!({"error": msg}));
    }
    let ext_dir = extensions_dir().join(&id);
    if !ext_dir.exists() {
        return Json(json!({"error": format!("Extension not installed: {id}")}));
    }

    let enabled_file = ext_dir.join("compose.yaml");
    let disabled_file = ext_dir.join("compose.yaml.disabled");

    if !enabled_file.exists() {
        return Json(json!({"error": format!("Extension already disabled: {id}")}));
    }

    match std::fs::rename(&enabled_file, &disabled_file) {
        Ok(_) => Json(json!({
            "id": id,
            "action": "disabled",
            "restart_required": true,
            "message": "Extension disabled. Run 'dream restart' to apply changes.",
        })),
        Err(e) => Json(json!({"error": format!("Failed to disable: {e}")})),
    }
}

/// POST /api/extensions/:id/logs — get container logs via host agent
pub async fn extension_logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    if let Err(msg) = validate_extension_id(&id) {
        return Json(json!({"error": msg}));
    }
    let agent_url = std::env::var("DREAM_AGENT_URL")
        .unwrap_or_else(|_| "http://host.docker.internal:9090".to_string());
    let agent_key = std::env::var("DREAM_AGENT_KEY").unwrap_or_default();

    let mut req = state.http.post(format!("{agent_url}/v1/extension/logs"));
    if !agent_key.is_empty() {
        req = req.bearer_auth(&agent_key);
    }

    match req
        .json(&json!({"service_id": id, "tail": 100}))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            Json(resp.json().await.unwrap_or(json!({})))
        }
        _ => Json(json!({"error": "Host agent unavailable — cannot fetch logs"})),
    }
}

/// DELETE /api/extensions/:id — uninstall a disabled extension
pub async fn uninstall_extension(
    Path(id): Path<String>,
) -> Json<Value> {
    if let Err(msg) = validate_extension_id(&id) {
        return Json(json!({"error": msg}));
    }

    // Reject core services
    if core_service_ids().contains(&id) {
        return Json(json!({"error": format!("Cannot uninstall core service: {id}")}));
    }

    let user_ext = user_extensions_dir().join(&id);

    // Canonicalize and verify the path stays under user_extensions_dir
    let base_canonical = match user_extensions_dir().canonicalize() {
        Ok(p) => p,
        Err(_) => return Json(json!({"error": "User extensions directory not found"})),
    };

    if !user_ext.exists() {
        return Json(json!({"error": format!("Extension not installed: {id}")}));
    }

    let ext_canonical = match user_ext.canonicalize() {
        Ok(p) => p,
        Err(_) => return Json(json!({"error": format!("Extension not found: {id}")})),
    };

    if !ext_canonical.starts_with(&base_canonical) {
        return Json(json!({"error": format!("Extension not found: {id}")}));
    }

    // Reject symlinks at the top level
    match std::fs::symlink_metadata(&user_ext) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Json(json!({"error": "Extension directory is a symlink"}));
        }
        Err(e) => {
            return Json(json!({"error": format!("Cannot read extension: {e}")}));
        }
        _ => {}
    }

    // Must be disabled before uninstall
    if user_ext.join("compose.yaml").exists() {
        return Json(json!({
            "error": format!("Disable extension before uninstalling. Run 'dream disable {id}' first."),
        }));
    }

    match std::fs::remove_dir_all(&ext_canonical) {
        Ok(_) => Json(json!({
            "id": id,
            "action": "uninstalled",
            "message": "Extension uninstalled. Docker volumes may remain — run 'docker volume ls' to check.",
            "cleanup_hint": format!("To remove orphaned volumes: docker volume ls --filter 'name={id}' -q | xargs docker volume rm"),
        })),
        Err(e) => Json(json!({"error": format!("Failed to remove extension files: {e}")})),
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
    // GET /api/extensions/catalog
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn catalog_returns_json_array() {
        let (status, data) = get_auth("/api/extensions/catalog").await;
        assert_eq!(status, http::StatusCode::OK);
        assert!(
            data.is_array(),
            "Expected JSON array from catalog, got: {data}"
        );
    }

    #[tokio::test]
    async fn catalog_requires_auth() {
        let app = app();
        let req = Request::builder()
            .uri("/api/extensions/catalog")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), http::StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // GET /api/extensions/{id} — nonexistent extension
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_nonexistent_extension_returns_error() {
        let (status, data) = get_auth("/api/extensions/nonexistent-id").await;
        assert_eq!(status, http::StatusCode::OK);
        assert_eq!(
            data["error"].as_str().unwrap(),
            "Extension not found",
            "Expected 'Extension not found' error for unknown id"
        );
    }

    #[tokio::test]
    async fn get_extension_with_path_traversal_returns_error() {
        let (status, data) = get_auth("/api/extensions/..%2Fetc").await;
        // URL-decoded ".." triggers the validate_extension_id guard
        assert_eq!(status, http::StatusCode::OK);
        assert!(
            data.get("error").is_some(),
            "Expected error for path-traversal id, got: {data}"
        );
    }

    // -----------------------------------------------------------------------
    // GET /api/extensions/{id} — known extension in state
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_known_extension_returns_details() {
        let mut services = HashMap::new();
        services.insert(
            "my-ext".to_string(),
            ServiceConfig {
                host: "my-ext".into(),
                port: 9000,
                external_port: 9001,
                health: "/health".into(),
                name: "My Extension".into(),
                ui_path: "/".into(),
                service_type: None,
                health_port: None,
            },
        );

        let router = app_with_services(services);
        let (status, data) = get_auth_with_app(router, "/api/extensions/my-ext").await;
        assert_eq!(status, http::StatusCode::OK);
        assert_eq!(data["id"], "my-ext");
        assert_eq!(data["name"], "My Extension");
        assert_eq!(data["port"], 9001);
        assert_eq!(data["status"], "unknown"); // no health cache populated
    }

    // -----------------------------------------------------------------------
    // validate_extension_id unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_extension_id_rejects_empty() {
        assert!(super::validate_extension_id("").is_err());
    }

    #[test]
    fn validate_extension_id_rejects_path_traversal() {
        assert!(super::validate_extension_id("..").is_err());
        assert!(super::validate_extension_id("foo/bar").is_err());
        assert!(super::validate_extension_id("foo\\bar").is_err());
        assert!(super::validate_extension_id(".hidden").is_err());
    }

    #[test]
    fn validate_extension_id_accepts_valid() {
        assert!(super::validate_extension_id("open-webui").is_ok());
        assert!(super::validate_extension_id("comfyui").is_ok());
        assert!(super::validate_extension_id("my_extension_v2").is_ok());
    }
}
