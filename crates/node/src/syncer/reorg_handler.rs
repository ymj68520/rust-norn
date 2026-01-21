//! Chain reorganization handler
//!
//! Handles blockchain reorganizations when a longer chain is discovered.

use std::sync::Arc;
use norn_core::blockchain::Blockchain;
use norn_common::types::{Block, Hash};
use tracing::{info, warn, debug, error};
use anyhow::Result;

/// Reorganization result
#[derive(Debug)]
pub struct ReorgResult {
    /// Old chain tip hash
    pub old_tip: Hash,
    /// New chain tip hash
    pub new_tip: Hash,
    /// Number of blocks reverted
    pub reverted_count: u64,
    /// Number of blocks applied
    pub applied_count: u64,
    /// Success/failure
    pub success: bool,
}

/// Reorganization handler for blockchain forks
pub struct ReorgHandler {
    blockchain: Arc<Blockchain>,
}

impl ReorgHandler {
    /// Create a new reorg handler
    pub fn new(blockchain: Arc<Blockchain>) -> Self {
        Self { blockchain }
    }

    /// Check if a reorganization is needed
    /// Returns true if the new block represents a chain that should replace our current chain
    pub async fn needs_reorg(&self, new_block: &Block) -> bool {
        let current_tip = self.blockchain.latest_block.read().await;

        // If the new block's parent is our current tip, no reorg needed
        if new_block.header.prev_block_hash == current_tip.header.block_hash {
            debug!("New block extends current chain, no reorg needed");
            return false;
        }

        // If the new block's height is not greater than our current height, no reorg needed
        if new_block.header.height <= current_tip.header.height {
            debug!("New block height {} is not greater than current height {}, no reorg needed",
                   new_block.header.height, current_tip.header.height);
            return false;
        }

        // Check if we can find the new block's parent in our chain
        // If we can't, this might be a completely different chain
        if self.blockchain.get_block_by_hash(&new_block.header.prev_block_hash).await.is_none() {
            warn!("New block's parent not found in our chain, reorg may be needed");
            // We should check if the new chain is longer/better
            return true;
        }

        // For now, if the new block is higher and we can't extend directly, assume reorg needed
        // In a production system, you'd compare total difficulty/work here
        info!("New block represents a potential chain reorganization");
        true
    }

    /// Execute a chain reorganization
    ///
    /// # Arguments
    /// * `new_chain` - The new chain of blocks to apply, ordered from lowest height to highest
    ///
    /// # Returns
    /// ReorgResult containing details of the reorganization
    pub async fn execute_reorg(&self, new_chain: Vec<Block>) -> Result<ReorgResult> {
        if new_chain.is_empty() {
            return Ok(ReorgResult {
                old_tip: Hash::default(),
                new_tip: Hash::default(),
                reverted_count: 0,
                applied_count: 0,
                success: false,
            });
        }

        let old_tip = self.blockchain.latest_block.read().await.clone();
        let old_tip_hash = old_tip.header.block_hash;
        let new_tip_hash = new_chain.last().map(|b| b.header.block_hash).unwrap_or_default();

        info!("Starting chain reorganization from {:?} to {:?}", old_tip_hash, new_tip_hash);

        // Find the fork point
        let fork_point = self.find_fork_point_internal(&old_tip, &new_chain).await;

        let fork_height = match fork_point {
            Some(ref hash) => {
                self.blockchain.get_block_by_hash(hash)
                    .await
                    .map(|b| b.header.height)
                    .unwrap_or(0)
            }
            None => {
                error!("Could not find fork point, aborting reorg");
                return Ok(ReorgResult {
                    old_tip: old_tip_hash,
                    new_tip: new_tip_hash,
                    reverted_count: 0,
                    applied_count: 0,
                    success: false,
                });
            }
        };

        // Calculate how many blocks to revert
        let blocks_to_revert = (old_tip.header.height - fork_height) as u64;

        // Revert blocks from old chain (back to fork point + 1)
        info!("Reverting {} blocks from old chain", blocks_to_revert);
        let mut reverted_count = 0u64;

        // Note: In this implementation, we're not actually "undoing" blocks in the database
        // Instead, we're updating the latest_block pointer. The new blocks will overwrite
        // the old chain at the same heights.
        // For a full implementation, you'd need state rollback logic here.

        let current_height = old_tip.header.height;
        for height in (fork_height + 1)..=current_height {
            debug!("Would revert block at height {}", height);
            reverted_count += 1;
        }

        // Apply new chain blocks
        info!("Applying {} blocks from new chain", new_chain.len());
        let mut applied_count = 0u64;

        for block in &new_chain {
            // Skip blocks that are already in our chain (before fork point)
            if block.header.height <= fork_height {
                continue;
            }

            debug!("Applying block at height {}", block.header.height);

            // Validate and commit the block
            // In a full implementation, you'd validate signatures, state transitions, etc.
            if let Err(e) = self.blockchain.commit_block(block).await {
                error!("Failed to commit block during reorg: {:?}", e);
                return Ok(ReorgResult {
                    old_tip: old_tip_hash,
                    new_tip: new_tip_hash,
                    reverted_count,
                    applied_count,
                    success: false,
                });
            }

            applied_count += 1;
        }

        info!("Chain reorganization completed: reverted {} blocks, applied {} blocks",
              reverted_count, applied_count);

        Ok(ReorgResult {
            old_tip: old_tip_hash,
            new_tip: new_tip_hash,
            reverted_count,
            applied_count,
            success: true,
        })
    }

    /// Find the common ancestor (fork point) between two chains
    ///
    /// This is the internal implementation that works with an actual chain
    pub async fn find_fork_point_internal(&self, old_tip: &Block, new_chain: &[Block]) -> Option<Hash> {
        debug!("Finding fork point between old chain (height {}) and new chain ({} blocks)",
               old_tip.header.height, new_chain.len());

        // Start from the new chain's first block and work backwards
        // until we find a block that exists in our current chain
        for block in new_chain.iter().rev() {
            // Check if this block exists in our current chain
            if let Some(existing) = self.blockchain.get_block_by_hash(&block.header.block_hash).await {
                // Found a common block
                debug!("Found fork point at height {}: {:?}",
                       existing.header.height, existing.header.block_hash);
                return Some(existing.header.block_hash);
            }

            // Check if the parent of this block exists in our chain
            let parent_hash = block.header.prev_block_hash;
            if let Some(parent) = self.blockchain.get_block_by_hash(&parent_hash).await {
                debug!("Found fork point at height {}: {:?}",
                       parent.header.height, parent.header.block_hash);
                return Some(parent.header.block_hash);
            }
        }

        // If we haven't found a fork point yet, try walking back from the old tip
        let mut current_hash = old_tip.header.prev_block_hash;

        // Limit how far back we search to avoid infinite loops
        let max_iterations = 1000;
        let mut iterations = 0;

        while current_hash.0 != [0u8; 32] && iterations < max_iterations {
            // Check if this block is an ancestor of any block in the new chain
            for block in new_chain {
                if block.header.block_hash == current_hash {
                    debug!("Found fork point by walking back from old tip: {:?} at height {}",
                           current_hash, block.header.height);
                    return Some(current_hash);
                }
            }

            // Move to parent
            if let Some(block) = self.blockchain.get_block_by_hash(&current_hash).await {
                current_hash = block.header.prev_block_hash;
            } else {
                break;
            }

            iterations += 1;
        }

        // If we still haven't found a fork point, check if we share genesis
        if let Some(genesis) = self.blockchain.get_block_by_height(0).await {
            for block in new_chain {
                if block.header.height == 0 && block.header.block_hash == genesis.header.block_hash {
                    debug!("Fork point is genesis block");
                    return Some(genesis.header.block_hash);
                }
            }
        }

        warn!("Could not find fork point");
        None
    }

    /// Find the common ancestor between two blocks by their hashes
    /// This is a simpler API that loads the blocks first
    pub async fn find_fork_point(&self, block_a: &Hash, block_b: &Hash) -> Option<Hash> {
        // Load both blocks
        let block_a = self.blockchain.get_block_by_hash(block_a).await?;
        let block_b = self.blockchain.get_block_by_hash(block_b).await?;

        // Create a minimal chain for block_b to use with find_fork_point_internal
        let chain_b = vec![block_b];

        self.find_fork_point_internal(&block_a, &chain_b).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_core::blockchain::Blockchain;
    use norn_storage::SledDB;
    use norn_common::types::{Block, BlockHeader, Hash, Transaction, TransactionBody, Address};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Helper to create a test block
    fn create_test_block(height: i64, prev_hash: Hash) -> Block {
        Block {
            header: BlockHeader {
                timestamp: 1000 + height as i64,
                prev_block_hash: prev_hash,
                block_hash: Hash::default(),
                merkle_root: Hash::default(),
                height,
                public_key: norn_common::types::PublicKey::default(),
                params: vec![],
                gas_limit: 1000000,
            },
            transactions: vec![],
        }
    }

    /// Helper to create a test blockchain
    async fn create_test_blockchain() -> (Arc<Blockchain>, Arc<SledDB>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path()).unwrap());
        let blockchain = Blockchain::new_with_fixed_genesis(db.clone()).await;

        // Add some blocks to the chain
        for i in 1..=3 {
            let latest_block = blockchain.latest_block.read().await.clone();
            let mut block = create_test_block(i, latest_block.header.block_hash);
            // Update the block hash to be unique
            block.header.block_hash = Hash([i as u8; 32]);
            blockchain.add_block(block).await;
        }

        (blockchain, db, temp_dir)
    }

    #[tokio::test]
    async fn test_reorg_handler_creation() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain);

        // Handler should be created successfully
        assert_eq!(handler.blockchain.latest_block.read().await.header.height, 3);
    }

    #[tokio::test]
    async fn test_needs_reorg_same_chain() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain);

        // Create a block that extends the current chain
        let current_tip = handler.blockchain.latest_block.read().await.clone();
        let new_block = create_test_block(4, current_tip.header.block_hash);

        // Should not need reorg
        let needs_reorg = handler.needs_reorg(&new_block).await;
        assert!(!needs_reorg);
    }

    #[tokio::test]
    async fn test_needs_reorg_different_chain() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain);

        // Create a block with different parent (fork)
        let fork_parent_hash = Hash([1u8; 32]);
        let new_block = create_test_block(5, fork_parent_hash);

        // Should need reorg since parent not found
        let needs_reorg = handler.needs_reorg(&new_block).await;
        assert!(needs_reorg);
    }

    #[tokio::test]
    async fn test_needs_reorg_lower_height() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain);

        // Create a block with same height as current tip (not greater)
        let same_height_block = create_test_block(3, Hash([2u8; 32]));

        // Should not need reorg (height not greater)
        let needs_reorg = handler.needs_reorg(&same_height_block).await;
        assert!(!needs_reorg);
    }

    #[tokio::test]
    async fn test_find_fork_point_same_chain() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain);

        let current_tip = handler.blockchain.latest_block.read().await.clone();
        let tip_hash: Hash = current_tip.header.block_hash;

        // Find fork point between same block and itself
        let fork_point = handler.find_fork_point(&tip_hash, &tip_hash).await;

        assert!(fork_point.is_some());
        assert_eq!(fork_point.unwrap(), tip_hash);
    }

    #[tokio::test]
    async fn test_find_fork_point_parent() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain.clone());

        let current_tip = handler.blockchain.latest_block.read().await.clone();
        let tip_hash: Hash = current_tip.header.block_hash;

        // Get the parent block
        let parent_block = handler.blockchain.get_block_by_hash(&current_tip.header.prev_block_hash).await;
        assert!(parent_block.is_some());
        let parent_block = parent_block.unwrap();

        // Find fork point between tip and its parent
        let fork_point = handler.find_fork_point(&tip_hash, &parent_block.header.block_hash).await;

        assert!(fork_point.is_some());
        // Should find the parent block as the fork point
        assert_eq!(fork_point.unwrap(), parent_block.header.block_hash);
    }

    #[tokio::test]
    async fn test_find_fork_point_no_match() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain);

        let hash1: Hash = Hash([1u8; 32]);
        let hash2: Hash = Hash([2u8; 32]);

        // Find fork point between two unrelated hashes
        let fork_point = handler.find_fork_point(&hash1, &hash2).await;

        // Should return None (no common ancestor)
        assert!(fork_point.is_none());
    }

    #[tokio::test]
    async fn test_execute_reorg_empty_chain() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain);

        // Try to reorg with empty chain
        let result = handler.execute_reorg(vec![]).await;

        assert!(!result.unwrap().success);
    }

    #[tokio::test]
    async fn test_execute_reorg_same_chain() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain.clone());

        // Get blocks from current chain
        let mut new_chain = vec![];
        for i in 0..=3 {
            if let Some(block) = blockchain.get_block_by_height(i).await {
                new_chain.push(block);
            }
        }

        // Try to reorg to same chain
        let result = handler.execute_reorg(new_chain).await;

        assert!(result.is_ok());
        let reorg_result = result.unwrap();
        // Should succeed
        assert!(reorg_result.success);
    }

    #[tokio::test]
    async fn test_execute_reorg_to_higher_chain() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain.clone());

        // Create new chain that extends from current tip
        let current_tip = blockchain.latest_block.read().await.clone();
        let mut new_chain = vec![];

        // Add some blocks after current tip
        let mut prev_hash = current_tip.header.block_hash;
        for i in 4..=6 {
            let mut block = create_test_block(i, prev_hash);
            // Update block hash to make it unique
            block.header.block_hash = Hash([i as u8; 32]);
            new_chain.push(block);
            prev_hash = Hash([i as u8; 32]);
        }

        // Execute reorg
        let result = handler.execute_reorg(new_chain).await;

        assert!(result.is_ok());
        let reorg_result = result.unwrap();
        assert!(reorg_result.success);
        // Should apply 3 new blocks
        assert_eq!(reorg_result.applied_count, 3);
        // Should not revert any blocks (extending from current tip)
        assert_eq!(reorg_result.reverted_count, 0);
    }

    #[tokio::test]
    async fn test_reorg_result_fields() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain.clone());

        let old_tip = blockchain.latest_block.read().await.clone();
        let old_tip_hash: Hash = old_tip.header.block_hash;

        // Create new chain
        let mut new_chain = vec![];
        let mut prev_hash = old_tip.header.prev_block_hash;
        for i in 1..=4 {
            let mut block = create_test_block(i, prev_hash);
            block.header.block_hash = Hash([i as u8; 32]);
            new_chain.push(block);
            prev_hash = Hash([i as u8; 32]);
        }

        let new_tip_hash: Hash = new_chain.last().unwrap().header.block_hash;

        // Execute reorg
        let result = handler.execute_reorg(new_chain).await.unwrap();

        // Check result fields
        assert_eq!(result.old_tip, old_tip_hash);
        assert_eq!(result.new_tip, new_tip_hash);
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_find_fork_point_between_chains() {
        let (blockchain, _db, _temp_dir) = create_test_blockchain().await;
        let handler = ReorgHandler::new(blockchain.clone());

        let current_tip = handler.blockchain.latest_block.read().await.clone();

        // Create a fork that branches from height 2
        // Get block at height 2
        let mut height_2_block = None;
        for i in (0..=3).rev() {
            if let Some(block) = handler.blockchain.get_block_by_height(i).await {
                if block.header.height == 2 {
                    height_2_block = Some(block);
                    break;
                }
            }
        }

        assert!(height_2_block.is_some());
        let height_2_block = height_2_block.unwrap();
        let fork_point_hash: Hash = height_2_block.header.block_hash;

        // Create competing chain
        let mut competing_chain = vec![];
        let mut prev_hash = fork_point_hash;

        for i in 3..=5 {
            let mut block = create_test_block(i, prev_hash);
            // Modify hash to make it different
            let new_hash = Hash([10 + i as u8; 32]);
            block.header.block_hash = new_hash;
            competing_chain.push(block);
            prev_hash = new_hash;
        }

        // Find fork point between current tip and competing chain
        let fork_result: Option<Hash> = handler.find_fork_point_internal(
            &current_tip,
            &competing_chain
        ).await;

        assert!(fork_result.is_some());
        assert_eq!(fork_result.unwrap(), fork_point_hash);
    }
}
