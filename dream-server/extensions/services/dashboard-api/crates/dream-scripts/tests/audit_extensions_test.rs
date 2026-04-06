use std::fs;
use tempfile::TempDir;

/// A directory with a valid extension manifest should pass cleanly.
#[test]
fn test_valid_extension() {
    let tmp = TempDir::new().unwrap();
    let ext_dir = tmp.path().join("my-ext");
    fs::create_dir_all(&ext_dir).unwrap();
    fs::write(
        ext_dir.join("manifest.yaml"),
        r#"schema_version: "dream.services.v1"
service:
  id: my-ext
  name: My Extension
  port: 9999
  health: /health
"#,
    )
    .unwrap();

    let result = dream_scripts::audit_extensions::run(Some(tmp.path().to_str().unwrap()));
    assert!(result.is_ok(), "Expected Ok for valid extension, got: {result:?}");
}

/// Nonexistent directory should return Err (bail!).
#[test]
fn test_nonexistent_dir() {
    let result =
        dream_scripts::audit_extensions::run(Some("/tmp/does_not_exist_audit_ext_12345"));
    assert!(result.is_err(), "Expected Err for nonexistent directory");
}

/// An empty directory (no extension subdirectories) should pass — nothing to audit.
#[test]
fn test_empty_dir() {
    let tmp = TempDir::new().unwrap();

    let result = dream_scripts::audit_extensions::run(Some(tmp.path().to_str().unwrap()));
    assert!(result.is_ok(), "Expected Ok for empty directory, got: {result:?}");
}

/// A subdirectory without a manifest file is reported as an issue (non-ERROR),
/// so run() still returns Ok(()).
///
/// NOTE: If the manifest exists but has an invalid schema_version or is
/// missing service.id, audit_manifest returns Err which becomes an "ERROR"
/// issue. That triggers `std::process::exit(1)` inside `run()`, which
/// cannot be caught in a test. We therefore only test the missing-manifest
/// path (non-fatal issue) and valid paths here.
#[test]
fn test_missing_manifest() {
    let tmp = TempDir::new().unwrap();
    let ext_dir = tmp.path().join("my-ext");
    fs::create_dir_all(&ext_dir).unwrap();
    // No manifest.yaml inside my-ext/

    let result = dream_scripts::audit_extensions::run(Some(tmp.path().to_str().unwrap()));
    assert!(
        result.is_ok(),
        "Expected Ok for missing manifest (non-fatal issue), got: {result:?}"
    );
}
