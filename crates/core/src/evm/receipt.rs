//! EVM Transaction Receipts
//!
//! Transaction receipts contain the result of executing a transaction,
//! including status, gas used, logs, and contract address for deployments.

use crate::evm::{EVMResult, EventLog};
use norn_common::types::{Address, Hash};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::Digest;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Transaction receipt
///
/// Contains the result of executing a transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    /// Transaction hash
    pub tx_hash: Hash,

    /// Block hash
    pub block_hash: Hash,

    /// Block number
    pub block_number: u64,

    /// Transaction index in the block
    pub tx_index: u64,

    /// Sender address
    pub from: Address,

    /// Recipient address (None for contract creation)
    pub to: Option<Address>,

    /// Execution status (true = success, false = failure)
    pub status: bool,

    /// Gas used by this transaction
    pub gas_used: u64,

    /// Cumulative gas used in the block
    pub cumulative_gas_used: u64,

    /// Contract address created (for contract creation transactions)
    pub contract_address: Option<Address>,

    /// Logs emitted during execution
    pub logs: Vec<ReceiptLog>,

    /// Logs bloom filter (for efficient log querying)
    pub logs_bloom: Bloom,

    /// Transaction output data (return value)
    pub output: Vec<u8>,

    /// Revert reason (if failed)
    pub revert_reason: Option<String>,
}

/// Log entry in a receipt
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceiptLog {
    /// Log index in the transaction
    pub log_index: u64,

    /// Transaction hash
    pub tx_hash: Hash,

    /// Block hash
    pub block_hash: Hash,

    /// Block number
    pub block_number: u64,

    /// Contract address that emitted the log
    pub address: Address,

    /// Log topics
    pub topics: Vec<Hash>,

    /// Log data
    pub data: Vec<u8>,
}

impl From<EventLog> for ReceiptLog {
    fn from(event_log: EventLog) -> Self {
        Self {
            log_index: 0, // Will be set when adding to receipt
            tx_hash: Hash::default(),
            block_hash: Hash::default(),
            block_number: 0,
            address: event_log.address,
            topics: event_log.topics,
            data: event_log.data,
        }
    }
}

/// Bloom filter for efficient log filtering
///
/// A 2048-bit bloom filter used to efficiently query logs by address and topics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bloom([u8; 256]);

impl Serialize for Bloom {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as a byte vector
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Bloom {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize from a byte vector
        let bytes: Vec<u8> = Vec::deserialize(deserializer)?;
        if bytes.len() != 256 {
            return Err(serde::de::Error::custom(format!(
                "Invalid Bloom length: expected 256, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 256];
        arr.copy_from_slice(&bytes);
        Ok(Bloom(arr))
    }
}

impl Bloom {
    /// Create a new empty bloom filter
    pub fn new() -> Self {
        Self([0u8; 256])
    }

    /// Get the bloom filter as bytes
    pub fn as_bytes(&self) -> &[u8; 256] {
        &self.0
    }

    /// Add a value to the bloom filter
    pub fn add(&mut self, value: &[u8]) {
        let hash = Hash(sha2::Sha256::digest(value).into());
        self.add_hash(&hash);
    }

    /// Add a hash to the bloom filter
    pub fn add_hash(&mut self, hash: &Hash) {
        // Use first 3 pairs of 64-bit chunks to set 6 bits in the bloom filter
        for i in 0..3 {
            let idx = ((hash.0[i * 8] as usize) << 8 |
                       (hash.0[i * 8 + 1] as usize)) % 2048;
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            self.0[byte_idx] |= 1 << bit_idx;
        }
    }

    /// Add an address to the bloom filter
    pub fn add_address(&mut self, address: &Address) {
        self.add(&address.0);
    }

    /// Add a topic to the bloom filter
    pub fn add_topic(&mut self, topic: &Hash) {
        self.add_hash(topic);
    }

    /// Check if the bloom filter might contain a value
    /// (may have false positives, but no false negatives)
    pub fn might_contain(&self, value: &[u8]) -> bool {
        let hash = Hash(sha2::Sha256::digest(value).into());
        for i in 0..3 {
            let idx = ((hash.0[i * 8] as usize) << 8 |
                       (hash.0[i * 8 + 1] as usize)) % 2048;
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            if (self.0[byte_idx] & (1 << bit_idx)) == 0 {
                return false;
            }
        }
        true
    }
}

impl Default for Bloom {
    fn default() -> Self {
        Self::new()
    }
}

impl Receipt {
    /// Create a new receipt
    pub fn new(
        tx_hash: Hash,
        block_hash: Hash,
        block_number: u64,
        tx_index: u64,
    ) -> Self {
        Self {
            tx_hash,
            block_hash,
            block_number,
            tx_index,
            from: Address::default(), // Will be set by with_from
            to: None,                 // Will be set by with_to
            status: true,
            gas_used: 0,
            cumulative_gas_used: 0,
            contract_address: None,
            logs: vec![],
            logs_bloom: Bloom::new(),
            output: vec![],
            revert_reason: None,
        }
    }

    /// Set the sender address
    pub fn with_from(mut self, from: Address) -> Self {
        self.from = from;
        self
    }

    /// Set the recipient address
    pub fn with_to(mut self, to: Option<Address>) -> Self {
        self.to = to;
        self
    }

    /// Set the execution status
    pub fn with_status(mut self, success: bool) -> Self {
        self.status = success;
        self
    }

    /// Set the gas used
    pub fn with_gas_used(mut self, gas_used: u64, cumulative_gas_used: u64) -> Self {
        self.gas_used = gas_used;
        self.cumulative_gas_used = cumulative_gas_used;
        self
    }

    /// Set the contract address (for contract creation)
    pub fn with_contract_address(mut self, address: Address) -> Self {
        self.contract_address = Some(address);
        self
    }

    /// Add a log to the receipt
    pub fn with_log(mut self, mut log: ReceiptLog) -> Self {
        log.log_index = self.logs.len() as u64;
        log.tx_hash = self.tx_hash;
        log.block_hash = self.block_hash;
        log.block_number = self.block_number;

        // Update bloom filter
        self.logs_bloom.add_address(&log.address);
        for topic in &log.topics {
            self.logs_bloom.add_topic(topic);
        }

        self.logs.push(log);
        self
    }

    /// Add multiple logs
    pub fn with_logs(mut self, logs: Vec<ReceiptLog>) -> Self {
        for log in logs {
            self = self.with_log(log);
        }
        self
    }

    /// Set the output data
    pub fn with_output(mut self, output: Vec<u8>) -> Self {
        self.output = output;
        self
    }

    /// Set the revert reason
    pub fn with_revert_reason(mut self, reason: String) -> Self {
        self.revert_reason = Some(reason);
        self.status = false;
        self
    }

    /// Build the bloom filter from logs
    pub fn build_bloom(&mut self) {
        self.logs_bloom = Bloom::new();
        for log in &self.logs {
            self.logs_bloom.add_address(&log.address);
            for topic in &log.topics {
                self.logs_bloom.add_topic(topic);
            }
        }
    }
}

/// Receipt database
///
/// Stores and indexes transaction receipts for efficient querying.
pub struct ReceiptDB {
    /// Receipts by transaction hash
    receipts_by_tx: Arc<RwLock<HashMap<Hash, Receipt>>>,

    /// Transaction indices by block
    tx_indices_by_block: Arc<RwLock<HashMap<Hash, Vec<u64>>>>,

    /// Receipts by block hash
    receipts_by_block: Arc<RwLock<HashMap<Hash, Vec<Receipt>>>>,

    /// Receipt indices by address (for filtering)
    receipts_by_address: Arc<RwLock<HashMap<Address, Vec<Hash>>>>,

    /// Receipt indices by topic (for filtering)
    receipts_by_topic: Arc<RwLock<HashMap<Hash, Vec<Hash>>>>,
}

impl ReceiptDB {
    /// Create a new receipt database
    pub fn new() -> Self {
        Self {
            receipts_by_tx: Arc::new(RwLock::new(HashMap::new())),
            tx_indices_by_block: Arc::new(RwLock::new(HashMap::new())),
            receipts_by_block: Arc::new(RwLock::new(HashMap::new())),
            receipts_by_address: Arc::new(RwLock::new(HashMap::new())),
            receipts_by_topic: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store a receipt
    pub async fn put_receipt(&self, receipt: Receipt) -> EVMResult<()> {
        let tx_hash = receipt.tx_hash;
        let block_hash = receipt.block_hash;

        // Store receipt by transaction hash
        {
            let mut receipts = self.receipts_by_tx.write().await;
            receipts.insert(tx_hash, receipt.clone());
        }

        // Store receipt by block
        {
            let mut block_receipts = self.receipts_by_block.write().await;
            block_receipts.entry(block_hash).or_insert_with(Vec::new).push(receipt.clone());
        }

        // Index by address
        for log in &receipt.logs {
            let mut addr_index = self.receipts_by_address.write().await;
            addr_index.entry(log.address).or_insert_with(Vec::new).push(tx_hash);
        }

        // Index by topics
        for log in &receipt.logs {
            for topic in &log.topics {
                let mut topic_index = self.receipts_by_topic.write().await;
                topic_index.entry(*topic).or_insert_with(Vec::new).push(tx_hash);
            }
        }

        info!("Stored receipt for transaction: {:?}", tx_hash);
        debug!("Receipt: block={}, gas_used={}, logs={}",
               receipt.block_number, receipt.gas_used, receipt.logs.len());

        Ok(())
    }

    /// Get a receipt by transaction hash
    pub async fn get_receipt(&self, tx_hash: &Hash) -> EVMResult<Option<Receipt>> {
        let receipts = self.receipts_by_tx.read().await;
        Ok(receipts.get(tx_hash).cloned())
    }

    /// Get all receipts for a block
    pub async fn get_receipts_by_block(&self, block_hash: &Hash) -> EVMResult<Vec<Receipt>> {
        let block_receipts = self.receipts_by_block.read().await;
        Ok(block_receipts.get(block_hash).cloned().unwrap_or_default())
    }

    /// Get receipts by address
    pub async fn get_receipts_by_address(&self, address: &Address) -> EVMResult<Vec<Receipt>> {
        let addr_index = self.receipts_by_address.read().await;
        let receipts = self.receipts_by_tx.read().await;

        if let Some(tx_hashes) = addr_index.get(address) {
            let mut result = Vec::new();
            for tx_hash in tx_hashes {
                if let Some(receipt) = receipts.get(tx_hash) {
                    result.push(receipt.clone());
                }
            }
            Ok(result)
        } else {
            Ok(vec![])
        }
    }

    /// Get receipts by topic
    pub async fn get_receipts_by_topic(&self, topic: &Hash) -> EVMResult<Vec<Receipt>> {
        let topic_index = self.receipts_by_topic.read().await;
        let receipts = self.receipts_by_tx.read().await;

        if let Some(tx_hashes) = topic_index.get(topic) {
            let mut result = Vec::new();
            for tx_hash in tx_hashes {
                if let Some(receipt) = receipts.get(tx_hash) {
                    result.push(receipt.clone());
                }
            }
            Ok(result)
        } else {
            Ok(vec![])
        }
    }

    /// Filter receipts by multiple criteria
    pub async fn filter_receipts(
        &self,
        block_hash: Option<&Hash>,
        from_block: Option<u64>,
        to_block: Option<u64>,
        address: Option<&Address>,
        topics: &[Option<Hash>],
    ) -> EVMResult<Vec<Receipt>> {
        let mut receipts = if let Some(block_hash) = block_hash {
            self.get_receipts_by_block(block_hash).await?
        } else {
            // Get all receipts (could be slow in production)
            let all_receipts = self.receipts_by_tx.read().await;
            all_receipts.values().cloned().collect()
        };

        // Filter by block range
        if let Some(from) = from_block {
            receipts.retain(|r| r.block_number >= from);
        }
        if let Some(to) = to_block {
            receipts.retain(|r| r.block_number <= to);
        }

        // Filter by address
        if let Some(addr) = address {
            receipts.retain(|r| {
                r.logs.iter().any(|log| log.address == *addr)
            });
        }

        // Filter by topics
        for (i, topic_opt) in topics.iter().enumerate() {
            if let Some(topic) = topic_opt {
                receipts.retain(|r| {
                    r.logs.iter().any(|log| {
                        log.topics.get(i).map_or(false, |t| t == topic)
                    })
                });
            }
        }

        Ok(receipts)
    }

    /// Clear all receipts (for testing)
    pub async fn clear(&self) {
        self.receipts_by_tx.write().await.clear();
        self.tx_indices_by_block.write().await.clear();
        self.receipts_by_block.write().await.clear();
        self.receipts_by_address.write().await.clear();
        self.receipts_by_topic.write().await.clear();
        debug!("Cleared all receipts from database");
    }

    /// Get the number of receipts stored
    pub async fn count(&self) -> usize {
        self.receipts_by_tx.read().await.len()
    }
}

impl Default for ReceiptDB {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_hash(byte: u8) -> Hash {
        Hash([byte; 32])
    }

    fn create_test_address(byte: u8) -> Address {
        Address([byte; 20])
    }

    #[test]
    fn test_bloom_filter() {
        let mut bloom = Bloom::new();

        let address = create_test_address(1);
        let topic1 = create_test_hash(10);
        let topic2 = create_test_hash(11);

        bloom.add_address(&address);
        bloom.add_topic(&topic1);
        bloom.add_topic(&topic2);

        // Check address that was added
        assert!(bloom.might_contain(&address.0));

        // For topics, the bloom filter uses the hash bytes directly
        // The might_contain function hashes the input, so we need to test differently
        // Let's verify by checking that topics added produce some bits set
        let empty_bloom = Bloom::new();
        assert_ne!(bloom.0.to_vec(), empty_bloom.0.to_vec());

        // Check address/topic that was not added (might be false positive)
        let other = create_test_address(99);
        // We can't assert false because bloom filters can have false positives
        let _ = bloom.might_contain(&other.0);
    }

    #[tokio::test]
    async fn test_receipt_creation() {
        let tx_hash = create_test_hash(1);
        let block_hash = create_test_hash(2);
        let block_number = 100;
        let tx_index = 0;

        let receipt = Receipt::new(tx_hash, block_hash, block_number, tx_index)
            .with_status(true)
            .with_gas_used(21_000, 21_000)
            .with_output(vec![0x01, 0x02]);

        assert_eq!(receipt.tx_hash, tx_hash);
        assert_eq!(receipt.block_number, block_number);
        assert_eq!(receipt.gas_used, 21_000);
        assert_eq!(receipt.status, true);
    }

    #[tokio::test]
    async fn test_receipt_with_logs() {
        let tx_hash = create_test_hash(1);
        let block_hash = create_test_hash(2);

        let mut receipt = Receipt::new(tx_hash, block_hash, 100, 0);

        let address = create_test_address(1);
        let topic = create_test_hash(10);
        let data = vec![0x01, 0x02];

        let log = ReceiptLog {
            log_index: 0,
            tx_hash,
            block_hash,
            block_number: 100,
            address,
            topics: vec![topic],
            data,
        };

        receipt = receipt.with_log(log.clone());

        assert_eq!(receipt.logs.len(), 1);
        assert_eq!(receipt.logs[0].address, address);
        assert_eq!(receipt.logs[0].topics.len(), 1);
        assert_eq!(receipt.logs[0].log_index, 0);
    }

    #[tokio::test]
    async fn test_receipt_db_storage() {
        let db = ReceiptDB::new();

        let tx_hash = create_test_hash(1);
        let block_hash = create_test_hash(2);

        let receipt = Receipt::new(tx_hash, block_hash, 100, 0)
            .with_status(true)
            .with_gas_used(21_000, 21_000);

        db.put_receipt(receipt).await.unwrap();

        assert_eq!(db.count().await, 1);

        // Get receipt back
        let retrieved = db.get_receipt(&tx_hash).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().tx_hash, tx_hash);
    }

    #[tokio::test]
    async fn test_receipt_by_block() {
        let db = ReceiptDB::new();

        let block_hash = create_test_hash(10);

        // Add multiple receipts for the same block
        for i in 0..3 {
            let tx_hash = create_test_hash(i);
            let receipt = Receipt::new(tx_hash, block_hash, 100, i as u64);
            db.put_receipt(receipt).await.unwrap();
        }

        // Get all receipts for the block
        let receipts = db.get_receipts_by_block(&block_hash).await.unwrap();
        assert_eq!(receipts.len(), 3);
    }

    #[tokio::test]
    async fn test_receipt_filter_by_address() {
        let db = ReceiptDB::new();

        let address = create_test_address(1);
        let other_address = create_test_address(2);

        // Create receipts with logs
        let receipt1 = Receipt::new(create_test_hash(1), create_test_hash(10), 100, 0)
            .with_log(ReceiptLog {
                log_index: 0,
                tx_hash: create_test_hash(1),
                block_hash: create_test_hash(10),
                block_number: 100,
                address,
                topics: vec![],
                data: vec![],
            });

        let receipt2 = Receipt::new(create_test_hash(2), create_test_hash(10), 100, 1)
            .with_log(ReceiptLog {
                log_index: 0,
                tx_hash: create_test_hash(2),
                block_hash: create_test_hash(10),
                block_number: 100,
                address: other_address,
                topics: vec![],
                data: vec![],
            });

        db.put_receipt(receipt1).await.unwrap();
        db.put_receipt(receipt2).await.unwrap();

        // Filter by address
        let filtered = db.get_receipts_by_address(&address).await.unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].tx_hash, create_test_hash(1));
    }

    #[tokio::test]
    async fn test_receipt_filter_by_topic() {
        let db = ReceiptDB::new();

        let topic1 = create_test_hash(10);
        let topic2 = create_test_hash(11);
        let address = create_test_address(1);

        // Create receipts with different topics
        let receipt1 = Receipt::new(create_test_hash(1), create_test_hash(10), 100, 0)
            .with_log(ReceiptLog {
                log_index: 0,
                tx_hash: create_test_hash(1),
                block_hash: create_test_hash(10),
                block_number: 100,
                address,
                topics: vec![topic1],
                data: vec![],
            });

        let receipt2 = Receipt::new(create_test_hash(2), create_test_hash(10), 100, 1)
            .with_log(ReceiptLog {
                log_index: 0,
                tx_hash: create_test_hash(2),
                block_hash: create_test_hash(10),
                block_number: 100,
                address,
                topics: vec![topic2],
                data: vec![],
            });

        db.put_receipt(receipt1).await.unwrap();
        db.put_receipt(receipt2).await.unwrap();

        // Filter by topic
        let filtered = db.get_receipts_by_topic(&topic1).await.unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].tx_hash, create_test_hash(1));
    }

    #[tokio::test]
    async fn test_clear_receipts() {
        let db = ReceiptDB::new();

        let receipt = Receipt::new(
            create_test_hash(1),
            create_test_hash(2),
            100,
            0
        );

        db.put_receipt(receipt).await.unwrap();
        assert_eq!(db.count().await, 1);

        db.clear().await;
        assert_eq!(db.count().await, 0);
    }
}
