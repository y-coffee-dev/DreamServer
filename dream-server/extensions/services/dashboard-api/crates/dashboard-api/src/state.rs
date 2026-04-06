//! Application state shared across all handlers via Axum's `State` extractor.

use moka::future::Cache;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;

use dream_common::manifest::ServiceConfig;
use dream_common::models::ServiceStatus;

/// Cache TTLs matching the Python API.
pub const GPU_CACHE_TTL: Duration = Duration::from_secs(3);
pub const STATUS_CACHE_TTL: Duration = Duration::from_secs(2);
pub const STORAGE_CACHE_TTL: Duration = Duration::from_secs(30);
pub const SERVICE_POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Shared application state, wrapped in `Arc` for cheap cloning into handlers.
#[derive(Clone)]
pub struct AppState {
    /// General-purpose TTL cache (string key -> JSON value).
    pub cache: Cache<String, serde_json::Value>,

    /// Shared HTTP client with connection pooling (replaces aiohttp/httpx sessions).
    pub http: Client,

    /// Loaded service registry (service_id -> config).
    pub services: Arc<std::collections::HashMap<String, ServiceConfig>>,

    /// Loaded feature definitions from manifests.
    pub features: Arc<Vec<serde_json::Value>>,

    /// Manifest loading errors to surface in /api/status.
    pub manifest_errors: Arc<Vec<serde_json::Value>>,

    /// API key for authentication.
    pub api_key: Arc<String>,

    /// API version string.
    pub version: Arc<String>,

    /// Cached service health statuses (written by background poll loop).
    pub services_cache: Arc<tokio::sync::RwLock<Option<Vec<ServiceStatus>>>>,
}

impl AppState {
    pub fn new(
        services: std::collections::HashMap<String, ServiceConfig>,
        features: Vec<serde_json::Value>,
        manifest_errors: Vec<serde_json::Value>,
        api_key: String,
    ) -> Self {
        // General cache: 10_000 entries max, 60s idle TTL
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_idle(Duration::from_secs(60))
            .build();

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(10)
            .build()
            .expect("failed to build HTTP client");

        Self {
            cache,
            http,
            services: Arc::new(services),
            features: Arc::new(features),
            manifest_errors: Arc::new(manifest_errors),
            api_key: Arc::new(api_key),
            version: Arc::new(
                std::env::var("DREAM_VERSION")
                    .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()),
            ),
            services_cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_new_creates_valid_state() {
        let state = AppState::new(HashMap::new(), vec![], vec![], "test-key".into());

        assert_eq!(state.api_key.as_str(), "test-key");
        assert!(state.services.is_empty());
        assert!(state.features.is_empty());
        assert!(state.manifest_errors.is_empty());
        // Falls back to CARGO_PKG_VERSION when DREAM_VERSION is unset
        assert_eq!(state.version.as_str(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_new_with_services() {
        let mut services = HashMap::new();
        services.insert(
            "open-webui".to_string(),
            ServiceConfig {
                host: "open-webui".into(),
                port: 8080,
                external_port: 3000,
                health: "/health".into(),
                name: "Open WebUI".into(),
                ui_path: "/".into(),
                service_type: None,
                health_port: None,
            },
        );

        let state = AppState::new(services, vec![], vec![], "key".into());
        assert_eq!(state.services.len(), 1);
        assert!(state.services.contains_key("open-webui"));
    }

    #[test]
    fn test_cache_ttl_constants() {
        assert_eq!(GPU_CACHE_TTL, Duration::from_secs(3));
        assert_eq!(STATUS_CACHE_TTL, Duration::from_secs(2));
        assert_eq!(STORAGE_CACHE_TTL, Duration::from_secs(30));
        assert_eq!(SERVICE_POLL_INTERVAL, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_services_cache_initially_none() {
        let state = AppState::new(HashMap::new(), vec![], vec![], "key".into());
        let cache = state.services_cache.read().await;
        assert!(cache.is_none());
    }
}
