use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use std::sync::Arc;
use tracing::{error, info};
use crate::utils::metrics::{NornMetrics, MetricsConfig};

/// HTTP server for exposing Prometheus metrics
pub struct MetricsServer {
    metrics: Arc<NornMetrics>,
    bind_address: String,
}

impl MetricsServer {
    /// Create new metrics server
    pub fn new(metrics: Arc<NornMetrics>, config: &MetricsConfig) -> Self {
        Self {
            metrics,
            bind_address: config.bind_address.clone(),
        }
    }

    /// Start metrics HTTP server
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let app = Router::new()
            .route("/metrics", get(metrics_handler))
            .route("/health", get(health_handler))
            .with_state(self.metrics.clone());

        let listener = tokio::net::TcpListener::bind(&self.bind_address).await?;
        info!("Metrics server listening on {}", self.bind_address);

        axum::serve(listener, app).await?;
        Ok(())
    }
}

/// Handler for /metrics endpoint
async fn metrics_handler(
    State(metrics): State<Arc<NornMetrics>>,
) -> Result<String, StatusCode> {
    match metrics.gather() {
        Ok(metrics_text) => Ok(metrics_text),
        Err(err) => {
            error!("Failed to gather metrics: {}", err);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Handler for /health endpoint
async fn health_handler(
    State(_metrics): State<Arc<NornMetrics>>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "metrics_enabled": true
    }))
}

/// Create metrics router
pub fn create_metrics_router(metrics: Arc<NornMetrics>) -> Router {
    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .with_state(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::metrics::MetricsConfig;

    #[tokio::test]
    async fn test_metrics_server_creation() {
        let config = MetricsConfig::default();
        let metrics = Arc::new(NornMetrics::new(&config).unwrap());
        let server = MetricsServer::new(metrics.clone(), &config);
        
        assert_eq!(server.bind_address, "0.0.0.0:9090");
    }

    #[tokio::test]
    async fn test_metrics_handler() {
        let config = MetricsConfig::default();
        let metrics = Arc::new(NornMetrics::new(&config).unwrap());
        
        // Test metrics gathering
        let result = metrics.gather();
        assert!(result.is_ok());
        
        // Test handler
        let response = metrics_handler(State(metrics)).await;
        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_health_handler() {
        let config = MetricsConfig::default();
        let metrics = Arc::new(NornMetrics::new(&config).unwrap());
        
        let response = health_handler(State(metrics)).await;
        let json = response.0;
        
        assert_eq!(json["status"], "healthy");
        assert_eq!(json["metrics_enabled"], true);
        assert!(json["timestamp"].is_string());
    }
}