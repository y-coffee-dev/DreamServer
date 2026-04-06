//! Shared helper functions for service health checking, LLM metrics, and system info.
//!
//! Mirrors `helpers.py` — uses `reqwest` for HTTP (replacing aiohttp/httpx),
//! reads /proc for Linux metrics, and provides cross-platform uptime/CPU/RAM.

use dream_common::manifest::ServiceConfig;
use dream_common::models::{BootstrapStatus, DiskUsage, ModelInfo, ServiceStatus};
use reqwest::Client;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Token Tracking
// ---------------------------------------------------------------------------

static PREV_TOKENS: Mutex<Option<PrevTokens>> = Mutex::new(None);

struct PrevTokens {
    count: f64,
    gen_secs: f64,
}

fn update_lifetime_tokens(data_dir: &Path, server_counter: f64) -> i64 {
    let token_file = data_dir.join("token_counter.json");
    let mut data: serde_json::Value = token_file
        .exists()
        .then(|| {
            std::fs::read_to_string(&token_file)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
        })
        .flatten()
        .unwrap_or_else(|| json!({"lifetime": 0, "last_server_counter": 0}));

    let prev = data["last_server_counter"].as_f64().unwrap_or(0.0);
    let delta = if server_counter < prev {
        server_counter
    } else {
        server_counter - prev
    };

    let lifetime = data["lifetime"].as_i64().unwrap_or(0) + delta as i64;
    data["lifetime"] = json!(lifetime);
    data["last_server_counter"] = json!(server_counter);

    if let Err(e) = std::fs::write(&token_file, serde_json::to_string(&data).unwrap_or_default()) {
        warn!("Failed to write token counter file: {e}");
    }
    lifetime
}

fn get_lifetime_tokens(data_dir: &Path) -> i64 {
    let token_file = data_dir.join("token_counter.json");
    std::fs::read_to_string(&token_file)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["lifetime"].as_i64())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// LLM Metrics
// ---------------------------------------------------------------------------

pub async fn get_llama_metrics(
    client: &Client,
    services: &HashMap<String, ServiceConfig>,
    data_dir: &Path,
    llm_backend: &str,
    model_hint: Option<&str>,
) -> serde_json::Value {
    let svc = match services.get("llama-server") {
        Some(s) => s,
        None => return json!({"tokens_per_second": 0, "lifetime_tokens": get_lifetime_tokens(data_dir)}),
    };

    let metrics_port: u16 = std::env::var("LLAMA_METRICS_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(svc.port);

    let model_name = match model_hint {
        Some(m) => m.to_string(),
        None => get_loaded_model(client, services, llm_backend).await.unwrap_or_default(),
    };

    let url = format!("http://{}:{}/metrics", svc.host, metrics_port);
    let resp = match client.get(&url).query(&[("model", &model_name)]).send().await {
        Ok(r) => r,
        Err(e) => {
            debug!("get_llama_metrics failed: {e}");
            return json!({"tokens_per_second": 0, "lifetime_tokens": get_lifetime_tokens(data_dir)});
        }
    };

    let text = resp.text().await.unwrap_or_default();
    let mut tokens_total = 0.0f64;
    let mut seconds_total = 0.0f64;
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        if line.contains("tokens_predicted_total") {
            tokens_total = line.split_whitespace().last().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        }
        if line.contains("tokens_predicted_seconds_total") {
            seconds_total = line.split_whitespace().last().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        }
    }

    let tps = {
        let mut prev = PREV_TOKENS.lock().unwrap();
        let mut new_tps = 0.0;
        if let Some(ref p) = *prev {
            if tokens_total > p.count {
                let delta_secs = seconds_total - p.gen_secs;
                if delta_secs > 0.0 {
                    new_tps = ((tokens_total - p.count) / delta_secs * 10.0).round() / 10.0;
                }
            }
        }
        *prev = Some(PrevTokens {
            count: tokens_total,
            gen_secs: seconds_total,
        });
        new_tps
    };

    let lifetime = update_lifetime_tokens(data_dir, tokens_total);
    json!({"tokens_per_second": tps, "lifetime_tokens": lifetime})
}

pub async fn get_loaded_model(
    client: &Client,
    services: &HashMap<String, ServiceConfig>,
    llm_backend: &str,
) -> Option<String> {
    let svc = services.get("llama-server")?;
    let api_prefix = if llm_backend == "lemonade" { "/api/v1" } else { "/v1" };
    let url = format!("http://{}:{}{}/models", svc.host, svc.port, api_prefix);

    let resp = client.get(&url).send().await.ok()?;
    let body: serde_json::Value = resp.json().await.ok()?;
    let models = body["data"].as_array()?;

    for m in models {
        if let Some(status) = m.get("status").and_then(|s| s.as_object()) {
            if status.get("value").and_then(|v| v.as_str()) == Some("loaded") {
                return m["id"].as_str().map(|s| s.to_string());
            }
        }
    }
    models.first().and_then(|m| m["id"].as_str().map(|s| s.to_string()))
}

pub async fn get_llama_context_size(
    client: &Client,
    services: &HashMap<String, ServiceConfig>,
    model_hint: Option<&str>,
    llm_backend: &str,
) -> Option<i64> {
    let svc = services.get("llama-server")?;
    let loaded = match model_hint {
        Some(m) => m.to_string(),
        None => get_loaded_model(client, services, llm_backend).await?,
    };
    let mut url = format!("http://{}:{}/props", svc.host, svc.port);
    if !loaded.is_empty() {
        url.push_str(&format!("?model={loaded}"));
    }
    let resp = client.get(&url).send().await.ok()?;
    let body: serde_json::Value = resp.json().await.ok()?;
    body["default_generation_settings"]["n_ctx"]
        .as_i64()
        .or_else(|| body["default_generation_settings"]["n_ctx"].as_f64().map(|f| f as i64))
}

// ---------------------------------------------------------------------------
// Service Health
// ---------------------------------------------------------------------------

pub async fn check_service_health(
    client: &Client,
    service_id: &str,
    config: &ServiceConfig,
) -> ServiceStatus {
    // Host-systemd services are managed externally
    if config.service_type.as_deref() == Some("host-systemd") {
        return ServiceStatus {
            id: service_id.to_string(),
            name: config.name.clone(),
            port: config.port as i64,
            external_port: config.external_port as i64,
            status: "healthy".to_string(),
            response_time_ms: None,
        };
    }

    let health_port = config.health_port.unwrap_or(config.port);
    let url = format!("http://{}:{}{}", config.host, health_port, config.health);
    let start = Instant::now();

    let (status, response_time) = match client.get(&url).send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let s = if resp.status().as_u16() < 400 { "healthy" } else { "unhealthy" };
            (s.to_string(), Some((elapsed * 10.0).round() / 10.0))
        }
        Err(e) => {
            let msg = e.to_string();
            if e.is_timeout() {
                ("degraded".to_string(), None)
            } else if msg.contains("dns error") || msg.contains("Name or service not known") {
                ("not_deployed".to_string(), None)
            } else {
                debug!("Health check failed for {service_id} at {url}: {e}");
                ("down".to_string(), None)
            }
        }
    };

    ServiceStatus {
        id: service_id.to_string(),
        name: config.name.clone(),
        port: config.port as i64,
        external_port: config.external_port as i64,
        status,
        response_time_ms: response_time,
    }
}

pub async fn get_all_services(
    client: &Client,
    services: &HashMap<String, ServiceConfig>,
) -> Vec<ServiceStatus> {
    let futs: Vec<_> = services
        .iter()
        .map(|(id, cfg)| check_service_health(client, id, cfg))
        .collect();
    futures::future::join_all(futs).await
}

// ---------------------------------------------------------------------------
// System Metrics (sync — run via spawn_blocking)
// ---------------------------------------------------------------------------

pub fn get_disk_usage(install_dir: &str) -> DiskUsage {
    let path = if Path::new(install_dir).exists() {
        install_dir.to_string()
    } else {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string())
    };

    let (total, used) = {
        #[cfg(unix)]
        {
            use std::ffi::CString;
            let c_path = CString::new(path.as_str()).unwrap_or_default();
            let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0 {
                let total = stat.f_blocks as u64 * stat.f_frsize as u64;
                let free = stat.f_bfree as u64 * stat.f_frsize as u64;
                (total, total - free)
            } else {
                (0, 0)
            }
        }
        #[cfg(not(unix))]
        {
            (0u64, 0u64)
        }
    };

    let total_gb = total as f64 / (1024.0 * 1024.0 * 1024.0);
    let used_gb = used as f64 / (1024.0 * 1024.0 * 1024.0);
    DiskUsage {
        path,
        used_gb: (used_gb * 100.0).round() / 100.0,
        total_gb: (total_gb * 100.0).round() / 100.0,
        percent: if total > 0 {
            (used as f64 / total as f64 * 1000.0).round() / 10.0
        } else {
            0.0
        },
    }
}

pub fn get_model_info(install_dir: &str) -> Option<ModelInfo> {
    let env_path = Path::new(install_dir).join(".env");
    let text = std::fs::read_to_string(&env_path).ok()?;
    for line in text.lines() {
        if let Some(val) = line.strip_prefix("LLM_MODEL=") {
            let model_name = val.trim().trim_matches(|c| c == '"' || c == '\'');
            let lower = model_name.to_lowercase();
            let size_gb = if lower.contains("7b") {
                4.0
            } else if lower.contains("14b") {
                8.0
            } else if lower.contains("32b") {
                16.0
            } else if lower.contains("70b") {
                35.0
            } else {
                15.0
            };
            let quant = if lower.contains("awq") {
                Some("AWQ".to_string())
            } else if lower.contains("gptq") {
                Some("GPTQ".to_string())
            } else if lower.contains("gguf") {
                Some("GGUF".to_string())
            } else {
                None
            };
            return Some(ModelInfo {
                name: model_name.to_string(),
                size_gb,
                context_length: 32768,
                quantization: quant,
            });
        }
    }
    None
}

pub fn get_bootstrap_status(data_dir: &str) -> BootstrapStatus {
    let status_file = Path::new(data_dir).join("bootstrap-status.json");
    let inactive = BootstrapStatus {
        active: false,
        model_name: None,
        percent: None,
        downloaded_gb: None,
        total_gb: None,
        speed_mbps: None,
        eta_seconds: None,
    };

    let text = match std::fs::read_to_string(&status_file) {
        Ok(t) => t,
        Err(_) => return inactive,
    };

    let data: serde_json::Value = match serde_json::from_str(&text) {
        Ok(d) => d,
        Err(_) => return inactive,
    };

    let status = data["status"].as_str().unwrap_or("");
    if status == "complete" {
        return inactive;
    }
    if status.is_empty()
        && data.get("bytesDownloaded").is_none()
        && data.get("percent").is_none()
    {
        return inactive;
    }

    let eta_str = data["eta"].as_str().unwrap_or("");
    let eta_seconds = if !eta_str.is_empty() && eta_str.trim() != "calculating..." {
        let cleaned = eta_str.replace('m', " ").replace('s', " ");
        let parts: Vec<i64> = cleaned
            .split_whitespace()
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .collect();
        match parts.len() {
            2 => Some(parts[0] * 60 + parts[1]),
            1 => Some(parts[0]),
            _ => None,
        }
    } else {
        None
    };

    let bytes_downloaded = data["bytesDownloaded"].as_f64().unwrap_or(0.0);
    let bytes_total = data["bytesTotal"].as_f64().unwrap_or(0.0);
    let speed_bps = data["speedBytesPerSec"].as_f64().unwrap_or(0.0);

    BootstrapStatus {
        active: true,
        model_name: data["model"].as_str().map(|s| s.to_string()),
        percent: data["percent"].as_f64(),
        downloaded_gb: if bytes_downloaded > 0.0 {
            Some(bytes_downloaded / (1024.0 * 1024.0 * 1024.0))
        } else {
            None
        },
        total_gb: if bytes_total > 0.0 {
            Some(bytes_total / (1024.0 * 1024.0 * 1024.0))
        } else {
            None
        },
        speed_mbps: if speed_bps > 0.0 {
            Some(speed_bps / (1024.0 * 1024.0))
        } else {
            None
        },
        eta_seconds,
    }
}

pub fn get_uptime() -> i64 {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
            .map(|f| f as i64)
            .unwrap_or(0)
    }
    #[cfg(target_os = "macos")]
    {
        let (ok, out) = crate::gpu::run_command(&["sysctl", "-n", "kern.boottime"], 5);
        if ok {
            if let Some(sec) = out.split("sec = ").nth(1).and_then(|s| s.split(',').next()?.trim().parse::<i64>().ok()) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                return now - sec;
            }
        }
        0
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    0
}

pub fn get_cpu_metrics() -> serde_json::Value {
    #[cfg(target_os = "linux")]
    {
        get_cpu_metrics_linux()
    }
    #[cfg(target_os = "macos")]
    {
        get_cpu_metrics_darwin()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        json!({"percent": 0, "temp_c": null})
    }
}

#[cfg(target_os = "linux")]
fn get_cpu_metrics_linux() -> serde_json::Value {
    use std::sync::Mutex;
    static CPU_PREV: Mutex<Option<(i64, i64)>> = Mutex::new(None);

    let mut result = json!({"percent": 0, "temp_c": null});

    if let Ok(text) = std::fs::read_to_string("/proc/stat") {
        if let Some(line) = text.lines().next() {
            let parts: Vec<i64> = line
                .split_whitespace()
                .skip(1)
                .take(7)
                .filter_map(|p| p.parse().ok())
                .collect();
            if parts.len() >= 7 {
                let idle = parts[3] + parts[4];
                let total: i64 = parts.iter().sum();
                let mut prev = CPU_PREV.lock().unwrap();
                if let Some((prev_idle, prev_total)) = *prev {
                    let d_idle = idle - prev_idle;
                    let d_total = total - prev_total;
                    if d_total > 0 {
                        let pct = (1.0 - d_idle as f64 / d_total as f64) * 100.0;
                        result["percent"] = json!((pct * 10.0).round() / 10.0);
                    }
                }
                *prev = Some((idle, total));
            }
        }
    }

    // CPU temperature from thermal zones
    if let Ok(entries) = std::fs::read_dir("/sys/class/thermal") {
        let mut zones: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        zones.sort_by_key(|e| e.file_name());
        for entry in zones {
            let type_path = entry.path().join("type");
            if let Ok(zone_type) = std::fs::read_to_string(&type_path) {
                let lower = zone_type.trim().to_lowercase();
                if ["k10temp", "coretemp", "cpu", "soc", "tctl"]
                    .iter()
                    .any(|k| lower.contains(k))
                {
                    let temp_path = entry.path().join("temp");
                    if let Ok(temp_str) = std::fs::read_to_string(&temp_path) {
                        if let Ok(temp) = temp_str.trim().parse::<i64>() {
                            result["temp_c"] = json!(temp / 1000);
                            break;
                        }
                    }
                }
            }
        }
    }

    result
}

#[cfg(target_os = "macos")]
fn get_cpu_metrics_darwin() -> serde_json::Value {
    let mut result = json!({"percent": 0, "temp_c": null});
    if let Ok(output) = std::process::Command::new("top")
        .args(["-l", "1", "-n", "0", "-stats", "cpu"])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(caps) = regex::Regex::new(r"CPU usage:\s+([\d.]+)%\s+user.*?([\d.]+)%\s+sys")
            .ok()
            .and_then(|re| re.captures(&text))
        {
            let user: f64 = caps[1].parse().unwrap_or(0.0);
            let sys: f64 = caps[2].parse().unwrap_or(0.0);
            result["percent"] = json!(((user + sys) * 10.0).round() / 10.0);
        }
    }
    result
}

pub fn get_ram_metrics() -> serde_json::Value {
    #[cfg(target_os = "linux")]
    {
        get_ram_metrics_linux()
    }
    #[cfg(target_os = "macos")]
    {
        get_ram_metrics_sysctl()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        json!({"used_gb": 0, "total_gb": 0, "percent": 0})
    }
}

#[cfg(target_os = "linux")]
fn get_ram_metrics_linux() -> serde_json::Value {
    let mut result = json!({"used_gb": 0, "total_gb": 0, "percent": 0});
    if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
        let mut meminfo: HashMap<String, i64> = HashMap::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let key = parts[0].trim_end_matches(':').to_string();
                if let Ok(val) = parts[1].parse::<i64>() {
                    meminfo.insert(key, val);
                }
            }
        }
        let total = meminfo.get("MemTotal").copied().unwrap_or(0);
        let available = meminfo.get("MemAvailable").copied().unwrap_or(0);
        let used = total - available;
        let total_gb = (total as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0;
        let used_gb = (used as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0;

        // Apple Silicon in container: override with HOST_RAM_GB
        let gpu_backend = std::env::var("GPU_BACKEND").unwrap_or_default().to_lowercase();
        let (final_total_gb, final_percent) = if gpu_backend == "apple" {
            if let Ok(host_gb) = std::env::var("HOST_RAM_GB").unwrap_or_default().parse::<f64>() {
                if host_gb > 0.0 {
                    let pct = (used as f64 / (host_gb * 1024.0 * 1024.0) * 1000.0).round() / 10.0;
                    ((host_gb * 10.0).round() / 10.0, pct)
                } else {
                    (total_gb, if total > 0 { (used as f64 / total as f64 * 1000.0).round() / 10.0 } else { 0.0 })
                }
            } else {
                (total_gb, if total > 0 { (used as f64 / total as f64 * 1000.0).round() / 10.0 } else { 0.0 })
            }
        } else {
            (total_gb, if total > 0 { (used as f64 / total as f64 * 1000.0).round() / 10.0 } else { 0.0 })
        };

        result["total_gb"] = json!(final_total_gb);
        result["used_gb"] = json!(used_gb);
        result["percent"] = json!(final_percent);
    }
    result
}

#[cfg(target_os = "macos")]
fn get_ram_metrics_sysctl() -> serde_json::Value {
    let mut result = json!({"used_gb": 0, "total_gb": 0, "percent": 0});
    if let Ok(output) = std::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output() {
        let total_bytes: u64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);
        let total_gb = (total_bytes as f64 / (1024.0 * 1024.0 * 1024.0) * 10.0).round() / 10.0;
        result["total_gb"] = json!(total_gb);

        if let Ok(vm_out) = std::process::Command::new("vm_stat").output() {
            let text = String::from_utf8_lossy(&vm_out.stdout);
            let page_size: u64 = text
                .lines()
                .find_map(|l| {
                    l.contains("page size of")
                        .then(|| l.split_whitespace().filter_map(|w| w.parse::<u64>().ok()).next())
                        .flatten()
                })
                .unwrap_or(16384);

            let mut pages: HashMap<String, u64> = HashMap::new();
            for line in text.lines() {
                if let Some((key, val)) = line.split_once(':') {
                    if let Ok(n) = val.trim().trim_end_matches('.').parse::<u64>() {
                        pages.insert(key.trim().to_string(), n);
                    }
                }
            }
            let active = pages.get("Pages active").copied().unwrap_or(0);
            let wired = pages.get("Pages wired down").copied().unwrap_or(0);
            let compressed = pages.get("Pages occupied by compressor").copied().unwrap_or(0);
            let used_bytes = (active + wired + compressed) * page_size;
            let used_gb = (used_bytes as f64 / (1024.0 * 1024.0 * 1024.0) * 10.0).round() / 10.0;
            result["used_gb"] = json!(used_gb);
            if total_bytes > 0 {
                result["percent"] = json!((used_bytes as f64 / total_bytes as f64 * 1000.0).round() / 10.0);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- get_model_info tests --

    #[test]
    fn test_model_info_qwen_7b() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "LLM_MODEL=Qwen2.5-7B-Instruct\n").unwrap();
        let info = get_model_info(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.name, "Qwen2.5-7B-Instruct");
        assert_eq!(info.size_gb, 4.0);
        assert!(info.quantization.is_none());
        assert_eq!(info.context_length, 32768);
    }

    #[test]
    fn test_model_info_deepseek_14b_awq() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "LLM_MODEL=deepseek-14b-awq\n").unwrap();
        let info = get_model_info(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.size_gb, 8.0);
        assert_eq!(info.quantization.as_deref(), Some("AWQ"));
    }

    #[test]
    fn test_model_info_llama3_70b_gptq() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "LLM_MODEL=llama3-70b-gptq\n").unwrap();
        let info = get_model_info(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.size_gb, 35.0);
        assert_eq!(info.quantization.as_deref(), Some("GPTQ"));
    }

    #[test]
    fn test_model_info_32b_gguf() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "LLM_MODEL=model-32b.gguf\n").unwrap();
        let info = get_model_info(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.size_gb, 16.0);
        assert_eq!(info.quantization.as_deref(), Some("GGUF"));
    }

    #[test]
    fn test_model_info_custom_model_default_fallback() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "LLM_MODEL=custom-model\n").unwrap();
        let info = get_model_info(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.size_gb, 15.0);
        assert!(info.quantization.is_none());
    }

    #[test]
    fn test_model_info_no_env_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(get_model_info(dir.path().to_str().unwrap()).is_none());
    }

    // -- get_bootstrap_status tests --

    #[test]
    fn test_bootstrap_status_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let status = get_bootstrap_status(dir.path().to_str().unwrap());
        assert!(!status.active);
        assert!(status.model_name.is_none());
        assert!(status.percent.is_none());
        assert!(status.eta_seconds.is_none());
    }

    #[test]
    fn test_bootstrap_status_complete() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("bootstrap-status.json"),
            r#"{"status": "complete"}"#,
        ).unwrap();
        let status = get_bootstrap_status(dir.path().to_str().unwrap());
        assert!(!status.active);
    }

    #[test]
    fn test_bootstrap_status_downloading_full() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("bootstrap-status.json"),
            r#"{"status": "downloading", "model": "qwen2.5-7b", "percent": 45.2, "bytesDownloaded": 2147483648, "bytesTotal": 4294967296, "speedBytesPerSec": 52428800, "eta": "1m30s"}"#,
        ).unwrap();
        let status = get_bootstrap_status(dir.path().to_str().unwrap());
        assert!(status.active);
        assert_eq!(status.model_name.as_deref(), Some("qwen2.5-7b"));
        assert_eq!(status.percent, Some(45.2));
        assert_eq!(status.eta_seconds, Some(90));
    }

    #[test]
    fn test_bootstrap_status_calculating_eta() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("bootstrap-status.json"),
            r#"{"status": "downloading", "eta": "calculating..."}"#,
        ).unwrap();
        let status = get_bootstrap_status(dir.path().to_str().unwrap());
        assert!(status.active);
        assert!(status.eta_seconds.is_none());
    }

    // -- Token tracking tests --

    #[test]
    fn test_update_lifetime_tokens_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let lifetime = update_lifetime_tokens(dir.path(), 100.0);
        assert_eq!(lifetime, 100);
        // Verify file was created
        let data: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("token_counter.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(data["lifetime"], 100);
        assert_eq!(data["last_server_counter"], 100.0);
    }

    #[test]
    fn test_update_lifetime_tokens_incremental() {
        let dir = tempfile::tempdir().unwrap();
        update_lifetime_tokens(dir.path(), 100.0);
        let lifetime = update_lifetime_tokens(dir.path(), 150.0);
        assert_eq!(lifetime, 150); // 100 + 50
    }

    #[test]
    fn test_update_lifetime_tokens_server_restart() {
        let dir = tempfile::tempdir().unwrap();
        update_lifetime_tokens(dir.path(), 100.0);
        // Server restarted (counter reset to lower value)
        let lifetime = update_lifetime_tokens(dir.path(), 30.0);
        assert_eq!(lifetime, 130); // 100 + 30 (reset detection)
    }

    #[test]
    fn test_get_lifetime_tokens_no_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(get_lifetime_tokens(dir.path()), 0);
    }

    #[test]
    fn test_get_lifetime_tokens_existing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("token_counter.json"),
            r#"{"lifetime": 42, "last_server_counter": 42}"#,
        )
        .unwrap();
        assert_eq!(get_lifetime_tokens(dir.path()), 42);
    }

    // -- check_service_health tests --

    #[tokio::test]
    async fn test_check_service_health_healthy() {
        use wiremock::{matchers, Mock, MockServer, ResponseTemplate};
        let mock = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock)
            .await;

        let port = mock.address().port();
        let client = Client::new();
        let config = ServiceConfig {
            host: "127.0.0.1".to_string(),
            port,
            external_port: port,
            health: "/health".to_string(),
            name: "test-svc".to_string(),
            ui_path: "/".to_string(),
            service_type: None,
            health_port: None,
        };
        let status = check_service_health(&client, "test-svc", &config).await;
        assert_eq!(status.status, "healthy");
        assert!(status.response_time_ms.is_some());
    }

    #[tokio::test]
    async fn test_check_service_health_unhealthy() {
        use wiremock::{matchers, Mock, MockServer, ResponseTemplate};
        let mock = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/health"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock)
            .await;

        let port = mock.address().port();
        let client = Client::new();
        let config = ServiceConfig {
            host: "127.0.0.1".to_string(),
            port,
            external_port: port,
            health: "/health".to_string(),
            name: "test-svc".to_string(),
            ui_path: "/".to_string(),
            service_type: None,
            health_port: None,
        };
        let status = check_service_health(&client, "test-svc", &config).await;
        assert_eq!(status.status, "unhealthy");
    }

    #[tokio::test]
    async fn test_check_service_health_host_systemd() {
        let client = Client::new();
        let config = ServiceConfig {
            host: "localhost".to_string(),
            port: 59999,
            external_port: 59999,
            health: "/health".to_string(),
            name: "systemd-svc".to_string(),
            ui_path: "/".to_string(),
            service_type: Some("host-systemd".to_string()),
            health_port: None,
        };
        let status = check_service_health(&client, "systemd-svc", &config).await;
        assert_eq!(status.status, "healthy");
        assert!(status.response_time_ms.is_none());
    }

    #[tokio::test]
    async fn test_check_service_health_down() {
        let client = Client::new();
        let config = ServiceConfig {
            host: "127.0.0.1".to_string(),
            port: 1, // Nothing listening
            external_port: 1,
            health: "/health".to_string(),
            name: "dead-svc".to_string(),
            ui_path: "/".to_string(),
            service_type: None,
            health_port: None,
        };
        let status = check_service_health(&client, "dead-svc", &config).await;
        assert!(status.status == "down" || status.status == "not_deployed");
    }

    // -- get_all_services test --

    #[tokio::test]
    async fn test_get_all_services() {
        use wiremock::{matchers, Mock, MockServer, ResponseTemplate};
        let mock = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock)
            .await;

        let port = mock.address().port();
        let client = Client::new();
        let mut services = HashMap::new();
        services.insert(
            "svc1".to_string(),
            ServiceConfig {
                host: "127.0.0.1".to_string(),
                port,
                external_port: port,
                health: "/health".to_string(),
                name: "Service 1".to_string(),
                ui_path: "/".to_string(),
                service_type: None,
                health_port: None,
            },
        );
        let results = get_all_services(&client, &services).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "svc1");
        assert_eq!(results[0].status, "healthy");
    }

    // -- get_disk_usage tests --

    #[test]
    fn test_get_disk_usage_returns_valid_struct() {
        let dir = tempfile::tempdir().unwrap();
        let usage = get_disk_usage(dir.path().to_str().unwrap());
        assert!(usage.total_gb > 0.0);
        assert!(usage.percent >= 0.0 && usage.percent <= 100.0);
    }

    #[test]
    fn test_get_disk_usage_nonexistent_falls_back() {
        let usage = get_disk_usage("/nonexistent/path/that/does/not/exist");
        // Should fall back to home dir
        assert!(usage.total_gb > 0.0);
    }

    // -- get_uptime test --

    #[test]
    fn test_get_uptime_returns_positive() {
        let uptime = get_uptime();
        assert!(uptime > 0);
    }

    // -- get_cpu_metrics and get_ram_metrics tests --

    #[test]
    fn test_get_cpu_metrics_returns_json_with_percent() {
        let metrics = get_cpu_metrics();
        assert!(metrics["percent"].is_number());
    }

    #[test]
    fn test_get_ram_metrics_returns_json_with_fields() {
        let metrics = get_ram_metrics();
        assert!(metrics["total_gb"].is_number());
        assert!(metrics["used_gb"].is_number());
        assert!(metrics["percent"].is_number());
        let total = metrics["total_gb"].as_f64().unwrap();
        assert!(total > 0.0);
    }
}
