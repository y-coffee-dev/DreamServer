//! dream-scripts: Operational script modules for Dream Server.
//!
//! Each module corresponds to a standalone CLI binary:
//! - healthcheck: Service health verification
//! - assign_gpus: GPU-to-service assignment
//! - audit_extensions: Extension manifest auditing
//! - validate_sim_summary: Simulation summary validation
//! - validate_models: Model configuration validation

pub mod assign_gpus;
pub mod audit_extensions;
pub mod healthcheck;
pub mod validate_models;
pub mod validate_sim_summary;
