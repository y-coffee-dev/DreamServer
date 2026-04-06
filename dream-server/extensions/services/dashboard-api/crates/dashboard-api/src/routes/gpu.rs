//! GPU router — /api/gpu/* endpoints. Mirrors routers/gpu.py.

use axum::extract::State;
use axum::Json;
use dream_common::error::AppError;
use dream_common::models::MultiGPUStatus;
use serde_json::{json, Value};
use std::sync::Mutex;

use crate::gpu::*;
use crate::state::AppState;

// GPU history buffer for sparkline charts
static GPU_HISTORY: Mutex<Vec<Value>> = Mutex::new(Vec::new());

/// GET /api/gpu/detailed — per-GPU breakdown with service assignments
pub async fn gpu_detailed(State(_state): State<AppState>) -> Result<Json<Value>, AppError> {
    let gpu_backend = std::env::var("GPU_BACKEND").unwrap_or_else(|_| "nvidia".to_string());

    let (detailed, aggregate) = tokio::task::spawn_blocking(move || {
        let detailed = if gpu_backend == "nvidia" || gpu_backend.is_empty() {
            get_gpu_info_nvidia_detailed()
        } else {
            // AMD detailed or fallback
            None // AMD detailed returns from separate function
        };
        let aggregate = get_gpu_info();
        (detailed, aggregate)
    })
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;

    let aggregate = aggregate
        .ok_or_else(|| AppError::ServiceUnavailable("GPU not available".to_string()))?;

    let gpus = detailed.unwrap_or_default();
    let topology = read_gpu_topology();
    let assignment = decode_gpu_assignment();

    let status = MultiGPUStatus {
        gpu_count: gpus.len().max(1) as i64,
        backend: aggregate.gpu_backend.clone(),
        gpus,
        topology,
        assignment,
        split_mode: std::env::var("GPU_SPLIT_MODE").ok(),
        tensor_split: std::env::var("TENSOR_SPLIT").ok(),
        aggregate,
    };

    Ok(Json(serde_json::to_value(&status).unwrap_or(json!({}))))
}

/// GET /api/gpu/topology — GPU topology from config file (cached 300s)
pub async fn gpu_topology(State(state): State<AppState>) -> Json<Value> {
    if let Some(cached) = state.cache.get(&"gpu_topology".to_string()).await {
        return Json(cached);
    }
    let topo = tokio::task::spawn_blocking(read_gpu_topology)
        .await
        .ok()
        .flatten()
        .unwrap_or(json!(null));
    state.cache.insert("gpu_topology".to_string(), topo.clone()).await;
    Json(topo)
}

/// GET /api/gpu/history — recent GPU metric snapshots for sparkline charts
pub async fn gpu_history() -> Json<Value> {
    let history = GPU_HISTORY.lock().unwrap();
    Json(json!({"history": *history}))
}

/// Background task: poll GPU metrics and append to history buffer
pub async fn poll_gpu_history() {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let info = tokio::task::spawn_blocking(get_gpu_info).await.ok().flatten();
        if let Some(info) = info {
            let point = json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "utilization": info.utilization_percent,
                "memory_percent": info.memory_percent,
                "temperature": info.temperature_c,
            });
            let mut history = GPU_HISTORY.lock().unwrap();
            history.push(point);
            // Keep last 720 samples (1 hour at 5s interval)
            let len = history.len();
            if len > 720 {
                history.drain(..len - 720);
            }
        }
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

    #[tokio::test]
    async fn gpu_history_returns_history_array() {
        let req = Request::builder()
            .uri("/api/gpu/history")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val["history"].is_array(), "Expected history array");
    }

    #[tokio::test]
    async fn gpu_history_is_public() {
        // gpu/history is a public route — no auth header needed
        let req = Request::builder()
            .uri("/api/gpu/history")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn gpu_topology_is_public() {
        let req = Request::builder()
            .uri("/api/gpu/topology")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn gpu_detailed_is_public() {
        // gpu/detailed is public but may return 503 without GPU hardware
        let req = Request::builder()
            .uri("/api/gpu/detailed")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        assert!(status == 200 || status == 503, "Expected 200 or 503, got {status}");
    }
}
