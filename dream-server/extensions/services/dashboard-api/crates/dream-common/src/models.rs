//! Pydantic model equivalents as serde structs.
//!
//! Every struct here maps 1:1 to its Python counterpart in `models.py`.
//! Field names use `#[serde(rename)]` where the JSON wire format differs
//! from Rust naming conventions.

use serde::{Deserialize, Serialize};

// -- GPU --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GPUInfo {
    pub name: String,
    pub memory_used_mb: i64,
    pub memory_total_mb: i64,
    pub memory_percent: f64,
    pub utilization_percent: i64,
    pub temperature_c: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_w: Option<f64>,
    #[serde(default = "default_memory_type")]
    pub memory_type: String,
    #[serde(default = "default_gpu_backend")]
    pub gpu_backend: String,
}

fn default_memory_type() -> String {
    "discrete".to_string()
}

fn default_gpu_backend() -> String {
    std::env::var("GPU_BACKEND").unwrap_or_else(|_| "nvidia".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndividualGPU {
    pub index: i64,
    pub uuid: String,
    pub name: String,
    pub memory_used_mb: i64,
    pub memory_total_mb: i64,
    pub memory_percent: f64,
    pub utilization_percent: i64,
    pub temperature_c: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_w: Option<f64>,
    #[serde(default)]
    pub assigned_services: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiGPUStatus {
    pub gpu_count: i64,
    pub backend: String,
    pub gpus: Vec<IndividualGPU>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topology: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tensor_split: Option<String>,
    pub aggregate: GPUInfo,
}

// -- Services --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub port: i64,
    pub external_port: i64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<f64>,
}

// -- Disk --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskUsage {
    pub path: String,
    pub used_gb: f64,
    pub total_gb: f64,
    pub percent: f64,
}

// -- Model --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub size_gb: f64,
    pub context_length: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
}

// -- Bootstrap --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapStatus {
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded_gb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_gb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<i64>,
}

// -- Full Status --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullStatus {
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GPUInfo>,
    pub services: Vec<ServiceStatus>,
    pub disk: DiskUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelInfo>,
    pub bootstrap: BootstrapStatus,
    pub uptime_seconds: i64,
}

// -- Preflight --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortCheckRequest {
    pub ports: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConflict {
    pub port: i64,
    pub service: String,
    pub in_use: bool,
}

// -- Chat / Persona --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaRequest {
    pub persona: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

// -- Updates --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub current: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(default)]
    pub update_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changelog_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAction {
    pub action: String,
}

// -- Privacy --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyShieldStatus {
    pub enabled: bool,
    pub container_running: bool,
    pub port: i64,
    pub target_api: String,
    pub pii_cache_enabled: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyShieldToggle {
    pub enable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Wire-format contract tests --

    #[test]
    fn test_service_status_json_keys() {
        let ss = ServiceStatus {
            id: "llama".into(),
            name: "Llama Server".into(),
            port: 8080,
            external_port: 8080,
            status: "healthy".into(),
            response_time_ms: None,
        };
        let val: serde_json::Value = serde_json::to_value(&ss).unwrap();
        assert!(val.get("id").is_some());
        assert!(val.get("name").is_some());
        assert!(val.get("port").is_some());
        assert!(val.get("external_port").is_some());
        assert!(val.get("status").is_some());
        assert!(val.get("response_time_ms").is_none());
    }

    #[test]
    fn test_privacy_shield_status_json_keys() {
        let ps = PrivacyShieldStatus {
            enabled: true,
            container_running: true,
            port: 8888,
            target_api: "http://localhost:11434".into(),
            pii_cache_enabled: false,
            message: "Shield active".into(),
        };
        let val: serde_json::Value = serde_json::to_value(&ps).unwrap();
        assert_eq!(val["enabled"], true);
        assert_eq!(val["container_running"], true);
        assert_eq!(val["port"], 8888);
        assert_eq!(val["target_api"], "http://localhost:11434");
        assert_eq!(val["pii_cache_enabled"], false);
        assert_eq!(val["message"], "Shield active");
    }

    #[test]
    fn test_disk_usage_json_keys() {
        let du = DiskUsage {
            path: "/data".into(),
            used_gb: 120.5,
            total_gb: 500.0,
            percent: 24.1,
        };
        let val: serde_json::Value = serde_json::to_value(&du).unwrap();
        assert_eq!(val["path"], "/data");
        assert_eq!(val["used_gb"], 120.5);
        assert_eq!(val["total_gb"], 500.0);
        assert_eq!(val["percent"], 24.1);
    }

    #[test]
    fn test_gpu_info_json_keys() {
        let gpu = GPUInfo {
            name: "RTX 4090".into(),
            memory_used_mb: 4096,
            memory_total_mb: 24576,
            memory_percent: 16.7,
            utilization_percent: 85,
            temperature_c: 72,
            power_w: Some(320.0),
            memory_type: "discrete".into(),
            gpu_backend: "nvidia".into(),
        };
        let val: serde_json::Value = serde_json::to_value(&gpu).unwrap();
        assert_eq!(val["name"], "RTX 4090");
        assert_eq!(val["memory_used_mb"], 4096);
        assert_eq!(val["memory_total_mb"], 24576);
        assert_eq!(val["memory_percent"], 16.7);
        assert_eq!(val["utilization_percent"], 85);
        assert_eq!(val["temperature_c"], 72);
        assert_eq!(val["power_w"], 320.0);
        assert_eq!(val["memory_type"], "discrete");
        assert_eq!(val["gpu_backend"], "nvidia");
    }

    #[test]
    fn test_gpu_info_omits_power_when_none() {
        let gpu = GPUInfo {
            name: "RX 7900 XTX".into(),
            memory_used_mb: 2048,
            memory_total_mb: 24576,
            memory_percent: 8.3,
            utilization_percent: 50,
            temperature_c: 65,
            power_w: None,
            memory_type: "discrete".into(),
            gpu_backend: "amd".into(),
        };
        let val: serde_json::Value = serde_json::to_value(&gpu).unwrap();
        assert!(val.get("power_w").is_none());
    }

    // -- Round-trip tests --

    #[test]
    fn test_gpu_info_roundtrip() {
        let gpu = GPUInfo {
            name: "RTX 3080".into(),
            memory_used_mb: 8000,
            memory_total_mb: 10240,
            memory_percent: 78.1,
            utilization_percent: 95,
            temperature_c: 80,
            power_w: Some(300.0),
            memory_type: "discrete".into(),
            gpu_backend: "nvidia".into(),
        };
        let json_str = serde_json::to_string(&gpu).unwrap();
        let rt: GPUInfo = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rt.name, gpu.name);
        assert_eq!(rt.memory_used_mb, gpu.memory_used_mb);
        assert_eq!(rt.memory_total_mb, gpu.memory_total_mb);
        assert_eq!(rt.power_w, gpu.power_w);
        assert_eq!(rt.memory_type, gpu.memory_type);
        assert_eq!(rt.gpu_backend, gpu.gpu_backend);
    }

    #[test]
    fn test_bootstrap_status_inactive() {
        let json = r#"{"active": false}"#;
        let bs: BootstrapStatus = serde_json::from_str(json).unwrap();
        assert!(!bs.active);
        assert!(bs.model_name.is_none());
        assert!(bs.percent.is_none());
        assert!(bs.downloaded_gb.is_none());
        assert!(bs.total_gb.is_none());
        assert!(bs.speed_mbps.is_none());
        assert!(bs.eta_seconds.is_none());
    }

    #[test]
    fn test_bootstrap_status_active() {
        let bs = BootstrapStatus {
            active: true,
            model_name: Some("llama-3.1-8b".into()),
            percent: Some(45.2),
            downloaded_gb: Some(2.1),
            total_gb: Some(4.7),
            speed_mbps: Some(150.0),
            eta_seconds: Some(120),
        };
        let json_str = serde_json::to_string(&bs).unwrap();
        let rt: BootstrapStatus = serde_json::from_str(&json_str).unwrap();
        assert!(rt.active);
        assert_eq!(rt.model_name.as_deref(), Some("llama-3.1-8b"));
        assert_eq!(rt.percent, Some(45.2));
        assert_eq!(rt.downloaded_gb, Some(2.1));
        assert_eq!(rt.total_gb, Some(4.7));
        assert_eq!(rt.speed_mbps, Some(150.0));
        assert_eq!(rt.eta_seconds, Some(120));
    }

    #[test]
    fn test_port_check_request_roundtrip() {
        let req = PortCheckRequest {
            ports: vec![8080, 3000, 11434],
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let rt: PortCheckRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rt.ports, vec![8080, 3000, 11434]);
    }

    #[test]
    fn test_version_info_roundtrip() {
        let vi = VersionInfo {
            current: "1.2.0".into(),
            latest: Some("1.3.0".into()),
            update_available: true,
            changelog_url: Some("https://github.com/example/releases".into()),
            checked_at: Some("2026-04-04T12:00:00Z".into()),
        };
        let json_str = serde_json::to_string(&vi).unwrap();
        let rt: VersionInfo = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rt.current, "1.2.0");
        assert_eq!(rt.latest.as_deref(), Some("1.3.0"));
        assert!(rt.update_available);
        assert_eq!(
            rt.changelog_url.as_deref(),
            Some("https://github.com/example/releases")
        );
        assert_eq!(rt.checked_at.as_deref(), Some("2026-04-04T12:00:00Z"));
    }

    #[test]
    fn test_full_status_roundtrip() {
        let fs = FullStatus {
            timestamp: "2026-04-04T12:00:00Z".into(),
            gpu: Some(GPUInfo {
                name: "RTX 4090".into(),
                memory_used_mb: 4096,
                memory_total_mb: 24576,
                memory_percent: 16.7,
                utilization_percent: 85,
                temperature_c: 72,
                power_w: Some(320.0),
                memory_type: "discrete".into(),
                gpu_backend: "nvidia".into(),
            }),
            services: vec![ServiceStatus {
                id: "llama".into(),
                name: "Llama Server".into(),
                port: 8080,
                external_port: 8080,
                status: "healthy".into(),
                response_time_ms: Some(12.5),
            }],
            disk: DiskUsage {
                path: "/data".into(),
                used_gb: 120.5,
                total_gb: 500.0,
                percent: 24.1,
            },
            model: Some(ModelInfo {
                name: "llama-3.1-8b".into(),
                size_gb: 4.7,
                context_length: 8192,
                quantization: Some("Q4_K_M".into()),
            }),
            bootstrap: BootstrapStatus {
                active: false,
                model_name: None,
                percent: None,
                downloaded_gb: None,
                total_gb: None,
                speed_mbps: None,
                eta_seconds: None,
            },
            uptime_seconds: 3600,
        };
        let json_str = serde_json::to_string(&fs).unwrap();
        let rt: FullStatus = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rt.timestamp, "2026-04-04T12:00:00Z");
        assert!(rt.gpu.is_some());
        assert_eq!(rt.services.len(), 1);
        assert_eq!(rt.services[0].id, "llama");
        assert_eq!(rt.disk.path, "/data");
        assert!(rt.model.is_some());
        assert_eq!(rt.model.as_ref().unwrap().name, "llama-3.1-8b");
        assert!(!rt.bootstrap.active);
        assert_eq!(rt.uptime_seconds, 3600);
    }

    #[test]
    fn test_model_info_roundtrip() {
        let mi = ModelInfo {
            name: "mistral-7b".into(),
            size_gb: 3.8,
            context_length: 32768,
            quantization: Some("Q5_K_M".into()),
        };
        let json_str = serde_json::to_string(&mi).unwrap();
        let rt: ModelInfo = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rt.name, "mistral-7b");
        assert_eq!(rt.size_gb, 3.8);
        assert_eq!(rt.context_length, 32768);
        assert_eq!(rt.quantization.as_deref(), Some("Q5_K_M"));
    }
}
