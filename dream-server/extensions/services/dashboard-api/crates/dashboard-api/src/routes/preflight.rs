//! Preflight endpoints: /api/preflight/{docker,gpu,required-ports,ports,disk}

use axum::extract::State;
use axum::Json;
use dream_common::models::PortCheckRequest;
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;

use crate::gpu::get_gpu_info;
use crate::state::AppState;

/// GET /api/preflight/docker
pub async fn preflight_docker() -> Json<Value> {
    if Path::new("/.dockerenv").exists() {
        return Json(json!({"available": true, "version": "available (host)"}));
    }

    match tokio::process::Command::new("docker")
        .arg("--version")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
            let version = parts
                .get(2)
                .map(|v| v.trim_end_matches(','))
                .unwrap_or("unknown");
            Json(json!({"available": true, "version": version}))
        }
        Ok(_) => Json(json!({"available": false, "error": "Docker command failed"})),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Json(json!({"available": false, "error": "Docker not installed"}))
        }
        Err(_) => Json(json!({"available": false, "error": "Docker check failed"})),
    }
}

/// GET /api/preflight/gpu
pub async fn preflight_gpu() -> Json<Value> {
    let gpu_info = tokio::task::spawn_blocking(get_gpu_info).await.ok().flatten();

    if let Some(info) = gpu_info {
        let vram_gb = (info.memory_total_mb as f64 / 1024.0 * 10.0).round() / 10.0;
        let mut result = json!({
            "available": true,
            "name": info.name,
            "vram": vram_gb,
            "backend": info.gpu_backend,
            "memory_type": info.memory_type,
        });
        if info.memory_type == "unified" {
            result["memory_label"] = json!(format!("{vram_gb} GB Unified"));
        }
        return Json(result);
    }

    let gpu_backend = std::env::var("GPU_BACKEND").unwrap_or_default().to_lowercase();
    let error = if gpu_backend == "amd" {
        "AMD GPU not detected via sysfs. Check /dev/kfd and /dev/dri access."
    } else {
        "No GPU detected. Ensure NVIDIA drivers or AMD amdgpu driver is loaded."
    };
    Json(json!({"available": false, "error": error}))
}

/// GET /api/preflight/required-ports (no auth)
pub async fn preflight_required_ports(State(state): State<AppState>) -> Json<Value> {
    let cached = state.services_cache.read().await;
    let deployed: Option<std::collections::HashSet<String>> = cached.as_ref().map(|statuses| {
        statuses
            .iter()
            .filter(|s| s.status != "not_deployed")
            .map(|s| s.id.clone())
            .collect()
    });

    let mut ports = Vec::new();
    for (sid, cfg) in state.services.iter() {
        if let Some(ref dep) = deployed {
            if !dep.contains(sid) {
                continue;
            }
        }
        let ext_port = cfg.external_port;
        if ext_port > 0 {
            ports.push(json!({"port": ext_port, "service": &cfg.name}));
        }
    }
    Json(json!({"ports": ports}))
}

/// POST /api/preflight/ports
pub async fn preflight_ports(
    State(state): State<AppState>,
    Json(req): Json<PortCheckRequest>,
) -> Json<Value> {
    let mut port_services: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    for (_, cfg) in state.services.iter() {
        if cfg.external_port > 0 {
            port_services.insert(cfg.external_port as i64, cfg.name.clone());
        }
    }

    let mut conflicts = Vec::new();
    for port in &req.ports {
        let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap_or_else(|_| {
            SocketAddr::from(([0, 0, 0, 0], *port as u16))
        });
        if TcpListener::bind(addr).is_err() {
            conflicts.push(json!({
                "port": port,
                "service": port_services.get(port).cloned().unwrap_or_else(|| "Unknown".to_string()),
                "in_use": true,
            }));
        }
    }

    Json(json!({
        "conflicts": conflicts,
        "available": conflicts.is_empty(),
    }))
}

/// GET /api/preflight/disk
pub async fn preflight_disk() -> Json<Value> {
    let data_dir = std::env::var("DREAM_DATA_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/.dream-server").to_string());
    let check_path = if Path::new(&data_dir).exists() {
        data_dir
    } else {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string())
    };

    #[cfg(unix)]
    {
        use std::ffi::CString;
        let c_path = CString::new(check_path.as_str()).unwrap_or_default();
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0 {
            let total = stat.f_blocks as u64 * stat.f_frsize as u64;
            let free = stat.f_bfree as u64 * stat.f_frsize as u64;
            let used = total - free;
            return Json(json!({
                "free": free,
                "total": total,
                "used": used,
                "path": check_path,
            }));
        }
    }

    Json(json!({
        "error": "Disk check failed",
        "free": 0, "total": 0, "used": 0, "path": "",
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

    fn auth_header() -> String {
        "Bearer test-key".to_string()
    }

    #[tokio::test]
    async fn required_ports_is_public() {
        // /api/preflight/required-ports is a public route
        let req = Request::builder()
            .uri("/api/preflight/required-ports")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val["ports"].is_array(), "Expected ports array");
    }

    #[tokio::test]
    async fn preflight_docker_requires_auth() {
        let req = Request::builder()
            .uri("/api/preflight/docker")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn preflight_docker_returns_available_key() {
        let req = Request::builder()
            .uri("/api/preflight/docker")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("available").is_some(), "Expected 'available' key");
    }

    #[tokio::test]
    async fn preflight_gpu_requires_auth() {
        let req = Request::builder()
            .uri("/api/preflight/gpu")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn preflight_gpu_returns_available_key() {
        let req = Request::builder()
            .uri("/api/preflight/gpu")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("available").is_some(), "Expected 'available' key");
    }

    #[tokio::test]
    async fn preflight_disk_requires_auth() {
        let req = Request::builder()
            .uri("/api/preflight/disk")
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn preflight_disk_returns_disk_info() {
        let req = Request::builder()
            .uri("/api/preflight/disk")
            .header("authorization", auth_header())
            .body(Body::empty())
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("total").is_some(), "Expected 'total' key");
        assert!(val.get("free").is_some(), "Expected 'free' key");
    }

    #[tokio::test]
    async fn preflight_ports_requires_auth() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/preflight/ports")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"ports":[8080]}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn preflight_ports_returns_conflicts_shape() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/preflight/ports")
            .header("authorization", auth_header())
            .header("content-type", "application/json")
            .body(Body::from(r#"{"ports":[59999]}"#))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let val: Value = serde_json::from_slice(&body).unwrap();
        assert!(val.get("conflicts").is_some(), "Expected 'conflicts' key");
        assert!(val.get("available").is_some(), "Expected 'available' key");
    }
}
