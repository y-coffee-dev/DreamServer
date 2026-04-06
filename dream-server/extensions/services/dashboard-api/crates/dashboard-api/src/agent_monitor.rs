//! Agent monitoring module — collects real-time metrics on agent swarms,
//! sessions, and throughput. Mirrors `agent_monitor.py`.

use serde_json::json;
use std::sync::Mutex;
use tracing::debug;

// ---------------------------------------------------------------------------
// Global metrics (module-level singletons matching the Python globals)
// ---------------------------------------------------------------------------

static AGENT_METRICS: Mutex<Option<AgentMetrics>> = Mutex::new(None);
static CLUSTER_STATUS: Mutex<Option<ClusterStatus>> = Mutex::new(None);
static THROUGHPUT: Mutex<Option<ThroughputMetrics>> = Mutex::new(None);

struct AgentMetrics {
    last_update: String,
    session_count: i64,
    tokens_per_second: f64,
    error_rate_1h: f64,
    queue_depth: i64,
}

struct ClusterStatus {
    nodes: Vec<serde_json::Value>,
    failover_ready: bool,
    total_gpus: i64,
    active_gpus: i64,
}

struct ThroughputMetrics {
    data_points: Vec<(String, f64)>,
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn get_full_agent_metrics() -> serde_json::Value {
    let agent = AGENT_METRICS.lock().unwrap();
    let cluster = CLUSTER_STATUS.lock().unwrap();
    let throughput = THROUGHPUT.lock().unwrap();

    let agent_data = match agent.as_ref() {
        Some(a) => json!({
            "session_count": a.session_count,
            "tokens_per_second": (a.tokens_per_second * 100.0).round() / 100.0,
            "error_rate_1h": (a.error_rate_1h * 100.0).round() / 100.0,
            "queue_depth": a.queue_depth,
            "last_update": a.last_update,
        }),
        None => json!({
            "session_count": 0,
            "tokens_per_second": 0.0,
            "error_rate_1h": 0.0,
            "queue_depth": 0,
            "last_update": now_iso(),
        }),
    };

    let cluster_data = match cluster.as_ref() {
        Some(c) => json!({
            "nodes": c.nodes,
            "total_gpus": c.total_gpus,
            "active_gpus": c.active_gpus,
            "failover_ready": c.failover_ready,
        }),
        None => json!({
            "nodes": [],
            "total_gpus": 0,
            "active_gpus": 0,
            "failover_ready": false,
        }),
    };

    let throughput_data = match throughput.as_ref() {
        Some(t) => {
            let values: Vec<f64> = t.data_points.iter().map(|(_, v)| *v).collect();
            let current = values.last().copied().unwrap_or(0.0);
            let average = if values.is_empty() {
                0.0
            } else {
                values.iter().sum::<f64>() / values.len() as f64
            };
            let peak = values.iter().cloned().fold(0.0f64, f64::max);
            let history: Vec<serde_json::Value> = t
                .data_points
                .iter()
                .rev()
                .take(30)
                .rev()
                .map(|(ts, tps)| json!({"timestamp": ts, "tokens_per_sec": tps}))
                .collect();
            json!({"current": current, "average": average, "peak": peak, "history": history})
        }
        None => json!({"current": 0, "average": 0, "peak": 0, "history": []}),
    };

    json!({
        "timestamp": now_iso(),
        "agent": agent_data,
        "cluster": cluster_data,
        "throughput": throughput_data,
    })
}

// ---------------------------------------------------------------------------
// Background collection task
// ---------------------------------------------------------------------------

pub async fn collect_metrics(http: reqwest::Client) {
    let token_spy_url =
        std::env::var("TOKEN_SPY_URL").unwrap_or_else(|_| "http://token-spy:8080".to_string());
    let token_spy_key = std::env::var("TOKEN_SPY_API_KEY").unwrap_or_default();

    loop {
        // Refresh cluster status
        refresh_cluster_status().await;

        // Fetch token-spy metrics
        fetch_token_spy_metrics(&http, &token_spy_url, &token_spy_key).await;

        // Update timestamp
        if let Ok(mut am) = AGENT_METRICS.lock() {
            let m = am.get_or_insert(AgentMetrics {
                last_update: now_iso(),
                session_count: 0,
                tokens_per_second: 0.0,
                error_rate_1h: 0.0,
                queue_depth: 0,
            });
            m.last_update = now_iso();
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn refresh_cluster_status() {
    let proxy_port =
        std::env::var("CLUSTER_PROXY_PORT").unwrap_or_else(|_| "9199".to_string());
    let url = format!("http://localhost:{proxy_port}/status");

    let result = tokio::process::Command::new("curl")
        .args(["-s", "--max-time", "4", &url])
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => {
            if let Ok(data) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                let nodes = data["nodes"].as_array().cloned().unwrap_or_default();
                let total = nodes.len() as i64;
                let active = nodes
                    .iter()
                    .filter(|n| n["healthy"].as_bool().unwrap_or(false))
                    .count() as i64;
                if let Ok(mut cs) = CLUSTER_STATUS.lock() {
                    *cs = Some(ClusterStatus {
                        nodes,
                        failover_ready: active > 1,
                        total_gpus: total,
                        active_gpus: active,
                    });
                }
            }
        }
        _ => {
            debug!("Cluster proxy not available");
        }
    }
}

async fn fetch_token_spy_metrics(http: &reqwest::Client, url: &str, api_key: &str) {
    if url.is_empty() {
        return;
    }

    let mut req = http.get(format!("{url}/api/summary"));
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }

    match tokio::time::timeout(std::time::Duration::from_secs(5), req.send()).await {
        Ok(Ok(resp)) if resp.status().is_success() => {
            if let Ok(data) = resp.json::<Vec<serde_json::Value>>().await {
                let session_count = data.len() as i64;
                let total_out: f64 = data
                    .iter()
                    .filter_map(|r| r["total_output_tokens"].as_f64())
                    .sum();
                let tps = total_out / 3600.0;

                if let Ok(mut am) = AGENT_METRICS.lock() {
                    let m = am.get_or_insert(AgentMetrics {
                        last_update: now_iso(),
                        session_count: 0,
                        tokens_per_second: 0.0,
                        error_rate_1h: 0.0,
                        queue_depth: 0,
                    });
                    m.session_count = session_count;
                }

                if let Ok(mut tp) = THROUGHPUT.lock() {
                    let t = tp.get_or_insert(ThroughputMetrics {
                        data_points: Vec::new(),
                    });
                    t.data_points.push((now_iso(), tps));
                    // Prune old data (keep last 15 minutes ~ 180 samples at 5s interval)
                    if t.data_points.len() > 180 {
                        t.data_points.drain(..t.data_points.len() - 180);
                    }
                }
            }
        }
        _ => {
            debug!("Token Spy unavailable");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset all global singletons to `None` before each test.
    fn reset_globals() {
        *AGENT_METRICS.lock().unwrap() = None;
        *CLUSTER_STATUS.lock().unwrap() = None;
        *THROUGHPUT.lock().unwrap() = None;
    }

    #[test]
    fn test_get_full_agent_metrics_defaults() {
        reset_globals();

        let result = get_full_agent_metrics();

        // Agent defaults
        assert_eq!(result["agent"]["session_count"], 0);
        assert_eq!(result["agent"]["tokens_per_second"], 0.0);
        assert_eq!(result["agent"]["error_rate_1h"], 0.0);
        assert_eq!(result["agent"]["queue_depth"], 0);

        // Cluster defaults
        assert_eq!(result["cluster"]["total_gpus"], 0);
        assert_eq!(result["cluster"]["active_gpus"], 0);
        assert_eq!(result["cluster"]["failover_ready"], false);
        let nodes = result["cluster"]["nodes"].as_array().unwrap();
        assert!(nodes.is_empty());

        // Throughput defaults
        assert_eq!(result["throughput"]["current"], 0);
        assert_eq!(result["throughput"]["average"], 0);
        assert_eq!(result["throughput"]["peak"], 0);
        let history = result["throughput"]["history"].as_array().unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_get_full_agent_metrics_with_data() {
        reset_globals();

        *AGENT_METRICS.lock().unwrap() = Some(AgentMetrics {
            last_update: now_iso(),
            session_count: 5,
            tokens_per_second: 12.345,
            error_rate_1h: 0.02,
            queue_depth: 3,
        });
        *CLUSTER_STATUS.lock().unwrap() = Some(ClusterStatus {
            nodes: vec![
                json!({"id": "node1", "healthy": true}),
                json!({"id": "node2", "healthy": false}),
            ],
            failover_ready: false,
            total_gpus: 2,
            active_gpus: 1,
        });
        *THROUGHPUT.lock().unwrap() = Some(ThroughputMetrics {
            data_points: vec![
                ("2026-01-01T00:00:00Z".into(), 5.0),
                ("2026-01-01T00:00:05Z".into(), 10.0),
                ("2026-01-01T00:00:10Z".into(), 15.0),
            ],
        });

        let result = get_full_agent_metrics();

        assert_eq!(result["agent"]["session_count"], 5);
        // 12.345 rounded to 2 decimal places: (12.345 * 100).round() / 100 = 12.35
        assert_eq!(result["agent"]["tokens_per_second"], 12.35);
        assert_eq!(result["agent"]["queue_depth"], 3);

        assert_eq!(result["cluster"]["total_gpus"], 2);
        assert_eq!(result["cluster"]["active_gpus"], 1);
        assert_eq!(result["cluster"]["failover_ready"], false);
        assert_eq!(result["cluster"]["nodes"].as_array().unwrap().len(), 2);

        assert_eq!(result["throughput"]["current"], 15.0);
        assert_eq!(result["throughput"]["peak"], 15.0);
        assert_eq!(result["throughput"]["history"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_throughput_history_limited_to_30() {
        reset_globals();

        let data_points: Vec<(String, f64)> = (0..50)
            .map(|i| (format!("2026-01-01T00:00:{:02}Z", i), i as f64))
            .collect();
        *THROUGHPUT.lock().unwrap() = Some(ThroughputMetrics { data_points });

        let result = get_full_agent_metrics();
        let history = result["throughput"]["history"].as_array().unwrap();
        assert_eq!(history.len(), 30);

        // Should be the last 30 data points (indices 20..50)
        let first_ts = history[0]["timestamp"].as_str().unwrap();
        assert_eq!(first_ts, "2026-01-01T00:00:20Z");
    }

    #[test]
    fn test_throughput_peak_and_average() {
        reset_globals();

        *THROUGHPUT.lock().unwrap() = Some(ThroughputMetrics {
            data_points: vec![
                ("t1".into(), 10.0),
                ("t2".into(), 20.0),
                ("t3".into(), 30.0),
            ],
        });

        let result = get_full_agent_metrics();
        assert_eq!(result["throughput"]["peak"], 30.0);
        assert_eq!(result["throughput"]["average"], 20.0);
        assert_eq!(result["throughput"]["current"], 30.0);
    }

    #[test]
    fn test_get_full_agent_metrics_has_timestamp() {
        reset_globals();
        let result = get_full_agent_metrics();
        assert!(result.get("timestamp").is_some(), "Missing timestamp key");
        let ts = result["timestamp"].as_str().unwrap();
        // Should be valid RFC3339
        assert!(ts.contains('T'), "Timestamp should be RFC3339 format");
    }

    #[test]
    fn test_agent_metrics_error_rate_rounding() {
        reset_globals();
        *AGENT_METRICS.lock().unwrap() = Some(AgentMetrics {
            last_update: now_iso(),
            session_count: 1,
            tokens_per_second: 0.0,
            error_rate_1h: 0.123456,
            queue_depth: 0,
        });
        let result = get_full_agent_metrics();
        // (0.123456 * 100).round() / 100 = 12.0 / 100 = 0.12
        assert_eq!(result["agent"]["error_rate_1h"], 0.12);
    }

    #[test]
    fn test_throughput_empty_data_points() {
        reset_globals();
        *THROUGHPUT.lock().unwrap() = Some(ThroughputMetrics {
            data_points: Vec::new(),
        });
        let result = get_full_agent_metrics();
        assert_eq!(result["throughput"]["current"], 0.0);
        assert_eq!(result["throughput"]["average"], 0.0);
        assert_eq!(result["throughput"]["peak"], 0.0);
        assert!(result["throughput"]["history"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_cluster_failover_ready() {
        reset_globals();

        // Two active (healthy) nodes => failover_ready should be true
        *CLUSTER_STATUS.lock().unwrap() = Some(ClusterStatus {
            nodes: vec![
                json!({"id": "n1", "healthy": true}),
                json!({"id": "n2", "healthy": true}),
            ],
            failover_ready: true,
            total_gpus: 2,
            active_gpus: 2,
        });

        let result = get_full_agent_metrics();
        assert_eq!(result["cluster"]["failover_ready"], true);

        // One active node => failover_ready should be false
        *CLUSTER_STATUS.lock().unwrap() = Some(ClusterStatus {
            nodes: vec![json!({"id": "n1", "healthy": true})],
            failover_ready: false,
            total_gpus: 1,
            active_gpus: 1,
        });

        let result = get_full_agent_metrics();
        assert_eq!(result["cluster"]["failover_ready"], false);
    }
}
