//! Dream Server Dashboard API — Rust/Axum port.
//!
//! Drop-in replacement for the Python FastAPI dashboard-api.
//! Default port: DASHBOARD_API_PORT (3002)

use std::net::SocketAddr;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use dashboard_api::agent_monitor;
use dashboard_api::config::{self, EnvConfig};
use dashboard_api::helpers;
use dashboard_api::middleware::resolve_api_key;
use dashboard_api::routes;
use dashboard_api::state::{self, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dashboard_api=info,tower_http=info".into()),
        )
        .init();

    let env = EnvConfig::from_env();

    let (services, features, manifest_errors) =
        config::load_extension_manifests(&env.extensions_dir, &env.gpu_backend, &env.install_dir);

    if services.is_empty() {
        tracing::error!(
            "No services loaded from manifests in {} — dashboard will have no services",
            env.extensions_dir.display()
        );
    }

    let mut services = services;
    if env.llm_backend == "lemonade" {
        if let Some(svc) = services.get_mut("llama-server") {
            svc.health = "/api/v1/health".to_string();
            info!("Lemonade backend detected — overriding llama-server health to /api/v1/health");
        }
    }

    let api_key = resolve_api_key();
    let app_state = AppState::new(services, features, manifest_errors, api_key);

    let origins = get_allowed_origins();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(
            origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect::<Vec<_>>(),
        ))
        .allow_credentials(true)
        .allow_methods(AllowMethods::list([
            http::Method::GET,
            http::Method::POST,
            http::Method::PUT,
            http::Method::PATCH,
            http::Method::DELETE,
            http::Method::OPTIONS,
        ]))
        .allow_headers(AllowHeaders::list([
            "authorization".parse().unwrap(),
            "content-type".parse().unwrap(),
            "x-requested-with".parse().unwrap(),
        ]));

    let app = dashboard_api::build_router(app_state.clone())
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    // Start background tasks
    let state_bg = app_state.clone();
    tokio::spawn(async move {
        agent_monitor::collect_metrics(state_bg.http.clone()).await;
    });
    tokio::spawn(poll_service_health(app_state.clone()));
    tokio::spawn(routes::gpu::poll_gpu_history());

    let addr = SocketAddr::from(([0, 0, 0, 0], env.dashboard_api_port));
    info!("Dashboard API listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.expect("failed to bind");
    axum::serve(listener, app.into_make_service()).await.expect("server error");
}

async fn poll_service_health(state: AppState) {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    loop {
        let statuses = helpers::get_all_services(&state.http, &state.services).await;
        {
            let mut cache = state.services_cache.write().await;
            *cache = Some(statuses);
        }
        tokio::time::sleep(state::SERVICE_POLL_INTERVAL).await;
    }
}

fn get_allowed_origins() -> Vec<String> {
    if let Ok(env_origins) = std::env::var("DASHBOARD_ALLOWED_ORIGINS") {
        if !env_origins.is_empty() {
            return env_origins.split(',').map(|s| s.to_string()).collect();
        }
    }

    let mut origins = vec![
        "http://localhost:3001".to_string(),
        "http://127.0.0.1:3001".to_string(),
        "http://localhost:3000".to_string(),
        "http://127.0.0.1:3000".to_string(),
    ];

    if let Ok(hostname) = hostname::get() {
        let hostname = hostname.to_string_lossy().to_string();
        if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&(&*hostname, 0u16)) {
            for addr in addrs {
                let ip = addr.ip().to_string();
                if ip.starts_with("192.168.") || ip.starts_with("10.") || ip.starts_with("172.") {
                    origins.push(format!("http://{ip}:3001"));
                    origins.push(format!("http://{ip}:3000"));
                }
            }
        }
    }

    origins
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env-var tests so they don't race each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_get_allowed_origins_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("DASHBOARD_ALLOWED_ORIGINS").ok();

        std::env::set_var("DASHBOARD_ALLOWED_ORIGINS", "http://example.com,http://other.com");
        let origins = get_allowed_origins();

        // Restore
        match prev {
            Some(v) => std::env::set_var("DASHBOARD_ALLOWED_ORIGINS", v),
            None => std::env::remove_var("DASHBOARD_ALLOWED_ORIGINS"),
        }

        assert_eq!(origins.len(), 2);
        assert_eq!(origins[0], "http://example.com");
        assert_eq!(origins[1], "http://other.com");
    }

    #[test]
    fn test_get_allowed_origins_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("DASHBOARD_ALLOWED_ORIGINS").ok();

        std::env::remove_var("DASHBOARD_ALLOWED_ORIGINS");
        let origins = get_allowed_origins();

        // Restore
        match prev {
            Some(v) => std::env::set_var("DASHBOARD_ALLOWED_ORIGINS", v),
            None => std::env::remove_var("DASHBOARD_ALLOWED_ORIGINS"),
        }

        assert!(origins.contains(&"http://localhost:3001".to_string()));
        assert!(origins.contains(&"http://127.0.0.1:3001".to_string()));
        assert!(origins.contains(&"http://localhost:3000".to_string()));
        assert!(origins.contains(&"http://127.0.0.1:3000".to_string()));
    }

    #[test]
    fn test_get_allowed_origins_empty_env_uses_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("DASHBOARD_ALLOWED_ORIGINS").ok();

        std::env::set_var("DASHBOARD_ALLOWED_ORIGINS", "");
        let origins = get_allowed_origins();

        // Restore
        match prev {
            Some(v) => std::env::set_var("DASHBOARD_ALLOWED_ORIGINS", v),
            None => std::env::remove_var("DASHBOARD_ALLOWED_ORIGINS"),
        }

        assert!(origins.contains(&"http://localhost:3001".to_string()));
        assert!(origins.contains(&"http://localhost:3000".to_string()));
    }
}
