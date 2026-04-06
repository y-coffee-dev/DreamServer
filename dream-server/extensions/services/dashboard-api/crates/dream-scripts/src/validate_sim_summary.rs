//! Simulation summary validator — validates installer simulation output.
//! Mirrors scripts/validate-sim-summary.py.

use anyhow::{Context, Result};
use serde_json::Value;

/// Validation result returned by `validate`.
pub struct ValidationResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Validate a parsed simulation summary. Returns errors and warnings
/// without performing I/O or process::exit — suitable for unit testing.
pub fn validate(summary: &Value) -> ValidationResult {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Validate required top-level fields
    for field in ["platform", "gpu_backend", "tier", "services", "phases"] {
        if summary.get(field).is_none() {
            errors.push(format!("Missing required field: {field}"));
        }
    }

    // Validate services
    if let Some(services) = summary.get("services").and_then(|s| s.as_array()) {
        if services.is_empty() {
            warnings.push("No services in summary".to_string());
        }
        for (i, svc) in services.iter().enumerate() {
            if svc["id"].as_str().is_none() {
                errors.push(format!("services[{i}].id is missing"));
            }
            if svc["status"].as_str().is_none() {
                errors.push(format!("services[{i}].status is missing"));
            }
        }
    }

    // Validate phases
    if let Some(phases) = summary.get("phases").and_then(|p| p.as_array()) {
        let mut prev_phase = 0;
        for (i, phase) in phases.iter().enumerate() {
            let num = phase["phase"].as_u64().unwrap_or(0);
            if num <= prev_phase && i > 0 {
                warnings.push(format!("Phase order issue at index {i}: phase {num} <= previous {prev_phase}"));
            }
            prev_phase = num;

            if phase["status"].as_str().is_none() {
                errors.push(format!("phases[{i}].status is missing"));
            }
        }
    }

    // Validate platform
    if let Some(platform) = summary["platform"].as_str() {
        let valid = ["linux-nvidia", "linux-amd", "macos", "wsl"];
        if !valid.contains(&platform) {
            warnings.push(format!("Unexpected platform: {platform}"));
        }
    }

    ValidationResult { errors, warnings }
}

pub fn run(file: &str) -> Result<()> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("Reading simulation summary: {file}"))?;

    let summary: Value = serde_json::from_str(&text)
        .with_context(|| "Parsing simulation summary JSON")?;

    let result = validate(&summary);

    // Report
    println!("Simulation Summary Validation: {file}");
    if result.errors.is_empty() && result.warnings.is_empty() {
        println!("  PASS - All checks passed");
        return Ok(());
    }

    if !result.warnings.is_empty() {
        println!("\n  Warnings:");
        for w in &result.warnings {
            println!("    - {w}");
        }
    }

    if !result.errors.is_empty() {
        println!("\n  Errors:");
        for e in &result.errors {
            println!("    - {e}");
        }
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_summary() -> Value {
        json!({
            "platform": "linux-nvidia",
            "gpu_backend": "nvidia",
            "tier": "Prosumer",
            "services": [
                {"id": "llama-server", "status": "healthy"},
                {"id": "open-webui", "status": "healthy"}
            ],
            "phases": [
                {"phase": 1, "status": "ok"},
                {"phase": 2, "status": "ok"},
                {"phase": 3, "status": "ok"}
            ]
        })
    }

    #[test]
    fn test_validate_valid_summary_no_errors() {
        let r = validate(&valid_summary());
        assert!(r.errors.is_empty(), "Expected no errors: {:?}", r.errors);
        assert!(r.warnings.is_empty(), "Expected no warnings: {:?}", r.warnings);
    }

    #[test]
    fn test_validate_missing_required_fields() {
        let r = validate(&json!({}));
        assert_eq!(r.errors.len(), 5, "Expected 5 missing-field errors: {:?}", r.errors);
        for field in ["platform", "gpu_backend", "tier", "services", "phases"] {
            assert!(
                r.errors.iter().any(|e| e.contains(field)),
                "Expected error for missing {field}"
            );
        }
    }

    #[test]
    fn test_validate_empty_services_warns() {
        let mut s = valid_summary();
        s["services"] = json!([]);
        let r = validate(&s);
        assert!(r.errors.is_empty());
        assert!(
            r.warnings.iter().any(|w| w.contains("No services")),
            "Expected empty services warning: {:?}", r.warnings
        );
    }

    #[test]
    fn test_validate_service_missing_id_errors() {
        let mut s = valid_summary();
        s["services"] = json!([{"status": "healthy"}]);
        let r = validate(&s);
        assert!(
            r.errors.iter().any(|e| e.contains("services[0].id")),
            "Expected service id error: {:?}", r.errors
        );
    }

    #[test]
    fn test_validate_service_missing_status_errors() {
        let mut s = valid_summary();
        s["services"] = json!([{"id": "test"}]);
        let r = validate(&s);
        assert!(
            r.errors.iter().any(|e| e.contains("services[0].status")),
            "Expected service status error: {:?}", r.errors
        );
    }

    #[test]
    fn test_validate_phase_order_warns() {
        let mut s = valid_summary();
        s["phases"] = json!([
            {"phase": 3, "status": "ok"},
            {"phase": 1, "status": "ok"}
        ]);
        let r = validate(&s);
        assert!(
            r.warnings.iter().any(|w| w.contains("Phase order")),
            "Expected phase order warning: {:?}", r.warnings
        );
    }

    #[test]
    fn test_validate_phase_missing_status_errors() {
        let mut s = valid_summary();
        s["phases"] = json!([{"phase": 1}]);
        let r = validate(&s);
        assert!(
            r.errors.iter().any(|e| e.contains("phases[0].status")),
            "Expected phase status error: {:?}", r.errors
        );
    }

    #[test]
    fn test_validate_unexpected_platform_warns() {
        let mut s = valid_summary();
        s["platform"] = json!("freebsd");
        let r = validate(&s);
        assert!(
            r.warnings.iter().any(|w| w.contains("Unexpected platform")),
            "Expected platform warning: {:?}", r.warnings
        );
    }

    #[test]
    fn test_validate_all_valid_platforms() {
        for platform in ["linux-nvidia", "linux-amd", "macos", "wsl"] {
            let mut s = valid_summary();
            s["platform"] = json!(platform);
            let r = validate(&s);
            assert!(
                !r.warnings.iter().any(|w| w.contains("platform")),
                "Platform {platform} should not warn"
            );
        }
    }
}
