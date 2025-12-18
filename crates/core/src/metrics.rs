//! Metrics and Monitoring Module
//! 
//! Provides comprehensive metrics collection for blockchain monitoring.

use std::sync::atomic::{AtomicU64, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

/// Global metrics instance
pub static METRICS: once_cell::sync::Lazy<Metrics> = once_cell::sync::Lazy::new(Metrics::new);

/// Comprehensive metrics collection
pub struct Metrics {
    // Block metrics
    pub block_height: AtomicI64,
    pub blocks_processed: AtomicU64,
    pub blocks_per_second: AtomicU64,
    
    // Transaction metrics  
    pub tx_pool_size: AtomicU64,
    pub txs_processed: AtomicU64,
    pub txs_per_second: AtomicU64,
    
    // Network metrics
    pub peer_count: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    
    // Consensus metrics
    pub round_number: AtomicU64,
    pub proposal_count: AtomicU64,
    pub vote_count: AtomicU64,
    
    // Performance metrics
    pub block_validation_time_ms: AtomicU64,
    pub tx_validation_time_ms: AtomicU64,
    pub sync_progress_percent: AtomicU64,
}

impl Metrics {
    /// Create new metrics instance
    pub fn new() -> Self {
        Self {
            block_height: AtomicI64::new(0),
            blocks_processed: AtomicU64::new(0),
            blocks_per_second: AtomicU64::new(0),
            
            tx_pool_size: AtomicU64::new(0),
            txs_processed: AtomicU64::new(0),
            txs_per_second: AtomicU64::new(0),
            
            peer_count: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            
            round_number: AtomicU64::new(0),
            proposal_count: AtomicU64::new(0),
            vote_count: AtomicU64::new(0),
            
            block_validation_time_ms: AtomicU64::new(0),
            tx_validation_time_ms: AtomicU64::new(0),
            sync_progress_percent: AtomicU64::new(100),
        }
    }

    /// Set block height
    pub fn set_block_height(&self, height: i64) {
        self.block_height.store(height, Ordering::Relaxed);
    }

    /// Increment blocks processed
    pub fn inc_blocks_processed(&self) {
        self.blocks_processed.fetch_add(1, Ordering::Relaxed);
    }

    /// Set tx pool size
    pub fn set_tx_pool_size(&self, size: u64) {
        self.tx_pool_size.store(size, Ordering::Relaxed);
    }

    /// Increment txs processed
    pub fn inc_txs_processed(&self, count: u64) {
        self.txs_processed.fetch_add(count, Ordering::Relaxed);
    }

    /// Set peer count
    pub fn set_peer_count(&self, count: u64) {
        self.peer_count.store(count, Ordering::Relaxed);
    }

    /// Add bytes sent
    pub fn add_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Add bytes received
    pub fn add_bytes_received(&self, bytes: u64) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Set sync progress
    pub fn set_sync_progress(&self, percent: u64) {
        self.sync_progress_percent.store(percent, Ordering::Relaxed);
    }

    /// Record block validation time
    pub fn record_block_validation(&self, duration_ms: u64) {
        self.block_validation_time_ms.store(duration_ms, Ordering::Relaxed);
    }

    /// Record tx validation time
    pub fn record_tx_validation(&self, duration_ms: u64) {
        self.tx_validation_time_ms.store(duration_ms, Ordering::Relaxed);
    }

    /// Get metrics snapshot
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            block_height: self.block_height.load(Ordering::Relaxed),
            blocks_processed: self.blocks_processed.load(Ordering::Relaxed),
            tx_pool_size: self.tx_pool_size.load(Ordering::Relaxed),
            txs_processed: self.txs_processed.load(Ordering::Relaxed),
            peer_count: self.peer_count.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            sync_progress_percent: self.sync_progress_percent.load(Ordering::Relaxed),
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics snapshot for reporting
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub block_height: i64,
    pub blocks_processed: u64,
    pub tx_pool_size: u64,
    pub txs_processed: u64,
    pub peer_count: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub sync_progress_percent: u64,
}

impl MetricsSnapshot {
    /// Format as JSON string
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"block_height":{},"blocks_processed":{},"tx_pool_size":{},"txs_processed":{},"peer_count":{},"bytes_sent":{},"bytes_received":{},"sync_progress":{}}}"#,
            self.block_height,
            self.blocks_processed,
            self.tx_pool_size,
            self.txs_processed,
            self.peer_count,
            self.bytes_sent,
            self.bytes_received,
            self.sync_progress_percent
        )
    }
}

/// Timer for measuring operation duration
pub struct Timer {
    start: Instant,
    name: String,
}

impl Timer {
    /// Start a new timer
    pub fn start(name: &str) -> Self {
        Self {
            start: Instant::now(),
            name: name.to_string(),
        }
    }

    /// Stop timer and return duration in milliseconds
    pub fn stop(self) -> u64 {
        let duration = self.start.elapsed();
        let ms = duration.as_millis() as u64;
        debug!("Timer [{}]: {}ms", self.name, ms);
        ms
    }
}

// Legacy stub functions for compatibility
pub fn routine_create_counter_observe(count: i64) {
    debug!("Metrics: Routine create counter observed: {}", count);
}

pub fn tx_pool_metrics_inc() {
    METRICS.tx_pool_size.fetch_add(1, Ordering::Relaxed);
    debug!("Metrics: TxPool count incremented");
}

pub fn tx_pool_metrics_dec() {
    METRICS.tx_pool_size.fetch_sub(1, Ordering::Relaxed);
    debug!("Metrics: TxPool count decremented");
}

pub fn second_buffer_inc() {
    debug!("Metrics: Second buffer count incremented");
}

pub fn second_buffer_dec() {
    debug!("Metrics: Second buffer count decremented");
}

pub fn verify_transaction_metrics_set(ms: f64) {
    METRICS.record_tx_validation(ms as u64);
    debug!("Metrics: Transaction verify time set: {}ms", ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new();
        assert_eq!(metrics.block_height.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_metrics_update() {
        let metrics = Metrics::new();
        
        metrics.set_block_height(100);
        assert_eq!(metrics.block_height.load(Ordering::Relaxed), 100);
        
        metrics.inc_blocks_processed();
        assert_eq!(metrics.blocks_processed.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_metrics_snapshot() {
        let metrics = Metrics::new();
        metrics.set_block_height(50);
        metrics.set_peer_count(10);
        
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.block_height, 50);
        assert_eq!(snapshot.peer_count, 10);
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start("test");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ms = timer.stop();
        assert!(ms >= 10);
    }
}
