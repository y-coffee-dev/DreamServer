//! Health check script — queries all service health endpoints and reports status.
//! Mirrors scripts/healthcheck.py.

use anyhow::Result;
use dream_common::manifest::ServiceConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub async fn run(format: &str) -> Result<()> {
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
    let extensions_dir = PathBuf::from(&install_dir).join("extensions").join("services");
    let gpu_backend = std::env::var("GPU_BACKEND").unwrap_or_else(|_| "nvidia".to_string());

    // Load service manifests
    let (services, _, errors) = dashboard_api_config_loader(&extensions_dir, &gpu_backend, &install_dir);

    if !errors.is_empty() {
        eprintln!("Warning: {} manifest(s) failed to load", errors.len());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let mut results: Vec<(String, String, Option<f64>)> = Vec::new();

    for (sid, cfg) in &services {
        let health_port = cfg.health_port.unwrap_or(cfg.port);
        let url = format!("http://{}:{}{}", cfg.host, health_port, cfg.health);
        let start = Instant::now();

        let (status, response_time) = match client.get(&url).send().await {
            Ok(resp) => {
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let s = if resp.status().as_u16() < 400 { "healthy" } else { "unhealthy" };
                (s.to_string(), Some(elapsed))
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("dns error") || msg.contains("Name or service not known") {
                    ("not_deployed".to_string(), None)
                } else {
                    ("down".to_string(), None)
                }
            }
        };

        results.push((sid.clone(), status, response_time));
    }

    // Output
    match format {
        "json" => {
            let json_results: Vec<serde_json::Value> = results
                .iter()
                .map(|(id, status, rt)| {
                    serde_json::json!({
                        "id": id,
                        "status": status,
                        "response_time_ms": rt,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_results)?);
        }
        _ => {
            let healthy = results.iter().filter(|(_, s, _)| s == "healthy").count();
            let total = results.len();
            println!("Service Health Check: {healthy}/{total} healthy\n");
            for (id, status, rt) in &results {
                let icon = match status.as_str() {
                    "healthy" => "OK",
                    "unhealthy" => "WARN",
                    "not_deployed" => "SKIP",
                    _ => "FAIL",
                };
                let rt_str = rt.map(|r| format!(" ({r:.0}ms)")).unwrap_or_default();
                println!("  [{icon}] {id}: {status}{rt_str}");
            }
        }
    }

    let all_ok = results.iter().all(|(_, s, _)| s == "healthy" || s == "not_deployed");
    if !all_ok {
        std::process::exit(1);
    }
    Ok(())
}

// Simplified manifest loader for scripts (reuses dream-common types)
fn dashboard_api_config_loader(
    extensions_dir: &std::path::Path,
    _gpu_backend: &str,
    _install_dir: &str,
) -> (HashMap<String, ServiceConfig>, Vec<serde_json::Value>, Vec<serde_json::Value>) {
    // Delegate to the same logic used by dashboard-api config module
    // For scripts, we inline a simplified version
    let mut services = HashMap::new();
    let features = Vec::new();
    let mut errors = Vec::new();

    if !extensions_dir.exists() {
        return (services, features, errors);
    }

    let mut entries: Vec<_> = std::fs::read_dir(extensions_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in &entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        for name in ["manifest.yaml", "manifest.yml", "manifest.json"] {
            let manifest_path = path.join(name);
            if manifest_path.exists() {
                match std::fs::read_to_string(&manifest_path) {
                    Ok(text) => {
                        let manifest: Result<dream_common::manifest::ExtensionManifest, _> =
                            serde_yaml::from_str(&text);
                        if let Ok(m) = manifest {
                            if m.schema_version.as_deref() != Some("dream.services.v1") {
                                continue;
                            }
                            if let Some(svc) = &m.service {
                                if let Some(id) = &svc.id {
                                    let host = svc.default_host.as_deref().unwrap_or("localhost").to_string();
                                    let port = svc.port.unwrap_or(0);
                                    services.insert(id.clone(), ServiceConfig {
                                        host,
                                        port,
                                        external_port: svc.external_port_default.unwrap_or(port),
                                        health: svc.health.clone().unwrap_or_else(|| "/health".to_string()),
                                        name: svc.name.clone().unwrap_or_else(|| id.clone()),
                                        ui_path: svc.ui_path.clone().unwrap_or_else(|| "/".to_string()),
                                        service_type: svc.service_type.clone(),
                                        health_port: svc.health_port,
                                    });
                                }
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(serde_json::json!({"file": manifest_path.to_string_lossy(), "error": e.to_string()}));
                    }
                }
                break;
            }
        }
    }

    (services, features, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_config_loader_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (services, _, errors) =
            dashboard_api_config_loader(tmp.path(), "nvidia", "/tmp");
        assert!(services.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_config_loader_valid_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let ext_dir = tmp.path().join("my-ext");
        std::fs::create_dir(&ext_dir).unwrap();

        let manifest = r#"
schema_version: "dream.services.v1"
service:
  id: my-ext
  name: My Extension
  port: 9999
  health: /health
"#;
        let manifest_path = ext_dir.join("manifest.yaml");
        let mut f = std::fs::File::create(&manifest_path).unwrap();
        f.write_all(manifest.as_bytes()).unwrap();

        let (services, _, errors) =
            dashboard_api_config_loader(tmp.path(), "nvidia", "/tmp");
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(services.contains_key("my-ext"), "expected 'my-ext' in services");

        let cfg = &services["my-ext"];
        assert_eq!(cfg.port, 9999);
        assert_eq!(cfg.health, "/health");
        assert_eq!(cfg.name, "My Extension");
    }

    #[test]
    fn test_config_loader_nonexistent_dir() {
        let path = std::path::Path::new("/nonexistent/extensions/dir");
        let (services, _, errors) =
            dashboard_api_config_loader(path, "nvidia", "/tmp");
        assert!(services.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_config_loader_invalid_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        let ext_dir = tmp.path().join("bad-ext");
        std::fs::create_dir(&ext_dir).unwrap();

        let manifest_path = ext_dir.join("manifest.yaml");
        std::fs::write(&manifest_path, "not: [valid: yaml: {{").unwrap();

        let (services, _, _errors) =
            dashboard_api_config_loader(tmp.path(), "nvidia", "/tmp");
        // Invalid YAML fails serde_yaml::from_str, so no service is added.
        // The error may or may not be captured depending on whether read_to_string
        // succeeds but deserialization fails (it won't push to errors in that case,
        // since the `if let Ok(m)` silently skips parse failures).
        assert!(services.is_empty());
    }

    #[test]
    fn test_config_loader_wrong_schema_version() {
        let tmp = tempfile::tempdir().unwrap();
        let ext_dir = tmp.path().join("wrong-ver");
        std::fs::create_dir(&ext_dir).unwrap();

        let manifest = r#"
schema_version: "wrong"
service:
  id: wrong-ver
  name: Wrong Version
  port: 1234
  health: /health
"#;
        let manifest_path = ext_dir.join("manifest.yaml");
        std::fs::write(&manifest_path, manifest).unwrap();

        let (services, _, errors) =
            dashboard_api_config_loader(tmp.path(), "nvidia", "/tmp");
        assert!(errors.is_empty());
        assert!(services.is_empty(), "wrong schema_version should be skipped");
    }
}
