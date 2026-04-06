//! GPU detection and metrics for NVIDIA, AMD, and Apple Silicon.
//!
//! Mirrors `gpu.py` — subprocess calls to nvidia-smi, sysfs reads for AMD,
//! sysctl/vm_stat for Apple Silicon, and env-var fallback for containers.

use dream_common::models::{GPUInfo, IndividualGPU};
use std::path::Path;
use std::process::Command;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Shell helper
// ---------------------------------------------------------------------------

fn run_command(cmd: &[&str], _timeout_secs: u64) -> (bool, String) {
    let result = Command::new(cmd[0])
        .args(&cmd[1..])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            (true, String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!("Command {:?} failed: {}", cmd, stderr.trim());
            (false, String::new())
        }
        Err(e) => {
            debug!("Command {:?} error: {e}", cmd);
            (false, String::new())
        }
    }
}

fn read_sysfs(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

// ---------------------------------------------------------------------------
// AMD
// ---------------------------------------------------------------------------

fn find_amd_gpu_sysfs() -> Option<String> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    let mut card_dirs: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path().join("device");
            if p.is_dir() {
                Some(p.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();
    card_dirs.sort();
    card_dirs.into_iter().find(|d| read_sysfs(&format!("{d}/vendor")).as_deref() == Some("0x1002"))
}

fn find_hwmon_dir(device_path: &str) -> Option<String> {
    let hwmon_base = format!("{device_path}/hwmon");
    let entries = std::fs::read_dir(&hwmon_base).ok()?;
    let mut dirs: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path().to_string_lossy().to_string())
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

pub fn get_gpu_info_amd() -> Option<GPUInfo> {
    let base = find_amd_gpu_sysfs()?;
    let hwmon = find_hwmon_dir(&base);

    let vram_total: i64 = read_sysfs(&format!("{base}/mem_info_vram_total"))?.parse().ok()?;
    let vram_used: i64 = read_sysfs(&format!("{base}/mem_info_vram_used"))?.parse().ok()?;
    let gtt_total: i64 = read_sysfs(&format!("{base}/mem_info_gtt_total")).and_then(|s| s.parse().ok()).unwrap_or(0);
    let gtt_used: i64 = read_sysfs(&format!("{base}/mem_info_gtt_used")).and_then(|s| s.parse().ok()).unwrap_or(0);
    let gpu_busy: i64 = read_sysfs(&format!("{base}/gpu_busy_percent")).and_then(|s| s.parse().ok()).unwrap_or(0);

    let is_unified = gtt_total > vram_total * 4;
    let (mem_total, mem_used) = if is_unified {
        (gtt_total, gtt_used)
    } else {
        (vram_total, vram_used)
    };

    let mut temp = 0i64;
    let mut power_w = None;
    if let Some(ref hw) = hwmon {
        if let Some(t) = read_sysfs(&format!("{hw}/temp1_input")).and_then(|s| s.parse::<i64>().ok()) {
            temp = t / 1000;
        }
        if let Some(p) = read_sysfs(&format!("{hw}/power1_average")).and_then(|s| s.parse::<f64>().ok()) {
            power_w = Some((p / 1e6 * 10.0).round() / 10.0);
        }
    }

    let memory_type = if is_unified { "unified" } else { "discrete" };
    let gpu_name = read_sysfs(&format!("{base}/product_name")).unwrap_or_else(|| {
        if is_unified {
            get_gpu_tier(mem_total as f64 / (1024.0 * 1024.0 * 1024.0), memory_type)
        } else {
            "AMD Radeon".to_string()
        }
    });

    let mem_used_mb = mem_used / (1024 * 1024);
    let mem_total_mb = mem_total / (1024 * 1024);

    Some(GPUInfo {
        name: gpu_name,
        memory_used_mb: mem_used_mb,
        memory_total_mb: mem_total_mb,
        memory_percent: if mem_total_mb > 0 {
            (mem_used_mb as f64 / mem_total_mb as f64 * 1000.0).round() / 10.0
        } else {
            0.0
        },
        utilization_percent: gpu_busy,
        temperature_c: temp,
        power_w,
        memory_type: memory_type.to_string(),
        gpu_backend: "amd".to_string(),
    })
}

// ---------------------------------------------------------------------------
// NVIDIA
// ---------------------------------------------------------------------------

pub fn get_gpu_info_nvidia() -> Option<GPUInfo> {
    let (success, output) = run_command(
        &[
            "nvidia-smi",
            "--query-gpu=name,memory.used,memory.total,utilization.gpu,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits",
        ],
        5,
    );
    if !success || output.is_empty() {
        return None;
    }

    let lines: Vec<&str> = output.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return None;
    }

    let na = ["[N/A]", "[Not Supported]", "N/A", "Not Supported", ""];

    let mut gpus: Vec<(String, i64, i64, i64, i64, Option<f64>)> = Vec::new();
    for line in &lines {
        let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
        if parts.len() < 5 {
            continue;
        }
        if na.contains(&parts[1]) || na.contains(&parts[2]) {
            continue;
        }
        let mem_used: i64 = parts[1].parse().ok()?;
        let mem_total: i64 = parts[2].parse().ok()?;
        let util: i64 = if na.contains(&parts[3]) { 0 } else { parts[3].parse().unwrap_or(0) };
        let temp: i64 = if na.contains(&parts[4]) { 0 } else { parts[4].parse().unwrap_or(0) };
        let power_w = parts.get(5).and_then(|p| {
            if na.contains(p) { None } else { p.parse::<f64>().ok().map(|v| (v * 10.0).round() / 10.0) }
        });
        gpus.push((parts[0].to_string(), mem_used, mem_total, util, temp, power_w));
    }

    if gpus.is_empty() {
        return None;
    }

    if gpus.len() == 1 {
        let g = &gpus[0];
        return Some(GPUInfo {
            name: g.0.clone(),
            memory_used_mb: g.1,
            memory_total_mb: g.2,
            memory_percent: if g.2 > 0 { (g.1 as f64 / g.2 as f64 * 1000.0).round() / 10.0 } else { 0.0 },
            utilization_percent: g.3,
            temperature_c: g.4,
            power_w: g.5,
            memory_type: "discrete".to_string(),
            gpu_backend: "nvidia".to_string(),
        });
    }

    // Multi-GPU aggregate
    let mem_used: i64 = gpus.iter().map(|g| g.1).sum();
    let mem_total: i64 = gpus.iter().map(|g| g.2).sum();
    let avg_util = (gpus.iter().map(|g| g.3).sum::<i64>() as f64 / gpus.len() as f64).round() as i64;
    let max_temp = gpus.iter().map(|g| g.4).max().unwrap_or(0);
    let power_values: Vec<f64> = gpus.iter().filter_map(|g| g.5).collect();
    let total_power = if power_values.is_empty() {
        None
    } else {
        Some((power_values.iter().sum::<f64>() * 10.0).round() / 10.0)
    };

    let names: Vec<&str> = gpus.iter().map(|g| g.0.as_str()).collect();
    let display_name = if names.iter().collect::<std::collections::HashSet<_>>().len() == 1 {
        format!("{} \u{00d7} {}", names[0], gpus.len())
    } else {
        let mut dn = names[..2.min(names.len())].join(" + ");
        if names.len() > 2 {
            dn.push_str(&format!(" + {} more", names.len() - 2));
        }
        dn
    };

    Some(GPUInfo {
        name: display_name,
        memory_used_mb: mem_used,
        memory_total_mb: mem_total,
        memory_percent: if mem_total > 0 { (mem_used as f64 / mem_total as f64 * 1000.0).round() / 10.0 } else { 0.0 },
        utilization_percent: avg_util,
        temperature_c: max_temp,
        power_w: total_power,
        memory_type: "discrete".to_string(),
        gpu_backend: "nvidia".to_string(),
    })
}

// ---------------------------------------------------------------------------
// Apple Silicon
// ---------------------------------------------------------------------------

pub fn get_gpu_info_apple() -> Option<GPUInfo> {
    let gpu_backend = std::env::var("GPU_BACKEND").unwrap_or_default().to_lowercase();

    #[cfg(target_os = "macos")]
    {
        let (ok, chip_output) = run_command(&["sysctl", "-n", "machdep.cpu.brand_string"], 5);
        let chip_name = if ok { chip_output } else { "Apple Silicon".to_string() };

        let (ok, mem_output) = run_command(&["sysctl", "-n", "hw.memsize"], 5);
        if !ok {
            return None;
        }
        let total_bytes: i64 = mem_output.parse().ok()?;
        let total_mb = total_bytes / (1024 * 1024);

        let mut used_mb = 0i64;
        let (ok, vm_output) = run_command(&["vm_stat"], 5);
        if ok {
            let page_size: i64 = vm_output
                .lines()
                .find_map(|l| {
                    l.contains("page size of").then(|| {
                        l.split_whitespace()
                            .filter_map(|w| w.parse::<i64>().ok())
                            .next()
                    })
                })
                .flatten()
                .unwrap_or(16384);

            let mut pages = std::collections::HashMap::new();
            for line in vm_output.lines() {
                if let Some((key, val)) = line.split_once(':') {
                    if let Ok(n) = val.trim().trim_end_matches('.').parse::<i64>() {
                        pages.insert(key.trim().to_string(), n);
                    }
                }
            }
            let active = pages.get("Pages active").copied().unwrap_or(0);
            let wired = pages.get("Pages wired down").copied().unwrap_or(0);
            let compressed = pages.get("Pages occupied by compressor").copied().unwrap_or(0);
            used_mb = (active + wired + compressed) * page_size / (1024 * 1024);
        }

        return Some(GPUInfo {
            name: chip_name,
            memory_used_mb: used_mb,
            memory_total_mb: total_mb,
            memory_percent: if total_mb > 0 { (used_mb as f64 / total_mb as f64 * 1000.0).round() / 10.0 } else { 0.0 },
            utilization_percent: 0,
            temperature_c: 0,
            power_w: None,
            memory_type: "unified".to_string(),
            gpu_backend: "apple".to_string(),
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Container path: GPU_BACKEND=apple + HOST_RAM_GB
        if gpu_backend != "apple" {
            return None;
        }
        let host_ram_gb: f64 = std::env::var("HOST_RAM_GB").ok()?.parse().ok()?;
        if host_ram_gb <= 0.0 {
            return None;
        }
        let total_mb = (host_ram_gb * 1024.0) as i64;
        let mut used_mb = 0i64;
        if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
            let mut mem_total_kb = 0i64;
            let mut mem_avail_kb = 0i64;
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    match parts[0].trim_end_matches(':') {
                        "MemTotal" => mem_total_kb = parts[1].parse().unwrap_or(0),
                        "MemAvailable" => mem_avail_kb = parts[1].parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            used_mb = (mem_total_kb - mem_avail_kb) / 1024;
        }
        Some(GPUInfo {
            name: format!("Apple M-Series ({} GB Unified)", host_ram_gb as i64),
            memory_used_mb: used_mb,
            memory_total_mb: total_mb,
            memory_percent: if total_mb > 0 { (used_mb as f64 / total_mb as f64 * 1000.0).round() / 10.0 } else { 0.0 },
            utilization_percent: 0,
            temperature_c: 0,
            power_w: None,
            memory_type: "unified".to_string(),
            gpu_backend: "apple".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Dispatcher (mirrors get_gpu_info in Python)
// ---------------------------------------------------------------------------

pub fn get_gpu_info() -> Option<GPUInfo> {
    let gpu_backend = std::env::var("GPU_BACKEND").unwrap_or_default().to_lowercase();

    if gpu_backend == "amd" {
        if let Some(info) = get_gpu_info_amd() {
            return Some(info);
        }
    }
    if gpu_backend == "apple" {
        if let Some(info) = get_gpu_info_apple() {
            return Some(info);
        }
    }
    if let Some(info) = get_gpu_info_nvidia() {
        return Some(info);
    }
    if gpu_backend != "amd" {
        if let Some(info) = get_gpu_info_amd() {
            return Some(info);
        }
    }
    #[cfg(target_os = "macos")]
    {
        return get_gpu_info_apple();
    }
    #[cfg(not(target_os = "macos"))]
    None
}

// ---------------------------------------------------------------------------
// Topology + assignment helpers
// ---------------------------------------------------------------------------

pub fn read_gpu_topology() -> Option<serde_json::Value> {
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let topo_path = Path::new(&install_dir).join("config").join("gpu-topology.json");
    if !topo_path.exists() {
        warn!("Topology file not found at {}", topo_path.display());
        return None;
    }
    match std::fs::read_to_string(&topo_path) {
        Ok(text) => serde_json::from_str(&text).ok(),
        Err(e) => {
            warn!("Failed to read topology file: {e}");
            None
        }
    }
}

pub fn decode_gpu_assignment() -> Option<serde_json::Value> {
    use base64::Engine;
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let b64 = {
        let from_file = crate::config::read_env_from_file(
            Path::new(&install_dir),
            "GPU_ASSIGNMENT_JSON_B64",
        );
        if from_file.is_empty() {
            std::env::var("GPU_ASSIGNMENT_JSON_B64").unwrap_or_default()
        } else {
            from_file
        }
    };
    if b64.is_empty() {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    serde_json::from_slice(&decoded).ok()
}

pub fn get_gpu_tier(vram_gb: f64, memory_type: &str) -> String {
    if memory_type == "unified" {
        return if vram_gb >= 90.0 {
            "Strix Halo 90+"
        } else {
            "Strix Halo Compact"
        }
        .to_string();
    }
    if vram_gb >= 80.0 {
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
    .to_string()
}

// ---------------------------------------------------------------------------
// Per-GPU detailed detection
// ---------------------------------------------------------------------------

pub fn get_gpu_info_nvidia_detailed() -> Option<Vec<IndividualGPU>> {
    let (success, output) = run_command(
        &[
            "nvidia-smi",
            "--query-gpu=index,uuid,name,memory.used,memory.total,utilization.gpu,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits",
        ],
        5,
    );
    if !success || output.is_empty() {
        return None;
    }

    let assignment = decode_gpu_assignment();
    let uuid_service_map = if let Some(ref a) = assignment {
        build_uuid_service_map(a)
    } else {
        infer_gpu_services_from_processes()
    };

    let na = ["[N/A]", "[Not Supported]", "N/A", "Not Supported", ""];
    let mut gpus = Vec::new();

    for line in output.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
        if parts.len() < 7 {
            continue;
        }
        let power_w = parts.get(7).and_then(|p| {
            if na.contains(p) { None } else { p.parse::<f64>().ok().map(|v| (v * 10.0).round() / 10.0) }
        });
        let mem_used: i64 = match parts[3].parse() { Ok(v) => v, Err(_) => continue };
        let mem_total: i64 = match parts[4].parse() { Ok(v) => v, Err(_) => continue };
        let uuid = parts[1].to_string();

        gpus.push(IndividualGPU {
            index: parts[0].parse().unwrap_or(0),
            uuid: uuid.clone(),
            name: parts[2].to_string(),
            memory_used_mb: mem_used,
            memory_total_mb: mem_total,
            memory_percent: if mem_total > 0 { (mem_used as f64 / mem_total as f64 * 1000.0).round() / 10.0 } else { 0.0 },
            utilization_percent: parts[5].parse().unwrap_or(0),
            temperature_c: parts[6].parse().unwrap_or(0),
            power_w,
            assigned_services: uuid_service_map.get(&uuid).cloned().unwrap_or_default(),
        });
    }

    if gpus.is_empty() { None } else { Some(gpus) }
}

fn build_uuid_service_map(assignment: &serde_json::Value) -> std::collections::HashMap<String, Vec<String>> {
    let mut result = std::collections::HashMap::new();
    if let Some(services) = assignment
        .get("gpu_assignment")
        .and_then(|a| a.get("services"))
        .and_then(|s| s.as_object())
    {
        for (svc_name, svc_data) in services {
            if let Some(gpu_uuids) = svc_data.get("gpus").and_then(|g| g.as_array()) {
                for uuid_val in gpu_uuids {
                    if let Some(uuid) = uuid_val.as_str() {
                        result
                            .entry(uuid.to_string())
                            .or_insert_with(Vec::new)
                            .push(svc_name.clone());
                    }
                }
            }
        }
    }
    result
}

fn infer_gpu_services_from_processes() -> std::collections::HashMap<String, Vec<String>> {
    let (success, output) = run_command(
        &[
            "nvidia-smi",
            "--query-compute-apps=gpu_uuid,pid,used_memory",
            "--format=csv,noheader,nounits",
        ],
        5,
    );
    if !success || output.is_empty() {
        return std::collections::HashMap::new();
    }

    let mut active: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
        if parts.len() >= 3 {
            let uuid = parts[0].to_string();
            let mem: i64 = parts[2].parse().unwrap_or(0);
            *active.entry(uuid).or_insert(0) += mem;
        }
    }

    active
        .into_iter()
        .filter(|(_, mem)| *mem > 100)
        .map(|(uuid, _)| (uuid, vec!["llama-server".to_string()]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- get_gpu_tier tests --

    #[test]
    fn test_gpu_tier_unified_96gb() {
        assert_eq!(get_gpu_tier(96.0, "unified"), "Strix Halo 90+");
    }

    #[test]
    fn test_gpu_tier_unified_32gb() {
        assert_eq!(get_gpu_tier(32.0, "unified"), "Strix Halo Compact");
    }

    #[test]
    fn test_gpu_tier_discrete_80gb() {
        assert_eq!(get_gpu_tier(80.0, "discrete"), "Professional");
    }

    #[test]
    fn test_gpu_tier_discrete_24gb() {
        assert_eq!(get_gpu_tier(24.0, "discrete"), "Prosumer");
    }

    #[test]
    fn test_gpu_tier_discrete_16gb() {
        assert_eq!(get_gpu_tier(16.0, "discrete"), "Standard");
    }

    #[test]
    fn test_gpu_tier_discrete_8gb() {
        assert_eq!(get_gpu_tier(8.0, "discrete"), "Entry");
    }

    #[test]
    fn test_gpu_tier_discrete_4gb() {
        assert_eq!(get_gpu_tier(4.0, "discrete"), "Minimal");
    }

    // -- build_uuid_service_map tests --

    #[test]
    fn test_build_uuid_service_map_basic() {
        let assignment = serde_json::json!({
            "gpu_assignment": {
                "services": {
                    "llama-server": {
                        "gpus": ["GPU-uuid-1"]
                    },
                    "comfyui": {
                        "gpus": ["GPU-uuid-2"]
                    }
                }
            }
        });
        let map = build_uuid_service_map(&assignment);
        assert_eq!(map.get("GPU-uuid-1").unwrap(), &vec!["llama-server".to_string()]);
        assert_eq!(map.get("GPU-uuid-2").unwrap(), &vec!["comfyui".to_string()]);
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_build_uuid_service_map_shared_gpu() {
        let assignment = serde_json::json!({
            "gpu_assignment": {
                "services": {
                    "llama-server": {
                        "gpus": ["GPU-shared"]
                    },
                    "comfyui": {
                        "gpus": ["GPU-shared"]
                    }
                }
            }
        });
        let map = build_uuid_service_map(&assignment);
        let services = map.get("GPU-shared").unwrap();
        assert_eq!(services.len(), 2);
        assert!(services.contains(&"llama-server".to_string()));
        assert!(services.contains(&"comfyui".to_string()));
    }

    #[test]
    fn test_build_uuid_service_map_empty_assignment() {
        let assignment = serde_json::json!({});
        let map = build_uuid_service_map(&assignment);
        assert!(map.is_empty());
    }
}
