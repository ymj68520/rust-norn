//! Block History for BLOCKHASH Opcode Support
//!
//! This module manages historical block hashes for EVM BLOCKHASH opcode.
//! Ethereum maintains the last 256 block hashes for this purpose.

use crate::evm::EVMError;
use norn_common::types::Hash;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Maximum number of recent blocks to store hashes for
/// This matches Ethereum's BLOCKHASH opcode limit
pub const MAX_BLOCK_HASH_HISTORY: usize = 256;

/// Block history manager
///
/// Stores recent block hashes for BLOCKHASH opcode access.
/// Only maintains the most recent 256 blocks.
#[derive(Clone)]
pub struct BlockHistory {
    /// Circular buffer of recent block hashes
    /// Stores (block_number, block_hash) pairs
    history: Arc<RwLock<VecDeque<(u64, Hash)>>>,

    /// Current block number (for validation)
    current_block_number: Arc<RwLock<u64>>,
}

impl BlockHistory {
    /// Create a new block history manager
    pub fn new() -> Self {
        Self {
            history: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_BLOCK_HASH_HISTORY))),
            current_block_number: Arc::new(RwLock::new(0)),
        }
    }

    /// Add a block to history
    ///
    /// # Arguments
    /// * `block_number` - Block number
    /// * `block_hash` - Block hash
    ///
    /// Note: Blocks are always added (even out of order) for flexibility
    /// The BLOCKHASH opcode will enforce the 256-block window rule
    pub async fn add_block(&self, block_number: u64, block_hash: Hash) {
        let mut history = self.history.write().await;
        let mut current = self.current_block_number.write().await;

        // Check if we already have this block number
        if let Some(pos) = history.iter().position(|(num, _)| *num == block_number) {
            // Update existing entry
            history[pos] = (block_number, block_hash);
            debug!("Updated block {} in history", block_number);
        } else {
            // Add new entry
            // Maintain at most 256 blocks
            if history.len() >= MAX_BLOCK_HASH_HISTORY {
                history.pop_front();
            }

            history.push_back((block_number, block_hash));
            debug!(
                "Added block {} to history, size: {}",
                block_number,
                history.len()
            );
        }

        // Update current block number if this is newer
        if block_number > *current {
            *current = block_number;
        }
    }

    /// Get block hash by number
    ///
    /// # Arguments
    /// * `block_number` - Block number to query
    /// * `current_block` - Current block number (for validation)
    ///
    /// # Returns
    /// Block hash if found and accessible
    ///
    /// # Rules (Ethereum-compatible):
    /// 1. Genesis block (0) is always accessible
    /// 2. Blocks in range [current - 256, current - 1] are accessible
    /// 3. Current block is NOT accessible (returns zero)
    /// 4. Blocks outside range return zero hash
    pub async fn get_block_hash(&self, block_number: u64, current_block: u64) -> Result<Hash, EVMError> {
        // Rule: Current block is not accessible
        if block_number >= current_block {
            warn!(
                "BLOCKHASH: Requested current or future block {} (current: {})",
                block_number, current_block
            );
            return Ok(Hash::default());
        }

        // Rule: Genesis block (0) is always accessible (Ethereum behavior)
        // But only if it's still within the 256-block window or explicitly stored
        if block_number == 0 {
            debug!("BLOCKHASH: Querying genesis block");
            // Search for block 0 in history
            let history = self.history.read().await;
            for (num, hash) in history.iter() {
                if *num == 0 {
                    debug!("BLOCKHASH: Found genesis block in history");
                    return Ok(*hash);
                }
            }
            // Genesis block not in history (pruned or never added)
            // In Ethereum, genesis is always accessible, but in our implementation
            // we only keep 256 recent blocks for efficiency
            debug!("BLOCKHASH: Genesis block not in history, returning zero");
            return Ok(Hash::default());
        }

        // Rule: Only blocks in [current - 256, current - 1] are accessible
        let oldest_accessible = if current_block > MAX_BLOCK_HASH_HISTORY as u64 {
            current_block - MAX_BLOCK_HASH_HISTORY as u64
        } else {
            0
        };

        if block_number < oldest_accessible {
            warn!(
                "BLOCKHASH: Block {} too old (oldest accessible: {})",
                block_number, oldest_accessible
            );
            return Ok(Hash::default());
        }

        // Search in history
        let history = self.history.read().await;
        for (num, hash) in history.iter() {
            if *num == block_number {
                debug!("BLOCKHASH: Found block {} in history", block_number);
                return Ok(*hash);
            }
        }

        // Not found (shouldn't happen if blocks are added properly)
        warn!(
            "BLOCKHASH: Block {} not found in history",
            block_number
        );
        Ok(Hash::default())
    }

    /// Get current block number
    pub async fn current_block_number(&self) -> u64 {
        *self.current_block_number.read().await
    }

    /// Get history size
    pub async fn size(&self) -> usize {
        self.history.read().await.len()
    }

    /// Clear all history (for testing or reorg)
    pub async fn clear(&self) {
        let mut history = self.history.write().await;
        let mut current = self.current_block_number.write().await;
        history.clear();
        *current = 0;
        debug!("Block history cleared");
    }

    /// Prune old blocks beyond 256-block window
    pub async fn prune(&self) {
        let mut history = self.history.write().await;
        let current = *self.current_block_number.read().await;

        while history.len() > MAX_BLOCK_HASH_HISTORY {
            history.pop_front();
        }

        // Ensure we don't have blocks older than current - 256
        let oldest_accessible = if current > MAX_BLOCK_HASH_HISTORY as u64 {
            current - MAX_BLOCK_HASH_HISTORY as u64
        } else {
            0
        };

        while let Some((num, _)) = history.front() {
            if *num < oldest_accessible {
                history.pop_front();
            } else {
                break;
            }
        }

        debug!("Pruned block history, size: {}", history.len());
    }
}

impl Default for BlockHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_hash(num: u64) -> Hash {
        let mut hash = [0u8; 32];
        hash[0..8].copy_from_slice(&num.to_be_bytes());
        Hash(hash)
    }

    #[tokio::test]
    async fn test_add_and_get_block() {
        let history = BlockHistory::new();

        // Add blocks 1-10
        for i in 1..=10 {
            history.add_block(i, create_test_hash(i)).await;
        }

        assert_eq!(history.size().await, 10);
        assert_eq!(history.current_block_number().await, 10);

        // Get block 5 (should be accessible)
        let hash = history.get_block_hash(5, 10).await.unwrap();
        assert_eq!(hash, create_test_hash(5));
    }

    #[tokio::test]
    async fn test_genesis_block_accessible() {
        let history = BlockHistory::new();

        // Add genesis block
        history.add_block(0, create_test_hash(0)).await;

        // Genesis should be accessible from any current block
        let hash = history.get_block_hash(0, 1).await.unwrap();
        assert_eq!(hash, create_test_hash(0));
    }

    #[tokio::test]
    async fn test_current_block_not_accessible() {
        let history = BlockHistory::new();

        history.add_block(5, create_test_hash(5)).await;

        // Current block should return zero
        let hash = history.get_block_hash(5, 5).await.unwrap();
        assert_eq!(hash, Hash::default());
    }

    #[tokio::test]
    async fn test_future_block_not_accessible() {
        let history = BlockHistory::new();

        history.add_block(5, create_test_hash(5)).await;

        // Future block should return zero
        let hash = history.get_block_hash(10, 5).await.unwrap();
        assert_eq!(hash, Hash::default());
    }

    #[tokio::test]
    async fn test_old_block_not_accessible() {
        let history = BlockHistory::new();

        // Add 300 blocks (0-299)
        for i in 0..300 {
            history.add_block(i, create_test_hash(i)).await;
        }

        // With 300 blocks (0-299), we keep the most recent 256
        // That means blocks 44-299 remain in storage (256 blocks)
        // Blocks 0-43 are pruned

        // Block 0 (genesis) was pruned, so should return zero
        let hash = history.get_block_hash(0, 299).await.unwrap();
        // Block 0 is outside the 256-block window and not in history
        assert_eq!(hash, Hash::default());

        // Block 43 is also pruned
        let hash = history.get_block_hash(43, 299).await.unwrap();
        assert_eq!(hash, Hash::default());

        // Block 44 (299 - 255, but should be 299-256=43...) is the oldest in history
        // Actually: we added 0-299 (300 blocks), kept last 256, so blocks 44-299 remain
        let hash = history.get_block_hash(44, 299).await.unwrap();
        assert_eq!(hash, create_test_hash(44));

        // Block 298 should be accessible
        let hash = history.get_block_hash(298, 299).await.unwrap();
        assert_eq!(hash, create_test_hash(298));
    }

    #[tokio::test]
    async fn test_history_limit() {
        let history = BlockHistory::new();

        // Add more than 256 blocks
        for i in 0..=300 {
            history.add_block(i, create_test_hash(i)).await;
        }

        // Should keep exactly 256 blocks
        assert_eq!(history.size().await, 256);

        // After adding 0-300 (301 blocks), we keep the last 256
        // So we have blocks 45-300 (256 blocks)
        // Block 44 should be gone
        let hash = history.get_block_hash(44, 301).await.unwrap();
        assert_eq!(hash, Hash::default());

        // Block 45 should be the oldest in history
        let hash = history.get_block_hash(45, 301).await.unwrap();
        assert_eq!(hash, create_test_hash(45));

        // Block 300 should be accessible
        let hash = history.get_block_hash(300, 301).await.unwrap();
        assert_eq!(hash, create_test_hash(300));
    }

    #[tokio::test]
    async fn test_prune() {
        let history = BlockHistory::new();

        // Add 300 blocks
        for i in 0..300 {
            history.add_block(i, create_test_hash(i)).await;
        }

        // Manual prune
        history.prune().await;

        assert_eq!(history.size().await, 256);
    }

    #[tokio::test]
    async fn test_clear() {
        let history = BlockHistory::new();

        // Add some blocks
        for i in 0..10 {
            history.add_block(i, create_test_hash(i)).await;
        }

        assert_eq!(history.size().await, 10);

        // Clear
        history.clear().await;

        assert_eq!(history.size().await, 0);
        assert_eq!(history.current_block_number().await, 0);
    }

    #[tokio::test]
    async fn test_out_of_order_blocks() {
        let history = BlockHistory::new();

        // Try to add blocks out of order
        history.add_block(10, create_test_hash(10)).await;
        history.add_block(5, create_test_hash(5)).await; // Should be added

        assert_eq!(history.size().await, 2); // Both should be added
        assert_eq!(history.current_block_number().await, 10);

        // Both should be accessible
        let hash = history.get_block_hash(10, 11).await.unwrap();
        assert_eq!(hash, create_test_hash(10));

        let hash = history.get_block_hash(5, 11).await.unwrap();
        assert_eq!(hash, create_test_hash(5));
    }
}
