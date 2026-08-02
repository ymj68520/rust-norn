//! Tendermint BFT Consensus Types & Integer Election Math
//! 
//! Pure integer math (no f32/f64/ln) for stake qualification and Tendermint state steps.

use num_bigint::BigUint;
use norn_common::consensus_types::StakeSnapshot;
use norn_common::types::{ChainId, Hash, ProtocolVersion, ValidatorId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ConsensusStep {
    #[default]
    NewHeight,
    NewRound,
    Propose,
    Prevote,
    PrevoteWait,
    Precommit,
    PrecommitWait,
    Commit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub epoch_length: u64,
    pub timeout_propose_ms: u64,
    pub timeout_prevote_ms: u64,
    pub timeout_precommit_ms: u64,
    pub target_numerator: u64,
    pub target_denominator: u64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(Hash([1u8; 32])),
            epoch: 1,
            epoch_length: 1000,
            timeout_propose_ms: 3000,
            timeout_prevote_ms: 2000,
            timeout_precommit_ms: 2000,
            target_numerator: 1,
            target_denominator: 1,
        }
    }
}

/// Pure integer math for VRF threshold selection and Proposer scheduling
pub struct ElectionMath;

impl ElectionMath {
    /// Verify whether a validator's 256-bit VRF score `y` satisfies threshold:
    /// y * denominator * total_stake < 2^256 * numerator * validator_stake
    pub fn verify_qualification(
        score_bytes: &[u8; 32],
        validator_stake: u64,
        total_stake: u128,
        numerator: u64,
        denominator: u64,
    ) -> bool {
        if validator_stake == 0 || total_stake == 0 || denominator == 0 {
            return false;
        }

        let y = BigUint::from_bytes_be(score_bytes);
        let num = BigUint::from(numerator);
        let den = BigUint::from(denominator);
        let v_stake = BigUint::from(validator_stake);
        let t_stake = BigUint::from(total_stake);

        let two_pow_256 = BigUint::from(1u32) << 256;

        let left = &y * &den * &t_stake;
        let right = &two_pow_256 * &num * &v_stake;

        left < right
    }

    /// Select deterministic proposer for a given (height, round) via weighted round-robin on StakeSnapshot
    pub fn select_deterministic_proposer(
        snapshot: &StakeSnapshot,
        height: u64,
        round: u32,
    ) -> Option<ValidatorId> {
        if snapshot.validators.is_empty() {
            return None;
        }

        let total_weight = snapshot.total_voting_power();
        if total_weight == 0 {
            return None;
        }

        // Mix height and round to ensure round 0 at different heights rotates proposer
        let seed = (height as u128).wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(round as u128);
        let target_weight = seed % total_weight;
        let mut accumulated: u128 = 0;

        for (validator_id, record) in &snapshot.validators {
            accumulated += record.voting_power as u128;
            if target_weight < accumulated {
                return Some(*validator_id);
            }
        }

        snapshot.validators.keys().next().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_qualification_pure_integer() {
        let mut score = [0u8; 32];
        score[31] = 1; // Very small score

        let qualified = ElectionMath::verify_qualification(&score, 100, 1000, 1, 1);
        assert!(qualified);

        let max_score = [0xFFu8; 32];
        let not_qualified = ElectionMath::verify_qualification(&max_score, 100, 1000, 1, 1);
        assert!(!not_qualified);
    }
}
