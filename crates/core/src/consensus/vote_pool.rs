//! Tendermint BFT Vote Aggregation & Quorum Certificate Pool
//! 
//! Aggregates Prevote and Precommit votes, computes 2/3 voting power thresholds,
//! and constructs CommitCertificates.

use norn_common::consensus_types::{CommitCertificate, SignedVote, StakeSnapshot, VoteStep};
use norn_common::types::{BlockId, ValidatorId};
use std::collections::{BTreeMap, HashMap};
use tracing::{debug, info};

pub struct VotePool {
    /// Maps (height, round, step) -> Map<ValidatorId, SignedVote>
    votes: HashMap<(u64, u32, VoteStep), BTreeMap<ValidatorId, SignedVote>>,
}

impl VotePool {
    pub fn new() -> Self {
        Self {
            votes: HashMap::new(),
        }
    }

    pub fn add_vote(&mut self, vote: SignedVote) -> bool {
        let key = (vote.height, vote.round, vote.step);
        let step_votes = self.votes.entry(key).or_insert_with(BTreeMap::new);
        
        if step_votes.contains_key(&vote.validator) {
            return false; // Already recorded
        }

        step_votes.insert(vote.validator, vote);
        true
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
                    accumulated_power += record.voting_power as u128;
                    matching_votes.push(vote.clone());
                }
            }
        }

        // BFT 2/3 threshold: accumulated_power * 3 > total_power * 2
        if accumulated_power * 3 > total_power * 2 {
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
        }); // Total power = 30. Quorum > 20 (requires at least 21, i.e., 3 validators of 10)

        let mut pool = VotePool::new();
        let bid = BlockId(norn_common::types::Hash([9u8; 32]));

        pool.add_vote(SignedVote {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId::default(),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(bid),
            validator: val1,
            signature: [0u8; 64],
        });

        pool.add_vote(SignedVote {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId::default(),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(bid),
            validator: val2,
            signature: [0u8; 64],
        });

        // 20 / 30 = 66.67%, 20 * 3 = 60, 30 * 2 = 60. 60 > 60 is false (requires > 2/3)
        assert!(pool.create_commit_certificate(1, 0, bid, &snapshot).is_none());

        // Add 3rd vote (30 / 30 = 100% > 2/3)
        pool.add_vote(SignedVote {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId::default(),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(bid),
            validator: val3,
            signature: [0u8; 64],
        });

        assert!(pool.create_commit_certificate(1, 0, bid, &snapshot).is_some());
    }
}
