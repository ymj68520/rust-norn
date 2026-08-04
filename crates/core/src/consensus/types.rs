//! Tendermint BFT Consensus Types & Integer Election Math
//!
//! Pure integer math (no f32/f64/ln) for stake qualification, proposer selection, and Tendermint state steps.

use anyhow::Result;
use norn_common::consensus_types::StakeSnapshot;
use norn_common::types::{ChainId, Hash, ProtocolVersion, ValidatorId};
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub trait ProposalSigner: Send + Sync {
    fn validator_id(&self) -> ValidatorId;
    fn sign_proposal(&self, sign_bytes: &[u8]) -> Result<[u8; 64]>;
}

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
    pub validator_update_delay: u64,
    pub unbonding_delay: u64,
    pub key_rotation_delay: u64,
    pub slashing_activation_delay: u64,
    pub timeout_propose_ms: u64,
    pub timeout_prevote_ms: u64,
    pub timeout_precommit_ms: u64,
    pub target_numerator: u64,
    pub target_denominator: u64,
    pub max_certificate_members: u32,
    pub max_future_height: u64,
    pub max_future_round: u32,
    pub max_consensus_round: u32,
    pub max_block_timestamp_step: u64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(Hash([1u8; 32])),
            epoch: 1,
            epoch_length: 1000,
            validator_update_delay: 1,
            unbonding_delay: 1,
            key_rotation_delay: 1,
            slashing_activation_delay: 1,
            timeout_propose_ms: 3000,
            timeout_prevote_ms: 2000,
            timeout_precommit_ms: 2000,
            target_numerator: 1,
            target_denominator: 1,
            max_certificate_members: 1024,
            max_future_height: 2,
            max_future_round: 2,
            max_consensus_round: 63,
            max_block_timestamp_step: 30,
        }
    }
}

impl ConsensusConfig {
    /// Return the protocol epoch for a block height.  Height one is the first
    /// block after Genesis and therefore belongs to the Genesis epoch; every
    /// subsequent complete `epoch_length` interval advances exactly once.
    pub fn epoch_for_height(&self, height: u64) -> Result<u64> {
        if self.epoch_length == 0 {
            return Err(anyhow::anyhow!("epoch length must be non-zero"));
        }
        let offset = height.saturating_sub(1) / self.epoch_length;
        self.epoch
            .checked_add(offset)
            .ok_or_else(|| anyhow::anyhow!("epoch overflow for height {}", height))
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

    /// Select deterministic proposer for a given (height, round) via cryptographic seed hashing over StakeSnapshot
    pub fn select_deterministic_proposer(
        chain_id: &ChainId,
        epoch: u64,
        height: u64,
        round: u32,
        parent_randomness: &Hash,
        snapshot: &StakeSnapshot,
    ) -> Option<ValidatorId> {
        if snapshot.validators.is_empty() {
            return None;
        }

        let total_weight = match snapshot.total_voting_power() {
            Ok(w) if w > 0 => w,
            _ => return None,
        };

        let mut hasher = Sha256::new();
        hasher.update(b"NORN_PROPOSER_V2");
        hasher.update(&chain_id.0 .0);
        hasher.update(&epoch.to_be_bytes());
        hasher.update(&height.to_be_bytes());
        hasher.update(&round.to_be_bytes());
        hasher.update(&parent_randomness.0);
        hasher.update(&snapshot.snapshot_hash.0);
        let digest = hasher.finalize();

        let seed = BigUint::from_bytes_be(&digest);
        let target_weight = (&seed % BigUint::from(total_weight)).to_u128().unwrap_or(0);

        let mut accumulated: u128 = 0;
        for (validator_id, record) in &snapshot.validators {
            if !record.is_active_at(snapshot.epoch) {
                continue;
            }
            accumulated += record.voting_power as u128;
            if target_weight < accumulated {
                return Some(*validator_id);
            }
        }

        snapshot.validators.keys().next().copied()
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

    #[test]
    fn test_epoch_schedule_matches_genesis_height_rule() {
        let config = ConsensusConfig {
            epoch: 7,
            epoch_length: 3,
            ..ConsensusConfig::default()
        };
        assert_eq!(config.epoch_for_height(1).unwrap(), 7);
        assert_eq!(config.epoch_for_height(3).unwrap(), 7);
        assert_eq!(config.epoch_for_height(4).unwrap(), 8);
        assert_eq!(config.epoch_for_height(7).unwrap(), 9);
    }
}
