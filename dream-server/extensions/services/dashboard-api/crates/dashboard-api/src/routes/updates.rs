//! Updates router — /api/updates/* endpoints. Mirrors routers/updates.py.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::state::AppState;

fn install_dir() -> String {
    std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string())
}

/// GET /api/updates/version — current version info
pub async fn version_info() -> Json<Value> {
    let install = install_dir();
    let version_file = PathBuf::from(&install).join("VERSION");
    let current = std::fs::read_to_string(&version_file)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Json(json!({
        "current": current,
        "latest": null,
        "update_available": false,
        "changelog_url": null,
        "checked_at": null,
    }))
}

/// POST /api/updates/check — check for available updates
pub async fn check_updates(State(state): State<AppState>) -> Json<Value> {
    let install = install_dir();
    let version_file = PathBuf::from(&install).join("VERSION");
    let current = std::fs::read_to_string(&version_file)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Try to fetch latest version from GitHub releases
    let latest = match state
        .http
        .get("https://api.github.com/repos/Light-Heart-Labs/DreamServer/releases/latest")
        .header("User-Agent", "DreamServer-Dashboard")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let data: Value = resp.json().await.unwrap_or(json!({}));
            data["tag_name"].as_str().map(|s| s.trim_start_matches('v').to_string())
        }
        _ => None,
    };

    let update_available = latest
        .as_ref()
        .map(|l| l != &current)
        .unwrap_or(false);

    Json(json!({
        "current": current,
        "latest": latest,
        "update_available": update_available,
        "changelog_url": latest.as_ref().map(|_| "https://github.com/Light-Heart-Labs/DreamServer/releases"),
        "checked_at": chrono::Utc::now().to_rfc3339(),
    }))
}

/// GET /api/releases/manifest — release manifest with version history
pub async fn releases_manifest(State(state): State<AppState>) -> Json<Value> {
    match state
        .http
        .get("https://api.github.com/repos/Light-Heart-Labs/DreamServer/releases?per_page=5")
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "DreamServer-Dashboard")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let releases: Vec<Value> = resp.json().await.unwrap_or_default();
            let formatted: Vec<Value> = releases
                .iter()
                .map(|r| {
                    let body = r["body"].as_str().unwrap_or("");
                    let truncated = if body.len() > 500 {
                        format!("{}...", &body[..500])
                    } else {
                        body.to_string()
                    };
                    json!({
                        "version": r["tag_name"].as_str().unwrap_or("").trim_start_matches('v'),
                        "date": r["published_at"],
                        "title": r["name"],
                        "changelog": truncated,
                        "url": r["html_url"],
                        "prerelease": r["prerelease"],
                    })
                })
                .collect();
            Json(json!({
                "releases": formatted,
                "checked_at": chrono::Utc::now().to_rfc3339(),
            }))
        }
        _ => {
            let current = std::fs::read_to_string(
                PathBuf::from(&install_dir()).join("VERSION"),
            )
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());
            Json(json!({
                "releases": [{
                    "version": current,
                    "date": chrono::Utc::now().to_rfc3339(),
                    "title": format!("Dream Server {current}"),
                    "changelog": "Release information unavailable. Check GitHub directly.",
                    "url": "https://github.com/Light-Heart-Labs/DreamServer/releases",
                    "prerelease": false,
                }],
                "checked_at": chrono::Utc::now().to_rfc3339(),
                "error": "Could not fetch release information",
            }))
        }
    }
}

/// GET /api/update/dry-run — preview what an update would change
pub async fn update_dry_run() -> Json<Value> {
    let install = install_dir();
    let install_path = PathBuf::from(&install);

    // Read current version from .env or .version
    let mut current = "0.0.0".to_string();
    let env_file = install_path.join(".env");
    if env_file.exists() {
        if let Ok(text) = std::fs::read_to_string(&env_file) {
            for line in text.lines() {
                if let Some(val) = line.strip_prefix("DREAM_VERSION=") {
                    current = val.trim().trim_matches('"').trim_matches('\'').to_string();
                    break;
                }
            }
        }
    }
    if current == "0.0.0" {
        let version_file = install_path.join(".version");
        if let Ok(text) = std::fs::read_to_string(&version_file) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                current = trimmed.to_string();
            }
        }
    }

    // Configured image tags from compose files
    let mut images: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&install_path) {
        let mut compose_files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("docker-compose") && name.ends_with(".yml")
            })
            .collect();
        compose_files.sort_by_key(|e| e.file_name());
        for entry in compose_files {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                for line in text.lines() {
                    let stripped = line.trim();
                    if let Some(tag) = stripped.strip_prefix("image:") {
                        let tag = tag.trim().to_string();
                        if !tag.is_empty() && !images.contains(&tag) {
                            images.push(tag);
                        }
                    }
                }
            }
        }
    }

    // .env keys relevant to update
    let update_keys: std::collections::HashSet<&str> = [
        "DREAM_VERSION", "TIER", "LLM_MODEL", "GGUF_FILE",
        "CTX_SIZE", "GPU_BACKEND", "N_GPU_LAYERS",
    ]
    .into_iter()
    .collect();

    let mut env_snapshot = serde_json::Map::new();
    if let Ok(text) = std::fs::read_to_string(&env_file) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || !line.contains('=') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                if update_keys.contains(key) {
                    env_snapshot.insert(key.to_string(), json!(val));
                }
            }
        }
    }

    Json(json!({
        "dry_run": true,
        "current_version": current,
        "latest_version": null,
        "update_available": false,
        "changelog_url": null,
        "images": images,
        "env_keys": env_snapshot,
    }))
}

/// POST /api/update — trigger update action (check, backup, update)
pub async fn update_action(Json(body): Json<Value>) -> Json<Value> {
    let action = body["action"].as_str().unwrap_or("check");
    match action {
        "check" => Json(json!({"status": "ok", "action": "check", "message": "Use POST /api/updates/check"})),
        "backup" => {
            // Trigger backup via dream-cli
            match tokio::process::Command::new("dream-cli")
                .args(["backup", "create"])
                .output()
                .await
            {
                Ok(output) if output.status.success() => {
                    Json(json!({"status": "ok", "action": "backup", "message": "Backup created"}))
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Json(json!({"status": "error", "action": "backup", "message": format!("Backup failed: {stderr}")}))
                }
                Err(e) => Json(json!({"status": "error", "action": "backup", "message": format!("Failed to run dream-cli: {e}")})),
            }
        }
        "update" => {
            Json(json!({"status": "error", "action": "update", "message": "In-place updates not yet supported via API"}))
        }
        _ => Json(json!({"status": "error", "message": format!("Unknown action: {action}")})),
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
    async fn version_info_requires_auth() {
        let req = Request::builder()
            .uri("/api/version")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn version_info_returns_shape() {
        let req = Request::builder()
            .uri("/api/version")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("current").is_some());
        assert!(val.get("update_available").is_some());
    }

    #[tokio::test]
    async fn update_dry_run_returns_shape() {
        let req = Request::builder()
            .uri("/api/update/dry-run")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["dry_run"], true);
        assert!(val.get("current_version").is_some());
        assert!(val.get("images").is_some());
    }

    #[tokio::test]
    async fn update_action_unknown_returns_error() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/update")
            .header("authorization", auth_header())
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"invalid"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["status"], "error");
    }

    #[tokio::test]
    async fn update_action_requires_auth() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/update")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"check"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn releases_manifest_requires_auth() {
        let req = Request::builder()
            .uri("/api/releases/manifest")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn releases_manifest_returns_releases_key() {
        let req = Request::builder()
            .uri("/api/releases/manifest")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("releases").is_some());
        assert!(val["releases"].is_array());
        assert!(val.get("checked_at").is_some());
    }

    #[tokio::test]
    async fn update_action_backup_returns_error_without_cli() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/update")
            .header("authorization", auth_header())
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"backup"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["action"], "backup");
        // dream-cli not available in test env => error
        assert_eq!(val["status"], "error");
    }

    #[tokio::test]
    async fn update_action_update_returns_not_supported() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/update")
            .header("authorization", auth_header())
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"update"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["status"], "error");
        assert_eq!(val["action"], "update");
    }

    #[tokio::test]
    async fn dry_run_requires_auth() {
        let req = Request::builder()
            .uri("/api/update/dry-run")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn update_action_check_returns_redirect() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/update")
            .header("authorization", auth_header())
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"check"}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["action"], "check");
        assert_eq!(val["status"], "ok");
    }
}
