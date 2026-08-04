//! Tendermint-style BFT State Machine (Propose, Prevote, Precommit, Commit)

use super::safety_store::{
    ConsensusSafetyStore, ConsensusSigner, DurableConsensusSafetyState, VoteSignRequest,
};
use super::types::{ConsensusConfig, ConsensusStep, ElectionMath};
use super::vote_pool::{AddVoteResult, VotePool};
use anyhow::{anyhow, Result};
use k256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use norn_common::consensus_types::{
    CommitCertificate, FinalizedConsensusState, PendingValidatorChange, PendingValidatorChanges,
    PrevoteCertificate, Proposal, SignedVote, StakeSnapshot, ValidatorChange,
    ValidatorKeyRotationProof, VoteStep,
};
use norn_common::types::{
    Block, BlockHeader, BlockId, BlockV2, ConsensusPublicKey, ValidatorId, VrfPublicKey,
};
use norn_crypto::vrf::{
    build_message_transcript, verify_vrf, VRFPreOutBytes, VRFProofBytes, VerifiedVrfOutput,
    VrfContext,
};
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
    /// Consensus timestamp of the canonical parent block. It is supplied by
    /// finalized durable state, never by the local wall clock.
    pub parent_timestamp: i64,
    pub vote_pool: VotePool,
    pub safety_store: Arc<dyn ConsensusSafetyStore>,
    pub local_validator_id: Option<ValidatorId>,
    pub pending_validator_changes: PendingValidatorChanges,
}

/// A side-effect-free request emitted by the consensus state machine.  The
/// driver must persist and sign it before acknowledging it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteIntent {
    pub request: VoteSignRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsensusAction {
    SignVote(VoteIntent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsensusEvent {
    VotePersisted(SignedVote),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::safety_store::MemorySafetyStore;
    use k256::ecdsa::{signature::Signer, SigningKey};
    use norn_common::types::{Hash, StakeSnapshotHash};
    use norn_crypto::vrf::{build_message_transcript, VRFKeyPair};
    use std::collections::BTreeMap;

    struct DummySigner;

    impl ConsensusSigner for DummySigner {
        fn sign_canonical_bytes(&self, _bytes: &[u8]) -> Result<[u8; 64]> {
            Ok([9u8; 64])
        }
    }

    struct FailingSigner;

    impl ConsensusSigner for FailingSigner {
        fn sign_canonical_bytes(&self, _bytes: &[u8]) -> Result<[u8; 64]> {
            Err(anyhow!("injected signer failure"))
        }
    }

    #[test]
    fn finalized_randomness_is_the_only_next_height_parent_seed() {
        let snapshot = StakeSnapshot {
            epoch: 1,
            validators: BTreeMap::new(),
            snapshot_hash: StakeSnapshotHash([7; 32]),
        };
        let safety_store = Arc::new(MemorySafetyStore::new());
        let mut state_machine = TendermintStateMachine::new(
            ConsensusConfig::default(),
            snapshot.clone(),
            Hash([1; 32]),
            safety_store,
            None,
        );
        let finalized = FinalizedConsensusState {
            height: 1,
            finalized_block_id: BlockId(Hash([2; 32])),
            state_root: Hash([5; 32]),
            timestamp: 1,
            next_randomness: Hash([4; 32]),
            active_stake_snapshot_hash: snapshot.snapshot_hash,
            epoch: 1,
            pending_validator_changes: PendingValidatorChanges::default(),
        };

        state_machine
            .start_new_height_from_finalized(&finalized, snapshot)
            .unwrap();
        assert_eq!(state_machine.height, 2);
        assert_eq!(state_machine.parent_randomness, Hash([4; 32]));
    }

    #[test]
    fn finalized_pending_validator_changes_survive_live_and_restart_transition() {
        let validator_id = ValidatorId([7; 32]);
        let record = norn_common::consensus_types::ValidatorRecord {
            validator_id,
            consensus_public_key: ConsensusPublicKey([1; 33]),
            vrf_public_key: VrfPublicKey([2; 32]),
            voting_power: 1,
            jailed_until_epoch: None,
            slashed: false,
        };
        let snapshot = StakeSnapshot::from_genesis(1, vec![record.clone()]).unwrap();
        let safety_store = Arc::new(MemorySafetyStore::new());
        let mut source = TendermintStateMachine::new(
            ConsensusConfig {
                epoch_length: 1,
                ..ConsensusConfig::default()
            },
            snapshot.clone(),
            Hash([1; 32]),
            safety_store.clone(),
            None,
        );
        source
            .queue_validator_change(
                ValidatorChange::SetVotingPower {
                    validator_id,
                    voting_power: 7,
                },
                3,
            )
            .unwrap();

        let finalized = FinalizedConsensusState {
            height: 1,
            finalized_block_id: BlockId(Hash([2; 32])),
            state_root: Hash([5; 32]),
            timestamp: 1,
            next_randomness: Hash([4; 32]),
            active_stake_snapshot_hash: snapshot.snapshot_hash,
            epoch: 1,
            pending_validator_changes: source.pending_validator_changes.clone(),
        };
        let next_snapshot = finalized
            .pending_validator_changes
            .snapshot_for_epoch(&snapshot, 2)
            .unwrap();

        let mut live = TendermintStateMachine::new(
            source.config.clone(),
            snapshot.clone(),
            Hash([1; 32]),
            safety_store.clone(),
            None,
        );
        live.start_new_height_from_finalized(&finalized, next_snapshot.clone())
            .unwrap();

        let mut restarted =
            TendermintStateMachine::new(source.config, snapshot, Hash([9; 32]), safety_store, None);
        restarted
            .restore_after_finalized(&finalized, next_snapshot)
            .unwrap();

        assert_eq!(live.height, 2);
        assert_eq!(restarted.height, 2);
        assert_eq!(live.parent_randomness, finalized.next_randomness);
        assert_eq!(restarted.parent_randomness, finalized.next_randomness);
        assert_eq!(
            live.pending_validator_changes,
            restarted.pending_validator_changes
        );

        let live_epoch_three = live.snapshot_for_height(3).unwrap();
        let restarted_epoch_three = restarted.snapshot_for_height(3).unwrap();
        assert_eq!(live_epoch_three, restarted_epoch_three);
        assert_eq!(live_epoch_three.validators[&validator_id].voting_power, 7);
    }

    #[test]
    fn vote_intent_ack_round_trip_does_not_mutate_consensus_state() {
        let snapshot = StakeSnapshot {
            epoch: 0,
            validators: BTreeMap::new(),
            snapshot_hash: StakeSnapshotHash([8; 32]),
        };
        let safety_store = Arc::new(MemorySafetyStore::new());
        let state_machine = TendermintStateMachine::new(
            ConsensusConfig::default(),
            snapshot,
            Hash([1; 32]),
            safety_store,
            Some(ValidatorId([3; 32])),
        );

        let original_step = state_machine.step;
        let intent = state_machine
            .build_vote_intent(VoteStep::Prevote, None)
            .unwrap();
        let event = state_machine
            .execute_action(ConsensusAction::SignVote(intent.clone()), &DummySigner)
            .unwrap();
        let vote = state_machine.apply_consensus_event(&intent, event).unwrap();

        assert_eq!(vote.step, VoteStep::Prevote);
        assert_eq!(vote.block_id, None);
        assert_eq!(state_machine.step, original_step);
        assert!(state_machine.locked_block.is_none());
    }

    #[test]
    fn signer_failure_does_not_advance_timeout_step() {
        let snapshot = StakeSnapshot {
            epoch: 0,
            validators: BTreeMap::new(),
            snapshot_hash: StakeSnapshotHash([8; 32]),
        };
        let safety_store = Arc::new(MemorySafetyStore::new());
        let mut state_machine = TendermintStateMachine::new(
            ConsensusConfig::default(),
            snapshot,
            Hash([1; 32]),
            safety_store,
            Some(ValidatorId([3; 32])),
        );

        assert!(state_machine.on_timeout_propose(&FailingSigner).is_err());
        assert_eq!(state_machine.step, ConsensusStep::NewHeight);
        assert!(state_machine.locked_block.is_none());
    }

    #[test]
    fn v5_block_timestamp_is_parent_relative_and_bounded() {
        let snapshot = StakeSnapshot {
            epoch: 1,
            validators: BTreeMap::new(),
            snapshot_hash: StakeSnapshotHash([8; 32]),
        };
        let mut config = ConsensusConfig::default();
        config.protocol_version = norn_common::types::ProtocolVersion(5);
        config.max_block_timestamp_step = 30;
        let state_machine = TendermintStateMachine::new_with_parent_timestamp(
            config,
            snapshot,
            Hash([1; 32]),
            100,
            Arc::new(MemorySafetyStore::new()),
            None,
        );
        assert!(state_machine.validate_block_timestamp(101).is_ok());
        assert!(state_machine.validate_block_timestamp(100).is_err());
        assert!(state_machine.validate_block_timestamp(131).is_err());
        assert!(state_machine.validate_block_timestamp(-1).is_err());
    }

    #[test]
    fn durable_lock_state_restores_before_processing_new_round() {
        let snapshot = StakeSnapshot {
            epoch: 1,
            validators: BTreeMap::new(),
            snapshot_hash: StakeSnapshotHash([8; 32]),
        };
        let safety_store = Arc::new(MemorySafetyStore::new());
        let config = ConsensusConfig::default();
        let mut source = TendermintStateMachine::new(
            config.clone(),
            snapshot.clone(),
            Hash([1; 32]),
            safety_store.clone(),
            None,
        );
        source.locked_block = Some(BlockId(Hash([4; 32])));
        source.locked_round = Some(2);
        source.round = 3;
        source.step = ConsensusStep::Propose;
        safety_store
            .persist_consensus_state(source.durable_safety_state().unwrap())
            .unwrap();

        let recovered_state = safety_store.load_consensus_state().unwrap().unwrap();
        let mut recovered =
            TendermintStateMachine::new(config, snapshot, Hash([1; 32]), safety_store, None);
        recovered
            .restore_durable_safety_state(recovered_state)
            .unwrap();
        assert_eq!(recovered.round, 3);
        assert_eq!(recovered.step, ConsensusStep::Propose);
        assert_eq!(recovered.locked_block, source.locked_block);
        assert_eq!(recovered.locked_round, source.locked_round);
        assert_eq!(recovered.valid_block, None);
        assert_eq!(recovered.valid_round, None);
    }

    #[test]
    fn key_rotation_requires_old_and_new_consensus_and_vrf_possession() {
        let old_consensus = SigningKey::from_bytes((&[21u8; 32]).into()).unwrap();
        let new_consensus = SigningKey::from_bytes((&[22u8; 32]).into()).unwrap();
        let old_vrf = VRFKeyPair::from_secret_bytes(&[23u8; 32]).unwrap();
        let new_vrf = VRFKeyPair::from_secret_bytes(&[24u8; 32]).unwrap();
        let validator_id = ValidatorId([7u8; 32]);
        let old_consensus_public_key = ConsensusPublicKey(
            old_consensus
                .verifying_key()
                .to_encoded_point(true)
                .as_bytes()
                .try_into()
                .unwrap(),
        );
        let new_consensus_public_key = ConsensusPublicKey(
            new_consensus
                .verifying_key()
                .to_encoded_point(true)
                .as_bytes()
                .try_into()
                .unwrap(),
        );
        let old_vrf_public_key = VrfPublicKey(old_vrf.public_key_bytes());
        let new_vrf_public_key = VrfPublicKey(new_vrf.public_key_bytes());
        let snapshot = StakeSnapshot::from_genesis(
            1,
            vec![norn_common::consensus_types::ValidatorRecord {
                validator_id,
                consensus_public_key: old_consensus_public_key,
                vrf_public_key: old_vrf_public_key,
                voting_power: 1,
                jailed_until_epoch: None,
                slashed: false,
            }],
        )
        .unwrap();
        let mut config = ConsensusConfig::default();
        config.key_rotation_delay = 1;
        let safety_store = Arc::new(MemorySafetyStore::new());
        let mut state_machine = TendermintStateMachine::new(
            config,
            snapshot,
            Hash([1; 32]),
            safety_store,
            Some(validator_id),
        );
        let signing_bytes = ValidatorKeyRotationProof::canonical_bytes(
            &state_machine.config.chain_id,
            &validator_id,
            2,
            &old_consensus_public_key,
            &new_consensus_public_key,
            &new_vrf_public_key,
        );
        let old_signature: k256::ecdsa::Signature = old_consensus.sign(&signing_bytes);
        let new_signature: k256::ecdsa::Signature = new_consensus.sign(&signing_bytes);
        let (vrf_preout, vrf_proof) = new_vrf.vrf_sign(build_message_transcript(&signing_bytes));
        let proof = ValidatorKeyRotationProof {
            old_consensus_signature: old_signature
                .normalize_s()
                .unwrap_or(old_signature)
                .to_bytes()
                .into(),
            new_consensus_signature: new_signature
                .normalize_s()
                .unwrap_or(new_signature)
                .to_bytes()
                .into(),
            vrf_preout: vrf_preout.0,
            vrf_proof: vrf_proof.0,
        };
        state_machine
            .queue_validator_change(
                ValidatorChange::RotateKeys {
                    validator_id,
                    consensus_public_key: new_consensus_public_key,
                    vrf_public_key: new_vrf_public_key,
                    proof,
                },
                2,
            )
            .unwrap();
    }

    #[test]
    fn bounded_lock_model_preserves_tendermint_unlock_rule() {
        let first = BlockId(Hash([1; 32]));
        let second = BlockId(Hash([2; 32]));

        for locked_round in 0..=3 {
            for candidate in [first, second] {
                for valid_round in (0..=4).map(Some).chain([None]) {
                    for certificate_round in (0..=4).map(Some).chain([None]) {
                        for certificate_block in [Some(first), Some(second), None] {
                            for certificate_valid in [false, true] {
                                let accepted = can_prevote_with_lock(
                                    Some(first),
                                    Some(locked_round),
                                    candidate,
                                    valid_round,
                                    certificate_round,
                                    certificate_block,
                                    certificate_valid,
                                );
                                if candidate != first && accepted {
                                    assert!(certificate_valid);
                                    assert!(valid_round.is_some_and(|round| round >= locked_round));
                                    assert_eq!(valid_round, certificate_round);
                                    assert_eq!(certificate_block, Some(candidate));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn can_prevote_with_lock(
    locked_block: Option<BlockId>,
    locked_round: Option<u32>,
    candidate: BlockId,
    valid_round: Option<u32>,
    certificate_round: Option<u32>,
    certificate_block: Option<BlockId>,
    certificate_valid: bool,
) -> bool {
    match (locked_block, locked_round) {
        (None, _) => true,
        (Some(locked), Some(_)) if locked == candidate => true,
        (Some(_), Some(locked_round)) => {
            certificate_valid
                && valid_round.is_some_and(|round| round >= locked_round)
                && valid_round == certificate_round
                && certificate_block == Some(candidate)
        }
        _ => false,
    }
}

impl TendermintStateMachine {
    pub fn new(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        parent_randomness: norn_common::types::Hash,
        safety_store: Arc<dyn ConsensusSafetyStore>,
        local_validator_id: Option<ValidatorId>,
    ) -> Self {
        Self::new_with_parent_timestamp(
            config,
            snapshot,
            parent_randomness,
            0,
            safety_store,
            local_validator_id,
        )
    }

    pub fn new_with_parent_timestamp(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        parent_randomness: norn_common::types::Hash,
        parent_timestamp: i64,
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
            parent_timestamp,
            vote_pool: VotePool::new(),
            safety_store,
            local_validator_id,
            pending_validator_changes: PendingValidatorChanges::default(),
        }
    }

    pub fn queue_validator_change(
        &mut self,
        change: ValidatorChange,
        effective_epoch: u64,
    ) -> Result<()> {
        let delay = match &change {
            ValidatorChange::RotateKeys { .. } => self.config.key_rotation_delay,
            ValidatorChange::Remove { .. } => self.config.unbonding_delay,
            ValidatorChange::Slash { .. } => self.config.slashing_activation_delay,
            _ => self.config.validator_update_delay,
        };
        let minimum_epoch = self
            .snapshot
            .epoch
            .checked_add(delay)
            .ok_or_else(|| anyhow!("validator change effective epoch overflow"))?;
        if effective_epoch < minimum_epoch {
            return Err(anyhow!(
                "validator change violates its protocol activation delay"
            ));
        }
        if let ValidatorChange::RotateKeys {
            validator_id,
            consensus_public_key,
            vrf_public_key,
            proof,
        } = &change
        {
            self.verify_key_rotation(
                *validator_id,
                effective_epoch,
                *consensus_public_key,
                *vrf_public_key,
                proof,
            )?;
        }
        self.pending_validator_changes
            .queue(PendingValidatorChange {
                effective_epoch,
                change,
            })
            .map_err(|e| anyhow!(e.to_string()))
    }

    fn verify_key_rotation(
        &self,
        validator_id: ValidatorId,
        effective_epoch: u64,
        new_consensus_public_key: ConsensusPublicKey,
        new_vrf_public_key: VrfPublicKey,
        proof: &ValidatorKeyRotationProof,
    ) -> Result<()> {
        let current = self
            .snapshot
            .validators
            .get(&validator_id)
            .ok_or_else(|| anyhow!("key rotation targets an unknown validator"))?;
        if !current.is_active_at(self.snapshot.epoch) {
            return Err(anyhow!(
                "inactive validator cannot authorize a key rotation"
            ));
        }
        if new_consensus_public_key == current.consensus_public_key
            || new_vrf_public_key == current.vrf_public_key
        {
            return Err(anyhow!(
                "key rotation must change both consensus and VRF keys"
            ));
        }
        if self.snapshot.validators.values().any(|record| {
            record.validator_id != validator_id
                && (record.consensus_public_key == new_consensus_public_key
                    || record.vrf_public_key == new_vrf_public_key)
        }) {
            return Err(anyhow!(
                "key rotation reuses a key already assigned to another validator"
            ));
        }
        if self
            .pending_validator_changes
            .changes
            .iter()
            .any(|pending| {
                matches!(
                    &pending.change,
                    ValidatorChange::RotateKeys {
                        consensus_public_key,
                        vrf_public_key,
                        ..
                    } if *consensus_public_key == new_consensus_public_key
                        || *vrf_public_key == new_vrf_public_key
                )
            })
        {
            return Err(anyhow!(
                "key rotation reuses a key from another pending rotation"
            ));
        }

        let signing_bytes = ValidatorKeyRotationProof::canonical_bytes(
            &self.config.chain_id,
            &validator_id,
            effective_epoch,
            &current.consensus_public_key,
            &new_consensus_public_key,
            &new_vrf_public_key,
        );
        let old_key = VerifyingKey::from_sec1_bytes(&current.consensus_public_key.0)
            .map_err(|_| anyhow!("current consensus key is malformed"))?;
        let new_key = VerifyingKey::from_sec1_bytes(&new_consensus_public_key.0)
            .map_err(|_| anyhow!("new consensus key is malformed"))?;
        let old_signature = Signature::from_slice(&proof.old_consensus_signature)
            .map_err(|_| anyhow!("old consensus key proof is malformed"))?;
        let new_signature = Signature::from_slice(&proof.new_consensus_signature)
            .map_err(|_| anyhow!("new consensus key proof is malformed"))?;
        if old_signature.normalize_s().is_some() || new_signature.normalize_s().is_some() {
            return Err(anyhow!("key rotation proofs must use low-S signatures"));
        }
        old_key
            .verify(&signing_bytes, &old_signature)
            .map_err(|_| anyhow!("old consensus key did not authorize rotation"))?;
        new_key
            .verify(&signing_bytes, &new_signature)
            .map_err(|_| anyhow!("new consensus key proof of possession failed"))?;

        if proof.vrf_preout == [0u8; 32] || proof.vrf_proof == [0u8; 64] {
            return Err(anyhow!("new VRF key proof of possession is empty"));
        }
        verify_vrf(
            &new_vrf_public_key.0,
            build_message_transcript(&signing_bytes),
            &VRFPreOutBytes(proof.vrf_preout),
            &VRFProofBytes(proof.vrf_proof),
        )
        .map_err(|_| anyhow!("new VRF key proof of possession failed"))?;
        Ok(())
    }

    pub fn snapshot_for_height(&self, height: u64) -> Result<StakeSnapshot> {
        let epoch = self.config.epoch_for_height(height)?;
        if epoch == self.snapshot.epoch {
            return Ok(self.snapshot.clone());
        }
        self.pending_validator_changes
            .snapshot_for_epoch(&self.snapshot, epoch)
            .map_err(|e| anyhow!(e.to_string()))
    }

    /// Advance to next height
    pub fn start_new_height(
        &mut self,
        height: u64,
        snapshot: StakeSnapshot,
        parent_randomness: norn_common::types::Hash,
    ) {
        self.start_new_height_with_timestamp(height, snapshot, parent_randomness, 0);
    }

    pub fn start_new_height_with_timestamp(
        &mut self,
        height: u64,
        snapshot: StakeSnapshot,
        parent_randomness: norn_common::types::Hash,
        parent_timestamp: i64,
    ) {
        self.height = height;
        self.round = 0;
        self.step = ConsensusStep::NewHeight;
        self.locked_block = None;
        self.locked_round = None;
        self.valid_block = None;
        self.valid_round = None;
        self.valid_round_certificate = None;
        self.snapshot = snapshot;
        self.pending_validator_changes
            .changes
            .retain(|change| change.effective_epoch > self.snapshot.epoch);
        self.parent_randomness = parent_randomness;
        self.parent_timestamp = parent_timestamp;
        info!("Consensus starting new height {}", height);
        self.start_new_round(0);
    }

    pub(crate) fn validate_block_timestamp(&self, timestamp: i64) -> Result<()> {
        if self.config.protocol_version.0 < 5 {
            return Ok(());
        }
        if timestamp < 0 {
            return Err(anyhow!("block timestamp must be non-negative"));
        }
        if timestamp <= self.parent_timestamp {
            return Err(anyhow!(
                "block timestamp must be greater than parent timestamp"
            ));
        }
        let max_step = i64::try_from(self.config.max_block_timestamp_step)
            .map_err(|_| anyhow!("block timestamp step exceeds i64 range"))?;
        let latest = self
            .parent_timestamp
            .checked_add(max_step)
            .ok_or_else(|| anyhow!("block timestamp upper bound overflow"))?;
        if timestamp > latest {
            return Err(anyhow!(
                "block timestamp exceeds the protocol parent-relative bound"
            ));
        }
        Ok(())
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
            self.current_epoch().ok()?,
            self.height,
            self.round,
            &self.parent_randomness,
            &self.snapshot,
        )
    }

    pub fn current_epoch(&self) -> Result<u64> {
        self.config.epoch_for_height(self.height)
    }

    pub fn durable_safety_state(&self) -> Result<DurableConsensusSafetyState> {
        Ok(DurableConsensusSafetyState {
            sequence: 0,
            protocol_version: self.config.protocol_version,
            chain_id: self.config.chain_id,
            epoch: self.current_epoch()?,
            height: self.height,
            round: self.round,
            step: self.step,
            stake_snapshot_hash: self.snapshot.snapshot_hash,
            parent_randomness: self.parent_randomness,
            locked_block: self.locked_block,
            locked_round: self.locked_round,
            valid_block: self.valid_block,
            valid_round: self.valid_round,
            valid_round_certificate: self.valid_round_certificate.clone(),
        })
    }

    pub fn restore_durable_safety_state(
        &mut self,
        state: DurableConsensusSafetyState,
    ) -> Result<()> {
        if state.protocol_version != self.config.protocol_version
            || state.chain_id != self.config.chain_id
            || state.height != self.height
            || state.epoch != self.current_epoch()?
            || state.stake_snapshot_hash != self.snapshot.snapshot_hash
            || state.parent_randomness != self.parent_randomness
        {
            return Err(anyhow!(
                "durable consensus safety state does not match the active context"
            ));
        }
        if state.locked_block.is_some() != state.locked_round.is_some()
            || state.valid_block.is_some() != state.valid_round.is_some()
        {
            return Err(anyhow!(
                "durable consensus safety state has an incomplete lock or valid-round pair"
            ));
        }
        if state
            .locked_round
            .is_some_and(|locked_round| locked_round > state.round)
            || state
                .valid_round
                .is_some_and(|valid_round| valid_round > state.round)
        {
            return Err(anyhow!(
                "durable consensus safety state references a future lock or valid round"
            ));
        }
        if let Some(certificate) = &state.valid_round_certificate {
            if state.valid_block != Some(certificate.block_id)
                || state.valid_round != Some(certificate.round)
                || certificate.height != state.height
                || certificate.stake_snapshot_hash != state.stake_snapshot_hash
                || certificate.protocol_version != state.protocol_version
                || certificate.chain_id != state.chain_id
            {
                return Err(anyhow!(
                    "durable valid-round certificate does not match safety state"
                ));
            }
            Self::verify_prevote_certificate(certificate, &self.snapshot)?;
        } else if state.valid_block.is_some() || state.valid_round.is_some() {
            return Err(anyhow!(
                "durable valid-round state is missing its certificate"
            ));
        }
        self.round = state.round;
        self.step = state.step;
        self.locked_block = state.locked_block;
        self.locked_round = state.locked_round;
        self.valid_block = state.valid_block;
        self.valid_round = state.valid_round;
        self.valid_round_certificate = state.valid_round_certificate;
        Ok(())
    }

    fn persist_durable_safety_state(&self) -> Result<()> {
        self.safety_store
            .persist_consensus_state(self.durable_safety_state()?)
            .map_err(|error| anyhow!("failed to persist consensus safety state: {error}"))
    }

    fn durable_safety_state_after(
        &self,
        step: ConsensusStep,
        locked_block: Option<BlockId>,
        locked_round: Option<u32>,
        valid_block: Option<BlockId>,
        valid_round: Option<u32>,
        valid_round_certificate: Option<PrevoteCertificate>,
    ) -> Result<DurableConsensusSafetyState> {
        Ok(DurableConsensusSafetyState {
            sequence: 0,
            protocol_version: self.config.protocol_version,
            chain_id: self.config.chain_id,
            epoch: self.current_epoch()?,
            height: self.height,
            round: self.round,
            step,
            stake_snapshot_hash: self.snapshot.snapshot_hash,
            parent_randomness: self.parent_randomness,
            locked_block,
            locked_round,
            valid_block,
            valid_round,
            valid_round_certificate,
        })
    }

    /// Check if local node is current round's proposer
    pub fn is_local_proposer(&self) -> bool {
        if let (Some(local_id), Some(proposer)) =
            (self.local_validator_id, self.get_current_proposer())
        {
            local_id == proposer
        } else {
            false
        }
    }

    /// Verify a PrevoteCertificate POL proof
    pub fn verify_prevote_certificate(
        cert: &PrevoteCertificate,
        snapshot: &StakeSnapshot,
    ) -> Result<()> {
        if cert.stake_snapshot_hash != snapshot.snapshot_hash {
            return Err(anyhow!("PrevoteCertificate snapshot hash mismatch"));
        }
        if cert.prevotes.len() > snapshot.validators.len() {
            return Err(anyhow!(
                "PrevoteCertificate has more votes than the stake snapshot"
            ));
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

        if pool
            .check_quorum(
                cert.protocol_version.clone(),
                cert.chain_id.clone(),
                cert.epoch,
                cert.height,
                cert.round,
                VoteStep::Prevote,
                Some(cert.block_id),
                snapshot,
            )
            .is_none()
        {
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
        self.handle_proposal_header(proposal, &block.header, signer)
            .map(|(vote, _)| vote)
    }

    /// Handle a V2 proposal after its full block/overlay commitments have
    /// been validated by the caller. The consensus safety logic is shared
    /// with legacy proposals, but the V2 payload is never converted into a
    /// legacy transaction list.
    pub fn handle_proposal_v2(
        &mut self,
        proposal: &Proposal,
        block: &BlockV2,
        signer: &dyn ConsensusSigner,
    ) -> Result<SignedVote> {
        validate_v2_proposal_builder_relation(proposal, &block.header)?;
        self.handle_proposal_header(proposal, &block.header, signer)
            .map(|(vote, _)| vote)
    }

    /// Handle a V2 proposal and return the verified randomness that will be
    /// committed as the parent randomness of the next finalized height.
    pub fn handle_proposal_v2_with_vrf(
        &mut self,
        proposal: &Proposal,
        block: &BlockV2,
        signer: &dyn ConsensusSigner,
    ) -> Result<(SignedVote, Option<VerifiedVrfOutput>)> {
        validate_v2_proposal_builder_relation(proposal, &block.header)?;
        self.handle_proposal_header(proposal, &block.header, signer)
    }

    /// Pure V2 proposal validation for validators and FullNodes. It verifies
    /// the active context, proposer membership/selection, canonical signature,
    /// and VRF derivation without casting a vote or mutating locks, rounds, or
    /// the vote pool.
    pub fn validate_proposal_v2_without_vote(
        &self,
        proposal: &Proposal,
        block: &BlockV2,
    ) -> Result<VerifiedVrfOutput> {
        let expected_epoch = self.current_epoch()?;
        if proposal.height != self.height
            || proposal.round != self.round
            || proposal.round > self.config.max_consensus_round
            || proposal.epoch != expected_epoch
            || proposal.protocol_version != self.config.protocol_version
            || proposal.chain_id != self.config.chain_id
            || proposal.stake_snapshot_hash != self.snapshot.snapshot_hash
            || block.header.height < 0
            || proposal.height != block.header.height as u64
            || proposal.parent_block_hash != block.header.prev_block_hash
            || block.header.epoch != expected_epoch
            || block.header.parent_randomness != self.parent_randomness
            || proposal.block_id != BlockId(block.header.block_hash)
        {
            return Err(anyhow!("V2 proposal context mismatch"));
        }
        self.validate_block_timestamp(block.header.timestamp)?;
        validate_v2_proposal_builder_relation(proposal, &block.header)?;
        if block.header.protocol_version.0 >= 4 {
            self.verify_block_builder_vrf(block)?;
        }
        let expected_proposer = self
            .get_current_proposer()
            .ok_or_else(|| anyhow!("no deterministic proposer is available"))?;
        if proposal.proposer != expected_proposer {
            return Err(anyhow!(
                "V2 proposal proposer is not selected for this round"
            ));
        }
        let record = self
            .snapshot
            .validators
            .get(&proposal.proposer)
            .ok_or_else(|| anyhow!("V2 proposal proposer is not in the active snapshot"))?;
        if !record.is_active_at(self.snapshot.epoch) {
            return Err(anyhow!(
                "V2 proposal proposer is jailed or slashed in the active snapshot"
            ));
        }
        if record.consensus_public_key.0 == [0u8; 33] || proposal.signature == [0u8; 64] {
            return Err(anyhow!("V2 proposal has a zero key or signature"));
        }
        let verifying_key = VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0)
            .map_err(|_| anyhow!("malformed V2 proposer public key"))?;
        let signature = Signature::from_slice(&proposal.signature)
            .map_err(|_| anyhow!("malformed V2 proposer signature"))?;
        if signature.normalize_s().is_some() {
            return Err(anyhow!("V2 proposer signature is not canonical"));
        }
        verifying_key
            .verify(&proposal.canonical_bytes(), &signature)
            .map_err(|_| anyhow!("V2 proposer signature verification failed"))?;
        self.verify_proposal_vrf(proposal, &block.header)
    }

    /// Validate a durable proposal attempt during restart recovery. Unlike
    /// the live admission path this deliberately does not require the
    /// proposal to be for the current round or current deterministic proposer;
    /// older attempts may be needed to finalize an older-round certificate.
    /// All cryptographic context remains bound to the active height snapshot.
    pub fn validate_v2_proposal_for_recovery(
        &self,
        proposal: &Proposal,
        block: &BlockV2,
    ) -> Result<VerifiedVrfOutput> {
        let expected_epoch = self.current_epoch()?;
        if proposal.height != self.height
            || proposal.round > self.config.max_consensus_round
            || proposal.epoch != expected_epoch
            || proposal.protocol_version != self.config.protocol_version
            || proposal.chain_id != self.config.chain_id
            || proposal.stake_snapshot_hash != self.snapshot.snapshot_hash
            || block.header.height < 0
            || proposal.height != block.header.height as u64
            || proposal.parent_block_hash != block.header.prev_block_hash
            || block.header.epoch != expected_epoch
            || block.header.protocol_version != self.config.protocol_version
            || block.header.chain_id != self.config.chain_id
            || block.header.stake_snapshot_hash != self.snapshot.snapshot_hash
            || block.header.parent_randomness != self.parent_randomness
            || proposal.block_id != BlockId(block.header.block_hash)
        {
            return Err(anyhow!("durable V2 proposal context mismatch"));
        }
        self.validate_block_timestamp(block.header.timestamp)?;
        validate_v2_proposal_builder_relation(proposal, &block.header)?;
        if block.header.protocol_version.0 >= 4 {
            self.verify_block_builder_vrf(block)?;
        }
        let record = self
            .snapshot
            .validators
            .get(&proposal.proposer)
            .ok_or_else(|| anyhow!("durable V2 proposer is not in the active snapshot"))?;
        if !record.is_active_at(self.snapshot.epoch) {
            return Err(anyhow!(
                "durable V2 proposer is jailed or slashed in the active snapshot"
            ));
        }
        if record.consensus_public_key.0 == [0u8; 33] || proposal.signature == [0u8; 64] {
            return Err(anyhow!("durable V2 proposal has a zero key or signature"));
        }
        let verifying_key = VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0)
            .map_err(|_| anyhow!("malformed durable V2 proposer public key"))?;
        let signature = Signature::from_slice(&proposal.signature)
            .map_err(|_| anyhow!("malformed durable V2 proposer signature"))?;
        if signature.normalize_s().is_some() {
            return Err(anyhow!("durable V2 proposer signature is not canonical"));
        }
        verifying_key
            .verify(&proposal.canonical_bytes(), &signature)
            .map_err(|_| anyhow!("durable V2 proposer signature verification failed"))?;

        let context = VrfContext {
            protocol_version: proposal.protocol_version,
            chain_id: proposal.chain_id,
            epoch: proposal.epoch,
            height: proposal.height,
            round: proposal.round,
            parent_block_hash: proposal.parent_block_hash,
            stake_snapshot_hash: proposal.stake_snapshot_hash,
            validator_id: proposal.proposer,
        };
        norn_crypto::vrf::verify_and_derive(
            &record.vrf_public_key.0,
            &context,
            &VRFPreOutBytes(proposal.vrf_preout),
            &VRFProofBytes(proposal.vrf_proof),
        )
        .map_err(|e| anyhow!("durable V2 proposal VRF verification failed: {e}"))
    }

    /// Verify the VRF commitment that belongs to the immutable block builder.
    /// This is separate from the current Proposal VRF: a later-round proposer
    /// may replace the latter, but never the builder material committed by the
    /// block ID.
    pub fn verify_block_builder_vrf(&self, block: &BlockV2) -> Result<VerifiedVrfOutput> {
        if block.header.protocol_version.0 < 4
            || block.consensus_data.builder_round != block.header.round
            || block.header.height < 0
            || block.header.protocol_version != self.config.protocol_version
            || block.header.chain_id != self.config.chain_id
            || block.header.epoch != self.current_epoch()?
            || block.header.stake_snapshot_hash != self.snapshot.snapshot_hash
            || block.header.parent_randomness != self.parent_randomness
        {
            return Err(anyhow!("immutable builder VRF context mismatch"));
        }
        let builder = block.header.block_builder;
        let record = self
            .snapshot
            .validators
            .get(&builder)
            .ok_or_else(|| anyhow!("immutable block builder is not in the active snapshot"))?;
        if !record.is_active_at(self.snapshot.epoch) {
            return Err(anyhow!(
                "immutable block builder is jailed or slashed in the active snapshot"
            ));
        }
        if record.vrf_public_key.0 == [0u8; 32]
            || block.consensus_data.builder_vrf_preout == [0u8; 32]
            || block.consensus_data.builder_vrf_proof == [0u8; 64]
        {
            return Err(anyhow!("immutable builder VRF material is zero"));
        }
        let context = VrfContext {
            protocol_version: block.header.protocol_version,
            chain_id: block.header.chain_id,
            epoch: block.header.epoch,
            height: block.header.height as u64,
            round: block.consensus_data.builder_round,
            parent_block_hash: block.header.prev_block_hash,
            stake_snapshot_hash: block.header.stake_snapshot_hash,
            validator_id: builder,
        };
        norn_crypto::vrf::verify_and_derive(
            &record.vrf_public_key.0,
            &context,
            &VRFPreOutBytes(block.consensus_data.builder_vrf_preout),
            &VRFProofBytes(block.consensus_data.builder_vrf_proof),
        )
        .map_err(|e| anyhow!("immutable builder VRF verification failed: {e}"))
    }

    /// Verify the proposer VRF against the complete active-height context.
    /// The returned randomness is derived only after proof verification and is
    /// therefore safe to persist as the next-height consensus seed.
    pub fn verify_proposal_vrf(
        &self,
        proposal: &Proposal,
        block_header: &BlockHeader,
    ) -> Result<VerifiedVrfOutput> {
        let expected_epoch = self.current_epoch()?;
        if proposal.height != self.height
            || proposal.round != self.round
            || proposal.epoch != expected_epoch
            || proposal.protocol_version != self.config.protocol_version
            || proposal.chain_id != self.config.chain_id
            || proposal.parent_block_hash != block_header.prev_block_hash
            || block_header.height < 0
            || proposal.height != block_header.height as u64
            || block_header.epoch != expected_epoch
            || block_header.parent_randomness != self.parent_randomness
        {
            return Err(anyhow!("proposal VRF context mismatch"));
        }
        let record = self
            .snapshot
            .validators
            .get(&proposal.proposer)
            .ok_or_else(|| anyhow!("proposal proposer is not in the active snapshot"))?;
        let context = VrfContext {
            protocol_version: self.config.protocol_version.clone(),
            chain_id: self.config.chain_id.clone(),
            epoch: expected_epoch,
            height: self.height,
            round: self.round,
            parent_block_hash: proposal.parent_block_hash,
            stake_snapshot_hash: self.snapshot.snapshot_hash,
            validator_id: proposal.proposer,
        };
        norn_crypto::vrf::verify_and_derive(
            &record.vrf_public_key.0,
            &context,
            &VRFPreOutBytes(proposal.vrf_preout),
            &VRFProofBytes(proposal.vrf_proof),
        )
        .map_err(|e| anyhow!("proposal VRF verification failed: {e}"))
    }

    fn handle_proposal_header(
        &mut self,
        proposal: &Proposal,
        block_header: &BlockHeader,
        signer: &dyn ConsensusSigner,
    ) -> Result<(SignedVote, Option<VerifiedVrfOutput>)> {
        let expected_epoch = self.current_epoch()?;
        if proposal.height != self.height
            || proposal.round != self.round
            || proposal.round > self.config.max_consensus_round
        {
            return Err(anyhow!(
                "Proposal height/round mismatch: expected ({},{}), got ({},{})",
                self.height,
                self.round,
                proposal.height,
                proposal.round
            ));
        }

        if proposal.chain_id != self.config.chain_id
            || proposal.protocol_version != self.config.protocol_version
            || proposal.epoch != expected_epoch
        {
            return Err(anyhow!(
                "Proposal chain_id / protocol_version / epoch mismatch"
            ));
        }

        if proposal.stake_snapshot_hash != self.snapshot.snapshot_hash {
            return Err(anyhow!("Proposal stake_snapshot_hash mismatch"));
        }
        if self.snapshot.epoch != expected_epoch {
            return Err(anyhow!(
                "local stake snapshot epoch does not match block height"
            ));
        }

        if block_header.height < 0
            || proposal.height != block_header.height as u64
            || proposal.parent_block_hash != block_header.prev_block_hash
            || block_header.epoch != expected_epoch
            || block_header.parent_randomness != self.parent_randomness
        {
            return Err(anyhow!(
                "proposal block header does not match the active consensus context"
            ));
        }
        self.validate_block_timestamp(block_header.timestamp)?;

        let calculated_block_id = BlockId(block_header.block_hash);
        if proposal.block_id != calculated_block_id {
            return Err(anyhow!(
                "Proposal block_id does not match actual block header hash"
            ));
        }

        let expected_proposer = self
            .get_current_proposer()
            .ok_or_else(|| anyhow!("No proposer available"))?;
        if proposal.proposer != expected_proposer {
            warn!(
                "Proposal from invalid proposer: expected {:?}, got {:?}",
                expected_proposer, proposal.proposer
            );
            return Ok((self.cast_vote(VoteStep::Prevote, None, signer)?, None));
        }

        let Some(record) = self.snapshot.validators.get(&proposal.proposer) else {
            warn!("Proposer not found in stake snapshot");
            return Ok((self.cast_vote(VoteStep::Prevote, None, signer)?, None));
        };

        // 1. Strict fail-closed ECDSA signature verification
        if record.consensus_public_key.0 == [0u8; 33] || proposal.signature == [0u8; 64] {
            warn!("Rejected proposal with zero key or zero signature");
            return Ok((self.cast_vote(VoteStep::Prevote, None, signer)?, None));
        }

        let verifying_key = VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0)
            .map_err(|_| anyhow!("Malformed SEC1 consensus public key"))?;
        let sig = Signature::from_slice(&proposal.signature)
            .map_err(|_| anyhow!("Malformed proposal signature"))?;

        if sig.normalize_s().is_some() {
            warn!("Rejected non-canonical high-S proposal signature");
            return Ok((self.cast_vote(VoteStep::Prevote, None, signer)?, None));
        }

        let msg_bytes = proposal.canonical_bytes();
        if verifying_key.verify(&msg_bytes, &sig).is_err() {
            warn!("Proposal ECDSA signature verification failed");
            return Ok((self.cast_vote(VoteStep::Prevote, None, signer)?, None));
        }

        // 2. Strict VRF verification with the complete V2 context
        let verified_vrf = match self.verify_proposal_vrf(proposal, block_header) {
            Ok(output) => output,
            Err(_) => {
                warn!("Proposal VRF proof verification failed");
                return Ok((self.cast_vote(VoteStep::Prevote, None, signer)?, None));
            }
        };

        // 3. Evaluate Tendermint unlocking rule
        let (certificate_round, certificate_block, certificate_valid) = proposal
            .valid_round_certificate
            .as_ref()
            .map(|certificate| {
                (
                    Some(certificate.round),
                    Some(certificate.block_id),
                    Self::verify_prevote_certificate(certificate, &self.snapshot).is_ok(),
                )
            })
            .unwrap_or((None, None, false));
        let can_prevote = can_prevote_with_lock(
            self.locked_block,
            self.locked_round,
            calculated_block_id,
            proposal.valid_round,
            certificate_round,
            certificate_block,
            certificate_valid,
        );

        if can_prevote {
            info!("Casting Prevote for block {:?}", calculated_block_id);
            Ok((
                self.cast_vote(VoteStep::Prevote, Some(calculated_block_id), signer)?,
                Some(verified_vrf),
            ))
        } else {
            warn!("Locked on different block, casting NIL Prevote");
            Ok((
                self.cast_vote(VoteStep::Prevote, None, signer)?,
                Some(verified_vrf),
            ))
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

        if vote.protocol_version != self.config.protocol_version
            || vote.chain_id != self.config.chain_id
            || vote.epoch != self.config.epoch_for_height(height)?
        {
            return Ok((None, None));
        }

        let height_in_window = height >= self.height
            && height
                .checked_sub(self.height)
                .is_some_and(|delta| delta <= self.config.max_future_height);
        let round_in_window = round >= self.round
            && round
                .checked_sub(self.round)
                .is_some_and(|delta| delta <= self.config.max_future_round);
        if !height_in_window || (height == self.height && !round_in_window) {
            return Ok((None, None));
        }

        match self.vote_pool.add_vote(vote.clone(), &self.snapshot) {
            AddVoteResult::Added => {}
            AddVoteResult::DuplicateVote => return Ok((None, None)),
            AddVoteResult::EquivocationDetected { validator, .. } => {
                warn!("Equivocation detected from validator {:?}", validator);
                return Ok((None, None));
            }
            AddVoteResult::UnknownValidator
            | AddVoteResult::InvalidSignature
            | AddVoteResult::SnapshotMismatch => {
                return Ok((None, None));
            }
        }

        // Future votes are bounded and retained for the corresponding round,
        // but they must not drive locks or certificates for the current
        // round. They become actionable when the state machine advances.
        if height != self.height || round != self.round {
            return Ok((None, None));
        }

        // Check if 2/3 Prevote Polka is reached
        if step == VoteStep::Prevote {
            let checked_block = vote.block_id;
            if let Some(prevotes) = self.vote_pool.check_quorum(
                self.config.protocol_version.clone(),
                self.config.chain_id.clone(),
                self.current_epoch()?,
                height,
                round,
                VoteStep::Prevote,
                checked_block,
                &self.snapshot,
            ) {
                if let Some(bid) = checked_block {
                    info!(
                        "2/3 Prevotes (Polka) reached for block {:?}, updating locks",
                        bid
                    );
                    let certificate = PrevoteCertificate {
                        protocol_version: self.config.protocol_version.clone(),
                        chain_id: self.config.chain_id.clone(),
                        epoch: self.current_epoch()?,
                        height: self.height,
                        round: self.round,
                        block_id: bid,
                        stake_snapshot_hash: self.snapshot.snapshot_hash.clone(),
                        prevotes,
                    };

                    // A validator's lock is a consequence of a durable
                    // Precommit intent, not of merely observing a Polka. A
                    // late Polka may arrive after this validator has already
                    // timed out and durably signed NIL Precommit. In that
                    // case the state is already in PrecommitWait and signing
                    // a block precommit would be equivocation. Continue to
                    // retain the certificate for validation, but never emit a
                    // second local precommit for the same height/round.
                    if matches!(
                        self.step,
                        ConsensusStep::NewHeight
                            | ConsensusStep::NewRound
                            | ConsensusStep::Propose
                            | ConsensusStep::PrevoteWait
                    ) {
                        let next_safety_state = self.durable_safety_state_after(
                            ConsensusStep::PrecommitWait,
                            Some(bid),
                            Some(round),
                            Some(bid),
                            Some(round),
                            Some(certificate.clone()),
                        )?;
                        let precommit_vote = if self.local_validator_id.is_some() {
                            Some(self.cast_vote_with_state(
                                VoteStep::Precommit,
                                Some(bid),
                                signer,
                                Some(next_safety_state),
                            )?)
                        } else {
                            None
                        };
                        self.locked_block = Some(bid);
                        self.locked_round = Some(round);
                        self.valid_block = Some(bid);
                        self.valid_round = Some(round);
                        self.valid_round_certificate = Some(certificate);
                        self.step = ConsensusStep::PrecommitWait;

                        // The locks above are committed only after the local
                        // precommit intent has been durably signed and
                        // acknowledged by SafetyStore.
                        if let Some(precommit_vote) = precommit_vote {
                            return Ok((Some(precommit_vote), None));
                        }
                        self.persist_durable_safety_state()?;
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
                    self.current_epoch()?,
                    height,
                    round,
                    bid,
                    &self.snapshot,
                ) {
                    if cert.precommits.len() > self.config.max_certificate_members as usize {
                        return Err(anyhow!(
                            "CommitCertificate exceeds Genesis certificate member limit"
                        ));
                    }
                    info!(
                        "2/3 Precommits reached! CommitCertificate formed for block {:?}",
                        bid
                    );
                    self.step = ConsensusStep::Commit;
                    self.persist_durable_safety_state()?;
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
        self.cast_vote_with_state(step, block_id, signer, None)
    }

    fn cast_vote_with_state(
        &self,
        step: VoteStep,
        block_id: Option<BlockId>,
        signer: &dyn ConsensusSigner,
        state_after: Option<DurableConsensusSafetyState>,
    ) -> Result<SignedVote> {
        let intent = self.build_vote_intent(step, block_id)?;
        let signed_vote = self
            .safety_store
            .sign_vote_once_with_state(intent.request.clone(), signer, state_after)
            .map_err(|e| anyhow!("Safety store signing error: {:?}", e))?;
        let event = ConsensusEvent::VotePersisted(signed_vote);
        self.apply_consensus_event(&intent, event)
    }

    /// Build a vote intent without changing any consensus state.
    pub fn build_vote_intent(
        &self,
        step: VoteStep,
        block_id: Option<BlockId>,
    ) -> Result<VoteIntent> {
        let local_id = self
            .local_validator_id
            .ok_or_else(|| anyhow!("Local node is not a validator"))?;

        Ok(VoteIntent {
            request: VoteSignRequest {
                protocol_version: self.config.protocol_version.clone(),
                chain_id: self.config.chain_id.clone(),
                epoch: self.current_epoch()?,
                height: self.height,
                round: self.round,
                step,
                block_id,
                stake_snapshot_hash: self.snapshot.snapshot_hash.clone(),
                validator_id: local_id,
            },
        })
    }

    /// Execute the driver side of an intent. SafetyStore persists the intent
    /// before invoking the signer and persists the completion before the
    /// resulting event is acknowledged to the state machine.
    pub fn execute_action(
        &self,
        action: ConsensusAction,
        signer: &dyn ConsensusSigner,
    ) -> Result<ConsensusEvent> {
        match action {
            ConsensusAction::SignVote(intent) => {
                let signed_vote = self
                    .safety_store
                    .sign_vote_once(intent.request, signer)
                    .map_err(|e| anyhow!("Safety store signing error: {:?}", e))?;
                Ok(ConsensusEvent::VotePersisted(signed_vote))
            }
        }
    }

    /// Apply the execution acknowledgement. This validates that the signer
    /// acknowledged exactly the intent that was persisted; callers cannot
    /// substitute another block after a WAL/signing/broadcast failure.
    pub fn apply_consensus_event(
        &self,
        intent: &VoteIntent,
        event: ConsensusEvent,
    ) -> Result<SignedVote> {
        let ConsensusEvent::VotePersisted(vote) = event;
        let request = &intent.request;
        if vote.protocol_version != request.protocol_version
            || vote.chain_id != request.chain_id
            || vote.epoch != request.epoch
            || vote.height != request.height
            || vote.round != request.round
            || vote.step != request.step
            || vote.block_id != request.block_id
            || vote.stake_snapshot_hash != request.stake_snapshot_hash
            || vote.validator != request.validator_id
            || vote.signature == [0u8; 64]
        {
            return Err(anyhow!("signed vote does not acknowledge its vote intent"));
        }
        Ok(vote)
    }

    /// Proposal timeout trigger -> cast NIL Prevote
    pub fn on_timeout_propose(&mut self, signer: &dyn ConsensusSigner) -> Result<SignedVote> {
        info!(
            "Proposal timeout in round {}, casting NIL Prevote",
            self.round
        );
        let state_after = self.durable_safety_state_after(
            ConsensusStep::PrevoteWait,
            self.locked_block,
            self.locked_round,
            self.valid_block,
            self.valid_round,
            self.valid_round_certificate.clone(),
        )?;
        let vote = self.cast_vote_with_state(VoteStep::Prevote, None, signer, Some(state_after))?;
        self.step = ConsensusStep::PrevoteWait;
        Ok(vote)
    }

    /// Prevote timeout trigger -> cast NIL Precommit
    pub fn on_timeout_prevote(&mut self, signer: &dyn ConsensusSigner) -> Result<SignedVote> {
        info!(
            "Prevote timeout in round {}, casting NIL Precommit",
            self.round
        );
        let state_after = self.durable_safety_state_after(
            ConsensusStep::PrecommitWait,
            self.locked_block,
            self.locked_round,
            self.valid_block,
            self.valid_round,
            self.valid_round_certificate.clone(),
        )?;
        let vote =
            self.cast_vote_with_state(VoteStep::Precommit, None, signer, Some(state_after))?;
        self.step = ConsensusStep::PrecommitWait;
        Ok(vote)
    }

    /// Precommit timeout trigger -> advance to next round
    pub fn on_timeout_precommit(&mut self) -> Result<()> {
        let next_round = self
            .round
            .checked_add(1)
            .ok_or_else(|| anyhow!("consensus round overflow"))?;
        if next_round > self.config.max_consensus_round {
            return Err(anyhow!(
                "consensus round exceeds the protocol bound after precommit timeout"
            ));
        }
        info!(
            "Precommit timeout in round {}, advancing to round {}",
            self.round, next_round
        );
        self.start_new_round(next_round);
        self.persist_durable_safety_state()
    }

    /// Advance only after the finalized block and its consensus state have
    /// been durably accepted by the finality driver.  The next height cannot
    /// choose an arbitrary randomness value: it must use the VRF-derived
    /// value recorded by the finalized state.
    pub fn start_new_height_from_finalized(
        &mut self,
        finalized: &FinalizedConsensusState,
        snapshot: StakeSnapshot,
    ) -> Result<()> {
        if finalized.height != self.height {
            return Err(anyhow!(
                "finalized height {} does not advance current height {}",
                finalized.height,
                self.height
            ));
        }
        if finalized.active_stake_snapshot_hash != self.snapshot.snapshot_hash {
            return Err(anyhow!(
                "finalized state active snapshot does not match current snapshot"
            ));
        }
        let next_height = finalized
            .height
            .checked_add(1)
            .ok_or_else(|| anyhow!("consensus height overflow"))?;
        let expected_epoch = self.config.epoch_for_height(next_height)?;
        if snapshot.epoch != expected_epoch {
            return Err(anyhow!(
                "next snapshot epoch {} does not match height {} epoch {}",
                snapshot.epoch,
                next_height,
                expected_epoch
            ));
        }
        let parent_randomness = finalized.parent_randomness_for_height(next_height)?;
        self.start_new_height_with_timestamp(
            next_height,
            snapshot,
            parent_randomness,
            finalized.timestamp,
        );
        self.pending_validator_changes = finalized.pending_validator_changes.clone();
        self.pending_validator_changes
            .changes
            .retain(|change| change.effective_epoch > self.snapshot.epoch);
        self.persist_durable_safety_state()?;
        Ok(())
    }

    /// Restore the next consensus height from a durable finalized record during
    /// startup. Unlike the live transition, this does not assume that the
    /// in-memory state has replayed every prior height.
    pub fn restore_after_finalized(
        &mut self,
        finalized: &FinalizedConsensusState,
        snapshot: StakeSnapshot,
    ) -> Result<()> {
        let next_height = finalized
            .height
            .checked_add(1)
            .ok_or_else(|| anyhow!("consensus height overflow"))?;
        let expected_epoch = self.config.epoch_for_height(next_height)?;
        if snapshot.epoch != expected_epoch {
            return Err(anyhow!(
                "recovery snapshot epoch {} does not match height {} epoch {}",
                snapshot.epoch,
                next_height,
                expected_epoch
            ));
        }
        let parent_randomness = finalized.parent_randomness_for_height(next_height)?;
        self.start_new_height_with_timestamp(
            next_height,
            snapshot,
            parent_randomness,
            finalized.timestamp,
        );
        self.pending_validator_changes = finalized.pending_validator_changes.clone();
        self.pending_validator_changes
            .changes
            .retain(|change| change.effective_epoch > self.snapshot.epoch);
        self.persist_durable_safety_state()?;
        Ok(())
    }
}

fn validate_v2_proposal_builder_relation(
    proposal: &Proposal,
    block_header: &BlockHeader,
) -> Result<()> {
    if block_header.block_builder.0 == [0u8; 32] {
        return Err(anyhow!("V2 proposal block builder must be non-zero"));
    }
    if proposal.protocol_version.0 < 3
        && (proposal.valid_round.is_some() || proposal.valid_round_certificate.is_some())
    {
        return Err(anyhow!(
            "valid-round re-proposals require protocol version 3 or newer"
        ));
    }
    match (
        proposal.valid_round,
        proposal.valid_round_certificate.as_ref(),
    ) {
        (None, None) if proposal.proposer == block_header.block_builder => {
            if block_header.round != proposal.round {
                return Err(anyhow!(
                    "fresh V2 proposal round does not match block round"
                ));
            }
        }
        (Some(valid_round), Some(certificate))
            if valid_round < proposal.round
                && certificate.round == valid_round
                && certificate.block_id == proposal.block_id
                && block_header.round == valid_round => {}
        (None, Some(_)) => {
            return Err(anyhow!(
                "fresh V2 proposal cannot carry a valid-round certificate"
            ));
        }
        (Some(_), None) => {
            return Err(anyhow!(
                "V2 valid-round proposal is missing its certificate"
            ));
        }
        _ => {
            return Err(anyhow!("V2 proposal valid-round relation is invalid"));
        }
    }
    Ok(())
}
