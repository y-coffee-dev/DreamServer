//! Configuration loading: environment variables, manifest discovery, and
//! service registry initialization. Mirrors `config.py`.

use anyhow::{Context, Result};
use dream_common::manifest::{ExtensionManifest, FeatureDefinition, ServiceConfig};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Environment-derived paths matching the Python constants.
pub struct EnvConfig {
    pub install_dir: PathBuf,
    pub data_dir: PathBuf,
    pub extensions_dir: PathBuf,
    pub gpu_backend: String,
    pub default_service_host: String,
    pub llm_backend: String,
    pub dashboard_api_port: u16,
}

impl EnvConfig {
    pub fn from_env() -> Self {
        let install_dir = PathBuf::from(
            std::env::var("DREAM_INSTALL_DIR")
                .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string()),
        );
        let data_dir = PathBuf::from(
            std::env::var("DREAM_DATA_DIR")
                .unwrap_or_else(|_| shellexpand::tilde("~/.dream-server").to_string()),
        );
        let extensions_dir = PathBuf::from(
            std::env::var("DREAM_EXTENSIONS_DIR").unwrap_or_else(|_| {
                install_dir
                    .join("extensions")
                    .join("services")
                    .to_string_lossy()
                    .to_string()
            }),
        );
        let gpu_backend =
            std::env::var("GPU_BACKEND").unwrap_or_else(|_| "nvidia".to_string());
        let default_service_host =
            std::env::var("SERVICE_HOST").unwrap_or_else(|_| "host.docker.internal".to_string());
        let llm_backend = std::env::var("LLM_BACKEND").unwrap_or_default();
        let dashboard_api_port: u16 = std::env::var("DASHBOARD_API_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3002);

        Self {
            install_dir,
            data_dir,
            extensions_dir,
            gpu_backend,
            default_service_host,
            llm_backend,
            dashboard_api_port,
        }
    }
}

/// Read a variable from the .env file when not in process environment.
pub fn read_env_from_file(install_dir: &Path, key: &str) -> String {
    let env_path = install_dir.join(".env");
    let prefix = format!("{key}=");
    match std::fs::read_to_string(&env_path) {
        Ok(text) => {
            for line in text.lines() {
                if let Some(val) = line.strip_prefix(&prefix) {
                    return val.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
                }
            }
            String::new()
        }
        Err(_) => String::new(),
    }
}

/// Load all extension manifests, returning (services, features, errors).
pub fn load_extension_manifests(
    manifest_dir: &Path,
    gpu_backend: &str,
    install_dir: &Path,
) -> (
    HashMap<String, ServiceConfig>,
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
) {
    let mut services = HashMap::new();
    let mut features: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut loaded = 0u32;

    if !manifest_dir.exists() {
        info!("Extension manifest directory not found: {}", manifest_dir.display());
        return (services, features, errors);
    }

    let mut manifest_files: Vec<PathBuf> = Vec::new();
    let mut entries: Vec<_> = match std::fs::read_dir(manifest_dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return (services, features, errors),
    };
    entries.sort_by_key(|e| e.file_name());

    for entry in &entries {
        let path = entry.path();
        if path.is_dir() {
            for name in ["manifest.yaml", "manifest.yml", "manifest.json"] {
                let candidate = path.join(name);
                if candidate.exists() {
                    manifest_files.push(candidate);
                    break;
                }
            }
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yaml" | "yml" | "json")
        ) {
            manifest_files.push(path);
        }
    }

    for path in &manifest_files {
        if let Err(e) = process_manifest(
            path,
            gpu_backend,
            install_dir,
            &mut services,
            &mut features,
            &mut loaded,
        ) {
            warn!("Failed loading manifest {}: {e}", path.display());
            errors.push(json!({"file": path.to_string_lossy(), "error": e.to_string()}));
        }
    }

    info!(
        "Loaded {} extension manifests ({} services, {} features)",
        loaded,
        services.len(),
        features.len()
    );
    (services, features, errors)
}

fn process_manifest(
    path: &Path,
    gpu_backend: &str,
    install_dir: &Path,
    services: &mut HashMap<String, ServiceConfig>,
    features: &mut Vec<serde_json::Value>,
    loaded: &mut u32,
) -> Result<()> {
    let ext_dir = path
        .parent()
        .context("manifest has no parent directory")?;

    // Skip disabled extensions
    if ext_dir.join("compose.yaml.disabled").exists()
        || ext_dir.join("compose.yml.disabled").exists()
    {
        return Ok(());
    }

    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;

    let manifest: ExtensionManifest = if path.extension().map_or(false, |e| e == "json") {
        serde_json::from_str(&text)?
    } else {
        serde_yaml::from_str(&text)?
    };

    if manifest.schema_version.as_deref() != Some("dream.services.v1") {
        anyhow::bail!("Unsupported schema_version");
    }

    // Process service
    if let Some(svc) = &manifest.service {
        if let Some(service_id) = &svc.id {
            let supported = &svc.gpu_backends;

            // Platform filtering
            if gpu_backend == "apple" {
                if svc.service_type.as_deref() == Some("host-systemd") {
                    return Ok(());
                }
            } else if !supported.contains(&gpu_backend.to_string())
                && !supported.contains(&"all".to_string())
            {
                return Ok(());
            }

            let default_host = svc.default_host.as_deref().unwrap_or("localhost");
            let host = svc
                .host_env
                .as_deref()
                .and_then(|env_key| std::env::var(env_key).ok())
                .unwrap_or_else(|| default_host.to_string());

            let port = svc.port.unwrap_or(0);
            let ext_port_default = svc.external_port_default.unwrap_or(port);
            let external_port = if let Some(env_key) = &svc.external_port_env {
                let val = std::env::var(env_key)
                    .ok()
                    .filter(|v| !v.is_empty())
                    .or_else(|| {
                        let v = read_env_from_file(install_dir, env_key);
                        if v.is_empty() { None } else { Some(v) }
                    });
                val.and_then(|v| v.parse().ok()).unwrap_or(ext_port_default)
            } else {
                ext_port_default
            };

            services.insert(
                service_id.clone(),
                ServiceConfig {
                    host,
                    port,
                    external_port,
                    health: svc.health.clone().unwrap_or_else(|| "/health".to_string()),
                    name: svc.name.clone().unwrap_or_else(|| service_id.clone()),
                    ui_path: svc.ui_path.clone().unwrap_or_else(|| "/".to_string()),
                    service_type: svc.service_type.clone(),
                    health_port: svc.health_port,
                },
            );
        }
    }

    // Process features
    for feature in &manifest.features {
        process_feature(feature, gpu_backend, features);
    }

    *loaded += 1;
    Ok(())
}

fn process_feature(
    feature: &FeatureDefinition,
    gpu_backend: &str,
    features: &mut Vec<serde_json::Value>,
) {
    let supported = &feature.gpu_backends;
    if gpu_backend != "apple"
        && !supported.contains(&gpu_backend.to_string())
        && !supported.contains(&"all".to_string())
    {
        return;
    }
    if feature.id.is_some() && feature.name.is_some() {
        if let Ok(val) = serde_json::to_value(feature) {
            features.push(val);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_env_from_file_key_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "MY_KEY=hello_world\nOTHER=123\n").unwrap();
        assert_eq!(read_env_from_file(dir.path(), "MY_KEY"), "hello_world");
    }

    #[test]
    fn test_read_env_from_file_key_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "OTHER_KEY=value\n").unwrap();
        assert_eq!(read_env_from_file(dir.path(), "MY_KEY"), "");
    }

    #[test]
    fn test_read_env_from_file_strips_quotes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "MY_KEY=\"quoted\"\n").unwrap();
        assert_eq!(read_env_from_file(dir.path(), "MY_KEY"), "quoted");
    }

    #[test]
    fn test_read_env_from_file_missing_env_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_env_from_file(dir.path(), "MY_KEY"), "");
    }

    // ---- load_extension_manifests / process_manifest tests ----

    /// Helper: write a manifest.yaml inside a subdirectory of the given parent dir.
    fn write_manifest(parent: &Path, ext_name: &str, yaml: &str) -> PathBuf {
        let ext_dir = parent.join(ext_name);
        std::fs::create_dir_all(&ext_dir).unwrap();
        let manifest_path = ext_dir.join("manifest.yaml");
        std::fs::write(&manifest_path, yaml).unwrap();
        manifest_path
    }

    #[test]
    fn test_load_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let (services, features, errors) =
            load_extension_manifests(dir.path(), "nvidia", install_dir.path());
        assert!(services.is_empty());
        assert!(features.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_load_nonexistent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nonexistent = dir.path().join("does-not-exist");
        let install_dir = tempfile::tempdir().unwrap();
        let (services, features, errors) =
            load_extension_manifests(&nonexistent, "nvidia", install_dir.path());
        assert!(services.is_empty());
        assert!(features.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_valid_manifest_with_service() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let yaml = r#"
schema_version: dream.services.v1
service:
  id: test-svc
  name: Test Service
  port: 9999
  health: /health
  container: test-svc
  gpu_backends: [nvidia, amd]
"#;
        write_manifest(dir.path(), "test-ext", yaml);

        let (services, features, errors) =
            load_extension_manifests(dir.path(), "nvidia", install_dir.path());
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(features.is_empty());
        assert_eq!(services.len(), 1);

        let svc = services.get("test-svc").expect("service not found");
        assert_eq!(svc.name, "Test Service");
        assert_eq!(svc.port, 9999);
        assert_eq!(svc.health, "/health");
    }

    #[test]
    fn test_valid_manifest_with_feature() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let yaml = r#"
schema_version: dream.services.v1
features:
  - id: test-feature
    name: Test Feature
    description: A test feature
    gpu_backends: [nvidia, amd]
"#;
        write_manifest(dir.path(), "feat-ext", yaml);

        let (services, features, errors) =
            load_extension_manifests(dir.path(), "nvidia", install_dir.path());
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(services.is_empty());
        assert_eq!(features.len(), 1);

        let feat = &features[0];
        assert_eq!(feat["id"], "test-feature");
        assert_eq!(feat["name"], "Test Feature");
        assert_eq!(feat["description"], "A test feature");
    }

    #[test]
    fn test_invalid_yaml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let ext_dir = dir.path().join("bad-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(ext_dir.join("manifest.yaml"), "not: [valid: yaml: {{").unwrap();

        let (services, features, errors) =
            load_extension_manifests(dir.path(), "nvidia", install_dir.path());
        assert!(services.is_empty());
        assert!(features.is_empty());
        assert_eq!(errors.len(), 1, "expected exactly one error");
        assert!(errors[0]["error"].as_str().unwrap().len() > 0);
    }

    #[test]
    fn test_wrong_schema_version_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let yaml = r#"
schema_version: dream.services.v99
service:
  id: bad-schema
  name: Bad Schema
  port: 1234
  gpu_backends: [nvidia]
"#;
        write_manifest(dir.path(), "bad-schema-ext", yaml);

        let (services, features, errors) =
            load_extension_manifests(dir.path(), "nvidia", install_dir.path());
        assert!(services.is_empty());
        assert!(features.is_empty());
        assert_eq!(errors.len(), 1);
        let err_msg = errors[0]["error"].as_str().unwrap();
        assert!(
            err_msg.contains("schema_version"),
            "error should mention schema_version, got: {err_msg}"
        );
    }

    #[test]
    fn test_disabled_extension_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let yaml = r#"
schema_version: dream.services.v1
service:
  id: disabled-svc
  name: Disabled Service
  port: 7777
  gpu_backends: [nvidia, amd]
"#;
        let manifest_path = write_manifest(dir.path(), "disabled-ext", yaml);
        // Create the disabled marker file alongside the manifest
        let ext_dir = manifest_path.parent().unwrap();
        std::fs::write(ext_dir.join("compose.yaml.disabled"), "").unwrap();

        let (services, features, errors) =
            load_extension_manifests(dir.path(), "nvidia", install_dir.path());
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(services.is_empty(), "disabled extension should be skipped");
        assert!(features.is_empty());
    }

    #[test]
    fn test_gpu_backend_filtering_service_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let yaml = r#"
schema_version: dream.services.v1
service:
  id: nvidia-only-svc
  name: NVIDIA Only Service
  port: 5555
  gpu_backends: [nvidia]
"#;
        write_manifest(dir.path(), "nvidia-ext", yaml);

        // Load with "amd" backend -- should skip the nvidia-only service
        let (services, features, errors) =
            load_extension_manifests(dir.path(), "amd", install_dir.path());
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(
            services.is_empty(),
            "nvidia-only service should be skipped on amd backend"
        );
        assert!(features.is_empty());
    }

    #[test]
    fn test_feature_gpu_backend_filtering() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let yaml = r#"
schema_version: dream.services.v1
features:
  - id: nvidia-feat
    name: NVIDIA Feature
    description: Only on NVIDIA
    gpu_backends: [nvidia]
  - id: amd-feat
    name: AMD Feature
    description: Only on AMD
    gpu_backends: [amd]
"#;
        write_manifest(dir.path(), "multi-feat-ext", yaml);

        // Load with "amd" backend -- only the amd feature should appear
        let (services, features, errors) =
            load_extension_manifests(dir.path(), "amd", install_dir.path());
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(services.is_empty());
        assert_eq!(features.len(), 1, "only the amd feature should be loaded");
        assert_eq!(features[0]["id"], "amd-feat");
    }

    #[test]
    fn test_process_feature_with_id_and_name() {
        let dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let yaml = r#"
schema_version: dream.services.v1
features:
  - id: my-feature
    name: My Feature
    description: Fully specified feature
    gpu_backends: [nvidia, amd]
"#;
        write_manifest(dir.path(), "feat-ext", yaml);

        let (_, features, errors) =
            load_extension_manifests(dir.path(), "nvidia", install_dir.path());
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(features.len(), 1);

        let feat = &features[0];
        assert_eq!(feat["id"], "my-feature");
        assert_eq!(feat["name"], "My Feature");
        assert_eq!(feat["description"], "Fully specified feature");
    }
}
