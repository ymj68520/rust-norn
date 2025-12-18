//! Block synchronization module
//! 
//! This module handles block synchronization between peers.

use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tokio::time::interval;
use norn_core::blockchain::Blockchain;
use norn_network::NetworkService;
use norn_common::types::{Block, Hash};
use tracing::{info, debug, warn, error};

/// Block syncer state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncState {
    /// Idle, not syncing
    Idle,
    /// Syncing headers
    SyncingHeaders,
    /// Syncing blocks
    SyncingBlocks,
    /// Sync complete
    Complete,
    /// Error during sync
    Error,
}

/// Sync configuration
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Number of blocks to request in each batch
    pub batch_size: usize,
    /// Timeout for sync operations in seconds
    pub timeout_secs: u64,
    /// Interval between sync checks in seconds
    pub check_interval_secs: u64,
    /// Maximum number of pending block requests
    pub max_pending_requests: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            timeout_secs: 30,
            check_interval_secs: 5,
            max_pending_requests: 10,
        }
    }
}

/// Block syncer for synchronizing blockchain state with peers
pub struct BlockSyncer {
    blockchain: Arc<Blockchain>,
    network: Arc<NetworkService>,
    config: SyncConfig,
    state: Arc<RwLock<SyncState>>,
    target_height: Arc<RwLock<i64>>,
    pending_blocks: Arc<RwLock<HashMap<i64, PendingBlock>>>,
}

/// Pending block request
#[derive(Debug, Clone)]
struct PendingBlock {
    height: i64,
    requested_at: std::time::Instant,
    retry_count: u32,
}

impl BlockSyncer {
    /// Create a new block syncer
    pub fn new(blockchain: Arc<Blockchain>, network: Arc<NetworkService>) -> Self {
        Self::with_config(blockchain, network, SyncConfig::default())
    }

    /// Create with custom config
    pub fn with_config(
        blockchain: Arc<Blockchain>,
        network: Arc<NetworkService>,
        config: SyncConfig,
    ) -> Self {
        Self {
            blockchain,
            network,
            config,
            state: Arc::new(RwLock::new(SyncState::Idle)),
            target_height: Arc::new(RwLock::new(0)),
            pending_blocks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start the syncer
    pub async fn start(&self) {
        info!("Block syncer started");
        
        let mut timer = interval(Duration::from_secs(self.config.check_interval_secs));
        
        loop {
            timer.tick().await;
            
            // Check if we need to sync
            if let Err(e) = self.sync_check().await {
                error!("Sync check failed: {}", e);
            }
        }
    }

    /// Perform a sync check
    async fn sync_check(&self) -> anyhow::Result<()> {
        let local_height = {
            let latest = self.blockchain.latest_block.read().await;
            latest.header.height
        };

        let target = *self.target_height.read().await;
        
        if local_height >= target {
            let mut state = self.state.write().await;
            if *state != SyncState::Idle {
                *state = SyncState::Complete;
                info!("Sync complete at height {}", local_height);
            }
            return Ok(());
        }

        // Need to sync
        let mut state = self.state.write().await;
        *state = SyncState::SyncingBlocks;
        drop(state);

        // Request missing blocks
        let from = local_height + 1;
        let to = std::cmp::min(from + self.config.batch_size as i64, target + 1);
        
        self.request_blocks_internal(from, to).await;
        
        Ok(())
    }

    /// Update target height from peer announcement
    pub async fn update_target_height(&self, height: i64) {
        let mut target = self.target_height.write().await;
        if height > *target {
            info!("Updated target height to {}", height);
            *target = height;
        }
    }

    /// Get current sync state
    pub async fn get_state(&self) -> SyncState {
        *self.state.read().await
    }

    /// Get current target height
    pub async fn get_target_height(&self) -> i64 {
        *self.target_height.read().await
    }

    /// Check if currently syncing
    pub async fn is_syncing(&self) -> bool {
        let state = self.state.read().await;
        matches!(*state, SyncState::SyncingHeaders | SyncState::SyncingBlocks)
    }

    /// Get sync progress (0.0 to 1.0)
    pub async fn get_progress(&self) -> f64 {
        let local_height = {
            let latest = self.blockchain.latest_block.read().await;
            latest.header.height
        };
        let target = *self.target_height.read().await;
        
        if target == 0 {
            return 1.0;
        }
        
        (local_height as f64) / (target as f64)
    }

    /// Request blocks from peer
    async fn request_blocks_internal(&self, from_height: i64, to_height: i64) {
        debug!("Requesting blocks from {} to {}", from_height, to_height);
        
        // Track pending requests
        let mut pending = self.pending_blocks.write().await;
        let now = std::time::Instant::now();
        
        for height in from_height..to_height {
            if pending.len() >= self.config.max_pending_requests {
                break;
            }
            
            if !pending.contains_key(&height) {
                pending.insert(height, PendingBlock {
                    height,
                    requested_at: now,
                    retry_count: 0,
                });
                
                // Create block request message
                let msg = BlockRequest { height }.to_bytes();
                
                // Broadcast request
                if let Err(e) = self.network.command_tx.send(
                    norn_network::service::NetworkCommand::BroadcastBlock(msg)
                ).await {
                    warn!("Failed to send block request: {}", e);
                }
            }
        }
    }

    /// Request blocks from peer (public interface)
    pub async fn request_blocks(&self, from_height: i64, to_height: i64) {
        self.request_blocks_internal(from_height, to_height).await;
    }

    /// Handle received block
    pub async fn handle_block(&self, block: Block) -> anyhow::Result<()> {
        let height = block.header.height;
        debug!("Received block at height {}", height);
        
        // Remove from pending
        let mut pending = self.pending_blocks.write().await;
        pending.remove(&height);
        drop(pending);
        
        // Validate block height
        let expected_height = {
            let latest = self.blockchain.latest_block.read().await;
            latest.header.height + 1
        };
        
        if height != expected_height {
            warn!("Received block {} but expected {}", height, expected_height);
            return Ok(());
        }
        
        // Save block
        self.blockchain.save_block(&block).await?;
        info!("Applied block at height {}", height);
        
        Ok(())
    }

    /// Handle received blocks (batch)
    pub async fn handle_blocks(&self, blocks: Vec<Block>) {
        for block in blocks {
            if let Err(e) = self.handle_block(block).await {
                error!("Failed to handle block: {}", e);
            }
        }
    }

    /// Clean up timed out requests
    pub async fn cleanup_pending(&self) {
        let timeout = Duration::from_secs(self.config.timeout_secs);
        let now = std::time::Instant::now();
        
        let mut pending = self.pending_blocks.write().await;
        pending.retain(|_, req| {
            now.duration_since(req.requested_at) < timeout
        });
    }
}

/// Block request message
struct BlockRequest {
    height: i64,
}

impl BlockRequest {
    fn to_bytes(&self) -> Vec<u8> {
        self.height.to_le_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_state() {
        assert_eq!(SyncState::Idle, SyncState::Idle);
        assert_ne!(SyncState::Idle, SyncState::Complete);
    }

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.timeout_secs, 30);
    }
}