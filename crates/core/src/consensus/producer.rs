//! Block Producer Module
//! 
//! Responsible for producing new blocks and signed proposals when this node is selected as proposer.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, Instant};
use tracing::{info, error};

use norn_common::types::{Block, BlockHeader, BlockId, Hash, Transaction, PublicKey, GeneralParams};
use norn_common::consensus_types::Proposal;
use anyhow::{anyhow, Result};
use norn_crypto::vrf::{VRFKeyPair, VRFCalculator, VrfContext};
use sha2::{Sha256, Digest};

use crate::blockchain::Blockchain;
use crate::txpool::TxPool;
use crate::merkle::build_merkle_tree;
use crate::consensus::povf::PoVFEngine;
use crate::consensus::types::ProposalSigner;
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
    /// Block ready to propose
    ReadyToPropose,
}

/// Block producer responsible for creating new blocks and proposals
pub struct BlockProducer {
    config: BlockProducerConfig,
    blockchain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    vrf_key_pair: VRFKeyPair,
    state_manager: Arc<AccountStateManager>,
    state: Arc<RwLock<ProducerState>>,
    last_produced: Arc<RwLock<Option<Instant>>>,
    consensus_engine: Option<Arc<PoVFEngine>>,
    proposal_signer: Option<Arc<dyn ProposalSigner>>,
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
        proposal_signer: Option<Arc<dyn ProposalSigner>>,
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
            proposal_signer,
            fee_calculator,
        }
    }

    /// Get current producer state
    pub async fn get_state(&self) -> ProducerState {
        *self.state.read().await
    }

    /// Check if local node is the current BFT proposer
    pub async fn should_produce(&self) -> bool {
        if !self.config.is_validator {
            return false;
        }

        let last = self.last_produced.read().await;
        if let Some(last_time) = *last {
            if last_time.elapsed() < Duration::from_secs(self.config.block_interval) {
                return false;
            }
        }

        if let Some(ref engine) = self.consensus_engine {
            let sm = engine.state_machine.read().await;
            sm.is_local_proposer()
        } else {
            true
        }
    }

    /// Produce a new block and signed Proposal for BFT broadcast
    pub async fn produce_proposal(&self) -> Result<(Proposal, Block)> {
        info!("Starting block and proposal production");
        
        {
            let mut state = self.state.write().await;
            *state = ProducerState::Preparing;
        }

        let transactions = self.select_transactions().await;
        
        let latest = self.blockchain.latest_block.read().await;
        let prev_hash = latest.header.block_hash;
        let new_height: u64 = (latest.header.height + 1) as u64;
        let parent_base_fee = latest.header.base_fee;
        drop(latest);

        let merkle_root = build_merkle_tree(&transactions);

        let gas_used: i64 = transactions.iter()
            .map(|tx| tx.body.gas)
            .sum();

        let base_fee = self.fee_calculator.calculate_next_base_fee(
            parent_base_fee,
            gas_used as u64,
        );

        let (config, snapshot, (round, valid_round, valid_round_cert), parent_rand) = if let Some(ref engine) = self.consensus_engine {
            let sm = engine.state_machine.read().await;
            (sm.config.clone(), sm.snapshot.clone(), (sm.round, sm.valid_round, sm.valid_round_certificate.clone()), sm.parent_randomness)
        } else {
            (Default::default(), Default::default(), (0, None, None), Hash::default())
        };

        let local_proposer = self.proposal_signer.as_ref()
            .map(|s| s.validator_id())
            .unwrap_or_else(|| norn_common::types::ValidatorId([0u8; 32]));

        let vrf_context = VrfContext {
            protocol_version: config.protocol_version.clone(),
            chain_id: config.chain_id.clone(),
            epoch: config.epoch,
            height: new_height,
            round,
            parent_block_hash: prev_hash,
            stake_snapshot_hash: snapshot.snapshot_hash.clone(),
            validator_id: local_proposer,
        };

        let vrf_output = VRFCalculator::calculate_with_context(&self.vrf_key_pair, &vrf_context)?;

        let state_root_calculator = StateRootCalculator::new(false);
        let state_root = state_root_calculator
            .calculate_from_manager(&self.state_manager)
            .await
            .unwrap_or_else(|_| Hash::default());

        let header = BlockHeader {
            protocol_version: config.protocol_version.clone(),
            chain_id: config.chain_id.clone(),
            height: new_height as i64,
            epoch: config.epoch,
            round,
            timestamp: chrono::Utc::now().timestamp(),
            prev_block_hash: prev_hash,
            block_hash: Hash::default(),
            merkle_root,
            state_root,
            proposer: local_proposer,
            stake_snapshot_hash: snapshot.snapshot_hash.clone(),
            parent_randomness: parent_rand,
            gas_limit: self.config.max_gas_per_block,
            base_fee,
            consensus_data_hash: Hash::default(),
        };

        let mut block = Block {
            header,
            transactions,
        };

        block.header.block_hash = block.header.calculate_hash()?;
        let block_id = BlockId(block.header.block_hash);

        let mut unsigned_proposal = Proposal {
            protocol_version: config.protocol_version,
            chain_id: config.chain_id,
            epoch: config.epoch,
            height: new_height,
            round,
            valid_round,
            valid_round_certificate: valid_round_cert,
            block_id,
            parent_block_hash: prev_hash,
            stake_snapshot_hash: snapshot.snapshot_hash,
            proposer: local_proposer,
            vrf_preout: vrf_output.preout.0,
            vrf_proof: vrf_output.proof.0,
            signature: [0u8; 64],
        };

        let sign_bytes = unsigned_proposal.canonical_bytes();
        let signature = if let Some(ref signer) = self.proposal_signer {
            signer.sign_proposal(&sign_bytes)?
        } else {
            [0u8; 64]
        };

        unsigned_proposal.signature = signature;

        {
            let mut state = self.state.write().await;
            *state = ProducerState::ReadyToPropose;
        }

        {
            let mut last = self.last_produced.write().await;
            *last = Some(Instant::now());
        }

        info!("Proposal produced at height {} round {}", new_height, round);
        Ok((unsigned_proposal, block))
    }

    async fn select_transactions(&self) -> Vec<Transaction> {
        self.tx_pool.package(&*self.blockchain).await
            .into_iter()
            .take(self.config.max_txs_per_block)
            .collect()
    }

    fn vrf_to_public_key(&self) -> PublicKey {
        let vrf_bytes = self.vrf_key_pair.public_key_bytes();
        let mut pub_key_bytes = [0u8; 33];
        pub_key_bytes[..32].copy_from_slice(&vrf_bytes);
        pub_key_bytes[32] = 0x02;
        PublicKey(pub_key_bytes)
    }

    fn calculate_block_hash(&self, block: &Block) -> Hash {
        block.header.calculate_hash().unwrap_or_default()
    }

    pub async fn run(&self) {
        info!("Block producer started");
        let mut timer = interval(Duration::from_secs(1));
        
        loop {
            timer.tick().await;
            if self.should_produce().await {
                if let Ok((proposal, block)) = self.produce_proposal().await {
                    info!("Produced signed proposal for block {:?} at height {}", proposal.block_id, block.header.height);
                    if let Some(ref engine) = self.consensus_engine {
                        let _ = engine.candidate_blocks.write().await.insert((block.header.height as u64, proposal.block_id), block);
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
        let producer = BlockProducer::new(config, blockchain, tx_pool, vrf_key_pair, state_manager, None, None);

        assert_eq!(producer.get_state().await, ProducerState::Idle);
    }
}
