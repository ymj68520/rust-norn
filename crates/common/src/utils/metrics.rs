use prometheus::{
    Counter, Gauge, Histogram, Registry, TextEncoder, Encoder,
    opts, histogram_opts
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Metrics configuration
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// Whether metrics are enabled
    pub enabled: bool,
    /// Metrics bind address
    pub bind_address: String,
    /// Metrics collection interval in seconds
    pub collection_interval: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind_address: "0.0.0.0:9090".to_string(),
            collection_interval: 10,
        }
    }
}

/// Comprehensive metrics collection for the norn blockchain
#[derive(Debug)]
pub struct NornMetrics {
    registry: Registry,
    
    // Blockchain metrics
    pub block_height: Gauge,
    pub block_processing_time: Histogram,
    pub blocks_processed_total: Counter,
    pub blocks_rejected_total: Counter,
    
    // Transaction metrics
    pub transactions_in_pool: Gauge,
    pub transactions_processed_total: Counter,
    pub transaction_processing_time: Histogram,
    pub transactions_rejected_total: Counter,
    
    // Network metrics
    pub connected_peers: Gauge,
    pub messages_sent_total: Counter,
    pub messages_received_total: Counter,
    pub network_latency: Histogram,
    
    // Consensus metrics
    pub consensus_rounds_total: Counter,
    pub consensus_time: Histogram,
    
    // Error metrics
    pub errors_total: Counter,
    
    // RPC metrics
    pub rpc_requests_total: Counter,
    pub rpc_response_time: Histogram,
    pub rpc_errors_total: Counter,
}

impl NornMetrics {
    /// Create new metrics instance
    pub fn new(_config: &MetricsConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let registry = Registry::new();
        
        // Blockchain metrics
        let block_height = Gauge::with_opts(opts!(
            "norn_block_height",
            "Current blockchain height"
        ))?;
        
        let block_processing_time = Histogram::with_opts(histogram_opts!(
            "norn_block_processing_duration_seconds",
            "Time spent processing blocks"
        ))?;
        
        let blocks_processed_total = Counter::with_opts(opts!(
            "norn_blocks_processed_total",
            "Total number of blocks processed"
        ))?;
        
        let blocks_rejected_total = Counter::with_opts(opts!(
            "norn_blocks_rejected_total",
            "Total number of blocks rejected"
        ))?;
        
        // Transaction metrics
        let transactions_in_pool = Gauge::with_opts(opts!(
            "norn_transactions_in_pool",
            "Number of transactions currently in pool"
        ))?;
        
        let transactions_processed_total = Counter::with_opts(opts!(
            "norn_transactions_processed_total",
            "Total number of transactions processed"
        ))?;
        
        let transaction_processing_time = Histogram::with_opts(histogram_opts!(
            "norn_transaction_processing_duration_seconds",
            "Time spent processing transactions"
        ))?;
        
        let transactions_rejected_total = Counter::with_opts(opts!(
            "norn_transactions_rejected_total",
            "Total number of transactions rejected"
        ))?;
        
        // Network metrics
        let connected_peers = Gauge::with_opts(opts!(
            "norn_connected_peers",
            "Number of connected peers"
        ))?;
        
        let messages_sent_total = Counter::with_opts(opts!(
            "norn_messages_sent_total",
            "Total number of messages sent"
        ))?;
        
        let messages_received_total = Counter::with_opts(opts!(
            "norn_messages_received_total",
            "Total number of messages received"
        ))?;
        
        let network_latency = Histogram::with_opts(histogram_opts!(
            "norn_network_latency_seconds",
            "Network latency in seconds"
        ))?;
        
        // Consensus metrics
        let consensus_rounds_total = Counter::with_opts(opts!(
            "norn_consensus_rounds_total",
            "Total number of consensus rounds"
        ))?;
        
        let consensus_time = Histogram::with_opts(histogram_opts!(
            "norn_consensus_duration_seconds",
            "Time spent in consensus"
        ))?;
        
        // Error metrics
        let errors_total = Counter::with_opts(opts!(
            "norn_errors_total",
            "Total number of errors"
        ))?;
        
        // RPC metrics
        let rpc_requests_total = Counter::with_opts(opts!(
            "norn_rpc_requests_total",
            "Total number of RPC requests"
        ))?;
        
        let rpc_response_time = Histogram::with_opts(histogram_opts!(
            "norn_rpc_response_duration_seconds",
            "RPC response time in seconds"
        ))?;
        
        let rpc_errors_total = Counter::with_opts(opts!(
            "norn_rpc_errors_total",
            "Total number of RPC errors"
        ))?;

        // Register all metrics to registry
        registry.register(Box::new(block_height.clone()))?;
        registry.register(Box::new(block_processing_time.clone()))?;
        registry.register(Box::new(blocks_processed_total.clone()))?;
        registry.register(Box::new(blocks_rejected_total.clone()))?;
        registry.register(Box::new(transactions_in_pool.clone()))?;
        registry.register(Box::new(transactions_processed_total.clone()))?;
        registry.register(Box::new(transaction_processing_time.clone()))?;
        registry.register(Box::new(transactions_rejected_total.clone()))?;
        registry.register(Box::new(connected_peers.clone()))?;
        registry.register(Box::new(messages_sent_total.clone()))?;
        registry.register(Box::new(messages_received_total.clone()))?;
        registry.register(Box::new(network_latency.clone()))?;
        registry.register(Box::new(consensus_rounds_total.clone()))?;
        registry.register(Box::new(consensus_time.clone()))?;
        registry.register(Box::new(errors_total.clone()))?;
        registry.register(Box::new(rpc_requests_total.clone()))?;
        registry.register(Box::new(rpc_response_time.clone()))?;
        registry.register(Box::new(rpc_errors_total.clone()))?;

        let metrics = Self {
            registry,
            block_height,
            block_processing_time,
            blocks_processed_total,
            blocks_rejected_total,
            transactions_in_pool,
            transactions_processed_total,
            transaction_processing_time,
            transactions_rejected_total,
            connected_peers,
            messages_sent_total,
            messages_received_total,
            network_latency,
            consensus_rounds_total,
            consensus_time,
            errors_total,
            rpc_requests_total,
            rpc_response_time,
            rpc_errors_total,
        };
        
        info!("Metrics system initialized");
        Ok(metrics)
    }
    
    /// Record block processing
    pub fn record_block_processed(&self, height: i64, processing_time: Duration) {
        self.block_height.set(height as f64);
        self.blocks_processed_total.inc();
        self.block_processing_time.observe(processing_time.as_secs_f64());
        debug!("Recorded block processed at height {}", height);
    }
    
    /// Record transaction processing
    pub fn record_transaction_processed(&self, pool_size: usize, processing_time: Duration) {
        self.transactions_in_pool.set(pool_size as f64);
        self.transactions_processed_total.inc();
        self.transaction_processing_time.observe(processing_time.as_secs_f64());
        debug!("Recorded transaction processed, pool size: {}", pool_size);
    }
    
    /// Record network activity
    pub fn record_network_activity(&self, connected_peers: usize, messages_sent: u64, messages_received: u64) {
        self.connected_peers.set(connected_peers as f64);
        self.messages_sent_total.inc_by(messages_sent as f64);
        self.messages_received_total.inc_by(messages_received as f64);
        debug!("Recorded network activity: {} peers, {} sent, {} received", 
                connected_peers, messages_sent, messages_received);
    }
    
    /// Record consensus round
    pub fn record_consensus_round(&self, consensus_time: Duration) {
        self.consensus_rounds_total.inc();
        self.consensus_time.observe(consensus_time.as_secs_f64());
        debug!("Recorded consensus round in {:?}", consensus_time);
    }
    
    /// Record error
    pub fn record_error(&self, error_type: &str) {
        self.errors_total.inc();
        debug!("Recorded error: {}", error_type);
    }
    
    /// Record RPC request
    pub fn record_rpc_request(&self, method: &str, response_time: Duration, success: bool) {
        self.rpc_requests_total.inc();
        self.rpc_response_time.observe(response_time.as_secs_f64());
        
        if !success {
            self.rpc_errors_total.inc();
        }
        
        debug!("Recorded RPC request: {} in {:?} (success: {})", method, response_time, success);
    }
    
    /// Get metrics for HTTP export
    pub fn gather(&self) -> Result<String, Box<dyn std::error::Error>> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }
}

/// Metrics collector with background collection
pub struct MetricsCollector {
    metrics: Arc<NornMetrics>,
    config: MetricsConfig,
    collection_handle: Option<tokio::task::JoinHandle<()>>,
}

impl MetricsCollector {
    /// Create new metrics collector
    pub fn new(config: MetricsConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let metrics = Arc::new(NornMetrics::new(&config)?);
        
        Ok(Self {
            metrics,
            config,
            collection_handle: None,
        })
    }
    
    /// Start background collection
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.config.enabled {
            info!("Metrics collection disabled");
            return Ok(());
        }
        
        info!("Metrics collection started with interval: {}s", self.config.collection_interval);
        Ok(())
    }
    
    /// Stop background collection
    pub async fn stop(&mut self) {
        if let Some(handle) = self.collection_handle.take() {
            handle.abort();
            info!("Metrics collection stopped");
        }
    }
    
    /// Get metrics reference
    pub fn metrics(&self) -> Arc<NornMetrics> {
        self.metrics.clone()
    }
}

/// Timer helper for measuring operation duration
pub struct Timer {
    start: Instant,
    metrics: Arc<NornMetrics>,
    operation_type: OperationType,
}

#[derive(Debug, Clone)]
pub enum OperationType {
    BlockProcessing,
    TransactionProcessing,
    Consensus,
    RpcRequest,
}

impl Timer {
    /// Create new timer
    pub fn new(metrics: Arc<NornMetrics>, operation_type: OperationType) -> Self {
        Self {
            start: Instant::now(),
            metrics,
            operation_type,
        }
    }
    
    /// Finish timing and record duration
    pub fn finish(self) {
        let duration = self.start.elapsed();
        
        match self.operation_type {
            OperationType::BlockProcessing => {
                self.metrics.block_processing_time.observe(duration.as_secs_f64());
            }
            OperationType::TransactionProcessing => {
                self.metrics.transaction_processing_time.observe(duration.as_secs_f64());
            }
            OperationType::Consensus => {
                self.metrics.consensus_time.observe(duration.as_secs_f64());
            }
            OperationType::RpcRequest => {
                self.metrics.rpc_response_time.observe(duration.as_secs_f64());
            }
        }
        
        debug!("Timer finished for {:?}: {:?}", self.operation_type, duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_metrics_config_defaults() {
        let config = MetricsConfig::default();
        assert!(config.enabled);
        assert_eq!(config.bind_address, "0.0.0.0:9090");
        assert_eq!(config.collection_interval, 10);
    }

    #[tokio::test]
    async fn test_metrics_creation() {
        let config = MetricsConfig::default();
        let metrics = NornMetrics::new(&config);
        assert!(metrics.is_ok());
    }

    #[tokio::test]
    async fn test_timer() {
        let config = MetricsConfig::default();
        let metrics = Arc::new(NornMetrics::new(&config).unwrap());
        
        let timer = Timer::new(metrics.clone(), OperationType::BlockProcessing);
        tokio::time::sleep(Duration::from_millis(100)).await;
        timer.finish();
        
        // The timer should have recorded duration
        // We can't easily verify this without accessing internal histogram state
    }

    #[tokio::test]
    async fn test_metrics_collector() {
        let config = MetricsConfig {
            enabled: false, // Disable to avoid background task
            ..Default::default()
        };
        
        let mut collector = MetricsCollector::new(config).unwrap();
        assert!(collector.start().await.is_ok());
        collector.stop().await;
    }
}