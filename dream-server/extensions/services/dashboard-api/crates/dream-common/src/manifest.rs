//! Service manifest types for the extension system.
//!
//! Maps the YAML/JSON `manifest.yaml` schema used by Dream Server extensions
//! into Rust types for deserialization and runtime use.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level manifest file structure (schema_version: "dream.services.v1").
#[derive(Debug, Clone, Deserialize)]
pub struct ExtensionManifest {
    pub schema_version: Option<String>,
    pub service: Option<ServiceDefinition>,
    #[serde(default)]
    pub features: Vec<FeatureDefinition>,
}

/// The `service` block inside a manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceDefinition {
    pub id: Option<String>,
    pub name: Option<String>,
    pub port: Option<u16>,
    pub health: Option<String>,
    pub health_port: Option<u16>,
    pub host_env: Option<String>,
    pub default_host: Option<String>,
    pub external_port_env: Option<String>,
    pub external_port_default: Option<u16>,
    pub ui_path: Option<String>,
    #[serde(rename = "type")]
    pub service_type: Option<String>,
    #[serde(default = "default_gpu_backends")]
    pub gpu_backends: Vec<String>,
}

fn default_gpu_backends() -> Vec<String> {
    vec!["amd".into(), "nvidia".into(), "apple".into()]
}

/// A feature definition from a manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureDefinition {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
    #[serde(default = "default_gpu_backends")]
    pub gpu_backends: Vec<String>,
    /// Catch-all for extra fields the dashboard UI may need.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Runtime representation of a loaded service (after manifest processing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub host: String,
    pub port: u16,
    pub external_port: u16,
    pub health: String,
    pub name: String,
    pub ui_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_minimal_manifest() {
        let yaml = "schema_version: dream.services.v1\n";
        let m: ExtensionManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(m.schema_version.as_deref(), Some("dream.services.v1"));
        assert!(m.service.is_none());
        assert!(m.features.is_empty());
    }

    #[test]
    fn test_deserialize_full_manifest() {
        let yaml = r#"
schema_version: dream.services.v1
service:
  id: open-webui
  name: Open WebUI
  port: 8080
  health: /health
  gpu_backends:
    - nvidia
    - amd
features:
  - id: chat
    name: Chat Interface
    description: Web chat UI
"#;
        let m: ExtensionManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(m.schema_version.as_deref(), Some("dream.services.v1"));

        let svc = m.service.as_ref().unwrap();
        assert_eq!(svc.id.as_deref(), Some("open-webui"));
        assert_eq!(svc.name.as_deref(), Some("Open WebUI"));
        assert_eq!(svc.port, Some(8080));
        assert_eq!(svc.health.as_deref(), Some("/health"));
        assert_eq!(svc.gpu_backends, vec!["nvidia", "amd"]);

        assert_eq!(m.features.len(), 1);
        let feat = &m.features[0];
        assert_eq!(feat.id.as_deref(), Some("chat"));
        assert_eq!(feat.name.as_deref(), Some("Chat Interface"));
        assert_eq!(feat.description.as_deref(), Some("Web chat UI"));
    }

    #[test]
    fn test_missing_optional_fields() {
        let yaml = r#"
service:
  id: myservice
  name: My Service
"#;
        let m: ExtensionManifest = serde_yaml::from_str(yaml).unwrap();
        let svc = m.service.as_ref().unwrap();
        assert_eq!(svc.id.as_deref(), Some("myservice"));
        assert_eq!(svc.name.as_deref(), Some("My Service"));
        assert!(svc.health.is_none());
        assert!(svc.ui_path.is_none());
        assert!(svc.health_port.is_none());
    }

    #[test]
    fn test_invalid_yaml_returns_error() {
        let result = serde_yaml::from_str::<ExtensionManifest>("not: [valid: yaml: {{");
        assert!(result.is_err());
    }

    #[test]
    fn test_gpu_backends_default() {
        let yaml = r#"
service:
  id: svc1
  name: Service One
"#;
        let m: ExtensionManifest = serde_yaml::from_str(yaml).unwrap();
        let svc = m.service.as_ref().unwrap();
        assert_eq!(svc.gpu_backends, vec!["amd", "nvidia", "apple"]);
    }

    #[test]
    fn test_gpu_backends_override() {
        let yaml = r#"
service:
  id: svc1
  name: Service One
  gpu_backends:
    - nvidia
"#;
        let m: ExtensionManifest = serde_yaml::from_str(yaml).unwrap();
        let svc = m.service.as_ref().unwrap();
        assert_eq!(svc.gpu_backends, vec!["nvidia"]);
    }

    #[test]
    fn test_feature_extra_fields() {
        let yaml = r#"
features:
  - id: feat1
    name: Feature One
    requirements:
      - docker
    custom_flag: true
"#;
        let m: ExtensionManifest = serde_yaml::from_str(yaml).unwrap();
        let feat = &m.features[0];
        assert!(feat.extra.contains_key("requirements"));
        assert!(feat.extra.contains_key("custom_flag"));
        assert_eq!(feat.extra["custom_flag"], serde_json::json!(true));
    }

    #[test]
    fn test_feature_skips_none_on_serialize() {
        let feat = FeatureDefinition {
            id: Some("f1".into()),
            name: Some("Feat".into()),
            description: None,
            icon: None,
            category: None,
            setup_time: None,
            priority: None,
            gpu_backends: default_gpu_backends(),
            extra: HashMap::new(),
        };
        let json_str = serde_json::to_string(&feat).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(val.get("description").is_none());
        assert!(val.get("icon").is_none());
        assert!(val.get("category").is_none());
        assert!(val.get("setup_time").is_none());
        assert!(val.get("priority").is_none());
    }

    #[test]
    fn test_feature_gpu_backends_default() {
        let yaml = r#"
features:
  - id: feat1
    name: Feature One
"#;
        let m: ExtensionManifest = serde_yaml::from_str(yaml).unwrap();
        let feat = &m.features[0];
        assert_eq!(feat.gpu_backends, vec!["amd", "nvidia", "apple"]);
    }

    #[test]
    fn test_service_config_roundtrip() {
        let cfg = ServiceConfig {
            host: "127.0.0.1".into(),
            port: 3000,
            external_port: 3000,
            health: "/api/health".into(),
            name: "dashboard".into(),
            ui_path: "/".into(),
            service_type: Some("frontend".into()),
            health_port: Some(3001),
        };
        let json_str = serde_json::to_string(&cfg).unwrap();
        let roundtripped: ServiceConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(roundtripped.host, "127.0.0.1");
        assert_eq!(roundtripped.port, 3000);
        assert_eq!(roundtripped.external_port, 3000);
        assert_eq!(roundtripped.health, "/api/health");
        assert_eq!(roundtripped.name, "dashboard");
        assert_eq!(roundtripped.ui_path, "/");
        assert_eq!(roundtripped.service_type.as_deref(), Some("frontend"));
        assert_eq!(roundtripped.health_port, Some(3001));
    }
}
