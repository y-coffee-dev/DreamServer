//! GET /api/status — Dashboard-compatible full status endpoint.
//! Mirrors the Python `_build_api_status()` function.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use tracing::error;

use crate::gpu::get_gpu_info;
use crate::helpers::*;
use crate::state::AppState;

pub async fn api_status(State(state): State<AppState>) -> Json<Value> {
    match build_api_status(state.clone()).await {
        Ok(val) => Json(val),
        Err(e) => {
            error!("/api/status handler failed — returning safe fallback: {e}");
            Json(json!({
                "gpu": null,
                "services": [],
                "model": null,
                "bootstrap": null,
                "uptime": 0,
                "version": *state.version,
                "tier": "Unknown",
                "cpu": {"percent": 0, "temp_c": null},
                "ram": {"used_gb": 0, "total_gb": 0, "percent": 0},
                "disk": {"used_gb": 0, "total_gb": 0, "percent": 0},
                "system": {"uptime": 0, "hostname": std::env::var("HOSTNAME").unwrap_or_else(|_| "dream-server".to_string())},
                "inference": {"tokensPerSecond": 0, "lifetimeTokens": 0, "loadedModel": null, "contextSize": null},
                "manifest_errors": *state.manifest_errors,
            }))
        }
    }
}

async fn build_api_status(state: AppState) -> anyhow::Result<Value> {
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let data_dir = std::env::var("DREAM_DATA_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/.dream-server").to_string());
    let llm_backend = std::env::var("LLM_BACKEND").unwrap_or_default();

    let install_dir2 = install_dir.clone();
    let data_dir2 = data_dir.clone();

    // Fan out: sync helpers in threads + async health checks simultaneously
    let (
        gpu_info,
        model_info,
        bootstrap_info,
        uptime,
        cpu_metrics,
        ram_metrics,
        disk_info,
        service_statuses,
        loaded_model,
    ) = tokio::join!(
        tokio::task::spawn_blocking(get_gpu_info),
        tokio::task::spawn_blocking(move || get_model_info(&install_dir)),
        tokio::task::spawn_blocking(move || get_bootstrap_status(&data_dir)),
        tokio::task::spawn_blocking(get_uptime),
        tokio::task::spawn_blocking(get_cpu_metrics),
        tokio::task::spawn_blocking(get_ram_metrics),
        tokio::task::spawn_blocking(move || get_disk_usage(&install_dir2)),
        async {
            let cached = state.services_cache.read().await;
            match cached.as_ref() {
                Some(s) => s.clone(),
                None => get_all_services(&state.http, &state.services).await,
            }
        },
        get_loaded_model(&state.http, &state.services, &llm_backend),
    );

    let gpu_info = gpu_info.ok().flatten();
    let model_info = model_info.ok().flatten();
    let bootstrap_info = bootstrap_info.ok().unwrap_or(dream_common::models::BootstrapStatus {
        active: false,
        model_name: None,
        percent: None,
        downloaded_gb: None,
        total_gb: None,
        speed_mbps: None,
        eta_seconds: None,
    });
    let uptime = uptime.ok().unwrap_or(0);
    let cpu_metrics = cpu_metrics.ok().unwrap_or_else(|| json!({"percent": 0, "temp_c": null}));
    let ram_metrics = ram_metrics.ok().unwrap_or_else(|| json!({"used_gb": 0, "total_gb": 0, "percent": 0}));
    let disk_info = disk_info.ok().unwrap_or(dream_common::models::DiskUsage {
        path: String::new(),
        used_gb: 0.0,
        total_gb: 0.0,
        percent: 0.0,
    });

    // Second fan-out: llama metrics + context size (need loaded_model)
    let (llama_metrics_data, context_size) = tokio::join!(
        get_llama_metrics(
            &state.http,
            &state.services,
            std::path::Path::new(&data_dir2),
            &llm_backend,
            loaded_model.as_deref(),
        ),
        get_llama_context_size(
            &state.http,
            &state.services,
            loaded_model.as_deref(),
            &llm_backend,
        ),
    );

    // Build GPU data
    let gpu_data = gpu_info.as_ref().map(|info| {
        let mut gpu_count = 1i64;
        if let Ok(env_count) = std::env::var("GPU_COUNT") {
            if let Ok(n) = env_count.parse::<i64>() {
                gpu_count = n;
            }
        } else if info.name.contains(" \u{00d7} ") {
            if let Some(n) = info.name.rsplit(" \u{00d7} ").next().and_then(|s| s.parse::<i64>().ok()) {
                gpu_count = n;
            }
        } else if info.name.contains(" + ") {
            gpu_count = info.name.matches(" + ").count() as i64 + 1;
        }

        let mut gd = json!({
            "name": info.name,
            "vramUsed": (info.memory_used_mb as f64 / 1024.0 * 10.0).round() / 10.0,
            "vramTotal": (info.memory_total_mb as f64 / 1024.0 * 10.0).round() / 10.0,
            "utilization": info.utilization_percent,
            "temperature": info.temperature_c,
            "memoryType": info.memory_type,
            "backend": info.gpu_backend,
            "gpu_count": gpu_count,
        });
        if let Some(pw) = info.power_w {
            gd["powerDraw"] = json!(pw);
        }
        gd["memoryLabel"] = json!(if info.memory_type == "unified" { "VRAM Partition" } else { "VRAM" });
        gd
    });

    // Services data
    let services_data: Vec<Value> = service_statuses
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "status": s.status,
                "port": s.external_port,
                "uptime": if s.status == "healthy" { Some(uptime) } else { None },
            })
        })
        .collect();

    // Model data
    let model_data = model_info.as_ref().map(|mi| {
        json!({
            "name": mi.name,
            "tokensPerSecond": llama_metrics_data.get("tokens_per_second").and_then(|v| v.as_f64()).filter(|v| *v > 0.0),
            "contextLength": context_size.unwrap_or(mi.context_length),
        })
    });

    // Bootstrap data
    let bootstrap_data = if bootstrap_info.active {
        Some(json!({
            "active": true,
            "model": bootstrap_info.model_name.as_deref().unwrap_or("Full Model"),
            "percent": bootstrap_info.percent.unwrap_or(0.0),
            "bytesDownloaded": bootstrap_info.downloaded_gb.map(|g| (g * 1024.0 * 1024.0 * 1024.0) as i64).unwrap_or(0),
            "bytesTotal": bootstrap_info.total_gb.map(|g| (g * 1024.0 * 1024.0 * 1024.0) as i64).unwrap_or(0),
            "eta": bootstrap_info.eta_seconds,
            "speedMbps": bootstrap_info.speed_mbps,
        }))
    } else {
        None
    };

    // Tier calculation
    let tier = if let Some(ref info) = gpu_info {
        let vram_gb = info.memory_total_mb as f64 / 1024.0;
        if info.memory_type == "unified" && info.gpu_backend == "amd" {
            if vram_gb >= 90.0 { "Strix Halo 90+" } else { "Strix Halo Compact" }
        } else if vram_gb >= 80.0 {
            "Professional"
        } else if vram_gb >= 24.0 {
            "Prosumer"
        } else if vram_gb >= 16.0 {
            "Standard"
        } else if vram_gb >= 8.0 {
            "Entry"
        } else {
            "Minimal"
        }
    } else {
        "Unknown"
    };

    Ok(json!({
        "gpu": gpu_data,
        "services": services_data,
        "model": model_data,
        "bootstrap": bootstrap_data,
        "uptime": uptime,
        "version": *state.version,
        "tier": tier,
        "cpu": cpu_metrics,
        "ram": ram_metrics,
        "disk": {"used_gb": disk_info.used_gb, "total_gb": disk_info.total_gb, "percent": disk_info.percent},
        "system": {"uptime": uptime, "hostname": std::env::var("HOSTNAME").unwrap_or_else(|_| "dream-server".to_string())},
        "inference": {
            "tokensPerSecond": llama_metrics_data.get("tokens_per_second").and_then(|v| v.as_f64()).unwrap_or(0.0),
            "lifetimeTokens": llama_metrics_data.get("lifetime_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            "loadedModel": loaded_model.as_deref().or(model_data.as_ref().and_then(|m| m["name"].as_str())),
            "contextSize": context_size.or(model_data.as_ref().and_then(|m| m["contextLength"].as_i64())),
        },
        "manifest_errors": *state.manifest_errors,
    }))
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

    #[tokio::test]
    async fn api_status_requires_auth() {
        let req = Request::builder()
            .uri("/api/status")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn api_status_returns_json_with_expected_keys() {
        let req = Request::builder()
            .uri("/api/status")
            .header("authorization", "Bearer test-key")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        // Should have core status keys (either from build or fallback)
        assert!(val.get("version").is_some());
        assert!(val.get("tier").is_some());
        assert!(val.get("services").is_some());
    }

    #[tokio::test]
    async fn api_status_returns_gpu_key() {
        let req = Request::builder()
            .uri("/api/status")
            .header("authorization", "Bearer test-key")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        // gpu key should exist (null if no GPU hardware)
        assert!(val.get("gpu").is_some());
    }

    #[tokio::test]
    async fn api_status_returns_system_section() {
        let req = Request::builder()
            .uri("/api/status")
            .header("authorization", "Bearer test-key")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("system").is_some());
        assert!(val["system"].get("hostname").is_some());
        assert!(val["system"].get("uptime").is_some());
    }

    #[tokio::test]
    async fn api_status_returns_inference_section() {
        let req = Request::builder()
            .uri("/api/status")
            .header("authorization", "Bearer test-key")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("inference").is_some());
        assert!(val["inference"].get("tokensPerSecond").is_some());
        assert!(val["inference"].get("lifetimeTokens").is_some());
    }

    #[tokio::test]
    async fn api_status_returns_resource_metrics() {
        let req = Request::builder()
            .uri("/api/status")
            .header("authorization", "Bearer test-key")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("cpu").is_some());
        assert!(val.get("ram").is_some());
        assert!(val.get("disk").is_some());
        assert!(val.get("uptime").is_some());
    }

    #[tokio::test]
    async fn api_status_services_is_array() {
        let req = Request::builder()
            .uri("/api/status")
            .header("authorization", "Bearer test-key")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val["services"].is_array(), "services should be an array");
    }
}
