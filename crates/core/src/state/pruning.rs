//! State pruning module
//!
//! This module provides state pruning functionality to reduce storage usage
//! by removing old historical state data while maintaining recent history
//! for time-travel queries and debugging.

use super::history::{StateHistory, StateSnapshot};
use norn_common::error::{NornError, Result};
use norn_common::types::Hash;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn};
use std::collections::HashMap;

/// Configuration for state pruning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningConfig {
    /// Minimum number of recent blocks to keep
    /// Default: 1000 blocks
    pub min_blocks_to_keep: u64,

    /// Maximum number of historical blocks to keep
    /// Default: 10000 blocks (for light clients and archival queries)
    pub max_blocks_to_keep: u64,

    /// Enable automatic pruning after each block
    /// Default: true
    pub auto_prune: bool,

    /// Pruning interval (in blocks)
    /// Prune every N blocks to reduce overhead
    /// Default: 100 blocks
    pub prune_interval: u64,

    /// Whether to prune state changes in addition to snapshots
    /// Default: true
    pub prune_changes: bool,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            min_blocks_to_keep: 1000,
            max_blocks_to_keep: 10000,
            auto_prune: true,
            prune_interval: 100,
            prune_changes: true,
        }
    }
}

impl PruningConfig {
    /// Create a new pruning config with custom values
    pub fn new(
        min_blocks_to_keep: u64,
        max_blocks_to_keep: u64,
        prune_interval: u64,
    ) -> Self {
        Self {
            min_blocks_to_keep,
            max_blocks_to_keep,
            auto_prune: true,
            prune_interval,
            prune_changes: true,
        }
    }

    /// Create a config for light pruning (keep more history)
    pub fn light() -> Self {
        Self {
            min_blocks_to_keep: 5000,
            max_blocks_to_keep: 50000,
            auto_prune: true,
            prune_interval: 500,
            prune_changes: true,
        }
    }

    /// Create a config for aggressive pruning (minimal history)
    pub fn aggressive() -> Self {
        Self {
            min_blocks_to_keep: 100,
            max_blocks_to_keep: 1000,
            auto_prune: true,
            prune_interval: 50,
            prune_changes: true,
        }
    }

    /// Create a config for archival mode (no pruning)
    pub fn archival() -> Self {
        Self {
            min_blocks_to_keep: u64::MAX,
            max_blocks_to_keep: u64::MAX,
            auto_prune: false,
            prune_interval: u64::MAX,
            prune_changes: false,
        }
    }
}

/// Statistics about pruning operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningStats {
    /// Total number of pruning operations performed
    pub total_prunings: u64,

    /// Total snapshots pruned
    pub snapshots_pruned: u64,

    /// Total state changes pruned
    pub changes_pruned: u64,

    /// Number of blocks freed by pruning
    pub blocks_freed: u64,

    /// Estimated space saved (in bytes, approximate)
    pub bytes_saved: u64,

    /// Last pruning block number
    pub last_pruning_block: u64,

    /// Last pruning timestamp
    pub last_pruning_time: u64,
}

impl Default for PruningStats {
    fn default() -> Self {
        Self {
            total_prunings: 0,
            snapshots_pruned: 0,
            changes_pruned: 0,
            blocks_freed: 0,
            bytes_saved: 0,
            last_pruning_block: 0,
            last_pruning_time: 0,
        }
    }
}

/// State pruning manager
pub struct StatePruningManager {
    /// Pruning configuration
    config: PruningConfig,

    /// State history reference
    history: Arc<StateHistory>,

    /// Pruning statistics
    stats: Arc<RwLock<PruningStats>>,

    /// Last pruning check block
    last_check_block: Arc<RwLock<u64>>,
}

impl StatePruningManager {
    /// Create a new state pruning manager
    pub fn new(config: PruningConfig, history: Arc<StateHistory>) -> Self {
        info!(
            "Creating state pruning manager: min_blocks={}, max_blocks={}, interval={}",
            config.min_blocks_to_keep, config.max_blocks_to_keep, config.prune_interval
        );

        Self {
            config,
            history,
            stats: Arc::new(RwLock::new(PruningStats::default())),
            last_check_block: Arc::new(RwLock::new(0)),
        }
    }

    /// Check if pruning should be performed at the current block
    pub async fn should_prune(&self, current_block: u64) -> bool {
        if !self.config.auto_prune {
            return false;
        }

        let last_check = *self.last_check_block.read().await;
        let blocks_since_last_check = current_block.saturating_sub(last_check);

        blocks_since_last_check >= self.config.prune_interval
    }

    /// Perform state pruning at the current block
    /// Returns the number of blocks pruned
    pub async fn prune_old_states(&self, current_block: u64) -> Result<PruningResult> {
        info!("Starting state pruning at block {}", current_block);

        let start_time = std::time::Instant::now();

        // Calculate the cutoff block (keep recent blocks)
        let cutoff_block = if current_block > self.config.max_blocks_to_keep {
            current_block - self.config.max_blocks_to_keep
        } else {
            0
        };

        // Ensure we keep at least min_blocks_to_keep
        let min_cutoff = current_block.saturating_sub(self.config.min_blocks_to_keep);
        let effective_cutoff = cutoff_block.max(min_cutoff);

        debug!(
            "Pruning blocks older than {} (current={}, min_keep={}, max_keep={})",
            effective_cutoff, current_block, self.config.min_blocks_to_keep, self.config.max_blocks_to_keep
        );

        // Get all snapshots to determine what to prune
        let snapshots = self.history.get_all_snapshots().await;
        let snapshot_count = snapshots.len();

        let mut snapshots_pruned = 0u64;
        let mut changes_pruned = 0u64;
        let mut blocks_freed = 0u64;

        // Prune old snapshots and changes
        if self.config.prune_changes {
            // Use the batch prune method for both snapshots and changes
            let (snaps, changes) = self.history.prune_before(effective_cutoff).await?;
            snapshots_pruned = snaps as u64;
            changes_pruned = changes as u64;
            blocks_freed = snapshots_pruned;
        } else {
            // Only prune snapshots
            for snapshot in &snapshots {
                if snapshot.block_number < effective_cutoff {
                    self.history.prune_snapshot(snapshot.block_number).await?;
                    snapshots_pruned += 1;
                    blocks_freed += 1;
                    debug!("Pruned snapshot at block {}", snapshot.block_number);
                }
            }
        }

        let elapsed = start_time.elapsed();
        let bytes_saved = self.estimate_space_saved(snapshot_count, snapshots_pruned as usize, changes_pruned as usize);

        // Update statistics
        let mut stats = self.stats.write().await;
        stats.total_prunings += 1;
        stats.snapshots_pruned += snapshots_pruned;
        stats.changes_pruned += changes_pruned;
        stats.blocks_freed += blocks_freed;
        stats.bytes_saved += bytes_saved;
        stats.last_pruning_block = current_block;
        stats.last_pruning_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Update last check block
        let mut last_check = self.last_check_block.write().await;
        *last_check = current_block;

        info!(
            "Pruning complete: pruned {} snapshots, {} changes, {} blocks freed in {:?}",
            snapshots_pruned, changes_pruned, blocks_freed, elapsed
        );

        Ok(PruningResult {
            snapshots_pruned,
            changes_pruned,
            blocks_freed,
            bytes_saved,
            duration_ms: elapsed.as_millis() as u64,
            cutoff_block: effective_cutoff,
        })
    }

    /// Estimate space saved by pruning (rough approximation)
    fn estimate_space_saved(&self, total_snapshots: usize, snapshots_pruned: usize, changes_pruned: usize) -> u64 {
        // Rough estimates:
        // - Snapshot: ~1KB per account, assume 1000 accounts per snapshot = ~1MB
        // - Change: ~500 bytes per change
        const AVG_SNAPSHOT_SIZE: u64 = 1_000_000; // 1 MB
        const AVG_CHANGE_SIZE: u64 = 500;

        let snapshot_bytes = snapshots_pruned as u64 * AVG_SNAPSHOT_SIZE;
        let change_bytes = changes_pruned as u64 * AVG_CHANGE_SIZE;

        snapshot_bytes + change_bytes
    }

    /// Get pruning statistics
    pub async fn get_stats(&self) -> PruningStats {
        self.stats.read().await.clone()
    }

    /// Reset pruning statistics (for testing)
    pub async fn reset_stats(&self) -> Result<()> {
        let mut stats = self.stats.write().await;
        *stats = PruningStats::default();
        Ok(())
    }

    /// Force pruning regardless of interval
    pub async fn force_prune(&self, current_block: u64) -> Result<PruningResult> {
        info!("Force pruning at block {}", current_block);
        self.prune_old_states(current_block).await
    }

    /// Update pruning configuration
    pub async fn update_config(&self, new_config: PruningConfig) -> Result<()> {
        info!("Updating pruning config: {:?}", new_config);
        let mut config = self.config.clone();
        config = new_config;
        Ok(())
    }
}

/// Result of a pruning operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningResult {
    /// Number of snapshots pruned
    pub snapshots_pruned: u64,

    /// Number of state changes pruned
    pub changes_pruned: u64,

    /// Number of blocks freed
    pub blocks_freed: u64,

    /// Estimated space saved (in bytes)
    pub bytes_saved: u64,

    /// Duration of pruning operation in milliseconds
    pub duration_ms: u64,

    /// Cutoff block number (blocks below this were pruned)
    pub cutoff_block: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::history::StateHistory;
    use norn_common::types::Address;
    use num_bigint::BigUint;

    #[tokio::test]
    async fn test_pruning_config_default() {
        let config = PruningConfig::default();
        assert_eq!(config.min_blocks_to_keep, 1000);
        assert_eq!(config.max_blocks_to_keep, 10000);
        assert!(config.auto_prune);
        assert_eq!(config.prune_interval, 100);
    }

    #[tokio::test]
    async fn test_pruning_config_light() {
        let config = PruningConfig::light();
        assert_eq!(config.min_blocks_to_keep, 5000);
        assert_eq!(config.max_blocks_to_keep, 50000);
    }

    #[tokio::test]
    async fn test_pruning_config_aggressive() {
        let config = PruningConfig::aggressive();
        assert_eq!(config.min_blocks_to_keep, 100);
        assert_eq!(config.max_blocks_to_keep, 1000);
    }

    #[tokio::test]
    async fn test_pruning_config_archival() {
        let config = PruningConfig::archival();
        assert!(!config.auto_prune);
        assert_eq!(config.prune_interval, u64::MAX);
    }

    #[tokio::test]
    async fn test_pruning_manager_creation() {
        let history = Arc::new(StateHistory::new(10));
        let config = PruningConfig::default();
        let manager = StatePruningManager::new(config, history);

        let stats = manager.get_stats().await;
        assert_eq!(stats.total_prunings, 0);
    }

    #[tokio::test]
    async fn test_should_prune() {
        let history = Arc::new(StateHistory::new(10));
        let config = PruningConfig {
            prune_interval: 100,
            ..Default::default()
        };
        let manager = StatePruningManager::new(config, history);

        // At block 0, should not prune
        assert!(!manager.should_prune(0).await);

        // At block 99, should not prune yet
        assert!(!manager.should_prune(99).await);

        // At block 100, should prune
        assert!(manager.should_prune(100).await);

        // At block 200, should prune
        assert!(manager.should_prune(200).await);
    }

    #[tokio::test]
    async fn test_pruning_stats_initial() {
        let stats = PruningStats::default();
        assert_eq!(stats.total_prunings, 0);
        assert_eq!(stats.snapshots_pruned, 0);
        assert_eq!(stats.changes_pruned, 0);
    }

    #[tokio::test]
    async fn test_space_estimation() {
        let history = Arc::new(StateHistory::new(10));
        let config = PruningConfig::default();
        let manager = StatePruningManager::new(config, history);

        let bytes = manager.estimate_space_saved(100, 10, 100);
        assert!(bytes > 0);

        // 10 snapshots * 1MB + 100 changes * 500 bytes
        let expected = 10 * 1_000_000 + 100 * 500;
        assert_eq!(bytes, expected);
    }
}
