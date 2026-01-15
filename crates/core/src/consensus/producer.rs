//! Block Producer Module
//! 
//! Responsible for producing new blocks when this node is selected as proposer.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, Instant};
use tracing::{debug, info, warn, error};

use norn_common::types::{Block, BlockHeader, Hash, Transaction, PublicKey, GeneralParams};
use anyhow::Result;
use norn_crypto::vrf::{VRFKeyPair, VRFCalculator, VRFOutput, VRFSelector};
use sha2::{Sha256, Digest};

use crate::blockchain::Blockchain;
use crate::txpool::TxPool;
use crate::merkle::build_merkle_tree;
use crate::consensus::povf::{PoVFConfig, PoVFEngine, ConsensusMessage, BlockProposal, ConsensusResult};


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
    /// Computing VDF
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
    state: Arc<RwLock<ProducerState>>,
    last_produced: Arc<RwLock<Option<Instant>>>,
    consensus_engine: Option<Arc<PoVFEngine>>,
}

impl BlockProducer {
    /// Create a new block producer
    pub fn new(
        config: BlockProducerConfig,
        blockchain: Arc<Blockchain>,
        tx_pool: Arc<TxPool>,
        vrf_key_pair: VRFKeyPair,
        consensus_engine: Option<Arc<PoVFEngine>>,
    ) -> Self {
        Self {
            config,
            blockchain,
            tx_pool,
            vrf_key_pair,
            state: Arc::new(RwLock::new(ProducerState::Idle)),
            last_produced: Arc::new(RwLock::new(None)),
            consensus_engine,
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

        // Check VRF selection
        self.check_vrf_selection().await
    }

    /// Check if this node is selected via VRF
    async fn check_vrf_selection(&self) -> bool {
        // Get latest block for seed
        let latest = self.blockchain.latest_block.read().await;
        let seed = latest.header.block_hash;
        let round = (latest.header.height + 1) as u64;
        
        // Calculate VRF output
        let mut message = seed.0.to_vec();
        message.extend_from_slice(&round.to_le_bytes());
        
        match VRFCalculator::calculate(&self.vrf_key_pair, &message) {
            Ok(output) => {
                // Simple threshold check: if VRF output first byte < threshold, we're selected
                // In production, this would be weighted by stake
                let threshold = 255u8; // Always produce for testing
                output.output[0] <= threshold
            }
            Err(e) => {
                warn!("VRF calculation failed: {}", e);
                false
            }
        }
    }

    /// Produce a new block
    pub async fn produce_block(&self) -> Result<(Block, VRFOutput)> {
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
        drop(latest);

        // Calculate merkle root from transactions
        let merkle_root = build_merkle_tree(&transactions);

        // Get VRF output for this round
        let pub_key = self.vrf_to_public_key();
        let mut address = [0u8; 20];
        address.copy_from_slice(&pub_key.0[..20]);
        
        // Calculate seed (must match PoVFEngine logic)
        let genesis_hash = norn_common::genesis::GENESIS_BLOCK_HASH;
        let mut hasher = Sha256::new();
        hasher.update(genesis_hash.0);
        hasher.update(&(new_height as u64).to_le_bytes());
        let seed = hasher.finalize();

        let message = VRFSelector::create_selection_message(
            &seed, 
            new_height as u64, 
            &address
        );
        let vrf_output = VRFCalculator::calculate(&self.vrf_key_pair, &message)?;

        // Create block params
        let params = self.create_block_params(&vrf_output, new_height as u64);
        let params_bytes = norn_common::utils::codec::serialize(&params)?;

        // Create block header
        let header = BlockHeader {
            timestamp: chrono::Utc::now().timestamp(),
            prev_block_hash: prev_hash,
            block_hash: Hash::default(), // Will be calculated
            merkle_root,
            height: new_height,
            public_key: self.vrf_to_public_key(),
            params: params_bytes,
            gas_limit: self.config.max_gas_per_block,
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

        // Update last produced time
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

    /// Create block params including VRF/VDF data
    fn create_block_params(&self, vrf_output: &VRFOutput, height: u64) -> GeneralParams {
        // Calculate base VDF iterations
        let base_iterations = 1000u64;
        let iterations = base_iterations + (height % 100);
        
        GeneralParams {
            result: vrf_output.output.to_vec(),
            random_number: self.vrf_to_public_key(),
            s: vec![], // Placeholder for VDF proof
            t: iterations.to_le_bytes().to_vec(),
            proof: vrf_output.proof.to_bytes().to_vec(),
        }
    }

    /// Convert VRF key pair to PublicKey (33 bytes)
    fn vrf_to_public_key(&self) -> PublicKey {
        let vrf_bytes = self.vrf_key_pair.public_key_bytes();
        let mut pub_key_bytes = [0u8; 33];
        pub_key_bytes[..32].copy_from_slice(&vrf_bytes);
        pub_key_bytes[32] = 0x02; // Prefix for compressed public key format
        PublicKey(pub_key_bytes)
    }

    /// Calculate block hash
    fn calculate_block_hash(&self, block: &Block) -> Hash {
        use sha2::{Sha256, Digest};
        
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
                    Ok((block, vrf_output)) => {
                        info!("Successfully produced block at height {}", block.header.height);
                        
                        if let Some(engine) = &self.consensus_engine {
                            // Propose to consensus engine
                            let proposal = ConsensusMessage::BlockProposal {
                                proposer: block.header.public_key,
                                block: block.clone(),
                                vrf_output,
                                round: block.header.height as u64, // Simplified round = height
                            };
                            
                            match engine.handle_message(proposal).await {
                                Ok(result) => {
                                    if result.is_finalized {
                                        info!("Block finalized by consensus, saving to chain");
                                        if let Err(e) = self.blockchain.commit_block(&result.block).await {
                                            error!("Failed to save finalized block: {}", e);
                                        }
                                    } else {
                                        info!("Block proposed but not yet finalized (waiting for votes)");
                                    }
                                }
                                Err(e) => {
                                    error!("Consensus proposal failed: {}", e);
                                }
                            }
                        } else {
                            // Direct save (fallback)
                            if let Err(e) = self.blockchain.commit_block(&block).await {
                                error!("Failed to save produced block: {}", e);
                            }
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
    use norn_storage::SledDB;

    #[tokio::test]
    async fn test_block_producer_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());
        let blockchain = Blockchain::new_with_fixed_genesis(db).await;
        let tx_pool = Arc::new(TxPool::new());
        let vrf_key_pair = VRFKeyPair::generate();
        
        let config = BlockProducerConfig::default();
        let producer = BlockProducer::new(config, blockchain, tx_pool, vrf_key_pair, None);
        
        assert_eq!(producer.get_state().await, ProducerState::Idle);
    }

    #[tokio::test]
    async fn test_block_production() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());
        let blockchain = Blockchain::new_with_fixed_genesis(db).await;
        let tx_pool = Arc::new(TxPool::new());
        let vrf_key_pair = VRFKeyPair::generate();
        
        let config = BlockProducerConfig {
            is_validator: true,
            ..Default::default()
        };
        let producer = BlockProducer::new(config, blockchain.clone(), tx_pool, vrf_key_pair, None);
        
        // Produce a block
        let (block, _) = producer.produce_block().await.unwrap();
        
        assert_eq!(block.header.height, 1);
        assert!(!block.header.block_hash.0.iter().all(|&b| b == 0));
    }
}
