use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use crate::types::{
    Block, BlockId, ChainId, ConsensusPublicKey, Hash, ProtocolVersion, StakeSnapshotHash,
    ValidatorId, VrfPublicKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoteStep {
    Prevote,
    Precommit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub valid_round: Option<u32>,
    pub block_id: BlockId,
    pub proposer: ValidatorId,
    pub vrf_preout: [u8; 32],
    #[serde(with = "crate::types::hex_serde_fixed_64")]
    pub vrf_proof: [u8; 64],
    #[serde(with = "crate::types::hex_serde_fixed_64")]
    pub signature: [u8; 64],
}

impl Proposal {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NORN_BFT_V1_PROPOSAL");
        bytes.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        bytes.extend_from_slice(&self.chain_id.0.0);
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.height.to_be_bytes());
        bytes.extend_from_slice(&self.round.to_be_bytes());
        if let Some(vr) = self.valid_round {
            bytes.push(1);
            bytes.extend_from_slice(&vr.to_be_bytes());
        } else {
            bytes.push(0);
        }
        bytes.extend_from_slice(&self.block_id.0.0);
        bytes.extend_from_slice(&self.proposer.0);
        bytes.extend_from_slice(&self.vrf_preout);
        bytes.extend_from_slice(&self.vrf_proof);
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedVote {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub step: VoteStep,
    pub block_id: Option<BlockId>,
    pub validator: ValidatorId,
    #[serde(with = "crate::types::hex_serde_fixed_64")]
    pub signature: [u8; 64],
}

impl SignedVote {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NORN_BFT_V1_VOTE");
        bytes.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        bytes.extend_from_slice(&self.chain_id.0.0);
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.height.to_be_bytes());
        bytes.extend_from_slice(&self.round.to_be_bytes());
        match self.step {
            VoteStep::Prevote => bytes.push(1),
            VoteStep::Precommit => bytes.push(2),
        }
        if let Some(bid) = &self.block_id {
            bytes.push(1);
            bytes.extend_from_slice(&bid.0.0);
        } else {
            bytes.push(0);
        }
        bytes.extend_from_slice(&self.validator.0);
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitCertificate {
    pub height: u64,
    pub round: u32,
    pub block_id: BlockId,
    pub votes: Vec<SignedVote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizedBlock {
    pub block: Block,
    pub commit: CommitCertificate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockEnvelope {
    pub block: Block,
    pub commit: Option<CommitCertificate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorRecord {
    pub validator_id: ValidatorId,
    pub consensus_public_key: ConsensusPublicKey,
    pub vrf_public_key: VrfPublicKey,
    pub voting_power: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StakeSnapshot {
    pub epoch: u64,
    /// BTreeMap sorted by ValidatorId for deterministic ordering
    pub validators: BTreeMap<ValidatorId, ValidatorRecord>,
    pub snapshot_hash: StakeSnapshotHash,
}

impl StakeSnapshot {
    pub fn total_voting_power(&self) -> u128 {
        self.validators
            .values()
            .fold(0u128, |acc, v| acc.saturating_add(v.voting_power as u128))
    }

    pub fn compute_hash(&self) -> StakeSnapshotHash {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"NORN_STAKE_SNAPSHOT_V1");
        hasher.update(&self.epoch.to_be_bytes());
        for (vid, record) in &self.validators {
            hasher.update(&vid.0);
            hasher.update(&record.consensus_public_key.0);
            hasher.update(&record.vrf_public_key.0);
            hasher.update(&record.voting_power.to_be_bytes());
        }
        let res = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&res);
        StakeSnapshotHash(arr)
    }
}
