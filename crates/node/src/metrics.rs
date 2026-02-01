//! Monitoring and Metrics Collection
//!
//! This module provides Prometheus metrics collection for the Norn blockchain node.

use prometheus::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramVec, Registry, HistogramOpts, Opts,
    TextEncoder, Encoder,
};
use std::sync::Arc;
use lazy_static::lazy_static;
use tracing::{debug, error};

lazy_static! {
    // Block production metrics
    pub static ref BLOCK_PRODUCTION_TOTAL: CounterVec = CounterVec::new(
        Opts::new("norn_block_production_total", "Total number of blocks produced"),
        &["success"]
    ).unwrap();

    pub static ref BLOCK_PRODUCTION_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("norn_block_production_duration_seconds", "Block production duration in seconds")
            .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0])
    ).unwrap();

    // Transaction pool metrics
    pub static ref TXPOOL_SIZE: Gauge = Gauge::new(
        "norn_txpool_size",
        "Current number of transactions in the pool"
    ).unwrap();

    pub static ref TXPOOL_ADD_TOTAL: CounterVec = CounterVec::new(
        Opts::new("norn_txpool_add_total", "Total number of transactions added to the pool"),
        &["status"]  // success | rejected
    ).unwrap();

    pub static ref TXPOOL_REMOVE_TOTAL: CounterVec = CounterVec::new(
        Opts::new("norn_txpool_remove_total", "Total number of transactions removed from the pool"),
        &["reason"]  // included | expired | replaced
    ).unwrap();

    // Enhanced transaction pool metrics
    pub static ref TXPACKAGED_TOTAL: Counter = Counter::new(
        "norn_txpackaged_total",
        "Total number of transactions packaged into blocks"
    ).unwrap();

    pub static ref TXPOOL_AVG_GAS_PRICE: Gauge = Gauge::new(
        "norn_txpool_avg_gas_price",
        "Average gas price of transactions in the pool"
    ).unwrap();

    pub static ref TXPOOL_REPLACEMENT_TOTAL: Counter = Counter::new(
        "norn_txpool_replacement_total",
        "Total number of transaction replacements (EIP-1559)"
    ).unwrap();

    pub static ref TXPOOL_EXPIRED_TOTAL: Counter = Counter::new(
        "norn_txpool_expired_total",
        "Total number of expired transactions removed"
    ).unwrap();

    // Fast sync metrics
    pub static ref SYNC_MODE: GaugeVec = GaugeVec::new(
        Opts::new("norn_sync_mode", "Current sync mode (1=fast, 0=full)"),
        &["node_type"]  // validator | observer
    ).unwrap();

    pub static ref SYNC_BLOCKS_TOTAL: CounterVec = CounterVec::new(
        Opts::new("norn_sync_blocks_total", "Total number of blocks synced"),
        &["mode"]  // fast | full
    ).unwrap();

    pub static ref SYNC_DURATION_SECONDS: HistogramVec = HistogramVec::new(
        HistogramOpts::new("norn_sync_duration_seconds", "Time taken to sync blocks")
            .buckets(vec![1.0, 10.0, 60.0, 300.0, 600.0, 1800.0]),
        &["mode"]
    ).unwrap();

    pub static ref SYNC_CURRENT_BLOCK: GaugeVec = GaugeVec::new(
        Opts::new("norn_sync_current_block", "Current block number synced to"),
        &["node_type"]
    ).unwrap();

    // Network metrics
    pub static ref PEER_CONNECTIONS: Gauge = Gauge::new(
        "norn_peer_connections",
        "Current number of connected peers"
    ).unwrap();

    pub static ref NETWORK_BYTES_TOTAL: CounterVec = CounterVec::new(
        Opts::new("norn_network_bytes_total", "Total number of bytes transferred over network"),
        &["direction"]  // sent | received
    ).unwrap();

    // Consensus metrics
    pub static ref CONSENSUS_ROUNDS_TOTAL: CounterVec = CounterVec::new(
        Opts::new("norn_consensus_rounds_total", "Total number of consensus rounds"),
        &["result"]  // success | failed
    ).unwrap();

    pub static ref VRF_EXECUTION_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("norn_vrf_execution_duration_seconds", "VRF execution duration in seconds")
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5])
    ).unwrap();

    pub static ref VDF_EXECUTION_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("norn_vdf_execution_duration_seconds", "VDF execution duration in seconds")
            .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0])
    ).unwrap();

    // Storage metrics
    pub static ref STORAGE_READ_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("norn_storage_read_duration_seconds", "Storage read operation duration in seconds")
            .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05])
    ).unwrap();

    pub static ref STORAGE_WRITE_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("norn_storage_write_duration_seconds", "Storage write operation duration in seconds")
            .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05])
    ).unwrap();

    // RPC metrics
    pub static ref RPC_REQUESTS_TOTAL: CounterVec = CounterVec::new(
        Opts::new("norn_rpc_requests_total", "Total number of RPC requests"),
        &["method", "status"]  // method name | success | failure
    ).unwrap();

    pub static ref RPC_REQUEST_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new("norn_rpc_request_duration_seconds", "RPC request duration in seconds")
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
        &["method"]
    ).unwrap();

    // TPS (Transactions Per Second) metrics
    pub static ref TPS: Gauge = Gauge::new(
        "norn_tps",
        "Current transactions per second"
    ).unwrap();

    pub static ref BLOCK_HEIGHT: Gauge = Gauge::new(
        "norn_block_height",
        "Current block height of the blockchain"
    ).unwrap();

    // State pruning metrics
    pub static ref PRUNING_TOTAL: Counter = Counter::new(
        "norn_state_pruning_total",
        "Total number of state pruning operations performed"
    ).unwrap();

    pub static ref PRUNING_SNAPSHOTS_REMOVED: Counter = Counter::new(
        "norn_state_pruning_snapshots_removed_total",
        "Total number of snapshots removed by pruning"
    ).unwrap();

    pub static ref PRUNING_CHANGES_REMOVED: Counter = Counter::new(
        "norn_state_pruning_changes_removed_total",
        "Total number of state changes removed by pruning"
    ).unwrap();

    pub static ref PRUNING_BYTES_SAVED: Counter = Counter::new(
        "norn_state_pruning_bytes_saved_total",
        "Total bytes saved by pruning operations"
    ).unwrap();

    pub static ref PRUNING_DURATION_SECONDS: Histogram = Histogram::with_opts(
        HistogramOpts::new("norn_state_pruning_duration_seconds", "State pruning operation duration in seconds")
            .buckets(vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0])
    ).unwrap();

    pub static ref PRUNING_LAST_BLOCK: Gauge = Gauge::new(
        "norn_state_pruning_last_block",
        "Block number of the last pruning operation"
    ).unwrap();
}

/// Metrics collector
#[derive(Clone)]
pub struct MetricsCollector {
    registry: Arc<Registry>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        let registry = Registry::new();

        // Register all metrics
        registry.register(Box::new(BLOCK_PRODUCTION_TOTAL.clone())).unwrap();
        registry.register(Box::new(BLOCK_PRODUCTION_DURATION.clone())).unwrap();
        registry.register(Box::new(TXPOOL_SIZE.clone())).unwrap();
        registry.register(Box::new(TXPOOL_ADD_TOTAL.clone())).unwrap();
        registry.register(Box::new(TXPOOL_REMOVE_TOTAL.clone())).unwrap();

        // Enhanced transaction pool metrics
        registry.register(Box::new(TXPACKAGED_TOTAL.clone())).unwrap();
        registry.register(Box::new(TXPOOL_AVG_GAS_PRICE.clone())).unwrap();
        registry.register(Box::new(TXPOOL_REPLACEMENT_TOTAL.clone())).unwrap();
        registry.register(Box::new(TXPOOL_EXPIRED_TOTAL.clone())).unwrap();

        // Fast sync metrics
        registry.register(Box::new(SYNC_MODE.clone())).unwrap();
        registry.register(Box::new(SYNC_BLOCKS_TOTAL.clone())).unwrap();
        registry.register(Box::new(SYNC_DURATION_SECONDS.clone())).unwrap();
        registry.register(Box::new(SYNC_CURRENT_BLOCK.clone())).unwrap();

        // TPS metrics
        registry.register(Box::new(TPS.clone())).unwrap();
        registry.register(Box::new(BLOCK_HEIGHT.clone())).unwrap();

        // State pruning metrics
        registry.register(Box::new(PRUNING_TOTAL.clone())).unwrap();
        registry.register(Box::new(PRUNING_SNAPSHOTS_REMOVED.clone())).unwrap();
        registry.register(Box::new(PRUNING_CHANGES_REMOVED.clone())).unwrap();
        registry.register(Box::new(PRUNING_BYTES_SAVED.clone())).unwrap();
        registry.register(Box::new(PRUNING_DURATION_SECONDS.clone())).unwrap();
        registry.register(Box::new(PRUNING_LAST_BLOCK.clone())).unwrap();

        registry.register(Box::new(PEER_CONNECTIONS.clone())).unwrap();
        registry.register(Box::new(NETWORK_BYTES_TOTAL.clone())).unwrap();
        registry.register(Box::new(CONSENSUS_ROUNDS_TOTAL.clone())).unwrap();
        registry.register(Box::new(VRF_EXECUTION_DURATION.clone())).unwrap();
        registry.register(Box::new(VDF_EXECUTION_DURATION.clone())).unwrap();
        registry.register(Box::new(STORAGE_READ_DURATION.clone())).unwrap();
        registry.register(Box::new(STORAGE_WRITE_DURATION.clone())).unwrap();
        registry.register(Box::new(RPC_REQUESTS_TOTAL.clone())).unwrap();
        registry.register(Box::new(RPC_REQUEST_DURATION.clone())).unwrap();

        Self {
            registry: Arc::new(registry),
        }
    }

    /// Get the Prometheus registry
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Gather metrics as Prometheus text format
    pub fn gather(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode_to_string(&metric_families)
    }

    /// Update transaction pool metrics
    pub fn update_txpool_metrics(&self, size: usize, avg_gas_price: u64) {
        TXPOOL_SIZE.set(size as f64);
        TXPOOL_AVG_GAS_PRICE.set(avg_gas_price as f64);
    }

    /// Increment packaged transactions counter
    pub fn inc_txpackaged(&self, count: u64) {
        TXPACKAGED_TOTAL.inc_by(count as f64);
    }

    /// Increment transaction replacement counter
    pub fn inc_txpool_replacement(&self) {
        TXPOOL_REPLACEMENT_TOTAL.inc();
    }

    /// Increment expired transactions counter
    pub fn inc_txpool_expired(&self) {
        TXPOOL_EXPIRED_TOTAL.inc();
    }

    /// Update block height
    pub fn update_block_height(&self, height: i64) {
        BLOCK_HEIGHT.set(height as f64);
    }

    /// Update peer count
    pub fn update_peer_count(&self, count: usize) {
        PEER_CONNECTIONS.set(count as f64);
    }

    /// Update sync metrics
    pub fn update_sync_metrics(&self, node_type: &str, current_block: i64, mode: &str) {
        SYNC_CURRENT_BLOCK.with_label_values(&[node_type]).set(current_block as f64);
        SYNC_MODE.with_label_values(&[node_type]).set(if mode == "fast" { 1.0 } else { 0.0 });
    }

    /// Record block sync
    pub fn record_sync_blocks(&self, mode: &str, count: u64) {
        SYNC_BLOCKS_TOTAL.with_label_values(&[mode]).inc_by(count as f64);
    }

    /// Update TPS
    pub fn update_tps(&self, tps: f64) {
        TPS.set(tps);
    }

    /// Record pruning operation
    pub fn record_pruning(&self, block_number: u64, snapshots: u64, changes: u64, bytes_saved: u64, duration_sec: f64) {
        PRUNING_TOTAL.inc();
        PRUNING_SNAPSHOTS_REMOVED.inc_by(snapshots as f64);
        PRUNING_CHANGES_REMOVED.inc_by(changes as f64);
        PRUNING_BYTES_SAVED.inc_by(bytes_saved as f64);
        PRUNING_LAST_BLOCK.set(block_number as f64);
        PRUNING_DURATION_SECONDS.observe(duration_sec);
    }

    /// Get current pruning statistics
    pub fn get_pruning_stats(&self) -> (u64, u64, u64, u64) {
        let total = PRUNING_TOTAL.get() as u64;
        let snapshots = PRUNING_SNAPSHOTS_REMOVED.get() as u64;
        let changes = PRUNING_CHANGES_REMOVED.get() as u64;
        let bytes = PRUNING_BYTES_SAVED.get() as u64;
        (total, snapshots, changes, bytes)
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Health check status
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub block_height: i64,
    pub peer_count: usize,
    pub txpool_size: usize,
    pub is_healthy: bool,
}

impl HealthStatus {
    /// Create a new health status
    pub fn new(
        uptime_seconds: u64,
        block_height: i64,
        peer_count: usize,
        txpool_size: usize,
    ) -> Self {
        let is_healthy = peer_count > 0 && block_height >= 0;

        Self {
            status: if is_healthy { "healthy".to_string() } else { "unhealthy".to_string() },
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds,
            block_height,
            peer_count,
            txpool_size,
            is_healthy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        let metrics = collector.gather();
        assert!(metrics.is_ok());
    }

    #[test]
    fn test_health_status() {
        let status = HealthStatus::new(3600, 12345, 5, 100);
        assert!(status.is_healthy);
        assert_eq!(status.status, "healthy");
        assert_eq!(status.block_height, 12345);
    }
}
