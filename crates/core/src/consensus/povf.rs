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
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub struct PoVFEngine {
    pub state_machine: Arc<RwLock<TendermintStateMachine>>,
    pub candidate_blocks: Arc<RwLock<HashMap<(u64, BlockId), Block>>>,
    pub candidate_blocks_v2: Arc<RwLock<HashMap<(u64, BlockId), BlockV2>>>,
    pub candidate_proposals_v2: Arc<RwLock<HashMap<(u64, BlockId), Proposal>>>,
    pub candidate_randomness_v2: Arc<RwLock<HashMap<(u64, BlockId), Hash>>>,
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
            candidate_blocks_v2: Arc::new(RwLock::new(HashMap::new())),
            candidate_proposals_v2: Arc::new(RwLock::new(HashMap::new())),
            candidate_randomness_v2: Arc::new(RwLock::new(HashMap::new())),
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

        let calculated_bid = BlockId(block.header.block_hash);
        if let Some(randomness) = verified_vrf.map(|output| Hash(output.randomness)) {
            let key = (proposal.height, calculated_bid);
            let mut proposals = self.candidate_proposals_v2.write().await;
            if let Some(existing) = proposals.get(&key) {
                if existing != &proposal {
                    return Err(NornError::ConsensusError(
                        "conflicting V2 proposals for the same block ID".into(),
                    ));
                }
            } else {
                proposals.insert(key, proposal.clone());
            }
            self.candidate_randomness_v2
                .write()
                .await
                .insert(key, randomness);
        }
        {
            let mut candidates = self.candidate_blocks_v2.write().await;
            if candidates.len() >= 32 {
                candidates.retain(|(h, _), _| *h >= proposal.height);
                self.candidate_proposals_v2
                    .write()
                    .await
                    .retain(|(h, _), _| *h >= proposal.height);
                self.candidate_randomness_v2
                    .write()
                    .await
                    .retain(|(h, _), _| *h >= proposal.height);
            }
            candidates.insert((proposal.height, calculated_bid), block);
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
        let mut sm = self.state_machine.write().await;
        // A proposal can arrive after this validator has already cast its
        // timeout NIL prevote.  It remains useful as a candidate for a later
        // certificate, but must not trigger a second prevote in the same
        // height/round.  The safety WAL rejects that equivocation; ignoring
        // the late vote here also avoids turning ordinary network delay into
        // a consensus-driver error.
        let (vote, verified_vrf) = if matches!(
            sm.step,
            crate::consensus::types::ConsensusStep::NewHeight
                | crate::consensus::types::ConsensusStep::Propose
        ) {
            let (vote, verified_vrf) = sm
                .handle_proposal_v2_with_vrf(&proposal, &block, signer)
                .map_err(|e| NornError::ConsensusError(e.to_string()))?;
            // The local prevote has been durably acknowledged by the
            // safety store before this transition is made.
            sm.step = crate::consensus::types::ConsensusStep::PrevoteWait;
            (Some(vote), verified_vrf)
        } else {
            (None, None)
        };
        drop(sm);

        let calculated_bid = BlockId(block.header.block_hash);
        let key = (proposal.height, calculated_bid);
        let randomness = verified_vrf
            .map(|output| Hash(output.randomness))
            .unwrap_or(derived_randomness);
        let mut proposals = self.candidate_proposals_v2.write().await;
        if let Some(existing) = proposals.get(&key) {
            if existing != &proposal {
                return Err(NornError::ConsensusError(
                    "conflicting V2 proposals for the same block ID".into(),
                ));
            }
        } else {
            proposals.insert(key, proposal.clone());
        }
        self.candidate_randomness_v2
            .write()
            .await
            .insert(key, randomness);
        self.candidate_blocks_v2.write().await.insert(key, block);
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

    pub async fn remember_validated_candidate(&self, candidate: &ValidatedCandidate) {
        let key = (candidate.proposal.height, candidate.proposal.block_id);
        self.candidate_proposals_v2
            .write()
            .await
            .insert(key, candidate.proposal.clone());
        self.candidate_randomness_v2
            .write()
            .await
            .insert(key, candidate.derived_randomness);
        self.candidate_blocks_v2
            .write()
            .await
            .insert(key, candidate.block.clone());
    }

    pub async fn has_v2_candidate(&self, height: u64, block_id: BlockId) -> bool {
        self.candidate_blocks_v2
            .read()
            .await
            .contains_key(&(height, block_id))
    }

    /// Return a validated proposal/block pair for a peer that missed the
    /// original proposal. Only candidates that passed the pure V2 validator
    /// are exposed; callers never serve an unvalidated in-memory block.
    pub async fn get_validated_candidate(
        &self,
        height: u64,
        block_id: BlockId,
    ) -> Option<(Proposal, BlockV2)> {
        let key = (height, block_id);
        let proposal = self
            .candidate_proposals_v2
            .read()
            .await
            .get(&key)
            .cloned()?;
        let block = self.candidate_blocks_v2.read().await.get(&key).cloned()?;
        Some((proposal, block))
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

        let block = {
            let candidates = self.candidate_blocks_v2.read().await;
            candidates
                .get(&(commit.height, commit.block_id))
                .cloned()
                .ok_or_else(|| {
                    NornError::ConsensusError(format!(
                        "V2 candidate block missing for height {} block {:?}",
                        commit.height, commit.block_id
                    ))
                })?
        };

        let proposal = self
            .candidate_proposals_v2
            .read()
            .await
            .get(&(commit.height, commit.block_id))
            .cloned()
            .ok_or_else(|| {
                NornError::ConsensusError(
                    "V2 finalized block is missing its verified proposal VRF".into(),
                )
            })?;

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

        let next_randomness = self
            .candidate_randomness_v2
            .read()
            .await
            .get(&(commit.height, commit.block_id))
            .copied()
            .ok_or_else(|| {
                NornError::ConsensusError(
                    "V2 finalized block is missing its derived proposal randomness".into(),
                )
            })?;
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
        self.candidate_blocks_v2
            .write()
            .await
            .retain(|(height, _), _| *height > finalized.block.header.height as u64);
        self.candidate_proposals_v2
            .write()
            .await
            .retain(|(height, _), _| *height > finalized.block.header.height as u64);
        self.candidate_randomness_v2
            .write()
            .await
            .retain(|(height, _), _| *height > finalized.block.header.height as u64);
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

    #[tokio::test]
    async fn test_povf_engine_creation() {
        let config = ConsensusConfig::default();
        let snapshot = StakeSnapshot::default();
        let safety_store = Arc::new(MemorySafetyStore::new());

        let engine = PoVFEngine::new(config, snapshot, safety_store, None);
        assert_eq!(engine.get_current_height().await, 1);
    }
}
