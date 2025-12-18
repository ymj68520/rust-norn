//! Event Subscription Module
//! 
//! Provides block and transaction event subscriptions for clients.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use norn_common::types::{Block, Transaction, Hash, Address};

/// Event types that can be subscribed to
#[derive(Debug, Clone)]
pub enum BlockchainEvent {
    /// New block was added
    NewBlock(Block),
    /// New transaction received
    NewTransaction(Transaction),
    /// Transaction confirmed (included in block)
    TransactionConfirmed {
        tx_hash: Hash,
        block_hash: Hash,
        block_height: i64,
    },
    /// Block finalized (irreversible)
    BlockFinalized {
        block_hash: Hash,
        block_height: i64,
    },
    /// Chain reorganization detected
    ChainReorg {
        old_height: i64,
        new_height: i64,
        common_ancestor: Hash,
    },
}

/// Subscription filter
#[derive(Debug, Clone, Default)]
pub struct SubscriptionFilter {
    /// Filter by address (sender or receiver)
    pub addresses: Vec<Address>,
    /// Filter by transaction types
    pub event_types: Vec<EventType>,
    /// Minimum block height
    pub from_height: Option<i64>,
}

/// Event types for filtering
#[derive(Debug, Clone, PartialEq)]
pub enum EventType {
    Blocks,
    Transactions,
    Confirmations,
    Finalizations,
    Reorgs,
}

/// Subscription ID
pub type SubscriptionId = u64;

/// Event subscriber handle
pub struct EventSubscriber {
    id: SubscriptionId,
    _filter: SubscriptionFilter,
    receiver: broadcast::Receiver<BlockchainEvent>,
}

impl EventSubscriber {
    /// Receive next event
    pub async fn recv(&mut self) -> Option<BlockchainEvent> {
        self.receiver.recv().await.ok()
    }

    /// Get subscription ID
    pub fn id(&self) -> SubscriptionId {
        self.id
    }
}

/// Event publisher for blockchain events
pub struct EventPublisher {
    sender: broadcast::Sender<BlockchainEvent>,
    subscriptions: Arc<RwLock<HashMap<SubscriptionId, SubscriptionFilter>>>,
    next_id: Arc<RwLock<SubscriptionId>>,
    capacity: usize,
}

impl EventPublisher {
    /// Create new event publisher
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(1)),
            capacity,
        }
    }

    /// Subscribe to events with filter
    pub async fn subscribe(&self, filter: SubscriptionFilter) -> EventSubscriber {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;
        drop(next_id);

        self.subscriptions.write().await.insert(id, filter.clone());

        debug!("New subscription created: {}", id);

        EventSubscriber {
            id,
            _filter: filter,
            receiver: self.sender.subscribe(),
        }
    }

    /// Unsubscribe
    pub async fn unsubscribe(&self, id: SubscriptionId) -> bool {
        let removed = self.subscriptions.write().await.remove(&id).is_some();
        if removed {
            debug!("Subscription removed: {}", id);
        }
        removed
    }

    /// Publish event to all subscribers
    pub fn publish(&self, event: BlockchainEvent) {
        if let Err(e) = self.sender.send(event) {
            debug!("No subscribers to receive event: {:?}", e);
        }
    }

    /// Publish new block event
    pub fn publish_new_block(&self, block: Block) {
        debug!("Publishing new block at height {}", block.header.height);
        self.publish(BlockchainEvent::NewBlock(block));
    }

    /// Publish new transaction event
    pub fn publish_new_transaction(&self, tx: Transaction) {
        debug!("Publishing new transaction: {:?}", tx.body.hash);
        self.publish(BlockchainEvent::NewTransaction(tx));
    }

    /// Publish transaction confirmed event
    pub fn publish_tx_confirmed(&self, tx_hash: Hash, block_hash: Hash, block_height: i64) {
        debug!("Publishing tx confirmed at height {}", block_height);
        self.publish(BlockchainEvent::TransactionConfirmed {
            tx_hash,
            block_hash,
            block_height,
        });
    }

    /// Publish block finalized event
    pub fn publish_block_finalized(&self, block_hash: Hash, block_height: i64) {
        info!("Block finalized at height {}", block_height);
        self.publish(BlockchainEvent::BlockFinalized {
            block_hash,
            block_height,
        });
    }

    /// Get subscription count
    pub async fn subscription_count(&self) -> usize {
        self.subscriptions.read().await.len()
    }

    /// Get receiver count (active subscribers)
    pub fn active_subscribers(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventPublisher {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Pending transactions query interface
pub struct PendingTxQuery {
    // Reference would be to TxPool in actual impl
}

impl PendingTxQuery {
    /// Create new query interface
    pub fn new() -> Self {
        Self {}
    }

    /// Get pending transaction count (placeholder)
    pub fn get_pending_count(&self) -> usize {
        0
    }

    /// Check if transaction is pending (placeholder)
    pub fn is_pending(&self, _tx_hash: &Hash) -> bool {
        false
    }
}

impl Default for PendingTxQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_publisher_creation() {
        let publisher = EventPublisher::new(100);
        assert_eq!(publisher.subscription_count().await, 0);
    }

    #[tokio::test]
    async fn test_subscribe_unsubscribe() {
        let publisher = EventPublisher::new(100);
        
        let filter = SubscriptionFilter::default();
        let subscriber = publisher.subscribe(filter).await;
        
        assert_eq!(publisher.subscription_count().await, 1);
        
        publisher.unsubscribe(subscriber.id()).await;
        assert_eq!(publisher.subscription_count().await, 0);
    }

    #[tokio::test]
    async fn test_publish_receive() {
        let publisher = EventPublisher::new(100);
        
        let filter = SubscriptionFilter::default();
        let mut subscriber = publisher.subscribe(filter).await;
        
        // Publish event
        let block = Block::default();
        publisher.publish_new_block(block.clone());
        
        // Receive event
        let event = subscriber.recv().await;
        assert!(matches!(event, Some(BlockchainEvent::NewBlock(_))));
    }
}
