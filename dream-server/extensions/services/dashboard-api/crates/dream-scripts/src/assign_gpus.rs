//! GPU assignment script — assigns GPUs to services based on topology.
//! Mirrors scripts/assign_gpus.py.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

pub fn run(topology_path: Option<&str>, dry_run: bool) -> Result<()> {
    let install_dir = std::env::var("DREAM_INSTALL_DIR")
        .unwrap_or_else(|_| shellexpand::tilde("~/dream-server").to_string());

    // Load topology
    let topo_path = topology_path
        .map(|p| p.to_string())
        .unwrap_or_else(|| format!("{install_dir}/config/gpu-topology.json"));

    let topo: Value = serde_json::from_str(
        &std::fs::read_to_string(&topo_path)
            .with_context(|| format!("Reading topology file: {topo_path}"))?,
    )?;

    let gpus = topo["gpus"]
        .as_array()
        .context("topology.gpus must be an array")?;

    println!("Found {} GPU(s) in topology", gpus.len());

    // Build assignment
    let mut assignment = json!({
        "gpu_assignment": {
            "strategy": "auto",
            "services": {},
        }
    });

    // Primary service (llama-server) gets all GPUs
    let all_uuids: Vec<&str> = gpus
        .iter()
        .filter_map(|g| g["uuid"].as_str())
        .collect();

    assignment["gpu_assignment"]["services"]["llama-server"] = json!({
        "gpus": all_uuids,
        "mode": if all_uuids.len() > 1 { "tensor_parallel" } else { "exclusive" },
    });

    if dry_run {
        println!("\n--- Dry Run: GPU Assignment ---");
        println!("{}", serde_json::to_string_pretty(&assignment)?);
        return Ok(());
    }

    // Write assignment
    let output_path = Path::new(&install_dir).join("config").join("gpu-assignment.json");
    std::fs::write(&output_path, serde_json::to_string_pretty(&assignment)?)?;
    println!("Assignment written to {}", output_path.display());

    // Encode as base64 for .env
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(serde_json::to_string(&assignment)?);
    println!("GPU_ASSIGNMENT_JSON_B64={b64}");

    Ok(())
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
    fn test_run_dry_run_single_gpu() {
        let f = write_temp_file(r#"{"gpus": [{"uuid": "GPU-123", "name": "RTX 4090"}]}"#);
        let result = run(Some(f.path().to_str().unwrap()), true);
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[test]
    fn test_run_dry_run_multi_gpu() {
        let f = write_temp_file(
            r#"{"gpus": [{"uuid": "GPU-AAA", "name": "RTX 4090"}, {"uuid": "GPU-BBB", "name": "RTX 4080"}]}"#,
        );
        let result = run(Some(f.path().to_str().unwrap()), true);
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[test]
    fn test_run_missing_topology() {
        let result = run(Some("/nonexistent/path.json"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_invalid_json() {
        let f = write_temp_file("not json");
        let result = run(Some(f.path().to_str().unwrap()), true);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_write_assignment() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let config_dir = tmp_dir.path().join("config");
        std::fs::create_dir(&config_dir).unwrap();

        let topo = write_temp_file(r#"{"gpus": [{"uuid": "GPU-123", "name": "RTX 4090"}]}"#);

        std::env::set_var("DREAM_INSTALL_DIR", tmp_dir.path().to_str().unwrap());
        let result = run(Some(topo.path().to_str().unwrap()), false);
        std::env::remove_var("DREAM_INSTALL_DIR");

        assert!(result.is_ok(), "expected Ok, got: {result:?}");

        let output = config_dir.join("gpu-assignment.json");
        assert!(output.exists(), "gpu-assignment.json should be written");

        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
        assert!(content["gpu_assignment"]["services"]["llama-server"].is_object());
    }
}
