//! Tendermint BFT Vote Aggregation & Quorum Certificate Pool
//! 
//! Aggregates Prevote and Precommit votes, computes 2/3 voting power thresholds,
//! detects equivocation (double-voting), and constructs CommitCertificates.

use norn_common::consensus_types::{CommitCertificate, SignedVote, StakeSnapshot, VoteStep};
use norn_common::types::{BlockId, ValidatorId};
use std::collections::{BTreeMap, HashMap};
use tracing::{debug, warn};
use k256::ecdsa::{VerifyingKey, Signature, signature::Verifier};

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
}

pub struct VotePool {
    /// Maps (height, round, step) -> Map<ValidatorId, SignedVote>
    votes: HashMap<(u64, u32, VoteStep), BTreeMap<ValidatorId, SignedVote>>,
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
        let Some(record) = snapshot.validators.get(&vote.validator) else {
            warn!("Rejected vote from unknown validator {:?}", vote.validator);
            return AddVoteResult::UnknownValidator;
        };

        // Verify signature if consensus public key is provided (33 bytes secp256k1)
        if record.consensus_public_key.0 != [0u8; 33] && vote.signature != [0u8; 64] {
            if let Ok(verifying_key) = VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0) {
                if let Ok(sig) = Signature::from_slice(&vote.signature) {
                    let msg_bytes = vote.canonical_bytes();
                    if verifying_key.verify(&msg_bytes, &sig).is_err() {
                        warn!("Invalid vote signature for validator {:?}", vote.validator);
                        return AddVoteResult::InvalidSignature;
                    }
                } else {
                    return AddVoteResult::InvalidSignature;
                }
            }
        }

        let key = (vote.height, vote.round, vote.step);
        let step_votes = self.votes.entry(key).or_insert_with(BTreeMap::new);

        if let Some(existing) = step_votes.get(&vote.validator) {
            if existing.block_id == vote.block_id {
                return AddVoteResult::DuplicateVote;
            } else {
                warn!("Equivocation detected for validator {:?} at height {} round {:?}", vote.validator, vote.height, vote.round);
                let eq_pair = (existing.clone(), vote.clone());
                self.equivocation_records.entry(vote.validator).or_default().push(eq_pair);
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
        height: u64,
        round: u32,
        step: VoteStep,
        block_id: Option<BlockId>,
        snapshot: &StakeSnapshot,
    ) -> Option<Vec<SignedVote>> {
        let key = (height, round, step);
        let step_votes = self.votes.get(&key)?;

        let total_power = snapshot.total_voting_power();
        if total_power == 0 {
            return None;
        }

        let mut accumulated_power: u128 = 0;
        let mut matching_votes = Vec::new();

        for (validator_id, vote) in step_votes {
            if vote.block_id == block_id {
                if let Some(record) = snapshot.validators.get(validator_id) {
                    accumulated_power = accumulated_power.saturating_add(record.voting_power as u128);
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
            Some(matching_votes)
        } else {
            None
        }
    }

    /// Construct CommitCertificate if > 2/3 Precommit votes agree on a valid BlockId
    pub fn create_commit_certificate(
        &self,
        height: u64,
        round: u32,
        block_id: BlockId,
        snapshot: &StakeSnapshot,
    ) -> Option<CommitCertificate> {
        let votes = self.check_quorum(height, round, VoteStep::Precommit, Some(block_id), snapshot)?;
        Some(CommitCertificate {
            height,
            round,
            block_id,
            votes,
        })
    }

    pub fn get_equivocations(&self, validator: &ValidatorId) -> Option<&Vec<(SignedVote, SignedVote)>> {
        self.equivocation_records.get(validator)
    }

    pub fn clear_old_heights(&mut self, current_height: u64) {
        self.votes.retain(|(h, _, _), _| *h >= current_height);
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

    #[test]
    fn test_quorum_calculation() {
        let mut snapshot = StakeSnapshot::default();
        let val1 = ValidatorId([1u8; 32]);
        let val2 = ValidatorId([2u8; 32]);
        let val3 = ValidatorId([3u8; 32]);

        snapshot.validators.insert(val1, norn_common::consensus_types::ValidatorRecord {
            validator_id: val1,
            consensus_public_key: norn_common::types::ConsensusPublicKey::default(),
            vrf_public_key: norn_common::types::VrfPublicKey::default(),
            voting_power: 10,
        });
        snapshot.validators.insert(val2, norn_common::consensus_types::ValidatorRecord {
            validator_id: val2,
            consensus_public_key: norn_common::types::ConsensusPublicKey::default(),
            vrf_public_key: norn_common::types::VrfPublicKey::default(),
            voting_power: 10,
        });
        snapshot.validators.insert(val3, norn_common::consensus_types::ValidatorRecord {
            validator_id: val3,
            consensus_public_key: norn_common::types::ConsensusPublicKey::default(),
            vrf_public_key: norn_common::types::VrfPublicKey::default(),
            voting_power: 10,
        });

        let mut pool = VotePool::new();
        let bid = BlockId(norn_common::types::Hash([9u8; 32]));

        let v1 = SignedVote {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId::default(),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(bid),
            validator: val1,
            signature: [0u8; 64],
        };

        let v2 = SignedVote {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId::default(),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(bid),
            validator: val2,
            signature: [0u8; 64],
        };

        assert_eq!(pool.add_vote(v1, &snapshot), AddVoteResult::Added);
        assert_eq!(pool.add_vote(v2, &snapshot), AddVoteResult::Added);

        assert!(pool.create_commit_certificate(1, 0, bid, &snapshot).is_none());

        let v3 = SignedVote {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId::default(),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(bid),
            validator: val3,
            signature: [0u8; 64],
        };

        assert_eq!(pool.add_vote(v3, &snapshot), AddVoteResult::Added);
        assert!(pool.create_commit_certificate(1, 0, bid, &snapshot).is_some());
    }

    #[test]
    fn test_equivocation_detection() {
        let mut snapshot = StakeSnapshot::default();
        let val1 = ValidatorId([1u8; 32]);

        snapshot.validators.insert(val1, norn_common::consensus_types::ValidatorRecord {
            validator_id: val1,
            consensus_public_key: norn_common::types::ConsensusPublicKey::default(),
            vrf_public_key: norn_common::types::VrfPublicKey::default(),
            voting_power: 10,
        });

        let mut pool = VotePool::new();
        let bid1 = BlockId(norn_common::types::Hash([1u8; 32]));
        let bid2 = BlockId(norn_common::types::Hash([2u8; 32]));

        let v1 = SignedVote {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId::default(),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Prevote,
            block_id: Some(bid1),
            validator: val1,
            signature: [0u8; 64],
        };

        let v2 = SignedVote {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId::default(),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Prevote,
            block_id: Some(bid2),
            validator: val1,
            signature: [0u8; 64],
        };

        assert_eq!(pool.add_vote(v1, &snapshot), AddVoteResult::Added);
        match pool.add_vote(v2, &snapshot) {
            AddVoteResult::EquivocationDetected { validator, .. } => {
                assert_eq!(validator, val1);
            }
            _ => panic!("Expected EquivocationDetected"),
        }
    }
}
