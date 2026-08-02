//! Tendermint-style BFT State Machine (Propose, Prevote, Precommit, Commit)

use super::safety_store::{ConsensusSafetyStore, ConsensusSigner, VoteSignRequest};
use super::types::{ConsensusConfig, ConsensusStep, ElectionMath};
use super::vote_pool::{AddVoteResult, VotePool};
use anyhow::{anyhow, Result};
use norn_common::consensus_types::{
    CommitCertificate, PrevoteCertificate, Proposal, SignedVote, StakeSnapshot, VoteStep,
};
use norn_common::types::{Block, BlockId, ValidatorId};
use norn_crypto::vrf::{VrfContext, VRFCalculator, VRFOutputData, VRFPreOutBytes, VRFProofBytes};
use k256::ecdsa::{VerifyingKey, Signature, signature::Verifier};
use std::sync::Arc;
use tracing::{info, warn};

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
    pub valid_round_certificate: Option<PrevoteCertificate>,

    pub snapshot: StakeSnapshot,
    pub parent_randomness: norn_common::types::Hash,
    pub vote_pool: VotePool,
    pub safety_store: Arc<dyn ConsensusSafetyStore>,
    pub local_validator_id: Option<ValidatorId>,
}

impl TendermintStateMachine {
    pub fn new(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        parent_randomness: norn_common::types::Hash,
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
            valid_round_certificate: None,
            snapshot,
            parent_randomness,
            vote_pool: VotePool::new(),
            safety_store,
            local_validator_id,
        }
    }

    /// Advance to next height
    pub fn start_new_height(&mut self, height: u64, snapshot: StakeSnapshot, parent_randomness: norn_common::types::Hash) {
        self.height = height;
        self.round = 0;
        self.step = ConsensusStep::NewHeight;
        self.locked_block = None;
        self.locked_round = None;
        self.valid_block = None;
        self.valid_round = None;
        self.valid_round_certificate = None;
        self.snapshot = snapshot;
        self.parent_randomness = parent_randomness;
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

    /// Determine the deterministic proposer for current (height, round)
    pub fn get_current_proposer(&self) -> Option<ValidatorId> {
        ElectionMath::select_deterministic_proposer(
            &self.config.chain_id,
            self.config.epoch,
            self.height,
            self.round,
            &self.parent_randomness,
            &self.snapshot,
        )
    }

    /// Check if local node is current round's proposer
    pub fn is_local_proposer(&self) -> bool {
        if let (Some(local_id), Some(proposer)) = (self.local_validator_id, self.get_current_proposer()) {
            local_id == proposer
        } else {
            false
        }
    }

    /// Verify a PrevoteCertificate POL proof
    pub fn verify_prevote_certificate(cert: &PrevoteCertificate, snapshot: &StakeSnapshot) -> Result<()> {
        if cert.stake_snapshot_hash != snapshot.snapshot_hash {
            return Err(anyhow!("PrevoteCertificate snapshot hash mismatch"));
        }

        let mut pool = VotePool::new();
        for vote in &cert.prevotes {
            if vote.step != VoteStep::Prevote {
                return Err(anyhow!("PrevoteCertificate contains non-prevote step"));
            }
            if vote.block_id != Some(cert.block_id) {
                return Err(anyhow!("PrevoteCertificate vote block_id mismatch"));
            }
            if pool.add_vote(vote.clone(), snapshot) != AddVoteResult::Added {
                return Err(anyhow!("PrevoteCertificate contains invalid vote"));
            }
        }

        if pool.check_quorum(
            cert.protocol_version.clone(),
            cert.chain_id.clone(),
            cert.epoch,
            cert.height,
            cert.round,
            VoteStep::Prevote,
            Some(cert.block_id),
            snapshot,
        ).is_none() {
            return Err(anyhow!("PrevoteCertificate failed > 2/3 quorum check"));
        }

        Ok(())
    }

    /// Handle incoming proposal with strict fail-closed signature and VRF verification
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

        if proposal.chain_id != self.config.chain_id || proposal.protocol_version != self.config.protocol_version {
            return Err(anyhow!("Proposal chain_id / protocol_version mismatch"));
        }

        if proposal.stake_snapshot_hash != self.snapshot.snapshot_hash {
            return Err(anyhow!("Proposal stake_snapshot_hash mismatch"));
        }

        let calculated_block_id = BlockId(block.header.block_hash);
        if proposal.block_id != calculated_block_id {
            return Err(anyhow!("Proposal block_id does not match actual block header hash"));
        }

        let expected_proposer = self
            .get_current_proposer()
            .ok_or_else(|| anyhow!("No proposer available"))?;
        if proposal.proposer != expected_proposer {
            warn!("Proposal from invalid proposer: expected {:?}, got {:?}", expected_proposer, proposal.proposer);
            return self.cast_vote(VoteStep::Prevote, None, signer);
        }

        let Some(record) = self.snapshot.validators.get(&proposal.proposer) else {
            warn!("Proposer not found in stake snapshot");
            return self.cast_vote(VoteStep::Prevote, None, signer);
        };

        // 1. Strict fail-closed ECDSA signature verification
        if record.consensus_public_key.0 == [0u8; 33] || proposal.signature == [0u8; 64] {
            warn!("Rejected proposal with zero key or zero signature");
            return self.cast_vote(VoteStep::Prevote, None, signer);
        }

        let verifying_key = VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0)
            .map_err(|_| anyhow!("Malformed SEC1 consensus public key"))?;
        let sig = Signature::from_slice(&proposal.signature)
            .map_err(|_| anyhow!("Malformed proposal signature"))?;

        if sig.normalize_s().is_some() {
            warn!("Rejected non-canonical high-S proposal signature");
            return self.cast_vote(VoteStep::Prevote, None, signer);
        }

        let msg_bytes = proposal.canonical_bytes();
        if verifying_key.verify(&msg_bytes, &sig).is_err() {
            warn!("Proposal ECDSA signature verification failed");
            return self.cast_vote(VoteStep::Prevote, None, signer);
        }

        // 2. Strict VRF verification with VrfContext
        let context = VrfContext {
            protocol_version: self.config.protocol_version.clone(),
            chain_id: self.config.chain_id.clone(),
            epoch: self.config.epoch,
            height: self.height,
            round: self.round,
            parent_block_hash: proposal.parent_block_hash.clone(),
            stake_snapshot_hash: self.snapshot.snapshot_hash.clone(),
            validator_id: proposal.proposer,
        };

        if norn_crypto::vrf::verify_and_derive(
            &record.vrf_public_key.0,
            &context,
            &VRFPreOutBytes(proposal.vrf_preout),
            &VRFProofBytes(proposal.vrf_proof),
        ).is_err() {
            warn!("Proposal VRF proof verification failed");
            return self.cast_vote(VoteStep::Prevote, None, signer);
        }

        // 3. Evaluate Tendermint unlocking rule
        let can_prevote = match (self.locked_block, self.locked_round) {
            (None, _) => true,
            (Some(locked_bid), Some(locked_r)) => {
                if locked_bid == calculated_block_id {
                    true
                } else if let (Some(vr), Some(cert)) = (proposal.valid_round, &proposal.valid_round_certificate) {
                    if vr >= locked_r && vr == cert.round && cert.block_id == calculated_block_id {
                        Self::verify_prevote_certificate(cert, &self.snapshot).is_ok()
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };

        if can_prevote {
            info!("Casting Prevote for block {:?}", calculated_block_id);
            self.cast_vote(VoteStep::Prevote, Some(calculated_block_id), signer)
        } else {
            warn!("Locked on different block, casting NIL Prevote");
            self.cast_vote(VoteStep::Prevote, None, signer)
        }
    }

    /// Process incoming vote and update BFT state and locking upon Polka
    pub fn handle_vote(
        &mut self,
        vote: SignedVote,
        signer: &dyn ConsensusSigner,
    ) -> Result<(Option<SignedVote>, Option<CommitCertificate>)> {
        let height = vote.height;
        let round = vote.round;
        let step = vote.step;

        match self.vote_pool.add_vote(vote.clone(), &self.snapshot) {
            AddVoteResult::Added => {}
            AddVoteResult::DuplicateVote => return Ok((None, None)),
            AddVoteResult::EquivocationDetected { validator, .. } => {
                warn!("Equivocation detected from validator {:?}", validator);
                return Ok((None, None));
            }
            AddVoteResult::UnknownValidator | AddVoteResult::InvalidSignature | AddVoteResult::SnapshotMismatch => {
                return Ok((None, None));
            }
        }

        // Check if 2/3 Prevote Polka is reached
        if step == VoteStep::Prevote {
            let checked_block = vote.block_id;
            if let Some(prevotes) = self.vote_pool.check_quorum(
                self.config.protocol_version.clone(),
                self.config.chain_id.clone(),
                self.config.epoch,
                height,
                round,
                VoteStep::Prevote,
                checked_block,
                &self.snapshot,
            ) {
                if let Some(bid) = checked_block {
                    info!("2/3 Prevotes (Polka) reached for block {:?}, updating locks", bid);
                    self.locked_block = Some(bid);
                    self.locked_round = Some(round);
                    self.valid_block = Some(bid);
                    self.valid_round = Some(round);
                    self.valid_round_certificate = Some(PrevoteCertificate {
                        protocol_version: self.config.protocol_version.clone(),
                        chain_id: self.config.chain_id.clone(),
                        epoch: self.config.epoch,
                        height: self.height,
                        round: self.round,
                        block_id: bid,
                        stake_snapshot_hash: self.snapshot.snapshot_hash.clone(),
                        prevotes,
                    });

                    // Cast local Precommit vote
                    if self.local_validator_id.is_some() {
                        if let Ok(precommit_vote) = self.cast_vote(VoteStep::Precommit, Some(bid), signer) {
                            return Ok((Some(precommit_vote), None));
                        }
                    }
                }
            }
        }

        // Check if 2/3 Precommit quorum is reached for finality
        if step == VoteStep::Precommit {
            if let Some(bid) = vote.block_id {
                if let Some(cert) = self.vote_pool.create_commit_certificate(
                    self.config.protocol_version.clone(),
                    self.config.chain_id.clone(),
                    self.config.epoch,
                    height,
                    round,
                    bid,
                    &self.snapshot,
                ) {
                    info!("2/3 Precommits reached! CommitCertificate formed for block {:?}", bid);
                    self.step = ConsensusStep::Commit;
                    return Ok((None, Some(cert)));
                }
            }
        }

        Ok((None, None))
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
            protocol_version: self.config.protocol_version.clone(),
            chain_id: self.config.chain_id.clone(),
            epoch: self.config.epoch,
            height: self.height,
            round: self.round,
            step,
            block_id,
            stake_snapshot_hash: self.snapshot.snapshot_hash.clone(),
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
