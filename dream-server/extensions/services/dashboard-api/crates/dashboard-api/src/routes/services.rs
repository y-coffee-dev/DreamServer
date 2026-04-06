//! Core data endpoints: /gpu, /services, /disk, /model, /bootstrap, /status

use axum::extract::State;
use axum::Json;
use dream_common::error::AppError;
use dream_common::models::*;
use serde_json::{json, Value};

use crate::gpu::get_gpu_info;
use crate::helpers::*;
use crate::state::*;

/// GET /gpu
pub async fn gpu_endpoint(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    // Check cache first
    if let Some(cached) = state.cache.get(&"gpu_info".to_string()).await {
        if cached.is_null() {
            return Err(AppError::ServiceUnavailable("GPU not available".to_string()));
        }
        return Ok(Json(cached));
    }

    let info = tokio::task::spawn_blocking(get_gpu_info)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;

    let val = serde_json::to_value(&info).unwrap_or(Value::Null);
    state.cache.insert("gpu_info".to_string(), val.clone()).await;

    if info.is_none() {
        return Err(AppError::ServiceUnavailable("GPU not available".to_string()));
    }
    Ok(Json(val))
}

/// GET /services
pub async fn services_endpoint(State(state): State<AppState>) -> Json<Value> {
    let cached = state.services_cache.read().await;
    if let Some(ref statuses) = *cached {
        return Json(serde_json::to_value(statuses).unwrap_or(json!([])));
    }
    drop(cached);

    let statuses = get_all_services(&state.http, &state.services).await;
    Json(serde_json::to_value(&statuses).unwrap_or(json!([])))
}

/// GET /disk
pub async fn disk_endpoint(State(_state): State<AppState>) -> Json<Value> {
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let info = tokio::task::spawn_blocking(move || get_disk_usage(&install_dir))
        .await
        .unwrap_or_else(|_| DiskUsage {
            path: String::new(),
            used_gb: 0.0,
            total_gb: 0.0,
            percent: 0.0,
        });
    Json(serde_json::to_value(&info).unwrap_or(json!({})))
}

/// GET /model
pub async fn model_endpoint() -> Json<Value> {
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let info = tokio::task::spawn_blocking(move || get_model_info(&install_dir))
        .await
        .ok()
        .flatten();
    Json(serde_json::to_value(&info).unwrap_or(Value::Null))
}

/// GET /bootstrap
pub async fn bootstrap_endpoint() -> Json<Value> {
    let data_dir = std::env::var("DREAM_DATA_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/.dream-server").to_string());
    let info = tokio::task::spawn_blocking(move || get_bootstrap_status(&data_dir))
        .await
        .unwrap_or(BootstrapStatus {
            active: false,
            model_name: None,
            percent: None,
            downloaded_gb: None,
            total_gb: None,
            speed_mbps: None,
            eta_seconds: None,
        });
    Json(serde_json::to_value(&info).unwrap_or(json!({})))
}

/// GET /status
pub async fn full_status_endpoint(State(state): State<AppState>) -> Json<Value> {
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let data_dir = std::env::var("DREAM_DATA_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/.dream-server").to_string());

    let install_dir2 = install_dir.clone();
    let _data_dir2 = data_dir.clone();
    let _install_dir3 = install_dir.clone();

    let (gpu_info, model_info, bootstrap_info, uptime, disk_info, service_statuses) = tokio::join!(
        tokio::task::spawn_blocking(get_gpu_info),
        tokio::task::spawn_blocking(move || get_model_info(&install_dir)),
        tokio::task::spawn_blocking(move || get_bootstrap_status(&data_dir)),
        tokio::task::spawn_blocking(get_uptime),
        tokio::task::spawn_blocking(move || get_disk_usage(&install_dir2)),
        async {
            let cached = state.services_cache.read().await;
            match cached.as_ref() {
                Some(s) => s.clone(),
                None => get_all_services(&state.http, &state.services).await,
            }
        },
    );

    let status = FullStatus {
        timestamp: chrono::Utc::now().to_rfc3339(),
        gpu: gpu_info.ok().flatten(),
        services: service_statuses,
        disk: disk_info.unwrap_or(DiskUsage {
            path: String::new(),
            used_gb: 0.0,
            total_gb: 0.0,
            percent: 0.0,
        }),
        model: model_info.ok().flatten(),
        bootstrap: bootstrap_info.unwrap_or(BootstrapStatus {
            active: false,
            model_name: None,
            percent: None,
            downloaded_gb: None,
            total_gb: None,
            speed_mbps: None,
            eta_seconds: None,
        }),
        uptime_seconds: uptime.unwrap_or(0),
    };
    Json(serde_json::to_value(&status).unwrap_or(json!({})))
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
    async fn services_requires_auth() {
        let req = Request::builder()
            .uri("/services")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn services_returns_json_array() {
        let req = Request::builder()
            .uri("/services")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.is_array(), "Expected array from /services, got: {val}");
    }

    #[tokio::test]
    async fn disk_returns_json() {
        let req = Request::builder()
            .uri("/disk")
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
    async fn model_returns_json() {
        let req = Request::builder()
            .uri("/model")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn bootstrap_returns_json() {
        let req = Request::builder()
            .uri("/bootstrap")
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
    async fn full_status_returns_json() {
        let req = Request::builder()
            .uri("/status")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("timestamp").is_some());
    }
}
