//! Block Producer Module
//!
//! Responsible for producing new blocks and signed proposals when this node is selected as proposer.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, Instant};
use tracing::{debug, info};

use anyhow::{anyhow, Result};
use norn_common::chain_context::ChainContext;
use norn_common::consensus_types::{
    ConsensusEnvelope, ConsensusMessage, Proposal, MAX_CONSENSUS_ENVELOPE_BYTES,
};
use norn_common::genesis::ProtocolResourceLimits;
use norn_common::types::{
    Address, Block, BlockConsensusData, BlockHeader, BlockId, BlockV2, Hash, PublicKey,
    Transaction, TransactionV2,
};
use norn_crypto::vrf::{VRFCalculator, VRFKeyPair, VrfContext};

use crate::blockchain::Blockchain;
use crate::consensus::povf::PoVFEngine;
use crate::consensus::types::ProposalSigner;
use crate::evm::{CodeStorage, EIP1559FeeCalculator};
use crate::execution::{calculate_v2_execution_data_hash, execute_v2_block, V2ExecutionContext};
use crate::finality::FinalityStore;
use crate::merkle::build_merkle_tree;
use crate::state::merkle::StateRootCalculator;
use crate::state::AccountStateManager;
use crate::txpool::TxPool;
use crate::txpool_v2::TransactionV2Pool;

/// Block producer configuration
#[derive(Debug, Clone)]
pub struct BlockProducerConfig {
    /// Target block interval in seconds
    pub block_interval: u64,
    /// Maximum transactions per block
    pub max_txs_per_block: usize,
    /// Maximum gas per block
    pub max_gas_per_block: i64,
    /// Maximum serialized block size from Genesis resource parameters
    pub max_block_bytes: usize,
    /// Maximum serialized transaction size from Genesis resource parameters
    pub max_transaction_bytes: usize,
    /// Whether this node is a validator
    pub is_validator: bool,
}

impl Default for BlockProducerConfig {
    fn default() -> Self {
        Self {
            block_interval: 5,
            max_txs_per_block: 1000,
            max_gas_per_block: 10_000_000,
            max_block_bytes: 8 * 1024 * 1024,
            max_transaction_bytes: 256 * 1024,
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
    v2_tx_pool: Option<Arc<TransactionV2Pool>>,
    v2_code_storage: Option<Arc<CodeStorage>>,
    vrf_key_pair: VRFKeyPair,
    state_manager: Arc<AccountStateManager>,
    state: Arc<RwLock<ProducerState>>,
    last_produced: Arc<RwLock<Option<Instant>>>,
    consensus_engine: Option<Arc<PoVFEngine>>,
    proposal_signer: Option<Arc<dyn ProposalSigner>>,
    finality_store: Option<Arc<FinalityStore>>,
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
            v2_tx_pool: None,
            v2_code_storage: None,
            vrf_key_pair,
            state_manager,
            state: Arc::new(RwLock::new(ProducerState::Idle)),
            last_produced: Arc::new(RwLock::new(None)),
            consensus_engine,
            proposal_signer,
            finality_store: None,
            fee_calculator,
        }
    }

    /// Attach the protocol-v2 pool before the producer is shared with node
    /// tasks.  The pool is optional only for the legacy compatibility
    /// producer; V2 production fails closed when it has not been attached.
    pub fn attach_v2_pool(&mut self, tx_pool: Arc<TransactionV2Pool>) {
        self.v2_tx_pool = Some(tx_pool);
    }

    pub fn attach_v2_code_storage(&mut self, code_storage: Arc<CodeStorage>) {
        self.v2_code_storage = Some(code_storage);
    }

    pub fn attach_finality_store(&mut self, finality_store: Arc<FinalityStore>) {
        self.finality_store = Some(finality_store);
    }

    /// Get current producer state
    pub async fn get_state(&self) -> ProducerState {
        *self.state.read().await
    }

    /// Check if local node is the current BFT proposer
    pub async fn should_produce(&self) -> bool {
        if !self.config.is_validator {
            info!("V2 producer disabled because node is not a validator");
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
            let selected = sm.is_local_proposer();
            debug!(
                "V2 producer eligibility check height={} round={} local={:?} proposer={:?} selected={}",
                sm.height,
                sm.round,
                sm.local_validator_id,
                sm.get_current_proposer(),
                selected
            );
            if selected {
                debug!(
                    "V2 producer eligible at height {} round {} for validator {:?}",
                    sm.height, sm.round, sm.local_validator_id
                );
            }
            selected
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

        let gas_used: i64 = transactions.iter().map(|tx| tx.body.gas).sum();

        let base_fee = self
            .fee_calculator
            .calculate_next_base_fee(parent_base_fee, gas_used as u64);

        let (config, snapshot, (round, valid_round, valid_round_cert), parent_rand, epoch) =
            if let Some(ref engine) = self.consensus_engine {
                let sm = engine.state_machine.read().await;
                (
                    sm.config.clone(),
                    sm.snapshot.clone(),
                    (sm.round, sm.valid_round, sm.valid_round_certificate.clone()),
                    sm.parent_randomness,
                    sm.current_epoch()?,
                )
            } else {
                (
                    Default::default(),
                    Default::default(),
                    (0, None, None),
                    Hash::default(),
                    1,
                )
            };

        let local_proposer = self
            .proposal_signer
            .as_ref()
            .map(|s| s.validator_id())
            .unwrap_or_else(|| norn_common::types::ValidatorId([0u8; 32]));

        let vrf_context = VrfContext {
            protocol_version: config.protocol_version.clone(),
            chain_id: config.chain_id.clone(),
            epoch,
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
            epoch,
            round,
            timestamp: chrono::Utc::now().timestamp(),
            prev_block_hash: prev_hash,
            block_hash: Hash::default(),
            merkle_root,
            state_root,
            block_builder: local_proposer,
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

    /// Produce a fully committed V2 block template.  This path deliberately
    /// does not mutate the live state or remove transactions from the pool:
    /// finality/recovery owns those side effects in the later commit stage.
    pub async fn produce_v2_block(
        &self,
        context: &ChainContext,
        limits: &ProtocolResourceLimits,
    ) -> Result<BlockV2> {
        {
            let mut state = self.state.write().await;
            *state = ProducerState::Preparing;
        }

        let result = self.produce_v2_block_inner(context, limits).await;
        match result {
            Ok(block) => {
                let mut state = self.state.write().await;
                *state = ProducerState::ReadyToPropose;
                let mut last = self.last_produced.write().await;
                *last = Some(Instant::now());
                Ok(block)
            }
            Err(error) => {
                let mut state = self.state.write().await;
                *state = ProducerState::Idle;
                Err(error)
            }
        }
    }

    async fn produce_v2_block_inner(
        &self,
        context: &ChainContext,
        limits: &ProtocolResourceLimits,
    ) -> Result<BlockV2> {
        limits.validate()?;
        let tx_pool = self
            .v2_tx_pool
            .as_ref()
            .ok_or_else(|| anyhow!("V2 transaction pool is not attached"))?;
        if self.proposal_signer.is_none() || self.consensus_engine.is_none() {
            return Err(anyhow!(
                "V2 block production requires a consensus engine and validator signer"
            ));
        }
        let transactions = self.select_v2_transactions(tx_pool, limits);

        let finality_store = self
            .finality_store
            .as_ref()
            .ok_or_else(|| anyhow!("V2 production requires the canonical finality store"))?;
        let tip = finality_store
            .recover_canonical_tip()
            .await?
            .ok_or_else(|| anyhow!("canonical finalized tip is not initialized"))?;
        let new_height = tip
            .next_height()
            .map_err(|error| anyhow!(error.to_string()))?;
        let prev_hash = tip.block_id.0;
        let parent_base_fee = tip.base_fee;

        let engine = self
            .consensus_engine
            .as_ref()
            .expect("V2 production checked consensus engine above");
        let sm = engine.state_machine.read().await;
        let epoch = sm.current_epoch()?;
        let round = sm.round;
        let snapshot = sm.snapshot.clone();
        if snapshot.snapshot_hash != tip.active_snapshot_hash {
            return Err(anyhow!(
                "in-memory validator snapshot does not match canonical finalized tip"
            ));
        }
        let parent_randomness = tip.next_randomness;
        drop(sm);
        let proposer = self
            .proposal_signer
            .as_ref()
            .map(|signer| signer.validator_id())
            .unwrap_or_else(|| norn_common::types::ValidatorId([0u8; 32]));
        if proposer.0 == [0u8; 32] || snapshot.snapshot_hash.0 == [0u8; 32] {
            return Err(anyhow!(
                "V2 block production requires non-zero proposer and snapshot identities"
            ));
        }

        let builder_vrf_context = VrfContext {
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            epoch,
            height: new_height,
            round,
            parent_block_hash: prev_hash,
            stake_snapshot_hash: snapshot.snapshot_hash,
            validator_id: proposer,
        };
        let builder_vrf =
            VRFCalculator::calculate_with_context(&self.vrf_key_pair, &builder_vrf_context)?;

        let timestamp = chrono::Utc::now()
            .timestamp()
            .max(tip.timestamp.saturating_add(1));
        if timestamp <= tip.timestamp {
            return Err(anyhow!(
                "local clock did not advance beyond the canonical parent timestamp"
            ));
        }
        let max_timestamp = tip
            .timestamp
            .checked_add(limits.max_block_timestamp_step as i64)
            .ok_or_else(|| anyhow!("block timestamp upper bound overflow"))?;
        if timestamp > max_timestamp {
            return Err(anyhow!(
                "local clock is beyond the protocol block timestamp window"
            ));
        }
        let evm_context = self
            .v2_code_storage
            .as_ref()
            .map(|code_storage| V2ExecutionContext {
                block_number: new_height,
                block_timestamp: timestamp.max(0) as u64,
                block_coinbase: Address(proposer.0[..20].try_into().unwrap_or([0u8; 20])),
                block_gas_limit: limits.max_block_gas,
                code_storage: code_storage.clone(),
            });
        let execution = execute_v2_block(
            &self.state_manager,
            &transactions,
            limits,
            evm_context.as_ref(),
        )
        .await?;
        let state_root = execution
            .overlay
            .projected_state_root(&self.state_manager)
            .await?;

        let gas_used = i64::try_from(execution.gas_used)
            .map_err(|_| anyhow!("V2 execution gas exceeds header range"))?;
        let base_fee = self
            .fee_calculator
            .calculate_next_base_fee(parent_base_fee, execution.gas_used);
        let header = BlockHeader {
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            height: new_height as i64,
            epoch,
            round,
            timestamp,
            prev_block_hash: prev_hash,
            block_hash: Hash::default(),
            merkle_root: Hash::default(),
            state_root,
            block_builder: proposer,
            stake_snapshot_hash: snapshot.snapshot_hash,
            parent_randomness,
            gas_limit: i64::try_from(limits.max_block_gas)
                .map_err(|_| anyhow!("Genesis block gas limit exceeds header range"))?,
            base_fee,
            consensus_data_hash: Hash::default(),
        };
        let mut block = BlockV2 {
            header,
            transactions,
            consensus_data: BlockConsensusData {
                builder_vrf_preout: builder_vrf.preout.0,
                builder_vrf_proof: builder_vrf.proof.0,
                builder_round: round,
                execution_data_hash: calculate_v2_execution_data_hash(&execution.results),
            },
        };
        block.finalize_header()?;
        block.validate_structure(context, limits)?;

        // The final encoded size check includes the complete header and is
        // intentionally performed after all commitments are populated.
        let encoded_size = bincode::serialized_size(&block)? as usize;
        if encoded_size > limits.max_block_bytes as usize {
            return Err(anyhow!("V2 block exceeds Genesis byte limit"));
        }
        if gas_used > block.header.gas_limit {
            return Err(anyhow!("V2 execution gas exceeds block gas limit"));
        }
        Ok(block)
    }

    fn select_v2_transactions(
        &self,
        tx_pool: &TransactionV2Pool,
        limits: &ProtocolResourceLimits,
    ) -> Vec<TransactionV2> {
        let mut selected = Vec::with_capacity(limits.max_transactions_per_block as usize);
        let mut total_bytes = 0usize;
        let mut total_gas = 0u64;
        for tx in tx_pool.select(limits.max_transactions_per_block as usize) {
            let Ok(tx_bytes) = bincode::serialize(&tx) else {
                continue;
            };
            let Some(next_bytes) = total_bytes.checked_add(tx_bytes.len()) else {
                break;
            };
            let Some(next_gas) = total_gas.checked_add(tx.gas_limit) else {
                break;
            };
            if tx_bytes.len() > limits.max_transaction_bytes as usize
                || next_bytes > limits.max_block_bytes as usize
                || tx.gas_limit > limits.max_transaction_gas
                || next_gas > limits.max_block_gas
            {
                continue;
            }
            selected.push(tx);
            total_bytes = next_bytes;
            total_gas = next_gas;
        }
        selected
    }

    /// Produce the V2 block together with the existing consensus proposal
    /// envelope.  Consensus voting is wired to this payload in the following
    /// consensus-state-machine stage; keeping the constructor explicit here
    /// prevents a legacy `Block` from being silently substituted.
    pub async fn produce_v2_proposal(
        &self,
        context: &ChainContext,
        limits: &ProtocolResourceLimits,
    ) -> Result<(Proposal, BlockV2)> {
        debug!("Starting V2 proposal production");
        let engine = self
            .consensus_engine
            .as_ref()
            .expect("V2 production checked consensus engine above");

        if !engine.reconcile_v2_candidate_retention().await {
            return Err(anyhow!(
                "consensus state references a V2 candidate that is not retained"
            ));
        }

        // Tendermint re-proposes the exact block that reached a valid-round
        // polka.  Generating a fresh block here would change its block ID
        // while carrying the old certificate, which is an invalid proposal
        // and must be rejected by the wire validator.
        let (height, round, valid_block, valid_round, valid_round_certificate) = {
            let sm = engine.state_machine.read().await;
            (
                sm.height,
                sm.round,
                sm.valid_block,
                sm.valid_round,
                sm.valid_round_certificate.clone(),
            )
        };
        let block = if let Some(valid_block_id) = valid_block {
            let candidate = engine
                .candidate_cache_v2
                .write()
                .await
                .get_block(height, valid_block_id)
                .ok_or_else(|| {
                    anyhow!(
                        "valid-round block {:?} is not available for safe re-proposal",
                        valid_block_id
                    )
                })?;
            if candidate.header.height < 0 || candidate.header.height as u64 != height {
                return Err(anyhow!(
                    "valid-round candidate height does not match consensus height"
                ));
            }
            candidate
        } else {
            if valid_round.is_some() || valid_round_certificate.is_some() {
                return Err(anyhow!(
                    "valid-round certificate exists without a valid block"
                ));
            }
            self.produce_v2_block(context, limits).await?
        };
        info!(
            "V2 block template produced at height {}",
            block.header.height
        );
        let proposer = self
            .proposal_signer
            .as_ref()
            .ok_or_else(|| anyhow!("V2 proposal requires a validator signer"))?
            .validator_id();
        let vrf_context = VrfContext {
            protocol_version: block.header.protocol_version,
            chain_id: block.header.chain_id,
            epoch: block.header.epoch,
            height: block.header.height as u64,
            round,
            parent_block_hash: block.header.prev_block_hash,
            stake_snapshot_hash: block.header.stake_snapshot_hash,
            validator_id: proposer,
        };
        let vrf_output = VRFCalculator::calculate_with_context(&self.vrf_key_pair, &vrf_context)?;
        let mut proposal = Proposal {
            protocol_version: block.header.protocol_version,
            chain_id: block.header.chain_id,
            epoch: block.header.epoch,
            height: block.header.height as u64,
            round,
            valid_round,
            valid_round_certificate,
            block_id: BlockId(block.header.block_hash),
            parent_block_hash: block.header.prev_block_hash,
            stake_snapshot_hash: block.header.stake_snapshot_hash,
            proposer,
            vrf_preout: vrf_output.preout.0,
            vrf_proof: vrf_output.proof.0,
            signature: [0u8; 64],
        };
        let signer = self.proposal_signer.as_ref().expect("signer checked above");
        proposal.signature = signer.sign_proposal(&proposal.canonical_bytes())?;
        let envelope = ConsensusEnvelope {
            wire_version: context.wire_version,
            protocol_version: proposal.protocol_version,
            chain_id: proposal.chain_id,
            genesis_hash: context.genesis_hash,
            payload: ConsensusMessage::ProposalV2 {
                proposal: proposal.clone(),
                block: block.clone(),
            },
        };
        if bincode::serialized_size(&envelope)? as usize > MAX_CONSENSUS_ENVELOPE_BYTES {
            return Err(anyhow!("V2 proposal exceeds consensus envelope limit"));
        }
        Ok((proposal, block))
    }

    async fn select_transactions(&self) -> Vec<Transaction> {
        let mut selected = Vec::with_capacity(self.config.max_txs_per_block);
        let mut total_bytes = 0usize;
        let mut total_gas = 0i64;
        for tx in self.tx_pool.package(&*self.blockchain).await {
            if selected.len() >= self.config.max_txs_per_block {
                break;
            }
            let Ok(tx_bytes) = bincode::serialize(&tx) else {
                continue;
            };
            if tx_bytes.len() > self.config.max_transaction_bytes
                || total_bytes.saturating_add(tx_bytes.len()) > self.config.max_block_bytes
            {
                continue;
            }
            let Some(next_gas) = total_gas.checked_add(tx.body.gas) else {
                continue;
            };
            if next_gas > self.config.max_gas_per_block {
                continue;
            }
            total_bytes += tx_bytes.len();
            total_gas = next_gas;
            selected.push(tx);
        }
        selected
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
                    info!(
                        "Produced signed proposal for block {:?} at height {}",
                        proposal.block_id, block.header.height
                    );
                    if let Some(ref engine) = self.consensus_engine {
                        let _ = engine
                            .candidate_blocks
                            .write()
                            .await
                            .insert((block.header.height as u64, proposal.block_id), block);
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
        let producer = BlockProducer::new(
            config,
            blockchain,
            tx_pool,
            vrf_key_pair,
            state_manager,
            None,
            None,
        );

        assert_eq!(producer.get_state().await, ProducerState::Idle);
    }

    #[tokio::test]
    async fn test_v2_block_production_fails_closed_without_consensus_identity() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());
        let blockchain = Blockchain::new_with_fixed_genesis(db).await;
        let tx_pool = Arc::new(TxPool::new());
        let tx_pool_v2 = Arc::new(TransactionV2Pool::new());
        let state_manager = Arc::new(AccountStateManager::default());
        let vrf_key_pair = VRFKeyPair::generate();

        let mut producer = BlockProducer::new(
            BlockProducerConfig::default(),
            blockchain,
            tx_pool,
            vrf_key_pair,
            state_manager,
            None,
            None,
        );
        producer.attach_v2_pool(tx_pool_v2.clone());

        let genesis = norn_common::genesis::GenesisConfig::from_fixed_genesis();
        let result = producer
            .produce_v2_block(&genesis.context(), &genesis.resource_limits)
            .await;

        assert!(result.is_err());
        assert_eq!(tx_pool_v2.len(), 0);
        assert_eq!(producer.get_state().await, ProducerState::Idle);
    }
}
