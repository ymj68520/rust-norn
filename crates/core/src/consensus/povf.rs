//! PoVF Orchestration Engine wrapping Tendermint BFT State Machine

use super::safety_store::{ConsensusSafetyStore, ConsensusSigner};
use super::state_machine::TendermintStateMachine;
use super::types::ConsensusConfig;
use norn_common::consensus_types::{
    CommitCertificate, FinalizedBlock, Proposal, SignedVote, StakeSnapshot, VoteStep,
};
use norn_common::error::{NornError, Result};
use norn_common::types::{Block, BlockId, Hash, ValidatorId};
use k256::ecdsa::{VerifyingKey, Signature, signature::Verifier};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

pub struct PoVFEngine {
    pub state_machine: Arc<RwLock<TendermintStateMachine>>,
    pub candidate_blocks: Arc<RwLock<HashMap<(u64, BlockId), Block>>>,
    pub finalized_blocks: Arc<RwLock<HashMap<Hash, FinalizedBlock>>>,
    pub current_height: Arc<RwLock<u64>>,
}

impl PoVFEngine {
    pub fn new(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        safety_store: Arc<dyn ConsensusSafetyStore>,
        local_validator_id: Option<ValidatorId>,
    ) -> Self {
        let state_machine = TendermintStateMachine::new(config, snapshot, safety_store, local_validator_id);
        Self {
            state_machine: Arc::new(RwLock::new(state_machine)),
            candidate_blocks: Arc::new(RwLock::new(HashMap::new())),
            finalized_blocks: Arc::new(RwLock::new(HashMap::new())),
            current_height: Arc::new(RwLock::new(1)),
        }
    }

    pub async fn handle_proposal(
        &self,
        proposal: Proposal,
        block: Block,
        signer: &dyn ConsensusSigner,
    ) -> Result<Option<SignedVote>> {
        let calculated_bid = BlockId(block.header.block_hash);
        {
            let mut candidates = self.candidate_blocks.write().await;
            candidates.insert((proposal.height, calculated_bid), block.clone());
        }

        let mut sm = self.state_machine.write().await;
        let vote = sm
            .handle_proposal(&proposal, &block, signer)
            .map_err(|e| NornError::ConsensusError(e.to_string()))?;
        Ok(Some(vote))
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

    /// Verify a CommitCertificate before applying block
    pub fn verify_commit_certificate(&self, block: &Block, cert: &CommitCertificate, snapshot: &StakeSnapshot) -> Result<()> {
        let calculated_bid = BlockId(block.header.block_hash);
        if cert.block_id != calculated_bid {
            return Err(NornError::ConsensusError("CommitCertificate block_id mismatch".into()));
        }
        if cert.height != block.header.height as u64 {
            return Err(NornError::ConsensusError("CommitCertificate height mismatch".into()));
        }
        if cert.stake_snapshot_hash != snapshot.snapshot_hash {
            return Err(NornError::ConsensusError("CommitCertificate snapshot hash mismatch".into()));
        }

        let total_power = snapshot.total_voting_power()?;
        if total_power == 0 {
            return Err(NornError::ConsensusError("Empty voting power in snapshot".into()));
        }

        let mut accumulated_power: u128 = 0;
        let mut seen_validators = std::collections::HashSet::new();

        for precommit in &cert.precommits {
            if precommit.step != VoteStep::Precommit {
                return Err(NornError::ConsensusError("Non-precommit vote in CommitCertificate".into()));
            }
            if precommit.block_id != Some(cert.block_id) {
                return Err(NornError::ConsensusError("Precommit block_id mismatch in CommitCertificate".into()));
            }
            if precommit.height != cert.height || precommit.round != cert.round {
                return Err(NornError::ConsensusError("Precommit height/round mismatch in CommitCertificate".into()));
            }

            if !seen_validators.insert(precommit.validator) {
                return Err(NornError::ConsensusError("Duplicate validator precommit in CommitCertificate".into()));
            }

            let record = snapshot.validators.get(&precommit.validator)
                .ok_or_else(|| NornError::ConsensusError("Unknown validator precommit in CommitCertificate".into()))?;

            if record.consensus_public_key.0 == [0u8; 33] || precommit.signature == [0u8; 64] {
                return Err(NornError::ConsensusError("Zero key or zero signature in CommitCertificate".into()));
            }

            let verifying_key = VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0)
                .map_err(|_| NornError::ConsensusError("Malformed SEC1 public key in CommitCertificate".into()))?;
            let sig = Signature::from_slice(&precommit.signature)
                .map_err(|_| NornError::ConsensusError("Malformed precommit signature in CommitCertificate".into()))?;

            if sig.normalize_s().is_some() {
                return Err(NornError::ConsensusError("Non-canonical high-S signature in CommitCertificate".into()));
            }

            let msg_bytes = precommit.canonical_bytes();
            verifying_key.verify(&msg_bytes, &sig)
                .map_err(|_| NornError::ConsensusError("Invalid precommit signature in CommitCertificate".into()))?;

            accumulated_power = accumulated_power.checked_add(record.voting_power as u128)
                .ok_or_else(|| NornError::ConsensusError("Voting power overflow in CommitCertificate".into()))?;
        }

        let has_quorum = accumulated_power
            .checked_mul(3)
            .zip(total_power.checked_mul(2))
            .map(|(lhs, rhs)| lhs > rhs)
            .unwrap_or(false);

        if !has_quorum {
            return Err(NornError::ConsensusError("CommitCertificate fails > 2/3 voting power quorum".into()));
        }

        Ok(())
    }

    pub async fn finalize_block(&self, commit: CommitCertificate) -> Result<FinalizedBlock> {
        let block = {
            let candidates = self.candidate_blocks.read().await;
            candidates.get(&(commit.height, commit.block_id))
                .cloned()
                .ok_or_else(|| NornError::ConsensusError(format!("Candidate block missing for height {} block {:?}", commit.height, commit.block_id)))?
        };

        let snapshot = {
            let sm = self.state_machine.read().await;
            sm.snapshot.clone()
        };

        self.verify_commit_certificate(&block, &commit, &snapshot)?;

        let hash = block.header.block_hash;
        info!("Finalizing block {:?} at height {}", hash, block.header.height);

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
