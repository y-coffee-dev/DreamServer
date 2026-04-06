use std::io::Write;
use tempfile::NamedTempFile;

/// Valid summary with all required fields and properly structured services/phases.
#[test]
fn test_valid_summary() {
    let summary = serde_json::json!({
        "platform": "linux-nvidia",
        "gpu_backend": "nvidia",
        "tier": "high",
        "services": [
            { "id": "llama-server", "status": "running" },
            { "id": "open-webui", "status": "running" }
        ],
        "phases": [
            { "phase": 1, "status": "complete" },
            { "phase": 2, "status": "complete" }
        ]
    });

    let mut f = NamedTempFile::new().unwrap();
    write!(f, "{}", summary).unwrap();
    let path = f.path().to_str().unwrap().to_string();

    let result = dream_scripts::validate_sim_summary::run(&path);
    assert!(result.is_ok(), "Expected Ok for valid summary, got: {result:?}");
}

/// Nonexistent file path should return Err (file I/O failure).
#[test]
fn test_missing_file() {
    let result = dream_scripts::validate_sim_summary::run("/tmp/does_not_exist_sim_summary_12345.json");
    assert!(result.is_err(), "Expected Err for missing file");
}

/// Malformed JSON should return Err (parse failure).
#[test]
fn test_invalid_json() {
    let mut f = NamedTempFile::new().unwrap();
    write!(f, "not json").unwrap();
    let path = f.path().to_str().unwrap().to_string();

    let result = dream_scripts::validate_sim_summary::run(&path);
    assert!(result.is_err(), "Expected Err for invalid JSON");
}

/// Empty services array produces a warning but not an error, so run()
/// returns Ok(()).
///
/// NOTE: Summaries with validation *errors* (e.g. missing required fields)
/// trigger `std::process::exit(1)` inside `run()`, which cannot be caught
/// in a test. We therefore only test the warning-only and fully-valid paths.
#[test]
fn test_empty_services() {
    let summary = serde_json::json!({
        "platform": "linux-nvidia",
        "gpu_backend": "nvidia",
        "tier": "high",
        "services": [],
        "phases": [
            { "phase": 1, "status": "complete" }
        ]
    });

    let mut f = NamedTempFile::new().unwrap();
    write!(f, "{}", summary).unwrap();
    let path = f.path().to_str().unwrap().to_string();

    let result = dream_scripts::validate_sim_summary::run(&path);
    assert!(result.is_ok(), "Expected Ok for empty services (warning only), got: {result:?}");
}
