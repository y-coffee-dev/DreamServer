//! Model configuration validator — validates backend config JSON files.
//! Mirrors scripts/validate-models.py.

use anyhow::{Context, Result};
use serde_json::Value;

pub fn run(config_path: Option<&str>) -> Result<()> {
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());

    let configs = if let Some(path) = config_path {
        vec![path.to_string()]
    } else {
        // Default: validate all backend configs
        let backends_dir = format!("{install_dir}/config/backends");
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&backends_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map_or(false, |e| e == "json") {
                    files.push(p.to_string_lossy().to_string());
                }
            }
        }
        files.sort();
        files
    };

    if configs.is_empty() {
        println!("No backend config files found to validate");
        return Ok(());
    }

    let mut all_valid = true;

    for path in &configs {
        print!("Validating {path} ... ");
        match validate_config(path) {
            Ok(warnings) => {
                if warnings.is_empty() {
                    println!("OK");
                } else {
                    println!("OK (with warnings)");
                    for w in &warnings {
                        println!("  - {w}");
                    }
                }
            }
            Err(e) => {
                println!("FAIL: {e}");
                all_valid = false;
            }
        }
    }

    if !all_valid {
        std::process::exit(1);
    }
    println!("\nAll model configurations valid.");
    Ok(())
}

fn validate_config(path: &str) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Reading {path}"))?;
    let config: Value = serde_json::from_str(&text)
        .with_context(|| "Parsing JSON")?;

    let mut warnings = Vec::new();

    // Must be an object
    if !config.is_object() {
        anyhow::bail!("Root must be a JSON object");
    }

    // Check for required tier entries
    if let Some(obj) = config.as_object() {
        for (tier, tier_config) in obj {
            if !tier_config.is_object() {
                anyhow::bail!("Tier '{tier}' must be an object");
            }
            if tier_config.get("model").is_none() && tier_config.get("models").is_none() {
                warnings.push(format!("Tier '{tier}' has no 'model' or 'models' field"));
            }
        }
    }

    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_validate_valid_config() {
        let f = write_temp_file(r#"{"entry": {"model": "qwen2.5-7b"}, "standard": {"model": "llama3-8b"}}"#);
        let warnings = validate_config(f.path().to_str().unwrap()).unwrap();
        assert!(warnings.is_empty(), "expected no warnings, got: {warnings:?}");
    }

    #[test]
    fn test_validate_config_missing_model() {
        let f = write_temp_file(r#"{"entry": {}}"#);
        let warnings = validate_config(f.path().to_str().unwrap()).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no 'model' or 'models' field"));
    }

    #[test]
    fn test_validate_config_not_object() {
        let f = write_temp_file(r#"[1,2,3]"#);
        let err = validate_config(f.path().to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("Root must be a JSON object"));
    }

    #[test]
    fn test_validate_config_tier_not_object() {
        let f = write_temp_file(r#"{"entry": "string"}"#);
        let err = validate_config(f.path().to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("Tier 'entry' must be an object"));
    }

    #[test]
    fn test_validate_config_invalid_json() {
        let f = write_temp_file("not json");
        let result = validate_config(f.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_run_valid_config() {
        let f = write_temp_file(r#"{"entry": {"model": "qwen2.5-7b"}}"#);
        let result = run(Some(f.path().to_str().unwrap()));
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_no_configs_found() {
        let tmp = tempfile::tempdir().unwrap();
        // Point DREAM_INSTALL_DIR to a temp dir with no config/backends/ subdir
        std::env::set_var("DREAM_INSTALL_DIR", tmp.path().to_str().unwrap());
        let result = run(None);
        assert!(result.is_ok());
        // Clean up to avoid polluting other tests
        std::env::remove_var("DREAM_INSTALL_DIR");
    }
}
