use crate::block_buffer::BlockBuffer;
use crate::data_processor::DataProcessor;
use crate::txpool::ChainReader;
use moka::future::Cache;
use norn_common::traits::DBInterface;
use norn_common::types::{Block, Hash, Transaction, GenesisParams};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

// Constants
const MAX_BLOCK_CACHE: u64 = 64;
const MAX_TX_CACHE: u64 = 40960;

pub struct Blockchain {
    db: Arc<dyn DBInterface>,

    // Caches
    block_cache: Cache<Hash, Block>,
    tx_cache: Cache<Hash, Transaction>,
    // Mapping Height -> Hash. Go used LRU, but for fast access maybe we want it?
    // If chain is long, we can't keep all in memory. LRU is correct.
    block_height_map: Cache<i64, Hash>,

    // State
    pub latest_block: Arc<RwLock<Block>>,

    // Components
    pub buffer: BlockBuffer,
    pub data_processor: Arc<DataProcessor>,

    // Genesis parameters (now fixed)
    genesis_params: GenesisParams,

    // Internal
    pop_rx: tokio::sync::Mutex<mpsc::Receiver<Block>>,
}

impl Blockchain {
    /// Create blockchain with fixed genesis block (recommended approach)
    /// This ensures all nodes use the same genesis block
    pub async fn new_with_fixed_genesis(db: Arc<dyn DBInterface>) -> Arc<Self> {
        let genesis = norn_common::genesis::get_genesis_block();
        Self::new_with_genesis(db, genesis).await
    }

    /// Create blockchain with existing blockchain data
    /// Loads existing chain or initializes with given genesis
    pub async fn new_with_genesis(db: Arc<dyn DBInterface>, genesis: Block) -> Arc<Self> {
        let (pop_tx, pop_rx) = mpsc::channel(128);
        let dp = DataProcessor::new(db.clone());

        let mut latest_block = genesis.clone();
        let mut loaded_from_db = false;

        // Try load latest from DB
        let latest_key = b"latest";
        if let Ok(Some(hash_bytes)) = db.get(latest_key).await {
            if hash_bytes.len() == 32 {
                let mut hash = Hash::default();
                hash.0.copy_from_slice(&hash_bytes);

                if let Ok(block_bytes) = db.get(&norn_common::utils::db_keys::block_hash_to_db_key(&hash)).await {
                    if let Some(b_bytes) = block_bytes {
                        if let Ok(loaded) = norn_common::utils::codec::deserialize::<Block>(&b_bytes) {
                            latest_block = loaded;
                            loaded_from_db = true;
                        }
                    }
                }
            }
        }

        // If not loaded from DB, this is a new chain
        if !loaded_from_db {
            // Verify genesis block is correct
            if !norn_common::genesis::is_valid_genesis_block(&genesis) {
                error!("Invalid genesis block provided!");
            }
        }

        let buffer = BlockBuffer::new(latest_block.clone(), pop_tx).await;

        // Extract genesis parameters
        let genesis_params = if genesis.header.height == 0 {
            norn_common::genesis::get_genesis_params()
        } else {
            // If we loaded from DB, try to get genesis block to extract params
            if let Some(g_block) = get_block_by_height_from_db(&db, 0).await {
                extract_genesis_params(&g_block)
            } else {
                norn_common::genesis::get_genesis_params()
            }
        };

        let chain = Arc::new(Self {
            db,
            block_cache: Cache::new(MAX_BLOCK_CACHE),
            tx_cache: Cache::new(MAX_TX_CACHE),
            block_height_map: Cache::new(MAX_BLOCK_CACHE),
            latest_block: Arc::new(RwLock::new(latest_block.clone())),
            buffer,
            data_processor: dp,
            genesis_params,
            pop_rx: tokio::sync::Mutex::new(pop_rx),
        });

        // If fresh chain, save genesis
        if !loaded_from_db {
            info!("Initializing new blockchain with genesis block");
            if let Err(e) = chain.save_block(&genesis).await {
                error!("Failed to save genesis block: {}", e);
            } else {
                if let Err(e) = chain.save_latest_index(&genesis.header.block_hash).await {
                    error!("Failed to save latest index: {}", e);
                }
            }
        } else {
            info!("Loaded existing blockchain, latest height: {}", latest_block.header.height);
        }

        let c = chain.clone();
        tokio::spawn(async move {
            c.finalize_loop().await;
        });

        chain
    }

    /// Legacy method for backward compatibility
    /// Note: This method is deprecated as it can cause genesis block inconsistencies
    #[deprecated(note = "Use new_with_fixed_genesis instead to ensure consistent genesis blocks")]
    pub async fn new(db: Arc<dyn DBInterface>, genesis: Block) -> Arc<Self> {
        Self::new_with_genesis(db, genesis).await
    }

    async fn save_latest_index(&self, hash: &Hash) -> anyhow::Result<()> {
        self.db.insert(b"latest", &hash.0).await?;
        Ok(())
    }

    async fn finalize_loop(&self) {
        let mut rx = self.pop_rx.lock().await;
        while let Some(block) = rx.recv().await {
            info!("Finalizing block height={}", block.header.height);
            if let Err(e) = self.save_block(&block).await {
                error!("Failed to save block: {}", e);
            } else {
                let mut latest = self.latest_block.write().await;
                *latest = block.clone();
                drop(latest); // Unlock before saving index

                if let Err(e) = self.save_latest_index(&block.header.block_hash).await {
                     error!("Failed to save latest index: {}", e);
                }
            }
        }
    }

    pub async fn add_block(&self, block: Block) {
        self.buffer.append_block(block).await;
    }

    pub async fn get_block_by_hash(&self, hash: &Hash) -> Option<Block> {
        // 1. Check Cache
        if let Some(block) = self.block_cache.get(hash).await {
            return Some(block);
        }

        // 2. Check DB
        let db_key = norn_common::utils::db_keys::block_hash_to_db_key(hash);
        if let Ok(Some(bytes)) = self.db.get(&db_key).await {
            // Deserialize (Karmem or JSON depending on impl. We used JSON/Hex in common/utils/codec for now?)
            // Go used Karmem. Rust `common::types::Block` derives Serialize/Deserialize (JSON default).  
            // We should use a consistent codec.
            // Let's assume JSON for now or use `norn_common::utils::codec::deserialize`.
            if let Ok(block) = norn_common::utils::codec::deserialize::<Block>(&bytes) {
                self.block_cache.insert(*hash, block.clone()).await;
                return Some(block);
            }
        }
        None
    }

    pub async fn get_block_by_height(&self, height: i64) -> Option<Block> {
        // 1. Check Cache (Height Map)
        if let Some(hash) = self.block_height_map.get(&height).await {
            return self.get_block_by_hash(&hash).await;
        }

        // 2. Check DB for Height->Hash mapping
        let key = norn_common::utils::db_keys::block_height_to_db_key(height);
        if let Ok(Some(hash_bytes)) = self.db.get(&key).await {
             // Go stores Hash bytes directly? Or string?
             // Go `BlockHeight2DBKey` returns key. Value is Hash?
             // Let's assume value is Hash bytes.
             // But wait, we need to know what `SaveBlock` writes.
             // If we write Hash bytes (32), we can parse.
             if hash_bytes.len() == 32 {
                 let mut h = Hash::default();
                 h.0.copy_from_slice(&hash_bytes);
                 self.block_height_map.insert(height, h).await;
                 return self.get_block_by_hash(&h).await;
             }
        }
        None
    }

    pub async fn get_transaction_by_hash(&self, hash: &Hash) -> Option<Transaction> {
        // 1. Check Cache
        if let Some(tx) = self.tx_cache.get(hash).await {
            return Some(tx);
        }

        // 2. Check DB
        let key = norn_common::utils::db_keys::tx_hash_to_db_key(hash);
        if let Ok(Some(bytes)) = self.db.get(&key).await {
            if let Ok(tx) = norn_common::utils::codec::deserialize::<Transaction>(&bytes) {
                self.tx_cache.insert(*hash, tx.clone()).await;
                return Some(tx);
            }
        }
        None
    }

    // --- Persistence ---

    pub async fn save_block(&self, block: &Block) -> anyhow::Result<()> {
        // Batch write: Block, Transactions, Indices
        let mut keys = Vec::new();
        let mut values = Vec::new();

        let block_hash = block.header.block_hash;

        // 1. Save Block
        let block_key = norn_common::utils::db_keys::block_hash_to_db_key(&block_hash);
        let block_data = norn_common::utils::codec::serialize(block)?;
        keys.push(block_key);
        values.push(block_data);

        // 2. Save Height -> Hash mapping
        let height_key = norn_common::utils::db_keys::block_height_to_db_key(block.header.height);        
        keys.push(height_key);
        values.push(block_hash.0.to_vec()); // Store raw 32 bytes hash

        // 3. Save Transactions
        for tx in &block.transactions {
            let tx_hash = tx.body.hash;
            let tx_key = norn_common::utils::db_keys::tx_hash_to_db_key(&tx_hash);
            let tx_data = norn_common::utils::codec::serialize(tx)?;
            keys.push(tx_key);
            values.push(tx_data);

            // Go: Also updates DataProcessor logic if needed?
            // Go `dp.Run` handles data tasks.
            // If tx has data commands, should we trigger DP?
            // Go: `SendTransactionWithData` RPC creates DataTask?
            // Or `BlockChain` creates DataTask when saving?
            // We'll leave that for integration.
        }

        self.db.batch_insert(&keys, &values).await?;

        Ok(())
    }
}

use async_trait::async_trait;

// Implement ChainReader for TxPool integration
#[async_trait]
impl ChainReader for Blockchain {
    async fn get_transaction_by_hash(&self, hash: &Hash) -> Option<Transaction> {
        self.get_transaction_by_hash(hash).await
    }
}

// Helper functions
async fn get_block_by_height_from_db(db: &Arc<dyn DBInterface>, height: i64) -> Option<Block> {
    if height != 0 {
        return None;
    }

    // For genesis block (height 0), use the fixed genesis block
    Some(norn_common::genesis::get_genesis_block())
}

fn extract_genesis_params(block: &Block) -> norn_common::types::GenesisParams {
    if block.header.params.is_empty() {
        return norn_common::genesis::get_genesis_params();
    }

    match norn_common::utils::codec::deserialize::<norn_common::types::GenesisParams>(&block.header.params) {
        Ok(params) => params,
        Err(_) => norn_common::genesis::get_genesis_params(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::Block;
    use async_trait::async_trait;
    use anyhow::Result;
    use std::collections::HashMap;
    use std::sync::Mutex;
    // use norn_common::utils::codec;

    struct MockDB {
        store: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    }

    impl MockDB {
        fn new() -> Self {
            Self { store: Mutex::new(HashMap::new()) }
        }
    }

    #[async_trait]
    impl DBInterface for MockDB {
        async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
            let store = self.store.lock().unwrap();
            Ok(store.get(key).cloned())
        }
        async fn insert(&self, key: &[u8], value: &[u8]) -> Result<()> {
            let mut store = self.store.lock().unwrap();
            store.insert(key.to_vec(), value.to_vec());
            Ok(())
        }
        async fn remove(&self, key: &[u8]) -> Result<()> {
            let mut store = self.store.lock().unwrap();
            store.remove(key);
            Ok(())
        }
        async fn batch_insert(&self, keys: &[Vec<u8>], values: &[Vec<u8>]) -> Result<()> {
            let mut store = self.store.lock().unwrap();
            for (k, v) in keys.iter().zip(values.iter()) {
                store.insert(k.clone(), v.clone());
            }
            Ok(())
        }
        async fn batch_delete(&self, keys: &[Vec<u8>]) -> Result<()> {
            let mut store = self.store.lock().unwrap();
            for k in keys {
                store.remove(k);
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_blockchain_init() {
        let db = Arc::new(MockDB::new());
        let genesis = Block::default();
        
        let chain = Blockchain::new(db.clone(), genesis.clone()).await;
        
        let latest = chain.latest_block.read().await;
        assert_eq!(latest.header.block_hash, genesis.header.block_hash);
        
        // Verify DB has genesis
        let key = norn_common::utils::db_keys::block_hash_to_db_key(&genesis.header.block_hash);
        let stored = db.get(&key).await.unwrap();
        assert!(stored.is_some());
    }

    #[tokio::test]
    async fn test_blockchain_save_get() {
        let db = Arc::new(MockDB::new());
        let genesis = Block::default();
        let chain = Blockchain::new(db, genesis.clone()).await;
        
        let mut b1 = Block::default();
        b1.header.height = 1;
        b1.header.block_hash.0[0] = 1;
        
        chain.save_block(&b1).await.unwrap();
        
        let retrieved = chain.get_block_by_hash(&b1.header.block_hash).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().header.height, 1);
        
        let retrieved_height = chain.get_block_by_height(1).await;
        assert!(retrieved_height.is_some());
        assert_eq!(retrieved_height.unwrap().header.block_hash, b1.header.block_hash);
    }
}