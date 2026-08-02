//! Block Producer Module
//! 
//! Responsible for producing new blocks when this node is selected as proposer.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, Instant};
use tracing::{info, warn, error};

use norn_common::types::{Block, BlockHeader, Hash, Transaction, PublicKey, GeneralParams};
use anyhow::Result;
use norn_crypto::vrf::{VRFKeyPair, VRFCalculator, VRFOutputData};
use sha2::{Sha256, Digest};

use crate::blockchain::Blockchain;
use crate::txpool::TxPool;
use crate::merkle::build_merkle_tree;
use crate::consensus::povf::PoVFEngine;
use crate::state::AccountStateManager;
use crate::state::merkle::StateRootCalculator;
use crate::evm::EIP1559FeeCalculator;

/// Block producer configuration
#[derive(Debug, Clone)]
pub struct BlockProducerConfig {
    /// Target block interval in seconds
    pub block_interval: u64,
    /// Maximum transactions per block
    pub max_txs_per_block: usize,
    /// Maximum gas per block
    pub max_gas_per_block: i64,
    /// Whether this node is a validator
    pub is_validator: bool,
}

impl Default for BlockProducerConfig {
    fn default() -> Self {
        Self {
            block_interval: 5,
            max_txs_per_block: 1000,
            max_gas_per_block: 10_000_000,
            is_validator: false,
        }
    }
}

/// Block producer state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProducerState {
    /// Idle, waiting for turn
    Idle,
    /// Preparing a new block
    Preparing,
    /// Computing VDF / Delay
    ComputingVdf,
    /// Block ready to propose
    ReadyToPropose,
    /// Waiting for votes
    WaitingForVotes,
}

/// Block producer responsible for creating new blocks
pub struct BlockProducer {
    config: BlockProducerConfig,
    blockchain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    vrf_key_pair: VRFKeyPair,
    state_manager: Arc<AccountStateManager>,
    state: Arc<RwLock<ProducerState>>,
    last_produced: Arc<RwLock<Option<Instant>>>,
    consensus_engine: Option<Arc<PoVFEngine>>,
    fee_calculator: EIP1559FeeCalculator,
}

impl BlockProducer {
    /// Create a new block producer
    pub fn new(
        config: BlockProducerConfig,
        blockchain: Arc<Blockchain>,
        tx_pool: Arc<TxPool>,
        vrf_key_pair: VRFKeyPair,
        state_manager: Arc<AccountStateManager>,
        consensus_engine: Option<Arc<PoVFEngine>>,
    ) -> Self {
        let fee_calculator = EIP1559FeeCalculator::default_config();

        Self {
            config,
            blockchain,
            tx_pool,
            vrf_key_pair,
            state_manager,
            state: Arc::new(RwLock::new(ProducerState::Idle)),
            last_produced: Arc::new(RwLock::new(None)),
            consensus_engine,
            fee_calculator,
        }
    }

    /// Get current producer state
    pub async fn get_state(&self) -> ProducerState {
        *self.state.read().await
    }

    /// Check if this node should produce a block
    pub async fn should_produce(&self) -> bool {
        if !self.config.is_validator {
            return false;
        }

        // Check if enough time has passed since last block
        let last = self.last_produced.read().await;
        if let Some(last_time) = *last {
            if last_time.elapsed() < Duration::from_secs(self.config.block_interval) {
                return false;
            }
        }

        true
    }

    /// Produce a new block
    pub async fn produce_block(&self) -> Result<(Block, VRFOutputData)> {
        info!("Starting block production");
        
        {
            let mut state = self.state.write().await;
            *state = ProducerState::Preparing;
        }

        // Get transactions from pool
        let transactions = self.select_transactions().await;
        
        // Get latest block
        let latest = self.blockchain.latest_block.read().await;
        let prev_hash = latest.header.block_hash;
        let new_height = latest.header.height + 1;
        let parent_base_fee = latest.header.base_fee;
        drop(latest);

        // Calculate merkle root from transactions
        let merkle_root = build_merkle_tree(&transactions);

        // Calculate gas used by transactions
        let gas_used: i64 = transactions.iter()
            .map(|tx| tx.body.gas)
            .sum();

        // Calculate EIP-1559 base fee for this block
        let base_fee = self.fee_calculator.calculate_next_base_fee(
            parent_base_fee,
            gas_used as u64,
        );

        // Calculate seed and message
        let genesis_hash = norn_common::genesis::GENESIS_BLOCK_HASH;
        let mut hasher = Sha256::new();
        hasher.update(genesis_hash.0);
        hasher.update(&(new_height as u64).to_le_bytes());
        let seed = hasher.finalize();

        let vrf_output = VRFCalculator::calculate(&self.vrf_key_pair, &seed)?;

        // Create block params
        let params = self.create_block_params(&vrf_output, new_height as u64);
        let params_bytes = norn_common::utils::codec::serialize(&params)?;

        // Calculate state root
        let state_root_calculator = StateRootCalculator::new(false);
        let state_root = state_root_calculator
            .calculate_from_manager(&self.state_manager)
            .await
            .unwrap_or_else(|_| Hash::default());

        // Create block header
        let header = BlockHeader {
            timestamp: chrono::Utc::now().timestamp(),
            prev_block_hash: prev_hash,
            block_hash: Hash::default(),
            merkle_root,
            state_root,
            height: new_height,
            public_key: self.vrf_to_public_key(),
            params: params_bytes,
            gas_limit: self.config.max_gas_per_block,
            base_fee,
        };

        // Create block
        let mut block = Block {
            header,
            transactions,
        };

        // Calculate block hash
        block.header.block_hash = self.calculate_block_hash(&block);

        {
            let mut state = self.state.write().await;
            *state = ProducerState::ReadyToPropose;
        }

        {
            let mut last = self.last_produced.write().await;
            *last = Some(Instant::now());
        }

        info!("Block produced at height {}", block.header.height);
        Ok((block, vrf_output))
    }

    /// Select transactions for the block
    async fn select_transactions(&self) -> Vec<Transaction> {
        self.tx_pool.package(&*self.blockchain).await
            .into_iter()
            .take(self.config.max_txs_per_block)
            .collect()
    }

    /// Create block params including VRF data
    fn create_block_params(&self, vrf_output: &VRFOutputData, height: u64) -> GeneralParams {
        let base_iterations = 1000u64;
        let iterations = base_iterations + (height % 100);
        
        GeneralParams {
            result: vrf_output.output_bytes.to_vec(),
            random_number: self.vrf_to_public_key(),
            s: vrf_output.preout.0.to_vec(),
            t: iterations.to_le_bytes().to_vec(),
            proof: vrf_output.proof.0.to_vec(),
        }
    }

    /// Convert VRF key pair to PublicKey (33 bytes)
    fn vrf_to_public_key(&self) -> PublicKey {
        let vrf_bytes = self.vrf_key_pair.public_key_bytes();
        let mut pub_key_bytes = [0u8; 33];
        pub_key_bytes[..32].copy_from_slice(&vrf_bytes);
        pub_key_bytes[32] = 0x02;
        PublicKey(pub_key_bytes)
    }

    /// Calculate block hash
    fn calculate_block_hash(&self, block: &Block) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(block.header.timestamp.to_le_bytes());
        hasher.update(block.header.prev_block_hash.0);
        hasher.update(block.header.merkle_root.0);
        hasher.update(block.header.height.to_le_bytes());
        hasher.update(block.header.public_key.0);
        hasher.update(&block.header.params);
        hasher.update(block.header.gas_limit.to_le_bytes());
        
        let result = hasher.finalize();
        let mut hash = Hash::default();
        hash.0.copy_from_slice(&result);
        hash
    }

    /// Run the block production loop
    pub async fn run(&self) {
        info!("Block producer started");
        let mut timer = interval(Duration::from_secs(1));
        
        loop {
            timer.tick().await;
            if self.should_produce().await {
                match self.produce_block().await {
                    Ok((block, _)) => {
                        info!("Successfully produced block at height {}", block.header.height);
                        if let Err(e) = self.blockchain.commit_block(&block).await {
                            error!("Failed to save produced block: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Block production failed: {}", e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AccountStateManager;
    use norn_storage::SledDB;

    #[tokio::test]
    async fn test_block_producer_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());
        let blockchain = Blockchain::new_with_fixed_genesis(db).await;
        let tx_pool = Arc::new(TxPool::new());
        let state_manager = Arc::new(AccountStateManager::default());
        let vrf_key_pair = VRFKeyPair::generate();

        let config = BlockProducerConfig::default();
        let producer = BlockProducer::new(config, blockchain, tx_pool, vrf_key_pair, state_manager, None);

        assert_eq!(producer.get_state().await, ProducerState::Idle);
    }

    #[tokio::test]
    async fn test_block_production() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());
        let blockchain = Blockchain::new_with_fixed_genesis(db).await;
        let tx_pool = Arc::new(TxPool::new());
        let state_manager = Arc::new(AccountStateManager::default());
        let vrf_key_pair = VRFKeyPair::generate();

        let config = BlockProducerConfig {
            is_validator: true,
            ..Default::default()
        };
        let producer = BlockProducer::new(config, blockchain.clone(), tx_pool, vrf_key_pair, state_manager, None);

        let (block, _) = producer.produce_block().await.unwrap();
        assert_eq!(block.header.height, 1);
        assert!(!block.header.block_hash.0.iter().all(|&b| b == 0));
    }
}
