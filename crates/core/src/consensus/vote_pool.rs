//! Tendermint BFT Vote Aggregation & Quorum Certificate Pool
//!
//! Aggregates Prevote and Precommit votes, computes 2/3 voting power thresholds,
//! detects equivocation (double-voting), and constructs CommitCertificates.

use k256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use norn_common::consensus_types::{CommitCertificate, SignedVote, StakeSnapshot, VoteStep};
use norn_common::types::{BlockId, ChainId, ProtocolVersion, StakeSnapshotHash, ValidatorId};
use std::collections::{BTreeMap, HashMap};
use tracing::{debug, warn};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VotePoolKey {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub step: VoteStep,
    pub stake_snapshot_hash: StakeSnapshotHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddVoteResult {
    Added,
    DuplicateVote,
    EquivocationDetected {
        validator: ValidatorId,
        existing_vote: SignedVote,
        new_vote: SignedVote,
    },
    UnknownValidator,
    InvalidSignature,
    SnapshotMismatch,
}

pub struct VotePool {
    /// Maps VotePoolKey -> Map<ValidatorId, SignedVote>
    votes: HashMap<VotePoolKey, BTreeMap<ValidatorId, SignedVote>>,
    /// Equivocation records: ValidatorId -> Vec<(SignedVote, SignedVote)>
    equivocation_records: HashMap<ValidatorId, Vec<(SignedVote, SignedVote)>>,
}

impl VotePool {
    pub fn new() -> Self {
        Self {
            votes: HashMap::new(),
            equivocation_records: HashMap::new(),
        }
    }

    /// Add and verify a vote against the stake snapshot
    pub fn add_vote(&mut self, vote: SignedVote, snapshot: &StakeSnapshot) -> AddVoteResult {
        if vote.stake_snapshot_hash != snapshot.snapshot_hash {
            warn!(
                "Vote snapshot hash mismatch for validator {:?}",
                vote.validator
            );
            return AddVoteResult::SnapshotMismatch;
        }

        let Some(record) = snapshot.validators.get(&vote.validator) else {
            warn!("Rejected vote from unknown validator {:?}", vote.validator);
            return AddVoteResult::UnknownValidator;
        };
        if !record.is_active_at(snapshot.epoch) {
            warn!(
                "Rejected vote from jailed or slashed validator {:?}",
                vote.validator
            );
            return AddVoteResult::UnknownValidator;
        }

        // Strict fail-closed secp256k1 ECDSA verification over canonical bytes
        if record.consensus_public_key.0 == [0u8; 33] || vote.signature == [0u8; 64] {
            warn!(
                "Rejected vote with zero public key or zero signature for validator {:?}",
                vote.validator
            );
            return AddVoteResult::InvalidSignature;
        }

        let Ok(verifying_key) = VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0)
        else {
            warn!(
                "Malformed SEC1 public key for validator {:?}",
                vote.validator
            );
            return AddVoteResult::InvalidSignature;
        };

        let Ok(sig) = Signature::from_slice(&vote.signature) else {
            warn!("Malformed signature for validator {:?}", vote.validator);
            return AddVoteResult::InvalidSignature;
        };

        // Reject non-canonical high-S signatures
        if sig.normalize_s().is_some() {
            warn!(
                "Non-canonical high-S signature for validator {:?}",
                vote.validator
            );
            return AddVoteResult::InvalidSignature;
        }

        let msg_bytes = vote.canonical_bytes();
        if verifying_key.verify(&msg_bytes, &sig).is_err() {
            warn!("Invalid vote signature for validator {:?}", vote.validator);
            return AddVoteResult::InvalidSignature;
        }

        let key = VotePoolKey {
            protocol_version: vote.protocol_version.clone(),
            chain_id: vote.chain_id.clone(),
            epoch: vote.epoch,
            height: vote.height,
            round: vote.round,
            step: vote.step,
            stake_snapshot_hash: vote.stake_snapshot_hash.clone(),
        };
        let step_votes = self.votes.entry(key).or_insert_with(BTreeMap::new);

        if let Some(existing) = step_votes.get(&vote.validator) {
            if existing.block_id == vote.block_id {
                return AddVoteResult::DuplicateVote;
            } else {
                warn!(
                    "Equivocation detected for validator {:?} at height {} round {:?}",
                    vote.validator, vote.height, vote.round
                );
                let eq_pair = (existing.clone(), vote.clone());
                self.equivocation_records
                    .entry(vote.validator)
                    .or_default()
                    .push(eq_pair);
                return AddVoteResult::EquivocationDetected {
                    validator: vote.validator,
                    existing_vote: existing.clone(),
                    new_vote: vote,
                };
            }
        }

        step_votes.insert(vote.validator, vote);
        AddVoteResult::Added
    }

    /// Check if a specific block_id has achieved > 2/3 total voting power for step (Prevote / Precommit)
    pub fn check_quorum(
        &self,
        protocol_version: ProtocolVersion,
        chain_id: ChainId,
        epoch: u64,
        height: u64,
        round: u32,
        step: VoteStep,
        block_id: Option<BlockId>,
        snapshot: &StakeSnapshot,
    ) -> Option<Vec<SignedVote>> {
        let key = VotePoolKey {
            protocol_version,
            chain_id,
            epoch,
            height,
            round,
            step,
            stake_snapshot_hash: snapshot.snapshot_hash.clone(),
        };
        let step_votes = self.votes.get(&key)?;

        let total_power = match snapshot.total_voting_power() {
            Ok(w) if w > 0 => w,
            _ => return None,
        };

        let mut accumulated_power: u128 = 0;
        let mut matching_votes = Vec::new();

        for (validator_id, vote) in step_votes {
            if vote.block_id == block_id {
                if let Some(record) = snapshot.validators.get(validator_id) {
                    if !record.is_active_at(snapshot.epoch) {
                        continue;
                    }
                    accumulated_power =
                        match accumulated_power.checked_add(record.voting_power as u128) {
                            Some(val) => val,
                            None => return None,
                        };
                    matching_votes.push(vote.clone());
                }
            }
        }

        // Safe overflow-checked BFT 2/3 threshold comparison: accumulated_power * 3 > total_power * 2
        let has_quorum = accumulated_power
            .checked_mul(3)
            .zip(total_power.checked_mul(2))
            .map(|(lhs, rhs)| lhs > rhs)
            .unwrap_or(false);

        if has_quorum {
            // Sort precommits deterministically by ValidatorId
            matching_votes.sort_by(|a, b| a.validator.0.cmp(&b.validator.0));
            matching_votes.dedup_by(|a, b| a.validator == b.validator);
            Some(matching_votes)
        } else {
            None
        }
    }

    /// Construct CommitCertificate if > 2/3 Precommit votes agree on a valid BlockId
    pub fn create_commit_certificate(
        &self,
        protocol_version: ProtocolVersion,
        chain_id: ChainId,
        epoch: u64,
        height: u64,
        round: u32,
        block_id: BlockId,
        snapshot: &StakeSnapshot,
    ) -> Option<CommitCertificate> {
        let precommits = self.check_quorum(
            protocol_version.clone(),
            chain_id.clone(),
            epoch,
            height,
            round,
            VoteStep::Precommit,
            Some(block_id),
            snapshot,
        )?;

        Some(CommitCertificate {
            protocol_version,
            chain_id,
            epoch,
            height,
            round,
            block_id,
            stake_snapshot_hash: snapshot.snapshot_hash.clone(),
            precommits,
        })
    }

    pub fn get_equivocations(
        &self,
        validator: &ValidatorId,
    ) -> Option<&Vec<(SignedVote, SignedVote)>> {
        self.equivocation_records.get(validator)
    }

    pub fn clear_old_heights(&mut self, current_height: u64) {
        self.votes.retain(|key, _| key.height >= current_height);
    }
}

impl Default for VotePool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::signature::Signer;
    use k256::ecdsa::SigningKey;
    use rand::thread_rng;

    #[test]
    fn test_quorum_calculation_with_real_signatures() {
        let mut snapshot = StakeSnapshot::default();
        let val1 = ValidatorId([1u8; 32]);
        let val2 = ValidatorId([2u8; 32]);
        let val3 = ValidatorId([3u8; 32]);

        let key1 = SigningKey::random(&mut thread_rng());
        let key2 = SigningKey::random(&mut thread_rng());
        let key3 = SigningKey::random(&mut thread_rng());

        let pk1_bytes: [u8; 33] = key1
            .verifying_key()
            .to_sec1_bytes()
            .as_ref()
            .try_into()
            .unwrap();
        let pk2_bytes: [u8; 33] = key2
            .verifying_key()
            .to_sec1_bytes()
            .as_ref()
            .try_into()
            .unwrap();
        let pk3_bytes: [u8; 33] = key3
            .verifying_key()
            .to_sec1_bytes()
            .as_ref()
            .try_into()
            .unwrap();

        snapshot.validators.insert(
            val1,
            norn_common::consensus_types::ValidatorRecord {
                validator_id: val1,
                consensus_public_key: norn_common::types::ConsensusPublicKey(pk1_bytes),
                vrf_public_key: norn_common::types::VrfPublicKey::default(),
                voting_power: 10,
                jailed_until_epoch: None,
                slashed: false,
            },
        );
        snapshot.validators.insert(
            val2,
            norn_common::consensus_types::ValidatorRecord {
                validator_id: val2,
                consensus_public_key: norn_common::types::ConsensusPublicKey(pk2_bytes),
                vrf_public_key: norn_common::types::VrfPublicKey::default(),
                voting_power: 10,
                jailed_until_epoch: None,
                slashed: false,
            },
        );
        snapshot.validators.insert(
            val3,
            norn_common::consensus_types::ValidatorRecord {
                validator_id: val3,
                consensus_public_key: norn_common::types::ConsensusPublicKey(pk3_bytes),
                vrf_public_key: norn_common::types::VrfPublicKey::default(),
                voting_power: 10,
                jailed_until_epoch: None,
                slashed: false,
            },
        );

        snapshot.snapshot_hash = snapshot.compute_hash();

        let mut pool = VotePool::new();
        let bid = BlockId(norn_common::types::Hash([9u8; 32]));

        let mut make_vote = |val: ValidatorId, key: &SigningKey| {
            let mut v = SignedVote {
                protocol_version: norn_common::types::ProtocolVersion(2),
                chain_id: norn_common::types::ChainId::default(),
                epoch: 1,
                height: 1,
                round: 0,
                step: VoteStep::Precommit,
                block_id: Some(bid),
                stake_snapshot_hash: snapshot.snapshot_hash.clone(),
                validator: val,
                signature: [0u8; 64],
            };
            let sign_bytes = v.canonical_bytes();
            let sig: Signature = key.sign(&sign_bytes);
            let sig_canonical = sig.normalize_s().unwrap_or(sig);
            let bytes_ref = sig_canonical.to_bytes();
            v.signature = bytes_ref.as_slice().try_into().unwrap();
            v
        };

        let v1 = make_vote(val1, &key1);
        let v2 = make_vote(val2, &key2);
        let v3 = make_vote(val3, &key3);

        assert_eq!(pool.add_vote(v1, &snapshot), AddVoteResult::Added);
        assert_eq!(pool.add_vote(v2, &snapshot), AddVoteResult::Added);

        assert!(pool
            .create_commit_certificate(
                norn_common::types::ProtocolVersion(2),
                norn_common::types::ChainId::default(),
                1,
                1,
                0,
                bid,
                &snapshot,
            )
            .is_none());

        assert_eq!(pool.add_vote(v3, &snapshot), AddVoteResult::Added);
        assert!(pool
            .create_commit_certificate(
                norn_common::types::ProtocolVersion(2),
                norn_common::types::ChainId::default(),
                1,
                1,
                0,
                bid,
                &snapshot,
            )
            .is_some());
    }
}
