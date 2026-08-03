//! PoVF Orchestration Engine wrapping Tendermint BFT State Machine

use super::safety_store::{ConsensusSafetyStore, ConsensusSigner};
use super::state_machine::TendermintStateMachine;
use super::types::ConsensusConfig;
use super::vote_pool::{AddVoteResult, VotePool};
use crate::evm::CodeStorage;
use crate::execution::{
    calculate_v2_execution_data_hash, execute_v2_block, V2BlockExecution, V2ExecutionContext,
};
use crate::state::AccountStateManager;
use k256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use norn_common::chain_context::ChainContext;
use norn_common::consensus_types::{
    CommitCertificate, FinalizedBlock, FinalizedBlockV2, FinalizedConsensusState, Proposal,
    SignedVote, StakeSnapshot, VoteStep,
};
use norn_common::error::{NornError, Result};
use norn_common::genesis::ProtocolResourceLimits;
use norn_common::types::{Block, BlockHeader, BlockId, BlockV2, Hash, ValidatorId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};

const V2_CANDIDATE_TTL: Duration = Duration::from_secs(60);

/// Protocol-derived bounds for the in-memory V2 candidate cache. The cache is
/// not consensus state, but its admission policy must still be identical for
/// every node and must not be a node-local unbounded setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V2CandidateCacheLimits {
    pub max_total_bytes: usize,
    pub max_items_per_height: usize,
    pub max_items_per_proposer: usize,
    pub max_future_height: u64,
    pub max_future_round: u32,
    pub ttl: Duration,
}

impl From<&ProtocolResourceLimits> for V2CandidateCacheLimits {
    fn from(limits: &ProtocolResourceLimits) -> Self {
        let max_total_bytes = limits
            .max_block_bytes
            .saturating_mul(4)
            .try_into()
            .unwrap_or(usize::MAX);
        Self {
            max_total_bytes,
            max_items_per_height: limits.max_certificate_members.max(1) as usize,
            max_items_per_proposer: limits.max_future_round.saturating_add(1) as usize,
            max_future_height: limits.max_future_height,
            max_future_round: limits.max_future_round,
            ttl: V2_CANDIDATE_TTL,
        }
    }
}

/// Retention classes are driven by consensus dependencies, not by local
/// cache pressure.  A pinned candidate may only be released when the
/// corresponding finalized height or state-machine dependency is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateRetention {
    Normal,
    ValidRoundPinned,
    LockedPinned,
    PendingFinalityPinned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct V2CandidateEntry {
    proposal: Proposal,
    block: BlockV2,
    derived_randomness: Hash,
    inserted_at: Instant,
    encoded_bytes: usize,
    retention: CandidateRetention,
}

/// A single bounded cache keeps proposal, block, and derived randomness
/// inseparable. This prevents the old three-HashMap design from retaining
/// mismatched entries or bypassing a shared byte/TTL limit.
#[derive(Debug)]
pub struct V2CandidateCache {
    limits: V2CandidateCacheLimits,
    entries: HashMap<(u64, BlockId), V2CandidateEntry>,
    pending_finality: HashSet<(u64, BlockId)>,
    total_bytes: usize,
}

impl V2CandidateCache {
    pub fn new(limits: V2CandidateCacheLimits) -> Self {
        Self {
            limits,
            entries: HashMap::new(),
            pending_finality: HashSet::new(),
            total_bytes: 0,
        }
    }

    fn encoded_size(proposal: &Proposal, block: &BlockV2) -> Option<usize> {
        let proposal_size = bincode::serialized_size(proposal).ok()?;
        let block_size = bincode::serialized_size(block).ok()?;
        proposal_size
            .checked_add(block_size)
            .and_then(|size| size.checked_add(std::mem::size_of::<Hash>() as u64))
            .and_then(|size| usize::try_from(size).ok())
    }

    fn remove_key(&mut self, key: &(u64, BlockId)) -> bool {
        if let Some(entry) = self.entries.remove(key) {
            self.pending_finality.remove(key);
            self.total_bytes = self.total_bytes.saturating_sub(entry.encoded_bytes);
            true
        } else {
            false
        }
    }

    fn prune_expired(&mut self, now: Instant) {
        let ttl = self.limits.ttl;
        let expired = self
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                (entry.retention == CandidateRetention::Normal
                    && now.saturating_duration_since(entry.inserted_at) >= ttl)
                    .then_some(*key)
            })
            .collect::<Vec<_>>();
        for key in expired {
            self.remove_key(&key);
        }
    }

    fn oldest_key(
        &self,
        predicate: impl Fn(&(u64, BlockId), &V2CandidateEntry) -> bool,
    ) -> Option<(u64, BlockId)> {
        self.entries
            .iter()
            .filter(|(key, entry)| {
                entry.retention == CandidateRetention::Normal && predicate(key, entry)
            })
            .min_by(|(left_key, left), (right_key, right)| {
                left.inserted_at
                    .cmp(&right.inserted_at)
                    .then_with(|| left_key.0.cmp(&right_key.0))
                    .then_with(|| left_key.1 .0 .0.cmp(&right_key.1 .0 .0))
            })
            .map(|(key, _)| *key)
    }

    fn count_at_height(&self, height: u64) -> usize {
        self.entries
            .keys()
            .filter(|(entry_height, _)| *entry_height == height)
            .count()
    }

    fn count_for_proposer(&self, height: u64, proposer: ValidatorId) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.proposal.height == height && entry.proposal.proposer == proposer)
            .count()
    }

    /// Admit a candidate after enforcing future windows, TTL, per-height,
    /// per-proposer and total-byte limits. Existing identical entries are
    /// idempotent; conflicting content for one block ID is rejected.
    pub fn insert(
        &mut self,
        proposal: Proposal,
        block: BlockV2,
        derived_randomness: Hash,
        current_height: u64,
        current_round: u32,
    ) -> bool {
        let now = Instant::now();
        self.prune_expired(now);
        let key = (proposal.height, proposal.block_id);
        if let Some(existing) = self.entries.get(&key) {
            return existing.proposal == proposal
                && existing.block == block
                && existing.derived_randomness == derived_randomness;
        }
        if proposal.height > current_height.saturating_add(self.limits.max_future_height)
            || proposal.round > current_round.saturating_add(self.limits.max_future_round)
        {
            return false;
        }
        let Some(encoded_bytes) = Self::encoded_size(&proposal, &block) else {
            return false;
        };
        if encoded_bytes > self.limits.max_total_bytes {
            return false;
        }

        while self.count_at_height(proposal.height) >= self.limits.max_items_per_height {
            let Some(oldest) = self.oldest_key(|(height, _), _| *height == proposal.height) else {
                break;
            };
            self.remove_key(&oldest);
        }
        while self.count_for_proposer(proposal.height, proposal.proposer)
            >= self.limits.max_items_per_proposer
        {
            let Some(oldest) = self.oldest_key(|(height, _), entry| {
                *height == proposal.height && entry.proposal.proposer == proposal.proposer
            }) else {
                break;
            };
            self.remove_key(&oldest);
        }
        if self.count_at_height(proposal.height) >= self.limits.max_items_per_height
            || self.count_for_proposer(proposal.height, proposal.proposer)
                >= self.limits.max_items_per_proposer
        {
            return false;
        }
        while self.total_bytes.saturating_add(encoded_bytes) > self.limits.max_total_bytes {
            let Some(oldest) = self.oldest_key(|_, _| true) else {
                break;
            };
            self.remove_key(&oldest);
        }
        if self.total_bytes.saturating_add(encoded_bytes) > self.limits.max_total_bytes {
            return false;
        }
        self.total_bytes = self.total_bytes.saturating_add(encoded_bytes);
        self.entries.insert(
            key,
            V2CandidateEntry {
                proposal,
                block,
                derived_randomness,
                inserted_at: now,
                encoded_bytes,
                retention: CandidateRetention::Normal,
            },
        );
        true
    }

    /// Pin a candidate needed by a finalized commit.  A missing candidate is
    /// a safety/liveness fault and must be handled by the caller rather than
    /// silently replacing it with a new block.
    pub fn pin_pending_finality(&mut self, height: u64, block_id: BlockId) -> bool {
        let key = (height, block_id);
        if let Some(entry) = self.entries.get_mut(&key) {
            self.pending_finality.insert(key);
            entry.retention = CandidateRetention::PendingFinalityPinned;
            true
        } else {
            false
        }
    }

    pub fn unpin_pending_finality(&mut self, height: u64, block_id: BlockId) {
        self.pending_finality.remove(&(height, block_id));
        if let Some(entry) = self.entries.get_mut(&(height, block_id)) {
            entry.retention = CandidateRetention::Normal;
        }
    }

    /// Reconcile cache retention with the durable live consensus state.  The
    /// required valid/locked candidates must already be present; returning
    /// false makes the caller fail-stop instead of allowing a later
    /// valid-round re-proposal to fail after the dependency was evicted.
    pub fn reconcile_state(
        &mut self,
        height: u64,
        valid_block: Option<BlockId>,
        locked_block: Option<BlockId>,
    ) -> bool {
        let required = [valid_block, locked_block]
            .into_iter()
            .flatten()
            .all(|block_id| self.entries.contains_key(&(height, block_id)));
        for ((entry_height, block_id), entry) in &mut self.entries {
            if *entry_height != height {
                continue;
            }
            entry.retention = if self.pending_finality.contains(&(*entry_height, *block_id)) {
                CandidateRetention::PendingFinalityPinned
            } else if locked_block == Some(*block_id) {
                CandidateRetention::LockedPinned
            } else if valid_block == Some(*block_id) {
                CandidateRetention::ValidRoundPinned
            } else {
                CandidateRetention::Normal
            };
        }
        required
    }

    pub fn get(&mut self, height: u64, block_id: BlockId) -> Option<ValidatedCandidate> {
        self.prune_expired(Instant::now());
        self.entries
            .get(&(height, block_id))
            .map(|entry| ValidatedCandidate {
                proposal: entry.proposal.clone(),
                block: entry.block.clone(),
                derived_randomness: entry.derived_randomness,
            })
    }

    pub fn remove_through_height(&mut self, finalized_height: u64) {
        let keys = self
            .entries
            .keys()
            .filter(|(height, _)| *height <= finalized_height)
            .copied()
            .collect::<Vec<_>>();
        for key in keys {
            self.remove_key(&key);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn retention(&self, height: u64, block_id: BlockId) -> Option<CandidateRetention> {
        self.entries
            .get(&(height, block_id))
            .map(|entry| entry.retention)
    }
}

pub struct PoVFEngine {
    pub state_machine: Arc<RwLock<TendermintStateMachine>>,
    pub candidate_blocks: Arc<RwLock<HashMap<(u64, BlockId), Block>>>,
    pub candidate_cache_v2: Arc<RwLock<V2CandidateCache>>,
    pub finalized_blocks: Arc<RwLock<HashMap<Hash, FinalizedBlock>>>,
    pub finalized_blocks_v2: Arc<RwLock<HashMap<Hash, FinalizedBlockV2>>>,
    pub current_height: Arc<RwLock<u64>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedCandidate {
    pub proposal: Proposal,
    pub block: BlockV2,
    pub derived_randomness: Hash,
}

impl PoVFEngine {
    pub fn new(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        safety_store: Arc<dyn ConsensusSafetyStore>,
        local_validator_id: Option<ValidatorId>,
    ) -> Self {
        Self::new_with_parent_randomness(
            config,
            snapshot,
            Hash::default(),
            safety_store,
            local_validator_id,
        )
    }

    pub fn new_with_parent_randomness(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        parent_randomness: Hash,
        safety_store: Arc<dyn ConsensusSafetyStore>,
        local_validator_id: Option<ValidatorId>,
    ) -> Self {
        Self::new_with_parent_randomness_and_limits(
            config,
            snapshot,
            parent_randomness,
            safety_store,
            local_validator_id,
            ProtocolResourceLimits::default(),
        )
    }

    pub fn new_with_parent_randomness_and_limits(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        parent_randomness: Hash,
        safety_store: Arc<dyn ConsensusSafetyStore>,
        local_validator_id: Option<ValidatorId>,
        resource_limits: ProtocolResourceLimits,
    ) -> Self {
        let state_machine = TendermintStateMachine::new(
            config,
            snapshot,
            parent_randomness,
            safety_store,
            local_validator_id,
        );
        Self {
            state_machine: Arc::new(RwLock::new(state_machine)),
            candidate_blocks: Arc::new(RwLock::new(HashMap::new())),
            candidate_cache_v2: Arc::new(RwLock::new(V2CandidateCache::new(
                V2CandidateCacheLimits::from(&resource_limits),
            ))),
            finalized_blocks: Arc::new(RwLock::new(HashMap::new())),
            finalized_blocks_v2: Arc::new(RwLock::new(HashMap::new())),
            current_height: Arc::new(RwLock::new(1)),
        }
    }

    pub async fn handle_proposal(
        &self,
        proposal: Proposal,
        block: Block,
        signer: &dyn ConsensusSigner,
    ) -> Result<Option<SignedVote>> {
        let mut sm = self.state_machine.write().await;
        let vote = sm
            .handle_proposal(&proposal, &block, signer)
            .map_err(|e| NornError::ConsensusError(e.to_string()))?;

        let calculated_bid = BlockId(block.header.block_hash);
        {
            let mut candidates = self.candidate_blocks.write().await;
            if candidates.len() >= 32 {
                candidates.retain(|(h, _), _| *h >= proposal.height);
            }
            candidates.insert((proposal.height, calculated_bid), block.clone());
        }

        Ok(Some(vote))
    }

    pub async fn handle_proposal_v2(
        &self,
        proposal: Proposal,
        block: BlockV2,
        signer: &dyn ConsensusSigner,
        state_manager: &AccountStateManager,
        limits: &ProtocolResourceLimits,
        context: &ChainContext,
        code_storage: &Arc<CodeStorage>,
    ) -> Result<Option<SignedVote>> {
        if block.header.block_hash != block.header.calculate_hash()? {
            return Err(NornError::ConsensusError(
                "V2 proposal header hash mismatch".into(),
            ));
        }
        if block.header.merkle_root != BlockV2::calculate_merkle_root(&block.transactions)? {
            return Err(NornError::ConsensusError(
                "V2 proposal Merkle root mismatch".into(),
            ));
        }
        block.validate_structure(context, limits)?;
        let evm_context = V2ExecutionContext {
            block_number: block.header.height.max(0) as u64,
            block_timestamp: block.header.timestamp.max(0) as u64,
            block_coinbase: norn_common::types::Address(
                block.header.proposer.0[..20]
                    .try_into()
                    .map_err(|_| NornError::ConsensusError("invalid proposer ID".into()))?,
            ),
            block_gas_limit: limits.max_block_gas,
            code_storage: code_storage.clone(),
        };
        let execution = execute_v2_block(
            state_manager,
            &block.transactions,
            limits,
            Some(&evm_context),
        )
        .await
        .map_err(|e| NornError::ConsensusError(format!("V2 execution rejected: {e}")))?;
        let projected_state_root = execution
            .overlay
            .projected_state_root(state_manager)
            .await
            .map_err(|e| NornError::ConsensusError(format!("V2 state projection failed: {e}")))?;
        if projected_state_root != block.header.state_root {
            return Err(NornError::ConsensusError(
                "V2 proposal state_root does not match deterministic execution".into(),
            ));
        }
        if calculate_v2_execution_data_hash(&execution.results) != block.header.consensus_data_hash
        {
            return Err(NornError::ConsensusError(
                "V2 proposal execution commitment does not match deterministic execution".into(),
            ));
        }
        let mut sm = self.state_machine.write().await;
        let (vote, verified_vrf) = sm
            .handle_proposal_v2_with_vrf(&proposal, &block, signer)
            .map_err(|e| NornError::ConsensusError(e.to_string()))?;
        // `handle_proposal_v2_with_vrf` creates the local Prevote through the
        // safety store. Only after that durable acknowledgement may the live
        // state enter the PrevoteWait phase.
        sm.step = crate::consensus::types::ConsensusStep::PrevoteWait;

        let current_height = sm.height;
        let current_round = sm.round;
        let derived_randomness = verified_vrf
            .map(|output| Hash(output.randomness))
            .ok_or_else(|| {
                NornError::ConsensusError("V2 proposal is missing derived randomness".into())
            })?;
        drop(sm);
        if !self.candidate_cache_v2.write().await.insert(
            proposal,
            block,
            derived_randomness,
            current_height,
            current_round,
        ) {
            return Err(NornError::ConsensusError(
                "V2 candidate exceeds bounded cache policy".into(),
            ));
        }
        Ok(Some(vote))
    }

    /// Apply a proposal that has already passed the pure V2 validation worker.
    /// This method performs only the consensus state transition and durable
    /// local vote creation; callers must not use it for unvalidated network
    /// payloads.
    pub async fn apply_validated_proposal_v2(
        &self,
        candidate: ValidatedCandidate,
        signer: &dyn ConsensusSigner,
    ) -> Result<Option<SignedVote>> {
        let ValidatedCandidate {
            proposal,
            block,
            derived_randomness,
        } = candidate;
        let (current_height, current_round) = {
            let sm = self.state_machine.read().await;
            (sm.height, sm.round)
        };
        // A validation result can outlive the driver context that created it
        // (for example, a commit may advance the state machine while a
        // proposal worker is still completing).  Never feed such a stale
        // proposal back into the state machine; it is safe to discard because
        // finalized state and the current round are authoritative.
        if proposal.height != current_height || proposal.round != current_round {
            return Ok(None);
        }
        if !self.candidate_cache_v2.write().await.insert(
            proposal.clone(),
            block.clone(),
            derived_randomness,
            current_height,
            current_round,
        ) {
            warn!(
                height = proposal.height,
                round = proposal.round,
                "dropping validated V2 proposal because the candidate cache is full or outside the future window"
            );
            return Ok(None);
        }
        let mut sm = self.state_machine.write().await;
        // A proposal can arrive after this validator has already cast its
        // timeout NIL prevote.  It remains useful as a candidate for a later
        // certificate, but must not trigger a second prevote in the same
        // height/round.  The safety WAL rejects that equivocation; ignoring
        // the late vote here also avoids turning ordinary network delay into
        // a consensus-driver error.
        let vote = if matches!(
            sm.step,
            crate::consensus::types::ConsensusStep::NewHeight
                | crate::consensus::types::ConsensusStep::Propose
        ) {
            let (vote, _) = sm
                .handle_proposal_v2_with_vrf(&proposal, &block, signer)
                .map_err(|e| NornError::ConsensusError(e.to_string()))?;
            // The local prevote has been durably acknowledged by the
            // safety store before this transition is made.
            sm.step = crate::consensus::types::ConsensusStep::PrevoteWait;
            Some(vote)
        } else {
            None
        };
        drop(sm);

        Ok(vote)
    }

    /// Validate a V2 proposal without requiring a local signer and without
    /// changing consensus state. FullNodes use this exact validator; a
    /// validator subsequently calls the voting path after this succeeds.
    pub async fn verify_proposal_v2(
        &self,
        proposal: Proposal,
        block: BlockV2,
        state_manager: &AccountStateManager,
        limits: &ProtocolResourceLimits,
        context: &ChainContext,
        code_storage: &Arc<CodeStorage>,
    ) -> Result<ValidatedCandidate> {
        if block.header.block_hash != block.header.calculate_hash()? {
            return Err(NornError::ConsensusError(
                "V2 proposal header hash mismatch".into(),
            ));
        }
        if block.header.merkle_root != BlockV2::calculate_merkle_root(&block.transactions)? {
            return Err(NornError::ConsensusError(
                "V2 proposal Merkle root mismatch".into(),
            ));
        }
        block.validate_structure(context, limits)?;
        let evm_context = V2ExecutionContext {
            block_number: block.header.height.max(0) as u64,
            block_timestamp: block.header.timestamp.max(0) as u64,
            block_coinbase: norn_common::types::Address(
                block.header.proposer.0[..20]
                    .try_into()
                    .map_err(|_| NornError::ConsensusError("invalid proposer ID".into()))?,
            ),
            block_gas_limit: limits.max_block_gas,
            code_storage: code_storage.clone(),
        };
        let execution = execute_v2_block(
            state_manager,
            &block.transactions,
            limits,
            Some(&evm_context),
        )
        .await
        .map_err(|e| NornError::ConsensusError(format!("V2 execution rejected: {e}")))?;
        let projected_state_root = execution
            .overlay
            .projected_state_root(state_manager)
            .await
            .map_err(|e| NornError::ConsensusError(format!("V2 state projection failed: {e}")))?;
        if projected_state_root != block.header.state_root {
            return Err(NornError::ConsensusError(
                "V2 proposal state_root does not match deterministic execution".into(),
            ));
        }
        if calculate_v2_execution_data_hash(&execution.results) != block.header.consensus_data_hash
        {
            return Err(NornError::ConsensusError(
                "V2 proposal execution commitment does not match deterministic execution".into(),
            ));
        }
        let verified_vrf = self
            .state_machine
            .read()
            .await
            .validate_proposal_v2_without_vote(&proposal, &block)
            .map_err(|error| NornError::ConsensusError(error.to_string()))?;
        Ok(ValidatedCandidate {
            proposal,
            block,
            derived_randomness: Hash(verified_vrf.randomness),
        })
    }

    pub async fn remember_validated_candidate(&self, candidate: &ValidatedCandidate) -> bool {
        let (current_height, current_round) = {
            let sm = self.state_machine.read().await;
            (sm.height, sm.round)
        };
        self.candidate_cache_v2.write().await.insert(
            candidate.proposal.clone(),
            candidate.block.clone(),
            candidate.derived_randomness,
            current_height,
            current_round,
        )
    }

    /// Reconcile valid-round and lock dependencies before any cache access
    /// can evict ordinary candidates. A missing required candidate is
    /// reported so the caller can fail-stop rather than continue with a
    /// state-machine reference that cannot be re-proposed or finalized.
    pub async fn reconcile_v2_candidate_retention(&self) -> bool {
        let (height, valid_block, locked_block) = {
            let sm = self.state_machine.read().await;
            (sm.height, sm.valid_block, sm.locked_block)
        };
        self.candidate_cache_v2
            .write()
            .await
            .reconcile_state(height, valid_block, locked_block)
    }

    pub async fn pin_v2_candidate_for_finality(&self, height: u64, block_id: BlockId) -> bool {
        self.candidate_cache_v2
            .write()
            .await
            .pin_pending_finality(height, block_id)
    }

    pub async fn unpin_v2_candidate_for_finality(&self, height: u64, block_id: BlockId) -> bool {
        self.candidate_cache_v2
            .write()
            .await
            .unpin_pending_finality(height, block_id);
        self.reconcile_v2_candidate_retention().await
    }

    pub async fn has_v2_candidate(&self, height: u64, block_id: BlockId) -> bool {
        self.candidate_cache_v2
            .write()
            .await
            .get(height, block_id)
            .is_some()
    }

    /// Return a validated proposal/block pair for a peer that missed the
    /// original proposal. Only candidates that passed the pure V2 validator
    /// are exposed; callers never serve an unvalidated in-memory block.
    pub async fn get_validated_candidate(
        &self,
        height: u64,
        block_id: BlockId,
    ) -> Option<(Proposal, BlockV2)> {
        self.candidate_cache_v2
            .write()
            .await
            .get(height, block_id)
            .map(|candidate| (candidate.proposal, candidate.block))
    }

    /// Re-execute a finalized candidate against the current finalized parent
    /// without mutating live state. The returned overlay is the exact write
    /// set that the finality driver persists and applies after the DB batch is
    /// durable.
    pub async fn execute_v2_block_for_finality(
        &self,
        block: &BlockV2,
        state_manager: &AccountStateManager,
        limits: &ProtocolResourceLimits,
        context: &ChainContext,
        code_storage: &Arc<CodeStorage>,
    ) -> Result<V2BlockExecution> {
        if block.header.block_hash != block.header.calculate_hash()? {
            return Err(NornError::ConsensusError(
                "V2 finality header hash mismatch".into(),
            ));
        }
        if block.header.merkle_root != BlockV2::calculate_merkle_root(&block.transactions)? {
            return Err(NornError::ConsensusError(
                "V2 finality Merkle root mismatch".into(),
            ));
        }
        block.validate_structure(context, limits)?;
        let evm_context = V2ExecutionContext {
            block_number: block.header.height.max(0) as u64,
            block_timestamp: block.header.timestamp.max(0) as u64,
            block_coinbase: norn_common::types::Address(
                block.header.proposer.0[..20]
                    .try_into()
                    .map_err(|_| NornError::ConsensusError("invalid proposer ID".into()))?,
            ),
            block_gas_limit: limits.max_block_gas,
            code_storage: code_storage.clone(),
        };
        let execution = execute_v2_block(
            state_manager,
            &block.transactions,
            limits,
            Some(&evm_context),
        )
        .await
        .map_err(|e| NornError::ConsensusError(format!("V2 finality execution rejected: {e}")))?;
        let projected_state_root = execution
            .overlay
            .projected_state_root(state_manager)
            .await
            .map_err(|e| {
                NornError::ConsensusError(format!("V2 finality state projection failed: {e}"))
            })?;
        if projected_state_root != block.header.state_root
            || calculate_v2_execution_data_hash(&execution.results)
                != block.header.consensus_data_hash
        {
            return Err(NornError::ConsensusError(
                "V2 finality execution commitment mismatch".into(),
            ));
        }
        Ok(execution)
    }

    pub async fn handle_vote(
        &self,
        vote: SignedVote,
        signer: &dyn ConsensusSigner,
    ) -> Result<(Option<SignedVote>, Option<CommitCertificate>)> {
        let mut sm = self.state_machine.write().await;
        let res = sm
            .handle_vote(vote, signer)
            .map_err(|e| NornError::ConsensusError(e.to_string()))?;

        if let (_, Some(ref cert)) = res {
            info!(
                "BFT Consensus reached CommitCertificate for height {} round {}",
                cert.height, cert.round
            );
        }

        Ok(res)
    }

    /// Verify a network vote without requiring a local signer or mutating the
    /// live vote pool. This is the FullNode verify-only path.
    pub async fn verify_vote(&self, vote: &SignedVote) -> Result<()> {
        let sm = self.state_machine.read().await;
        if vote.protocol_version != sm.config.protocol_version
            || vote.chain_id != sm.config.chain_id
            || vote.epoch
                != sm
                    .config
                    .epoch_for_height(vote.height)
                    .map_err(|error| NornError::ConsensusError(error.to_string()))?
        {
            return Err(NornError::ConsensusError(
                "vote context does not match the active chain".into(),
            ));
        }
        let mut pool = VotePool::new();
        match pool.add_vote(vote.clone(), &sm.snapshot) {
            AddVoteResult::Added => Ok(()),
            other => Err(NornError::ConsensusError(format!(
                "vote verification failed: {other:?}"
            ))),
        }
    }

    /// Verify a CommitCertificate before applying block
    pub fn verify_commit_certificate(
        &self,
        block: &Block,
        cert: &CommitCertificate,
        snapshot: &StakeSnapshot,
    ) -> Result<()> {
        self.verify_commit_certificate_header(&block.header, cert, snapshot)
    }

    pub fn verify_commit_certificate_v2(
        &self,
        block: &BlockV2,
        cert: &CommitCertificate,
        snapshot: &StakeSnapshot,
    ) -> Result<()> {
        self.verify_commit_certificate_header(&block.header, cert, snapshot)
    }

    fn verify_commit_certificate_header(
        &self,
        header: &BlockHeader,
        cert: &CommitCertificate,
        snapshot: &StakeSnapshot,
    ) -> Result<()> {
        let calculated_bid = BlockId(header.block_hash);
        if cert.block_id != calculated_bid {
            return Err(NornError::ConsensusError(
                "CommitCertificate block_id mismatch".into(),
            ));
        }
        if cert.height != header.height as u64 {
            return Err(NornError::ConsensusError(
                "CommitCertificate height mismatch".into(),
            ));
        }
        if cert.stake_snapshot_hash != snapshot.snapshot_hash {
            return Err(NornError::ConsensusError(
                "CommitCertificate snapshot hash mismatch".into(),
            ));
        }

        let total_power = snapshot.total_voting_power()?;
        if total_power == 0 {
            return Err(NornError::ConsensusError(
                "Empty voting power in snapshot".into(),
            ));
        }
        if cert.precommits.len() > snapshot.validators.len() {
            return Err(NornError::ConsensusError(
                "CommitCertificate has more votes than the stake snapshot".into(),
            ));
        }

        let mut accumulated_power: u128 = 0;
        let mut seen_validators = std::collections::HashSet::new();

        for precommit in &cert.precommits {
            if precommit.step != VoteStep::Precommit {
                return Err(NornError::ConsensusError(
                    "Non-precommit vote in CommitCertificate".into(),
                ));
            }
            if precommit.block_id != Some(cert.block_id) {
                return Err(NornError::ConsensusError(
                    "Precommit block_id mismatch in CommitCertificate".into(),
                ));
            }
            if precommit.height != cert.height || precommit.round != cert.round {
                return Err(NornError::ConsensusError(
                    "Precommit height/round mismatch in CommitCertificate".into(),
                ));
            }

            if !seen_validators.insert(precommit.validator) {
                return Err(NornError::ConsensusError(
                    "Duplicate validator precommit in CommitCertificate".into(),
                ));
            }

            let record = snapshot
                .validators
                .get(&precommit.validator)
                .ok_or_else(|| {
                    NornError::ConsensusError(
                        "Unknown validator precommit in CommitCertificate".into(),
                    )
                })?;

            if record.consensus_public_key.0 == [0u8; 33] || precommit.signature == [0u8; 64] {
                return Err(NornError::ConsensusError(
                    "Zero key or zero signature in CommitCertificate".into(),
                ));
            }

            let verifying_key = VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0)
                .map_err(|_| {
                    NornError::ConsensusError(
                        "Malformed SEC1 public key in CommitCertificate".into(),
                    )
                })?;
            let sig = Signature::from_slice(&precommit.signature).map_err(|_| {
                NornError::ConsensusError(
                    "Malformed precommit signature in CommitCertificate".into(),
                )
            })?;

            if sig.normalize_s().is_some() {
                return Err(NornError::ConsensusError(
                    "Non-canonical high-S signature in CommitCertificate".into(),
                ));
            }

            let msg_bytes = precommit.canonical_bytes();
            verifying_key.verify(&msg_bytes, &sig).map_err(|_| {
                NornError::ConsensusError("Invalid precommit signature in CommitCertificate".into())
            })?;

            accumulated_power = accumulated_power
                .checked_add(record.voting_power as u128)
                .ok_or_else(|| {
                    NornError::ConsensusError("Voting power overflow in CommitCertificate".into())
                })?;
        }

        let has_quorum = accumulated_power
            .checked_mul(3)
            .zip(total_power.checked_mul(2))
            .map(|(lhs, rhs)| lhs > rhs)
            .unwrap_or(false);

        if !has_quorum {
            return Err(NornError::ConsensusError(
                "CommitCertificate fails > 2/3 voting power quorum".into(),
            ));
        }

        Ok(())
    }

    pub async fn finalize_block(&self, commit: CommitCertificate) -> Result<FinalizedBlock> {
        let block = {
            let candidates = self.candidate_blocks.read().await;
            candidates
                .get(&(commit.height, commit.block_id))
                .cloned()
                .ok_or_else(|| {
                    NornError::ConsensusError(format!(
                        "Candidate block missing for height {} block {:?}",
                        commit.height, commit.block_id
                    ))
                })?
        };

        let snapshot = { self.state_machine.read().await.snapshot.clone() };

        self.verify_commit_certificate(&block, &commit, &snapshot)?;

        let hash = block.header.block_hash;
        info!(
            "Finalizing block {:?} at height {}",
            hash, block.header.height
        );

        let finalized = FinalizedBlock {
            block: block.clone(),
            commit,
        };

        {
            let mut fb = self.finalized_blocks.write().await;
            fb.insert(hash, finalized.clone());
        }

        {
            let mut h = self.current_height.write().await;
            *h = *h + 1;
        }

        {
            let mut candidates = self.candidate_blocks.write().await;
            candidates.retain(|(h, _), _| *h >= block.header.height as u64);
        }

        Ok(finalized)
    }

    /// Finalize a V2 candidate after the commit certificate is verified. This
    /// only records the finalized payload; applying its overlay to durable
    /// state belongs to the atomic finality/storage stage.
    pub async fn finalize_block_v2(&self, commit: CommitCertificate) -> Result<FinalizedBlockV2> {
        let finalized_hash = commit.block_id.0;
        if let Some(existing) = self.finalized_blocks_v2.read().await.get(&finalized_hash) {
            if existing.commit == commit {
                return Ok(existing.clone());
            }
            return Err(NornError::ConsensusError(
                "V2 block was already finalized with a different certificate".into(),
            ));
        }

        let candidate = self
            .candidate_cache_v2
            .write()
            .await
            .get(commit.height, commit.block_id)
            .ok_or_else(|| {
                NornError::ConsensusError(format!(
                    "V2 candidate missing for height {} block {:?}",
                    commit.height, commit.block_id
                ))
            })?;
        let ValidatedCandidate {
            proposal,
            block,
            derived_randomness: next_randomness,
        } = candidate;

        let (snapshot, pending_validator_changes) = {
            let sm = self.state_machine.read().await;
            (sm.snapshot.clone(), sm.pending_validator_changes.clone())
        };
        let max_certificate_members = self
            .state_machine
            .read()
            .await
            .config
            .max_certificate_members as usize;
        if commit.precommits.len() > max_certificate_members {
            return Err(NornError::ConsensusError(
                "CommitCertificate exceeds Genesis certificate member limit".into(),
            ));
        }
        self.verify_commit_certificate_v2(&block, &commit, &snapshot)?;

        let mut consensus_state =
            FinalizedConsensusState::from_v2(&block, &commit, next_randomness)?;
        consensus_state.pending_validator_changes = pending_validator_changes;

        let finalized = FinalizedBlockV2 {
            proposal,
            block,
            commit,
            consensus_state,
        };
        Ok(finalized)
    }

    /// Publish a verified finalized payload to in-memory caches only after its
    /// canonical state/block/finality batch has been flushed durably.
    pub async fn record_finalized_v2_after_durable(
        &self,
        finalized: &FinalizedBlockV2,
    ) -> Result<()> {
        let hash = finalized.block.header.block_hash;
        {
            let mut finalized_blocks = self.finalized_blocks_v2.write().await;
            if let Some(existing) = finalized_blocks.get(&hash) {
                if existing.commit != finalized.commit {
                    return Err(NornError::ConsensusError(
                        "V2 block was already finalized with a different certificate".into(),
                    ));
                }
            } else {
                finalized_blocks.insert(hash, finalized.clone());
            }
        }
        self.candidate_cache_v2
            .write()
            .await
            .remove_through_height(finalized.block.header.height as u64);
        Ok(())
    }

    /// Apply the finalized consensus transition after the finality/storage
    /// driver has durably committed the block.  This is intentionally
    /// separate from certificate verification so a failed durable commit
    /// cannot advance the in-memory height or randomness.
    pub async fn advance_after_finalized_v2(
        &self,
        finalized: &FinalizedBlockV2,
        next_snapshot: StakeSnapshot,
    ) -> Result<()> {
        let mut sm = self.state_machine.write().await;
        if sm.height == finalized.consensus_state.height.saturating_add(1) {
            if sm.parent_randomness != finalized.consensus_state.next_randomness
                || sm.snapshot.snapshot_hash != next_snapshot.snapshot_hash
            {
                return Err(NornError::ConsensusError(
                    "in-memory consensus state conflicts with durable finalized state".into(),
                ));
            }
            *self.current_height.write().await = sm.height;
            return Ok(());
        }
        sm.start_new_height_from_finalized(&finalized.consensus_state, next_snapshot)
            .map_err(|e| NornError::ConsensusError(e.to_string()))?;
        *self.current_height.write().await = sm.height;
        Ok(())
    }

    pub async fn get_current_height(&self) -> u64 {
        *self.current_height.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::safety_store::MemorySafetyStore;

    fn cache_proposal(height: u64, round: u32, block_id: u8, proposer: u8) -> Proposal {
        Proposal {
            protocol_version: Default::default(),
            chain_id: Default::default(),
            epoch: 1,
            height,
            round,
            valid_round: None,
            valid_round_certificate: None,
            block_id: BlockId(Hash([block_id; 32])),
            parent_block_hash: Hash([9; 32]),
            stake_snapshot_hash: Default::default(),
            proposer: ValidatorId([proposer; 32]),
            vrf_preout: [1; 32],
            vrf_proof: [2; 64],
            signature: [3; 64],
        }
    }

    #[test]
    fn v2_candidate_cache_enforces_context_and_shared_bounds() {
        let mut cache = V2CandidateCache::new(V2CandidateCacheLimits {
            max_total_bytes: 1_000_000,
            max_items_per_height: 2,
            max_items_per_proposer: 1,
            max_future_height: 0,
            max_future_round: 0,
            ttl: Duration::from_secs(60),
        });
        let first = cache_proposal(1, 0, 1, 1);
        let second = cache_proposal(1, 0, 2, 2);
        let replacement = cache_proposal(1, 0, 3, 1);
        assert!(cache.insert(first.clone(), BlockV2::default(), Hash([4; 32]), 1, 0));
        assert!(cache.insert(second, BlockV2::default(), Hash([5; 32]), 1, 0));
        assert!(cache.insert(replacement, BlockV2::default(), Hash([6; 32]), 1, 0));
        assert_eq!(cache.len(), 2);
        assert!(cache.total_bytes() <= 1_000_000);
        assert!(cache.get(1, first.block_id).is_none());
        assert!(!cache.insert(
            cache_proposal(2, 0, 4, 3),
            BlockV2::default(),
            Hash([7; 32]),
            1,
            0
        ));
        assert!(!cache.insert(
            cache_proposal(1, 1, 5, 3),
            BlockV2::default(),
            Hash([8; 32]),
            1,
            0
        ));
    }

    #[test]
    fn v2_candidate_cache_expires_entries_and_cleans_finalized_heights() {
        let mut cache = V2CandidateCache::new(V2CandidateCacheLimits {
            max_total_bytes: 1_000_000,
            max_items_per_height: 4,
            max_items_per_proposer: 4,
            max_future_height: 2,
            max_future_round: 2,
            ttl: Duration::ZERO,
        });
        let proposal = cache_proposal(1, 0, 1, 1);
        assert!(cache.insert(proposal.clone(), BlockV2::default(), Hash([4; 32]), 1, 0));
        assert!(cache.get(1, proposal.block_id).is_none());

        let mut cache = V2CandidateCache::new(V2CandidateCacheLimits {
            max_total_bytes: 1_000_000,
            max_items_per_height: 4,
            max_items_per_proposer: 4,
            max_future_height: 2,
            max_future_round: 2,
            ttl: Duration::from_secs(60),
        });
        let proposal = cache_proposal(3, 0, 2, 1);
        assert!(cache.insert(proposal.clone(), BlockV2::default(), Hash([5; 32]), 1, 0));
        assert!(cache.get(3, proposal.block_id).is_some());
        cache.remove_through_height(3);
        assert!(cache.get(3, proposal.block_id).is_none());
    }

    #[test]
    fn pinned_v2_candidates_survive_ttl_and_capacity_pressure() {
        let mut cache = V2CandidateCache::new(V2CandidateCacheLimits {
            max_total_bytes: 1_000_000,
            max_items_per_height: 1,
            max_items_per_proposer: 1,
            max_future_height: 1,
            max_future_round: 1,
            ttl: Duration::ZERO,
        });
        let pinned = cache_proposal(1, 0, 1, 1);
        assert!(cache.insert(pinned.clone(), BlockV2::default(), Hash([4; 32]), 1, 0));
        assert!(cache.pin_pending_finality(1, pinned.block_id));
        assert_eq!(
            cache.retention(1, pinned.block_id),
            Some(CandidateRetention::PendingFinalityPinned)
        );
        assert!(cache.get(1, pinned.block_id).is_some());

        assert!(!cache.insert(
            cache_proposal(1, 0, 2, 2),
            BlockV2::default(),
            Hash([5; 32]),
            1,
            0
        ));
        assert!(cache.get(1, pinned.block_id).is_some());

        cache.unpin_pending_finality(1, pinned.block_id);
        assert!(cache.get(1, pinned.block_id).is_none());
    }

    #[tokio::test]
    async fn test_povf_engine_creation() {
        let config = ConsensusConfig::default();
        let snapshot = StakeSnapshot::default();
        let safety_store = Arc::new(MemorySafetyStore::new());

        let engine = PoVFEngine::new(config, snapshot, safety_store, None);
        assert_eq!(engine.get_current_height().await, 1);
    }
}
