//! Settings endpoints: /api/service-tokens, /api/external-links, /api/storage

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use std::path::Path;

use crate::state::AppState;

// Sidebar icon mapping (mirrors SIDEBAR_ICONS in config.py)
fn sidebar_icon(service_id: &str) -> &'static str {
    match service_id {
        "open-webui" => "MessageSquare",
        "n8n" => "Network",
        "openclaw" => "Bot",
        "opencode" => "Code",
        "perplexica" => "Search",
        "comfyui" => "Image",
        "token-spy" => "Terminal",
        "langfuse" => "BarChart2",
        _ => "ExternalLink",
    }
}

/// GET /api/service-tokens
pub async fn service_tokens() -> Json<Value> {
    let result = tokio::task::spawn_blocking(|| {
        let mut tokens = json!({});
        let mut oc_token = std::env::var("OPENCLAW_TOKEN").unwrap_or_default();
        if oc_token.is_empty() {
            let paths = [
                Path::new("/data/openclaw/home/gateway-token"),
                Path::new("/dream-server/.env"),
            ];
            for path in &paths {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if path.extension().map_or(false, |e| e == "env") {
                        for line in content.lines() {
                            if let Some(val) = line.strip_prefix("OPENCLAW_TOKEN=") {
                                oc_token = val.trim().to_string();
                                break;
                            }
                        }
                    } else {
                        oc_token = content.trim().to_string();
                    }
                }
                if !oc_token.is_empty() {
                    break;
                }
            }
        }
        if !oc_token.is_empty() {
            tokens["openclaw"] = json!(oc_token);
        }
        tokens
    })
    .await
    .unwrap_or_else(|_| json!({}));

    Json(result)
}

/// GET /api/external-links
pub async fn external_links(State(state): State<AppState>) -> Json<Value> {
    let mut links = Vec::new();
    for (sid, cfg) in state.services.iter() {
        if cfg.external_port == 0 || sid == "dashboard-api" {
            continue;
        }
        links.push(json!({
            "id": sid,
            "label": cfg.name,
            "port": cfg.external_port,
            "ui_path": cfg.ui_path,
            "icon": sidebar_icon(sid),
            "healthNeedles": [sid, cfg.name.to_lowercase()],
        }));
    }
    Json(json!(links))
}

/// GET /api/storage
pub async fn api_storage(State(state): State<AppState>) -> Json<Value> {
    // Check cache
    if let Some(cached) = state.cache.get(&"storage".to_string()).await {
        return Json(cached);
    }

    let data_dir = std::env::var("DREAM_DATA_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/.dream-server").to_string());
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());

    let result = tokio::task::spawn_blocking(move || {
        let data_path = Path::new(&data_dir);
        let models_dir = data_path.join("models");
        let vector_dir = data_path.join("qdrant");

        fn dir_size_gb(path: &Path) -> f64 {
            if !path.exists() {
                return 0.0;
            }
            fn walk_size(path: &Path) -> u64 {
                let mut total = 0u64;
                if let Ok(entries) = std::fs::read_dir(path) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file() {
                            total += p.metadata().map(|m| m.len()).unwrap_or(0);
                        } else if p.is_dir() {
                            total += walk_size(&p);
                        }
                    }
                }
                total
            }
            let total = walk_size(path);
            (total as f64 / (1024.0 * 1024.0 * 1024.0) * 100.0).round() / 100.0
        }

        let disk_info = crate::helpers::get_disk_usage(&install_dir);
        let models_gb = dir_size_gb(&models_dir);
        let vector_gb = dir_size_gb(&vector_dir);
        let data_total = dir_size_gb(data_path);
        let other_gb = data_total - models_gb - vector_gb;
        let total_data_gb = models_gb + vector_gb + other_gb.max(0.0);

        json!({
            "models": {
                "formatted": format!("{models_gb:.1} GB"),
                "gb": models_gb,
                "percent": if disk_info.total_gb > 0.0 { (models_gb / disk_info.total_gb * 1000.0).round() / 10.0 } else { 0.0 },
            },
            "vector_db": {
                "formatted": format!("{vector_gb:.1} GB"),
                "gb": vector_gb,
                "percent": if disk_info.total_gb > 0.0 { (vector_gb / disk_info.total_gb * 1000.0).round() / 10.0 } else { 0.0 },
            },
            "total_data": {
                "formatted": format!("{total_data_gb:.1} GB"),
                "gb": total_data_gb,
                "percent": if disk_info.total_gb > 0.0 { (total_data_gb / disk_info.total_gb * 1000.0).round() / 10.0 } else { 0.0 },
            },
            "disk": {
                "used_gb": disk_info.used_gb,
                "total_gb": disk_info.total_gb,
                "percent": disk_info.percent,
            },
        })
    })
    .await
    .unwrap_or_else(|_| json!({}));

    // Cache for 30s
    state
        .cache
        .insert("storage".to_string(), result.clone())
        .await;
    Json(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sidebar_icon: known services ──

    #[test]
    fn sidebar_icon_open_webui() {
        assert_eq!(sidebar_icon("open-webui"), "MessageSquare");
    }

    #[test]
    fn sidebar_icon_n8n() {
        assert_eq!(sidebar_icon("n8n"), "Network");
    }

    #[test]
    fn sidebar_icon_openclaw() {
        assert_eq!(sidebar_icon("openclaw"), "Bot");
    }

    #[test]
    fn sidebar_icon_opencode() {
        assert_eq!(sidebar_icon("opencode"), "Code");
    }

    #[test]
    fn sidebar_icon_perplexica() {
        assert_eq!(sidebar_icon("perplexica"), "Search");
    }

    #[test]
    fn sidebar_icon_comfyui() {
        assert_eq!(sidebar_icon("comfyui"), "Image");
    }

    #[test]
    fn sidebar_icon_token_spy() {
        assert_eq!(sidebar_icon("token-spy"), "Terminal");
    }

    #[test]
    fn sidebar_icon_langfuse() {
        assert_eq!(sidebar_icon("langfuse"), "BarChart2");
    }

    // ── sidebar_icon: fallback ──

    #[test]
    fn sidebar_icon_unknown_service_returns_external_link() {
        assert_eq!(sidebar_icon("unknown-service"), "ExternalLink");
    }

    #[test]
    fn sidebar_icon_empty_string_returns_external_link() {
        assert_eq!(sidebar_icon(""), "ExternalLink");
    }

    // ── service_tokens ──

    #[tokio::test]
    async fn service_tokens_returns_json_object() {
        // Clear env so the function falls through to file reads (which won't
        // exist in a test environment), producing an empty object.
        std::env::remove_var("OPENCLAW_TOKEN");
        let Json(value) = service_tokens().await;
        assert!(value.is_object(), "expected JSON object, got {value}");
    }

    #[tokio::test]
    async fn service_tokens_includes_openclaw_when_env_set() {
        std::env::set_var("OPENCLAW_TOKEN", "test-token-123");
        let Json(value) = service_tokens().await;
        assert_eq!(
            value.get("openclaw").and_then(|v| v.as_str()),
            Some("test-token-123"),
        );
        // Clean up
        std::env::remove_var("OPENCLAW_TOKEN");
    }

    #[tokio::test]
    async fn service_tokens_omits_openclaw_when_env_empty() {
        std::env::set_var("OPENCLAW_TOKEN", "");
        let Json(value) = service_tokens().await;
        assert!(
            value.get("openclaw").is_none(),
            "expected no openclaw key when token is empty, got {value}",
        );
        std::env::remove_var("OPENCLAW_TOKEN");
    }
}
