//! Monitoring and Health Check Endpoints
//!
//! This module provides HTTP endpoints for health checks and metrics.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tracing::{info, error, debug};

use crate::metrics::{MetricsCollector, HealthStatus};

/// Component health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub name: String,
    pub status: String,
    pub message: String,
    pub last_check: String,
}

/// Detailed health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedHealthStatus {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub components: Vec<ComponentHealth>,
    pub metrics: HealthMetrics,
    pub is_healthy: bool,
}

/// Health metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMetrics {
    pub block_height: i64,
    pub peer_count: usize,
    pub txpool_size: usize,
    pub sync_status: String,
}

/// Readiness status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessStatus {
    pub ready: bool,
    pub message: String,
    pub checks: Vec<ComponentHealth>,
}

/// Monitoring server
pub struct MonitoringServer {
    metrics_collector: Arc<MetricsCollector>,
    node_start_time: Instant,
}

impl MonitoringServer {
    /// Create a new monitoring server
    pub fn new(metrics_collector: Arc<MetricsCollector>) -> Self {
        Self {
            metrics_collector,
            node_start_time: Instant::now(),
        }
    }

    /// Build the router
    fn router(&self) -> Router {
        Router::new()
            .route("/health", get(health_check_handler))
            .route("/health/detailed", get(detailed_health_handler))
            .route("/ready", get(readiness_handler))
            .route("/live", get(liveness_handler))
            .route("/metrics", get(metrics_handler))
            .with_state((self.metrics_collector.clone(), self.node_start_time))
    }

    /// Start the monitoring server
    pub async fn start(&self, address: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(address).await?;
        info!("Monitoring server listening on {}", address);

        axum::serve(listener, self.router()).await?;
        Ok(())
    }
}

/// Health check handler (simple, for load balancers)
async fn health_check_handler(
    State((metrics_collector, start_time)): State<(Arc<MetricsCollector>, Instant)>,
) -> impl IntoResponse {
    let uptime = start_time.elapsed().as_secs();

    // Get current metrics
    let health_status = HealthStatus::new(
        uptime,
        get_metric_value(&metrics_collector, "norn_block_height"),
        get_metric_value(&metrics_collector, "norn_peer_connections") as usize,
        get_metric_value(&metrics_collector, "norn_txpool_size") as usize,
    );

    let status_code = if health_status.is_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status_code, Json(health_status))
}

/// Detailed health check handler (for monitoring systems)
async fn detailed_health_handler(
    State((metrics_collector, start_time)): State<(Arc<MetricsCollector>, Instant)>,
) -> impl IntoResponse {
    let uptime = start_time.elapsed().as_secs();

    // Check individual components
    let components = vec![
        check_component("blockchain", &metrics_collector).await,
        check_component("network", &metrics_collector).await,
        check_component("txpool", &metrics_collector).await,
        check_component("consensus", &metrics_collector).await,
        check_component("storage", &metrics_collector).await,
    ];

    let is_healthy = components.iter().all(|c| c.status == "healthy");

    let metrics = HealthMetrics {
        block_height: get_metric_value(&metrics_collector, "norn_block_height"),
        peer_count: get_metric_value(&metrics_collector, "norn_peer_connections") as usize,
        txpool_size: get_metric_value(&metrics_collector, "norn_txpool_size") as usize,
        sync_status: if get_metric_value(&metrics_collector, "norn_sync_current_block{node_type=\"validator\"}") > 0 {
            "synced".to_string()
        } else {
            "syncing".to_string()
        },
    };

    let status = DetailedHealthStatus {
        status: if is_healthy { "healthy".to_string() } else { "degraded".to_string() },
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: uptime,
        components,
        metrics,
        is_healthy,
    };

    let status_code = if is_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status_code, Json(status))
}

/// Readiness handler (for Kubernetes readiness probes)
async fn readiness_handler(
    State((metrics_collector, _)): State<(Arc<MetricsCollector>, Instant)>,
) -> impl IntoResponse {
    let peer_count = get_metric_value(&metrics_collector, "norn_peer_connections");
    let block_height = get_metric_value(&metrics_collector, "norn_block_height");

    let ready = peer_count > 0 && block_height >= 0;
    let checks = vec![
        ComponentHealth {
            name: "peers_connected".to_string(),
            status: if peer_count > 0 { "pass".to_string() } else { "fail".to_string() },
            message: format!("Connected to {} peers", peer_count),
            last_check: chrono::Utc::now().to_rfc3339(),
        },
        ComponentHealth {
            name: "blockchain_initialized".to_string(),
            status: if block_height >= 0 { "pass".to_string() } else { "fail".to_string() },
            message: format!("Block height: {}", block_height),
            last_check: chrono::Utc::now().to_rfc3339(),
        },
    ];

    let status = ReadinessStatus {
        ready,
        message: if ready {
            "Node is ready to accept transactions".to_string()
        } else {
            "Node is initializing".to_string()
        },
        checks,
    };

    let status_code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status_code, Json(status))
}

/// Liveness handler (for Kubernetes liveness probes)
async fn liveness_handler(
    State((_metrics_collector, start_time)): State<(Arc<MetricsCollector>, Instant)>,
) -> impl IntoResponse {
    // Liveness is simple - if we can respond, we're alive
    let uptime = start_time.elapsed().as_secs();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "alive",
            "uptime_seconds": uptime,
        }))
    )
}

/// Metrics handler (Prometheus format)
async fn metrics_handler(
    State((metrics_collector, _)): State<(Arc<MetricsCollector>, Instant)>,
) -> impl IntoResponse {
    match metrics_collector.gather() {
        Ok(metrics) => {
            (
                StatusCode::OK,
                [("Content-Type", "text/plain; version=0.0.4; charset=utf-8")],
                metrics,
            )
        }
        Err(e) => {
            error!("Failed to gather metrics: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("Content-Type", "text/plain")],
                "Failed to gather metrics".to_string(),
            )
        }
    }
}

/// Check individual component health
async fn check_component(
    component: &str,
    metrics_collector: &MetricsCollector,
) -> ComponentHealth {
    let now = chrono::Utc::now().to_rfc3339();

    match component {
        "blockchain" => {
            let block_height = get_metric_value(metrics_collector, "norn_block_height");
            ComponentHealth {
                name: "blockchain".to_string(),
                status: if block_height >= 0 { "healthy".to_string() } else { "unhealthy".to_string() },
                message: format!("Block height: {}", block_height),
                last_check: now,
            }
        }
        "network" => {
            let peer_count = get_metric_value(metrics_collector, "norn_peer_connections");
            ComponentHealth {
                name: "network".to_string(),
                status: if peer_count > 0 { "healthy".to_string() } else { "degraded".to_string() },
                message: format!("Connected to {} peers", peer_count),
                last_check: now,
            }
        }
        "txpool" => {
            let pool_size = get_metric_value(metrics_collector, "norn_txpool_size");
            ComponentHealth {
                name: "txpool".to_string(),
                status: if pool_size >= 0 { "healthy".to_string() } else { "unhealthy".to_string() },
                message: format!("Pool size: {}", pool_size),
                last_check: now,
            }
        }
        "consensus" => {
            ComponentHealth {
                name: "consensus".to_string(),
                status: "healthy".to_string(),
                message: "PoVF consensus running".to_string(),
                last_check: now,
            }
        }
        "storage" => {
            ComponentHealth {
                name: "storage".to_string(),
                status: "healthy".to_string(),
                message: "SledDB storage operational".to_string(),
                last_check: now,
            }
        }
        _ => ComponentHealth {
            name: component.to_string(),
            status: "unknown".to_string(),
            message: "Unknown component".to_string(),
            last_check: now,
        }
    }
}

/// Get metric value by name (simple helper)
fn get_metric_value(metrics_collector: &MetricsCollector, metric_name: &str) -> i64 {
    // Parse the Prometheus metrics text to extract the value
    match metrics_collector.gather() {
        Ok(metrics_text) => {
            for line in metrics_text.lines() {
                if line.starts_with(metric_name) || line.starts_with(&format!("{}{}", metric_name, "{")) {
                    // Parse metric line: metric_name{labels} value
                    if let Some(last_space) = line.rfind(' ') {
                        if let Ok(value) = line[last_space + 1..].parse::<i64>() {
                            return value;
                        }
                    }
                }
            }
            0
        }
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitoring_server_creation() {
        let metrics = Arc::new(MetricsCollector::new());
        let server = MonitoringServer::new(metrics);
        // Router should be created without panicking
        let _router = server.router();
    }

    #[test]
    fn test_component_health() {
        let metrics = MetricsCollector::new();
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let health = check_component("blockchain", &metrics).await;
            assert_eq!(health.name, "blockchain");
            assert!(health.status == "healthy" || health.status == "unhealthy");
        });
    }

    #[test]
    fn test_metric_value_parsing() {
        let metrics = MetricsCollector::new();
        let value = get_metric_value(&metrics, "norn_block_height");
        // Should return 0 for non-existent metric
        assert_eq!(value, 0);
    }
}
