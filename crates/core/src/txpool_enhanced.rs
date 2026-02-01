//! Enhanced Transaction Pool with Priority Queue and EIP-1559 Support
//!
//! This module provides an advanced transaction pool implementation with:
//! - Priority-based transaction ordering by gas price
//! - EIP-1559 transaction replacement
//! - Pending transaction tracking
//! - Transaction expiration and cleanup

use crate::txpool::{ChainReader, TransactionPool, TxPoolStats as CommonTxPoolStats};
use norn_common::types::{Hash, Transaction, Address};
use std::collections::{HashMap, BinaryHeap};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use serde::{Deserialize, Serialize};
use async_trait::async_trait;

const MAX_TX_POOL_SIZE: usize = 20480;
const MAX_TX_PACKAGE_COUNT: usize = 10000;
const TX_EXPIRATION_TIME: i64 = 3600; // 1 hour in seconds

/// Transaction with priority information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrioritizedTransaction {
    /// The transaction
    pub tx: Transaction,
    /// Effective gas price (for sorting)
    pub effective_gas_price: u64,
    /// Time added to pool (for expiration)
    pub added_at: i64,
    /// Nonce for replacement tracking
    pub nonce: i64,
    /// Sender address
    pub sender: Address,
}

impl PrioritizedTransaction {
    fn new(tx: Transaction) -> Self {
        let effective_gas_price = tx.body.max_fee_per_gas
            .or(tx.body.gas_price)
            .unwrap_or(0) as u64;

        let added_at = chrono::Utc::now().timestamp();
        let nonce = tx.body.nonce;
        let sender = tx.body.address;

        Self {
            tx,
            effective_gas_price,
            added_at,
            nonce,
            sender,
        }
    }

    /// Check if transaction is expired
    fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        (now - self.added_at) > TX_EXPIRATION_TIME
    }
}

/// Priority ordering for transactions (max-heap by gas price)
impl PartialEq for PrioritizedTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.effective_gas_price == other.effective_gas_price
    }
}

impl Eq for PrioritizedTransaction {}

impl PartialOrd for PrioritizedTransaction {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTransaction {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher gas price = higher priority (max-heap pops largest first)
        self.effective_gas_price.cmp(&other.effective_gas_price)
            .then_with(|| other.added_at.cmp(&self.added_at))  // Earlier timestamp = higher priority
    }
}

/// Enhanced transaction pool with priority queue
pub struct EnhancedTxPool {
    /// All pending transactions by hash
    transactions: Arc<RwLock<HashMap<Hash, PrioritizedTransaction>>>,
    /// Priority queue for transaction selection
    priority_queue: Arc<RwLock<BinaryHeap<PrioritizedTransaction>>>,
    /// Pending transactions by sender and nonce (for replacement)
    pending_by_sender: Arc<RwLock<HashMap<Address, HashMap<i64, Hash>>>>,
    /// Pool size counter
    size: Arc<RwLock<usize>>,
}

impl EnhancedTxPool {
    /// Create a new enhanced transaction pool
    pub fn new() -> Self {
        Self {
            transactions: Arc::new(RwLock::new(HashMap::new())),
            priority_queue: Arc::new(RwLock::new(BinaryHeap::new())),
            pending_by_sender: Arc::new(RwLock::new(HashMap::new())),
            size: Arc::new(RwLock::new(0)),
        }
    }

    /// Add a transaction to the pool
    pub async fn add(&self, tx: Transaction) -> Result<(), TxPoolError> {
        // Check pool size limit first (without holding the write lock yet)
        {
            let size = self.size.read().await;
            if *size >= MAX_TX_POOL_SIZE {
                return Err(TxPoolError::PoolFull);
            }
        }

        let hash = tx.body.hash;
        let sender = tx.body.address;
        let nonce = tx.body.nonce;

        // Check if transaction already exists
        {
            let txs = self.transactions.read().await;
            if txs.contains_key(&hash) {
                return Err(TxPoolError::DuplicateTransaction);
            }
        }

        // Check for replace-by-fee (EIP-1559)
        let should_replace = {
            let pending_by_sender = self.pending_by_sender.read().await;
            if let Some(nonce_map) = pending_by_sender.get(&sender) {
                if let Some(&existing_hash) = nonce_map.get(&nonce) {
                    // Check if new transaction has higher gas price
                    let txs = self.transactions.read().await;
                    if let Some(existing) = txs.get(&existing_hash) {
                        let new_gas_price = tx.body.max_fee_per_gas
                            .or(tx.body.gas_price)
                            .unwrap_or(0) as u64;
                        let price_increase = new_gas_price.saturating_sub(existing.effective_gas_price);

                        // Require at least 10% gas price increase
                        if price_increase > 0 && price_increase >= (existing.effective_gas_price / 10) {
                            debug!("Replacing transaction {:?} with higher fee version", existing_hash);
                            true
                        } else {
                            return Err(TxPoolError::ReplacementFeeTooLow);
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        };

        let prioritized = PrioritizedTransaction::new(tx);

        // Remove old transaction if replacing (do this BEFORE acquiring write locks)
        if should_replace {
            self.remove_by_sender_nonce(sender, nonce).await;
        }

        // Add to pool (now acquire all write locks together)
        {
            let mut size = self.size.write().await;
            let mut txs = self.transactions.write().await;
            let mut queue = self.priority_queue.write().await;
            let mut pending = self.pending_by_sender.write().await;

            txs.insert(hash, prioritized.clone());
            queue.push(prioritized.clone());

            pending.entry(sender).or_insert_with(HashMap::new).insert(nonce, hash);
            *size += 1;
        }

        info!("Added transaction {:?} to pool", hash);
        Ok(())
    }

    /// Remove a transaction by sender and nonce (for replacement)
    async fn remove_by_sender_nonce(&self, sender: Address, nonce: i64) {
        let pending_by_sender = self.pending_by_sender.read().await;
        if let Some(nonce_map) = pending_by_sender.get(&sender) {
            if let Some(&hash) = nonce_map.get(&nonce) {
                drop(pending_by_sender);
                self.remove(&hash).await;
            }
        }
    }

    /// Remove a transaction from the pool
    pub async fn remove(&self, hash: &Hash) {
        let mut size = self.size.write().await;

        let (sender, nonce) = {
            let txs = self.transactions.read().await;
            if let Some(prioritized) = txs.get(hash) {
                let sender = prioritized.sender;
                let nonce = prioritized.nonce;
                (Some(sender), Some(nonce))
            } else {
                (None, None)
            }
        };

        // Remove from all structures
        {
            let mut txs = self.transactions.write().await;
            let mut pending = self.pending_by_sender.write().await;

            txs.remove(hash);

            if let (Some(sender), Some(nonce)) = (sender, nonce) {
                if let Some(nonce_map) = pending.get_mut(&sender) {
                    nonce_map.remove(&nonce);
                    if nonce_map.is_empty() {
                        pending.remove(&sender);
                    }
                }
            }

            *size = txs.len();
        }

        // Note: We don't remove from priority_queue here as it's expensive.
        // Instead, we filter out stale transactions during package().
    }

    /// Get a transaction by hash
    pub async fn get(&self, hash: &Hash) -> Option<Transaction> {
        let txs = self.transactions.read().await;
        txs.get(hash).map(|p| p.tx.clone())
    }

    /// Check if a transaction exists in the pool
    pub async fn contains(&self, hash: &Hash) -> bool {
        let txs = self.transactions.read().await;
        txs.contains_key(hash)
    }

    /// Get pending transactions for a sender
    pub async fn get_pending_by_sender(&self, sender: &Address) -> Vec<Transaction> {
        let txs = self.transactions.read().await;
        let pending = self.pending_by_sender.read().await;

        if let Some(nonce_map) = pending.get(sender) {
            let mut transactions = Vec::new();
            for hash in nonce_map.values() {
                if let Some(prioritized) = txs.get(hash) {
                    transactions.push(prioritized.tx.clone());
                }
            }
            transactions
        } else {
            Vec::new()
        }
    }

    /// Package transactions for block production
    ///
    /// Returns transactions ordered by gas price (highest first)
    pub async fn package<C: ChainReader>(&self, chain: &C) -> Vec<Transaction> {
        debug!("Packaging transactions from enhanced pool...");

        // Phase 1: Collect candidate transactions from priority queue (holding locks)
        let (candidates, mut queue_to_restore): (Vec<_>, _) = {
            let mut queue = self.priority_queue.write().await;
            let txs = self.transactions.read().await;

            let mut candidates = Vec::new();
            let mut new_queue = BinaryHeap::new();
            let mut seen_hashes = std::collections::HashSet::new();

            // Collect all candidates, holding locks
            while let Some(prioritized) = queue.pop() {
                let hash = prioritized.tx.body.hash;

                // Skip if already processed
                if !seen_hashes.insert(hash) {
                    continue;
                }

                // Check if transaction is still in pool (not removed)
                if txs.contains_key(&hash) {
                    candidates.push((hash, prioritized.clone()));
                } else {
                    // Transaction was removed, add back to queue
                    new_queue.push(prioritized);
                }
            }

            // Put remaining back in queue for next round
            (candidates, new_queue)
        }; // Locks released here

        // Phase 2: Check chain and filter candidates WITHOUT holding locks
        let mut to_keep = Vec::new();
        let mut to_remove = Vec::new();

        for (hash, prioritized) in candidates {
            // Skip if expired
            if prioritized.is_expired() {
                debug!("Transaction {:?} expired, removing", hash);
                to_remove.push(hash);
                continue;
            }

            // Skip if already in chain (await WITHOUT holding locks)
            if chain.get_transaction_by_hash(&hash).await.is_some() {
                debug!("Transaction {:?} already in chain", hash);
                to_remove.push(hash);
                continue;
            }

            // Add to result if we have space
            if to_keep.len() < MAX_TX_PACKAGE_COUNT {
                to_keep.push((hash, prioritized));
            } else {
                // Put back in queue for next block
                queue_to_restore.push(prioritized);
                to_remove.push(hash);
                break;
            }
        }

        // Phase 3: Extract transactions to package
        let packaged: Vec<Transaction> = to_keep.iter().map(|(_, p)| p.tx.clone()).collect();
        for (hash, _) in &to_keep {
            to_remove.push(*hash);
        }

        // Phase 4: Update queue (re-acquire locks briefly)
        {
            let mut queue = self.priority_queue.write().await;
            *queue = queue_to_restore;
        }

        // Phase 5: Remove packaged transactions (no locks held)
        self.internal_remove_many(&to_remove).await;

        debug!("Packaged {} transactions", packaged.len());
        packaged
    }

    /// Internal method to remove multiple transactions without calling remove()
    /// This avoids deadlock when called from package()
    async fn internal_remove_many(&self, hashes: &[Hash]) {
        if hashes.is_empty() {
            return;
        }

        // Remove from transactions map
        {
            let mut txs = self.transactions.write().await;
            let mut pending = self.pending_by_sender.write().await;
            let mut size = self.size.write().await;

            for hash in hashes {
                if let Some(prioritized) = txs.remove(hash) {
                    // Remove from pending_by_sender
                    if let Some(nonce_map) = pending.get_mut(&prioritized.sender) {
                        nonce_map.remove(&prioritized.nonce);
                        if nonce_map.is_empty() {
                            pending.remove(&prioritized.sender);
                        }
                    }
                }
            }

            *size = txs.len();
        }

        debug!("Removed {} transactions (internal)", hashes.len());
    }

    /// Clean up expired transactions
    pub async fn cleanup_expired(&self) {
        let mut to_remove = Vec::new();

        {
            let txs = self.transactions.read().await;
            for (hash, prioritized) in txs.iter() {
                if prioritized.is_expired() {
                    to_remove.push(*hash);
                }
            }
        }

        for hash in to_remove.iter() {
            debug!("Removing expired transaction {:?}", hash);
            self.remove(&hash).await;
        }

        info!("Cleaned up {} expired transactions", to_remove.len());
    }

    /// Get pool statistics
    pub async fn stats(&self) -> TxPoolStats {
        let size = *self.size.read().await;
        let txs = self.transactions.read().await;

        let total_gas_price: u64 = txs.values()
            .map(|p| p.effective_gas_price)
            .sum();

        let avg_gas_price = if size > 0 {
            total_gas_price / size as u64
        } else {
            0
        };

        TxPoolStats {
            size,
            avg_gas_price,
            max_size: MAX_TX_POOL_SIZE,
        }
    }
}

/// Implement the common TransactionPool trait
#[async_trait]
impl TransactionPool for EnhancedTxPool {
    async fn add(&self, tx: Transaction) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.add(tx).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn remove(&self, hash: &Hash) {
        self.remove(hash).await;
    }

    async fn get(&self, hash: &Hash) -> Option<Transaction> {
        self.get(hash).await
    }

    async fn contains(&self, hash: &Hash) -> bool {
        self.contains(hash).await
    }

    async fn package<C: ChainReader>(&self, chain: &C) -> Vec<Transaction> {
        self.package(chain).await
    }

    async fn stats(&self) -> CommonTxPoolStats {
        let stats = self.stats().await;
        CommonTxPoolStats {
            size: stats.size,
            total_gas_price: stats.avg_gas_price * stats.size as u64,
            avg_gas_price: stats.avg_gas_price,
        }
    }
}

impl Default for EnhancedTxPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Transaction pool errors
#[derive(Debug, thiserror::Error)]
pub enum TxPoolError {
    #[error("Transaction pool is full")]
    PoolFull,

    #[error("Duplicate transaction")]
    DuplicateTransaction,

    #[error("Replacement fee too low")]
    ReplacementFeeTooLow,

    #[error("Transaction validation failed: {0}")]
    ValidationFailed(String),
}

/// Transaction pool statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxPoolStats {
    /// Current pool size
    pub size: usize,
    /// Average gas price
    pub avg_gas_price: u64,
    /// Maximum pool size
    pub max_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Local MockChain for testing
    struct MockChain;

    #[async_trait::async_trait]
    impl crate::txpool::ChainReader for MockChain {
        async fn get_transaction_by_hash(&self, _hash: &Hash) -> Option<Transaction> {
            None
        }
    }

    #[tokio::test]
    async fn test_add_and_remove_transaction() {
        let pool = EnhancedTxPool::new();
        let mut tx = Transaction::default();
        tx.body.hash.0[0] = 1;

        pool.add(tx.clone()).await.unwrap();
        assert!(pool.contains(&tx.body.hash).await);

        pool.remove(&tx.body.hash).await;
        assert!(!pool.contains(&tx.body.hash).await);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let pool = EnhancedTxPool::new();

        let mut tx1 = Transaction::default();
        tx1.body.hash.0[0] = 1;
        tx1.body.gas_price = Some(100);
        tx1.body.max_fee_per_gas = None;
        tx1.body.address = Address([1u8; 20]);  // Different sender
        tx1.body.nonce = 0;

        let mut tx2 = Transaction::default();
        tx2.body.hash.0[0] = 2;
        tx2.body.gas_price = Some(200);
        tx2.body.max_fee_per_gas = None;
        tx2.body.address = Address([2u8; 20]);  // Different sender
        tx2.body.nonce = 0;

        pool.add(tx1.clone()).await.unwrap();
        pool.add(tx2.clone()).await.unwrap();

        let chain = MockChain;
        let packaged = pool.package(&chain).await;

        assert_eq!(packaged.len(), 2);
        // Higher gas price should be packaged first
        assert_eq!(packaged[0].body.hash, tx2.body.hash);
        assert_eq!(packaged[1].body.hash, tx1.body.hash);
    }

    #[tokio::test]
    async fn test_transaction_replacement() {
        let pool = EnhancedTxPool::new();

        let mut tx1 = Transaction::default();
        tx1.body.hash.0[0] = 1;
        tx1.body.address = Address([1u8; 20]);
        tx1.body.nonce = 0;
        tx1.body.gas_price = Some(100);

        let mut tx2 = Transaction::default();
        tx2.body.hash.0[0] = 2;
        tx2.body.address = Address([1u8; 20]);
        tx2.body.nonce = 0; // Same nonce
        tx2.body.gas_price = Some(120); // 20% higher

        pool.add(tx1.clone()).await.unwrap();
        pool.add(tx2.clone()).await.unwrap();

        // Only tx2 should remain (replaced tx1)
        assert!(!pool.contains(&tx1.body.hash).await);
        assert!(pool.contains(&tx2.body.hash).await);
    }

    #[tokio::test]
    async fn test_pool_stats() {
        let pool = EnhancedTxPool::new();

        let mut tx = Transaction::default();
        tx.body.gas_price = Some(100);
        pool.add(tx).await.unwrap();

        let stats = pool.stats().await;
        assert_eq!(stats.size, 1);
        assert_eq!(stats.avg_gas_price, 100);
    }
}
