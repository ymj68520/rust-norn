//! EVM Event Logging System
//!
//! Manages emission and storage of event logs during EVM execution.
//!
//! # LOG Opcodes
//! - LOG0: Emit event with no topics (just data)
//! - LOG1: Emit event with 1 topic
//! - LOG2: Emit event with 2 topics
//! - LOG3: Emit event with 3 topics
//! - LOG4: Emit event with 4 topics
//!
//! # Topics
//! Topics are 32-byte hashes used to index events:
//! - First topic: Event signature hash (keccak256("EventName(type,type)"))
//! - Remaining topics: Indexed parameters (up to 3)

use crate::evm::EVMResult;
use norn_common::types::{Address, Hash};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use std::collections::HashMap;

/// EVM event log
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventLog {
    /// Contract address that emitted the log
    pub address: Address,

    /// Log topics (0-4 topics)
    pub topics: Vec<Hash>,

    /// Log data (non-indexed parameters)
    pub data: Vec<u8>,
}

impl EventLog {
    /// Create a new event log
    pub fn new(address: Address, topics: Vec<Hash>, data: Vec<u8>) -> Self {
        Self {
            address,
            topics,
            data,
        }
    }

    /// Create a LOG0 (no topics)
    pub fn log0(address: Address, data: Vec<u8>) -> Self {
        Self {
            address,
            topics: vec![],
            data,
        }
    }

    /// Create a LOG1 (1 topic)
    pub fn log1(address: Address, topic0: Hash, data: Vec<u8>) -> Self {
        Self {
            address,
            topics: vec![topic0],
            data,
        }
    }

    /// Create a LOG2 (2 topics)
    pub fn log2(address: Address, topic0: Hash, topic1: Hash, data: Vec<u8>) -> Self {
        Self {
            address,
            topics: vec![topic0, topic1],
            data,
        }
    }

    /// Create a LOG3 (3 topics)
    pub fn log3(address: Address, topic0: Hash, topic1: Hash, topic2: Hash, data: Vec<u8>) -> Self {
        Self {
            address,
            topics: vec![topic0, topic1, topic2],
            data,
        }
    }

    /// Create a LOG4 (4 topics)
    pub fn log4(
        address: Address,
        topic0: Hash,
        topic1: Hash,
        topic2: Hash,
        topic3: Hash,
        data: Vec<u8>,
    ) -> Self {
        Self {
            address,
            topics: vec![topic0, topic1, topic2, topic3],
            data,
        }
    }

    /// Validate the log (max 4 topics)
    pub fn validate(&self) -> EVMResult<()> {
        if self.topics.len() > 4 {
            return Err(crate::evm::EVMError::Execution(format!(
                "Event log cannot have more than 4 topics, got {}",
                self.topics.len()
            )));
        }
        Ok(())
    }
}

/// Event log manager
///
/// Collects and manages event logs during EVM execution.
pub struct LogManager {
    /// Logs emitted during current execution
    logs: Arc<RwLock<Vec<EventLog>>>,

    /// Logs indexed by address for efficient querying
    logs_by_address: Arc<RwLock<HashMap<Address, Vec<usize>>>>,

    /// Logs indexed by topics for efficient filtering
    logs_by_topic: Arc<RwLock<HashMap<Hash, Vec<usize>>>>,
}

impl LogManager {
    /// Create a new log manager
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(Vec::new())),
            logs_by_address: Arc::new(RwLock::new(HashMap::new())),
            logs_by_topic: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Emit a log (LOG0-LOG4)
    pub async fn emit(&self, log: EventLog) -> EVMResult<()> {
        // Validate the log
        log.validate()?;

        let log_index = self.logs.read().await.len();

        // Store the log
        {
            let mut logs = self.logs.write().await;
            logs.push(log.clone());
        }

        // Index by address
        {
            let mut logs_by_addr = self.logs_by_address.write().await;
            logs_by_addr.entry(log.address).or_insert_with(Vec::new).push(log_index);
        }

        // Index by topics
        for topic in &log.topics {
            let mut logs_by_topic = self.logs_by_topic.write().await;
            logs_by_topic.entry(*topic).or_insert_with(Vec::new).push(log_index);
        }

        info!(
            "Emitted event: address={:?}, topics={}, data_len={}",
            log.address,
            log.topics.len(),
            log.data.len()
        );

        debug!("Event details: topics={:?}, data={:?}", log.topics, log.data);

        Ok(())
    }

    /// Get all logs
    pub async fn get_all_logs(&self) -> Vec<EventLog> {
        self.logs.read().await.clone()
    }

    /// Get logs by contract address
    pub async fn get_logs_by_address(&self, address: &Address) -> Vec<EventLog> {
        let logs_by_addr = self.logs_by_address.read().await;
        let logs = self.logs.read().await;

        if let Some(indices) = logs_by_addr.get(address) {
            indices.iter().map(|&i| logs[i].clone()).collect()
        } else {
            vec![]
        }
    }

    /// Get logs by topic
    pub async fn get_logs_by_topic(&self, topic: &Hash) -> Vec<EventLog> {
        let logs_by_topic = self.logs_by_topic.read().await;
        let logs = self.logs.read().await;

        if let Some(indices) = logs_by_topic.get(topic) {
            indices.iter().map(|&i| logs[i].clone()).collect()
        } else {
            vec![]
        }
    }

    /// Get logs by multiple topics (all must match)
    pub async fn get_logs_by_topics(&self, topics: &[Hash]) -> Vec<EventLog> {
        if topics.is_empty() {
            return self.get_all_logs().await;
        }

        // Get logs for first topic
        let mut result = self.get_logs_by_topic(&topics[0]).await;

        // Filter by remaining topics
        for topic in &topics[1..] {
            result.retain(|log| log.topics.contains(topic));
        }

        result
    }

    /// Filter logs by address and topics
    pub async fn filter_logs(
        &self,
        address: Option<&Address>,
        topics: &[Option<Hash>],
    ) -> Vec<EventLog> {
        let mut logs = if let Some(addr) = address {
            self.get_logs_by_address(addr).await
        } else {
            self.get_all_logs().await
        };

        // Filter by topics if specified
        for (i, topic_opt) in topics.iter().enumerate() {
            if let Some(topic) = topic_opt {
                logs.retain(|log| {
                    log.topics.get(i).map_or(false, |t| t == topic)
                });
            }
        }

        logs
    }

    /// Clear all logs (for new execution)
    pub async fn clear(&self) {
        self.logs.write().await.clear();
        self.logs_by_address.write().await.clear();
        self.logs_by_topic.write().await.clear();
        debug!("Cleared all event logs");
    }

    /// Get the number of logs
    pub async fn log_count(&self) -> usize {
        self.logs.read().await.len()
    }
}

impl Default for LogManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_address(byte: u8) -> Address {
        Address([byte; 20])
    }

    fn create_test_hash(byte: u8) -> Hash {
        Hash([byte; 32])
    }

    #[tokio::test]
    async fn test_log_creation() {
        let addr = create_test_address(1);
        let topic0 = create_test_hash(10);
        let data = vec![1, 2, 3];

        // Test LOG0
        let log0 = EventLog::log0(addr, data.clone());
        assert_eq!(log0.topics.len(), 0);
        assert_eq!(log0.data, data);

        // Test LOG1
        let log1 = EventLog::log1(addr, topic0, data.clone());
        assert_eq!(log1.topics.len(), 1);
        assert_eq!(log1.topics[0], topic0);

        // Test LOG2
        let topic1 = create_test_hash(11);
        let log2 = EventLog::log2(addr, topic0, topic1, data.clone());
        assert_eq!(log2.topics.len(), 2);

        // Test LOG3
        let topic2 = create_test_hash(12);
        let log3 = EventLog::log3(addr, topic0, topic1, topic2, data.clone());
        assert_eq!(log3.topics.len(), 3);

        // Test LOG4
        let topic3 = create_test_hash(13);
        let log4 = EventLog::log4(addr, topic0, topic1, topic2, topic3, data);
        assert_eq!(log4.topics.len(), 4);
    }

    #[tokio::test]
    async fn test_log_validation() {
        let addr = create_test_address(1);
        let topic0 = create_test_hash(10);
        let data = vec![1, 2, 3];

        // Valid logs
        let log0 = EventLog::log0(addr, data.clone());
        assert!(log0.validate().is_ok());

        let log4 = EventLog::log4(addr, topic0, topic0, topic0, topic0, data.clone());
        assert!(log4.validate().is_ok());

        // Invalid log (5 topics)
        let invalid = EventLog {
            address: addr,
            topics: vec![topic0; 5],
            data,
        };
        assert!(invalid.validate().is_err());
    }

    #[tokio::test]
    async fn test_log_manager_emit() {
        let manager = LogManager::new();
        let addr = create_test_address(1);
        let topic0 = create_test_hash(10);
        let data = vec![1, 2, 3];

        // Emit some logs
        let log1 = EventLog::log1(addr, topic0, data.clone());
        manager.emit(log1).await.unwrap();

        assert_eq!(manager.log_count().await, 1);

        // Emit another log
        let addr2 = create_test_address(2);
        let log2 = EventLog::log0(addr2, vec![4, 5]);
        manager.emit(log2).await.unwrap();

        assert_eq!(manager.log_count().await, 2);
    }

    #[tokio::test]
    async fn test_filter_by_address() {
        let manager = LogManager::new();
        let addr1 = create_test_address(1);
        let addr2 = create_test_address(2);
        let topic0 = create_test_hash(10);
        let data = vec![1, 2, 3];

        // Emit logs for different addresses
        manager.emit(EventLog::log1(addr1, topic0, data.clone())).await.unwrap();
        manager.emit(EventLog::log1(addr2, topic0, data.clone())).await.unwrap();
        manager.emit(EventLog::log0(addr1, vec![4, 5])).await.unwrap();

        // Filter by address
        let addr1_logs = manager.get_logs_by_address(&addr1).await;
        assert_eq!(addr1_logs.len(), 2);

        let addr2_logs = manager.get_logs_by_address(&addr2).await;
        assert_eq!(addr2_logs.len(), 1);
    }

    #[tokio::test]
    async fn test_filter_by_topic() {
        let manager = LogManager::new();
        let addr = create_test_address(1);
        let topic0 = create_test_hash(10);
        let topic1 = create_test_hash(11);
        let data = vec![1, 2, 3];

        // Emit logs with different topics
        manager.emit(EventLog::log1(addr, topic0, data.clone())).await.unwrap();
        manager.emit(EventLog::log1(addr, topic1, data.clone())).await.unwrap();
        manager.emit(EventLog::log2(addr, topic0, topic1, data.clone())).await.unwrap();

        // Filter by topic
        let topic0_logs = manager.get_logs_by_topic(&topic0).await;
        assert_eq!(topic0_logs.len(), 2); // log1 and log2

        let topic1_logs = manager.get_logs_by_topic(&topic1).await;
        assert_eq!(topic1_logs.len(), 2); // log1 and log2
    }

    #[tokio::test]
    async fn test_filter_by_multiple_topics() {
        let manager = LogManager::new();
        let addr = create_test_address(1);
        let topic0 = create_test_hash(10);
        let topic1 = create_test_hash(11);
        let topic2 = create_test_hash(12);
        let data = vec![1, 2, 3];

        // Emit logs
        manager.emit(EventLog::log2(addr, topic0, topic1, data.clone())).await.unwrap();
        manager.emit(EventLog::log3(addr, topic0, topic1, topic2, data.clone())).await.unwrap();
        manager.emit(EventLog::log1(addr, topic0, data.clone())).await.unwrap();

        // Filter by multiple topics (must have both)
        let logs = manager.get_logs_by_topics(&[topic0, topic1]).await;
        assert_eq!(logs.len(), 2); // log2 and log3
    }

    #[tokio::test]
    async fn test_clear_logs() {
        let manager = LogManager::new();
        let addr = create_test_address(1);
        let topic0 = create_test_hash(10);
        let data = vec![1, 2, 3];

        // Emit logs
        manager.emit(EventLog::log1(addr, topic0, data.clone())).await.unwrap();
        manager.emit(EventLog::log0(addr, vec![4, 5])).await.unwrap();

        assert_eq!(manager.log_count().await, 2);

        // Clear
        manager.clear().await;
        assert_eq!(manager.log_count().await, 0);
        assert!(manager.get_all_logs().await.is_empty());
    }
}
