//! Extension manifest auditor — validates all extension manifests for consistency.
//! Mirrors scripts/audit-extensions.py.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn run(dir: Option<&str>) -> Result<()> {
    let extensions_dir = dir
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let install = std::env::var("DREAM_INSTALL_DIR")
                .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());
            PathBuf::from(&install).join("extensions").join("services")
        });

    println!("Auditing extensions in: {}", extensions_dir.display());

    if !extensions_dir.exists() {
        anyhow::bail!("Extensions directory not found: {}", extensions_dir.display());
    }

    let mut issues: Vec<String> = Vec::new();
    let mut total = 0u32;
    let mut valid = 0u32;

    let mut entries: Vec<_> = std::fs::read_dir(&extensions_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in &entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let ext_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let mut manifest_path = None;
        for name in ["manifest.yaml", "manifest.yml", "manifest.json"] {
            let candidate = path.join(name);
            if candidate.exists() {
                manifest_path = Some(candidate);
                break;
            }
        }

        let manifest_path = match manifest_path {
            Some(p) => p,
            None => {
                issues.push(format!("{ext_name}: no manifest file found"));
                total += 1;
                continue;
            }
        };

        total += 1;
        match audit_manifest(&manifest_path, &ext_name) {
            Ok(warnings) => {
                if warnings.is_empty() {
                    valid += 1;
                } else {
                    for w in warnings {
                        issues.push(format!("{ext_name}: {w}"));
                    }
                    valid += 1; // warnings are non-fatal
                }
            }
            Err(e) => {
                issues.push(format!("{ext_name}: ERROR - {e}"));
            }
        }
    }

    println!("\nResults: {valid}/{total} valid");
    if !issues.is_empty() {
        println!("\nIssues found:");
        for issue in &issues {
            println!("  - {issue}");
        }
        if issues.iter().any(|i| i.contains("ERROR")) {
            std::process::exit(1);
        }
    } else {
        println!("All manifests valid!");
    }

    Ok(())
}

// Made visible for unit testing
fn audit_manifest(path: &Path, ext_name: &str) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    let manifest: serde_json::Value = if path.extension().map_or(false, |e| e == "json") {
        serde_json::from_str(&text)?
    } else {
        serde_yaml::from_str(&text)?
    };

    let mut warnings = Vec::new();

    // Check schema_version
    if manifest["schema_version"].as_str() != Some("dream.services.v1") {
        anyhow::bail!("missing or invalid schema_version");
    }

    // Check service block
    if let Some(service) = manifest.get("service") {
        if service["id"].as_str().is_none() {
            anyhow::bail!("service.id is required");
        }
        if service["name"].as_str().is_none() {
            warnings.push("service.name is missing".to_string());
        }
        if service["port"].as_u64().is_none() {
            warnings.push("service.port is missing".to_string());
        }
        if service["health"].as_str().is_none() {
            warnings.push("service.health endpoint not specified (defaults to /health)".to_string());
        }

        // Check that service.id matches directory name
        if let Some(id) = service["id"].as_str() {
            if id != ext_name {
                warnings.push(format!("service.id '{id}' does not match directory name '{ext_name}'"));
            }
        }
    }

    // Check features
    if let Some(features) = manifest.get("features").and_then(|f| f.as_array()) {
        for (i, feat) in features.iter().enumerate() {
            if feat["id"].as_str().is_none() {
                warnings.push(format!("features[{i}].id is missing"));
            }
            if feat["name"].as_str().is_none() {
                warnings.push(format!("features[{i}].name is missing"));
            }
            for field in ["description", "icon", "category"] {
                if feat.get(field).is_none() {
                    warnings.push(format!(
                        "features[{i}] missing optional field: {field}"
                    ));
                }
            }
        }
    }

    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_audit_manifest_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = r#"
schema_version: dream.services.v1
service:
  id: my-ext
  name: My Extension
  port: 8080
  health: /health
"#;
        let path = tmp.path().join("manifest.yaml");
        fs::write(&path, manifest).unwrap();

        let warnings = audit_manifest(&path, "my-ext").unwrap();
        assert!(warnings.is_empty(), "Expected no warnings, got: {warnings:?}");
    }

    #[test]
    fn test_audit_manifest_missing_schema_version() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = r#"
service:
  id: test
  name: Test
"#;
        let path = tmp.path().join("manifest.yaml");
        fs::write(&path, manifest).unwrap();

        let err = audit_manifest(&path, "test").unwrap_err();
        assert!(err.to_string().contains("schema_version"));
    }

    #[test]
    fn test_audit_manifest_missing_service_id() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = r#"
schema_version: dream.services.v1
service:
  name: Test
"#;
        let path = tmp.path().join("manifest.yaml");
        fs::write(&path, manifest).unwrap();

        let err = audit_manifest(&path, "test").unwrap_err();
        assert!(err.to_string().contains("service.id"));
    }

    #[test]
    fn test_audit_manifest_id_mismatch_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = r#"
schema_version: dream.services.v1
service:
  id: wrong-name
  name: My Service
  port: 8080
  health: /health
"#;
        let path = tmp.path().join("manifest.yaml");
        fs::write(&path, manifest).unwrap();

        let warnings = audit_manifest(&path, "my-ext").unwrap();
        assert!(
            warnings.iter().any(|w| w.contains("does not match")),
            "Expected id mismatch warning, got: {warnings:?}"
        );
    }

    #[test]
    fn test_audit_manifest_missing_optional_fields_warns() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = r#"
schema_version: dream.services.v1
service:
  id: test
"#;
        let path = tmp.path().join("manifest.yaml");
        fs::write(&path, manifest).unwrap();

        let warnings = audit_manifest(&path, "test").unwrap();
        assert!(
            warnings.iter().any(|w| w.contains("name is missing")),
            "Expected missing name warning, got: {warnings:?}"
        );
    }

    #[test]
    fn test_audit_manifest_json_format() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = r#"{"schema_version": "dream.services.v1", "service": {"id": "test", "name": "Test", "port": 3000, "health": "/health"}}"#;
        let path = tmp.path().join("manifest.json");
        fs::write(&path, manifest).unwrap();

        let warnings = audit_manifest(&path, "test").unwrap();
        assert!(warnings.is_empty(), "Expected no warnings for valid JSON, got: {warnings:?}");
    }

    #[test]
    fn test_audit_manifest_feature_missing_id_warns() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = r#"
schema_version: dream.services.v1
service:
  id: test
  name: Test
  port: 8080
  health: /health
features:
  - name: Chat
"#;
        let path = tmp.path().join("manifest.yaml");
        fs::write(&path, manifest).unwrap();

        let warnings = audit_manifest(&path, "test").unwrap();
        assert!(
            warnings.iter().any(|w| w.contains("features[0].id")),
            "Expected feature id warning, got: {warnings:?}"
        );
    }
}
