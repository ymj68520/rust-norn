//! Tendermint-style BFT State Machine (Propose, Prevote, Precommit, Commit)
//! 
//! Implements strict BFT locking rules (locked_block, locked_round, valid_block, valid_round),
//! NIL votes, timeout escalation, and deterministic single-proposer round-robin scheduling.

use super::safety_store::{ConsensusSafetyStore, ConsensusSigner, VoteSignRequest};
use super::types::{ConsensusConfig, ConsensusStep, ElectionMath};
use super::vote_pool::VotePool;
use anyhow::{anyhow, Result};
use norn_common::consensus_types::{
    BlockEnvelope, CommitCertificate, FinalizedBlock, Proposal, SignedVote, StakeSnapshot,
    VoteStep,
};
use norn_common::types::{Block, BlockId, Hash, ValidatorId};
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct TendermintStateMachine {
    pub config: ConsensusConfig,
    pub height: u64,
    pub round: u32,
    pub step: ConsensusStep,

    // Safety state locks
    pub locked_block: Option<BlockId>,
    pub locked_round: Option<u32>,
    pub valid_block: Option<BlockId>,
    pub valid_round: Option<u32>,

    pub snapshot: StakeSnapshot,
    pub vote_pool: VotePool,
    pub safety_store: Arc<dyn ConsensusSafetyStore>,
    pub local_validator_id: Option<ValidatorId>,
}

impl TendermintStateMachine {
    pub fn new(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        safety_store: Arc<dyn ConsensusSafetyStore>,
        local_validator_id: Option<ValidatorId>,
    ) -> Self {
        Self {
            config,
            height: 1,
            round: 0,
            step: ConsensusStep::NewHeight,
            locked_block: None,
            locked_round: None,
            valid_block: None,
            valid_round: None,
            snapshot,
            vote_pool: VotePool::new(),
            safety_store,
            local_validator_id,
        }
    }

    /// Advance to next height
    pub fn start_new_height(&mut self, height: u64, snapshot: StakeSnapshot) {
        self.height = height;
        self.round = 0;
        self.step = ConsensusStep::NewHeight;
        self.locked_block = None;
        self.locked_round = None;
        self.valid_block = None;
        self.valid_round = None;
        self.snapshot = snapshot;
        self.vote_pool.clear_old_heights(height);
        info!("Consensus starting new height {}", height);
        self.start_new_round(0);
    }

    /// Advance to next round
    pub fn start_new_round(&mut self, round: u32) {
        self.round = round;
        self.step = ConsensusStep::NewRound;
        info!("Consensus height {} entering round {}", self.height, round);
        self.step = ConsensusStep::Propose;
    }

    /// Determine the deterministic proposer for the current round
    pub fn get_current_proposer(&self) -> Option<ValidatorId> {
        ElectionMath::select_deterministic_proposer(&self.snapshot, self.round)
    }

    /// Check if local node is the current round's proposer
    pub fn is_local_proposer(&self) -> bool {
        if let (Some(local_id), Some(proposer)) = (self.local_validator_id, self.get_current_proposer()) {
            local_id == proposer
        } else {
            false
        }
    }

    /// Handle incoming proposal and decide whether to Prevote for it or NIL Prevote
    pub fn handle_proposal(
        &mut self,
        proposal: &Proposal,
        block: &Block,
        signer: &dyn ConsensusSigner,
    ) -> Result<SignedVote> {
        if proposal.height != self.height || proposal.round != self.round {
            return Err(anyhow!(
                "Proposal height/round mismatch: expected ({},{}), got ({},{})",
                self.height, self.round, proposal.height, proposal.round
            ));
        }

        // Validate proposer
        let expected_proposer = self
            .get_current_proposer()
            .ok_or_else(|| anyhow!("No proposer available"))?;
        if proposal.proposer != expected_proposer {
            warn!(
                "Proposal from invalid proposer: expected {:?}, got {:?}",
                expected_proposer, proposal.proposer
            );
            return self.cast_vote(VoteStep::Prevote, None, signer);
        }

        // Check Tendermint unlocking rule
        let block_id = BlockId(block.header.block_hash);
        let can_prevote = match (self.locked_block, self.locked_round) {
            (None, _) => true,
            (Some(locked_bid), Some(locked_r)) => {
                if locked_bid == block_id {
                    true
                } else if let Some(valid_r) = proposal.valid_round {
                    valid_r >= locked_r
                } else {
                    false
                }
            }
            _ => false,
        };

        if can_prevote {
            info!("Casting Prevote for block {:?}", block_id);
            self.cast_vote(VoteStep::Prevote, Some(block_id), signer)
        } else {
            warn!("Locked on different block, casting NIL Prevote");
            self.cast_vote(VoteStep::Prevote, None, signer)
        }
    }

    /// Process incoming vote and update BFT state and locking
    pub fn handle_vote(
        &mut self,
        vote: SignedVote,
        signer: &dyn ConsensusSigner,
    ) -> Result<Option<CommitCertificate>> {
        let height = vote.height;
        let round = vote.round;
        let step = vote.step;

        if !self.vote_pool.add_vote(vote) {
            return Ok(None);
        }

        // Check if 2/3 Prevote quorum is reached
        if step == VoteStep::Prevote {
            if let Some(votes) = self.vote_pool.check_quorum(
                height,
                round,
                VoteStep::Prevote,
                self.valid_block,
                &self.snapshot,
            ) {
                if let Some(bid) = self.valid_block {
                    info!("2/3 Prevotes reached for block {:?}, locking block", bid);
                    self.locked_block = Some(bid);
                    self.locked_round = Some(round);
                    self.valid_block = Some(bid);
                    self.valid_round = Some(round);
                    return Ok(None);
                }
            }
        }

        // Check if 2/3 Precommit quorum is reached for finality
        if step == VoteStep::Precommit {
            if let Some(locked_bid) = self.locked_block {
                if let Some(cert) = self.vote_pool.create_commit_certificate(
                    height,
                    round,
                    locked_bid,
                    &self.snapshot,
                ) {
                    info!("2/3 Precommits reached! Finalizing block {:?}", locked_bid);
                    self.step = ConsensusStep::Commit;
                    return Ok(Some(cert));
                }
            }
        }

        Ok(None)
    }

    /// Cast a vote (Prevote or Precommit) via atomic safety store
    pub fn cast_vote(
        &self,
        step: VoteStep,
        block_id: Option<BlockId>,
        signer: &dyn ConsensusSigner,
    ) -> Result<SignedVote> {
        let local_id = self
            .local_validator_id
            .ok_or_else(|| anyhow!("Local node is not a validator"))?;

        let sign_req = VoteSignRequest {
            protocol_version: self.config.protocol_version,
            chain_id: self.config.chain_id,
            epoch: self.config.epoch,
            height: self.height,
            round: self.round,
            step,
            block_id,
            validator_id: local_id,
        };

        let signed_vote = self
            .safety_store
            .sign_vote_once(sign_req, signer)
            .map_err(|e| anyhow!("Safety store signing error: {:?}", e))?;

        Ok(signed_vote)
    }

    /// Proposal timeout trigger -> cast NIL Prevote
    pub fn on_timeout_propose(&mut self, signer: &dyn ConsensusSigner) -> Result<SignedVote> {
        info!("Proposal timeout in round {}, casting NIL Prevote", self.round);
        self.step = ConsensusStep::PrevoteWait;
        self.cast_vote(VoteStep::Prevote, None, signer)
    }

    /// Prevote timeout trigger -> cast NIL Precommit
    pub fn on_timeout_prevote(&mut self, signer: &dyn ConsensusSigner) -> Result<SignedVote> {
        info!("Prevote timeout in round {}, casting NIL Precommit", self.round);
        self.step = ConsensusStep::PrecommitWait;
        self.cast_vote(VoteStep::Precommit, None, signer)
    }

    /// Precommit timeout trigger -> advance to next round
    pub fn on_timeout_precommit(&mut self) {
        info!("Precommit timeout in round {}, advancing to round {}", self.round, self.round + 1);
        self.start_new_round(self.round + 1);
    }
}
