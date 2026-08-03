use crate::chain_context::{protocol_error, ChainContext};
use crate::error::{NornError, Result};
use crate::types::{
    Block, BlockId, BlockV2, ChainId, ConsensusPublicKey, Hash, ProtocolVersion, StakeSnapshotHash,
    ValidatorId, VrfPublicKey,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoteStep {
    Prevote,
    Precommit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrevoteCertificate {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub block_id: BlockId,
    pub stake_snapshot_hash: StakeSnapshotHash,
    pub prevotes: Vec<SignedVote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub valid_round: Option<u32>,
    pub valid_round_certificate: Option<PrevoteCertificate>,
    pub block_id: BlockId,
    pub parent_block_hash: Hash,
    pub stake_snapshot_hash: StakeSnapshotHash,
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
        bytes.extend_from_slice(b"NORN_BFT_V2_PROPOSAL");
        bytes.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        bytes.extend_from_slice(&self.chain_id.0 .0);
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.height.to_be_bytes());
        bytes.extend_from_slice(&self.round.to_be_bytes());
        if let Some(vr) = self.valid_round {
            bytes.push(1);
            bytes.extend_from_slice(&vr.to_be_bytes());
        } else {
            bytes.push(0);
        }
        bytes.extend_from_slice(&self.block_id.0 .0);
        bytes.extend_from_slice(&self.parent_block_hash.0);
        bytes.extend_from_slice(&self.stake_snapshot_hash.0);
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
    pub stake_snapshot_hash: StakeSnapshotHash,
    pub validator: ValidatorId,
    #[serde(with = "crate::types::hex_serde_fixed_64")]
    pub signature: [u8; 64],
}

impl SignedVote {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        match self.step {
            VoteStep::Prevote => bytes.extend_from_slice(b"NORN_BFT_V2_PREVOTE"),
            VoteStep::Precommit => bytes.extend_from_slice(b"NORN_BFT_V2_PRECOMMIT"),
        }
        bytes.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        bytes.extend_from_slice(&self.chain_id.0 .0);
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.height.to_be_bytes());
        bytes.extend_from_slice(&self.round.to_be_bytes());
        if let Some(bid) = &self.block_id {
            bytes.push(1);
            bytes.extend_from_slice(&bid.0 .0);
        } else {
            bytes.push(0);
        }
        bytes.extend_from_slice(&self.stake_snapshot_hash.0);
        bytes.extend_from_slice(&self.validator.0);
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitCertificate {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub block_id: BlockId,
    pub stake_snapshot_hash: StakeSnapshotHash,
    pub precommits: Vec<SignedVote>,
}

impl CommitCertificate {
    /// Canonical bytes used when a finalized certificate becomes part of the
    /// durable consensus state.  The member order is normalized here as well
    /// as when certificates are produced, so a reordered wire certificate
    /// cannot produce a different state hash.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(128 + self.precommits.len() * 256);
        bytes.extend_from_slice(b"NORN_COMMIT_CERTIFICATE_V2");
        bytes.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        bytes.extend_from_slice(&self.chain_id.0 .0);
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.height.to_be_bytes());
        bytes.extend_from_slice(&self.round.to_be_bytes());
        bytes.extend_from_slice(&self.block_id.0 .0);
        bytes.extend_from_slice(&self.stake_snapshot_hash.0);

        let mut precommits = self.precommits.iter().collect::<Vec<_>>();
        precommits.sort_by_key(|vote| vote.validator);
        bytes.extend_from_slice(&(precommits.len() as u32).to_be_bytes());
        for vote in precommits {
            let vote_bytes = vote.canonical_bytes();
            bytes.extend_from_slice(&(vote_bytes.len() as u32).to_be_bytes());
            bytes.extend_from_slice(&vote_bytes);
            bytes.extend_from_slice(&vote.signature);
        }
        bytes
    }

    pub fn certificate_hash(&self) -> Hash {
        use sha2::{Digest, Sha256};
        Hash(Sha256::digest(self.canonical_bytes()).into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusMessage {
    Proposal {
        proposal: Proposal,
        block: Block,
    },
    ProposalV2 {
        proposal: Proposal,
        block: BlockV2,
    },
    /// Request a V2 block/proposal pair needed to verify a Commit received
    /// after a restart or while the original proposal was not observed.
    BlockRequest {
        height: u64,
        block_id: BlockId,
    },
    /// Response to BlockRequest. The proposal is carried as well as the block
    /// because the VRF-derived next randomness is not recoverable from a
    /// block header alone.
    BlockResponse {
        proposal: Proposal,
        block: BlockV2,
    },
    /// Request one durable V2 finalized record by height. FullNodes use this
    /// after restart to catch up through heights whose Commit broadcasts were
    /// missed while they were offline.
    FinalityRequest {
        height: u64,
    },
    /// Response to FinalityRequest. The durable record contains the exact
    /// proposal/block pair and certificate required for verify-only replay.
    FinalityResponse {
        finalized: FinalizedBlockV2,
    },
    Vote(SignedVote),
    Commit(CommitCertificate),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusEnvelope {
    pub wire_version: u16,
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub genesis_hash: Hash,
    pub payload: ConsensusMessage,
}

/// Hard wire ceilings prevent an attacker from forcing unbounded allocation
/// before protocol-level Genesis limits are applied by later validation.
pub const MAX_CONSENSUS_ENVELOPE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CONSENSUS_CERTIFICATE_VOTES: usize = 1024;

impl ConsensusEnvelope {
    /// Validate an already decoded envelope against the local chain identity.
    /// This checks only wire/context invariants; cryptographic membership,
    /// quorum, and full block execution validation remain separate steps.
    pub fn validate_for_context(&self, context: &ChainContext) -> Result<()> {
        if self.wire_version != context.wire_version {
            return Err(protocol_error("consensus envelope wire version mismatch"));
        }
        if self.protocol_version != context.protocol_version {
            return Err(protocol_error(
                "consensus envelope protocol version mismatch",
            ));
        }
        if self.chain_id != context.chain_id {
            return Err(protocol_error("consensus envelope chain ID mismatch"));
        }
        if self.genesis_hash != context.genesis_hash {
            return Err(protocol_error("consensus envelope Genesis hash mismatch"));
        }

        match &self.payload {
            ConsensusMessage::Proposal { proposal, block } => {
                validate_shared_context(
                    proposal.protocol_version,
                    proposal.chain_id,
                    proposal.epoch,
                    proposal.height,
                    proposal.round,
                    proposal.stake_snapshot_hash,
                    self,
                )?;

                if proposal.block_id != BlockId(block.header.block_hash) {
                    return Err(protocol_error(
                        "proposal block ID does not match block header",
                    ));
                }
                if proposal.parent_block_hash != block.header.prev_block_hash {
                    return Err(protocol_error(
                        "proposal parent hash does not match block header",
                    ));
                }
                if proposal.protocol_version != block.header.protocol_version
                    || proposal.chain_id != block.header.chain_id
                    || proposal.epoch != block.header.epoch
                    || proposal.round != block.header.round
                    || proposal.stake_snapshot_hash != block.header.stake_snapshot_hash
                    || proposal.proposer != block.header.proposer
                {
                    return Err(protocol_error("proposal and block contexts do not match"));
                }
                if block.header.height < 0 || proposal.height != block.header.height as u64 {
                    return Err(protocol_error(
                        "proposal height does not match block header",
                    ));
                }
                if block.header.block_hash == Hash::default()
                    || proposal.proposer.0 == [0u8; 32]
                    || proposal.stake_snapshot_hash.0 == [0u8; 32]
                    || proposal.vrf_preout == [0u8; 32]
                    || proposal.vrf_proof == [0u8; 64]
                    || proposal.signature == [0u8; 64]
                {
                    return Err(protocol_error(
                        "proposal contains a zero identity or proof field",
                    ));
                }

                if proposal.valid_round.is_some() && proposal.valid_round_certificate.is_none() {
                    return Err(protocol_error(
                        "proposal valid round is missing its certificate",
                    ));
                }
                if let Some(certificate) = &proposal.valid_round_certificate {
                    validate_prevote_certificate(certificate, proposal)?;
                }
            }
            ConsensusMessage::ProposalV2 { proposal, block } => {
                validate_shared_context(
                    proposal.protocol_version,
                    proposal.chain_id,
                    proposal.epoch,
                    proposal.height,
                    proposal.round,
                    proposal.stake_snapshot_hash,
                    self,
                )?;
                if proposal.block_id != BlockId(block.header.block_hash)
                    || proposal.parent_block_hash != block.header.prev_block_hash
                    || proposal.protocol_version != block.header.protocol_version
                    || proposal.chain_id != block.header.chain_id
                    || proposal.epoch != block.header.epoch
                    || proposal.round != block.header.round
                    || proposal.stake_snapshot_hash != block.header.stake_snapshot_hash
                    || proposal.proposer != block.header.proposer
                    || block.header.height < 0
                    || proposal.height != block.header.height as u64
                {
                    return Err(protocol_error(
                        "V2 proposal and block contexts do not match",
                    ));
                }
                // The node applies its Genesis-specific limits after this
                // context-only envelope check. The default here catches
                // malformed roots/hashes before any consensus allocation.
                block.validate_structure(
                    &ChainContext {
                        wire_version: self.wire_version,
                        genesis_schema_version: 0,
                        protocol_version: self.protocol_version,
                        chain_id: self.chain_id,
                        genesis_hash: self.genesis_hash,
                    },
                    &crate::genesis::ProtocolResourceLimits::default(),
                )?;
                if proposal.proposer.0 == [0u8; 32]
                    || proposal.stake_snapshot_hash.0 == [0u8; 32]
                    || proposal.vrf_preout == [0u8; 32]
                    || proposal.vrf_proof == [0u8; 64]
                    || proposal.signature == [0u8; 64]
                {
                    return Err(protocol_error(
                        "V2 proposal contains a zero identity or proof field",
                    ));
                }
                if proposal.valid_round.is_some() && proposal.valid_round_certificate.is_none() {
                    return Err(protocol_error(
                        "V2 proposal valid round is missing its certificate",
                    ));
                }
                if let Some(certificate) = &proposal.valid_round_certificate {
                    validate_prevote_certificate(certificate, proposal)?;
                }
            }
            ConsensusMessage::BlockRequest { height, block_id } => {
                if *height == 0 || *block_id == BlockId(Hash::default()) {
                    return Err(protocol_error(
                        "V2 block request has an invalid height or block ID",
                    ));
                }
            }
            ConsensusMessage::BlockResponse { proposal, block } => {
                validate_shared_context(
                    proposal.protocol_version,
                    proposal.chain_id,
                    proposal.epoch,
                    proposal.height,
                    proposal.round,
                    proposal.stake_snapshot_hash,
                    self,
                )?;
                if proposal.block_id != BlockId(block.header.block_hash)
                    || proposal.parent_block_hash != block.header.prev_block_hash
                    || proposal.protocol_version != block.header.protocol_version
                    || proposal.chain_id != block.header.chain_id
                    || proposal.epoch != block.header.epoch
                    || proposal.round != block.header.round
                    || proposal.stake_snapshot_hash != block.header.stake_snapshot_hash
                    || proposal.proposer != block.header.proposer
                    || block.header.height < 0
                    || proposal.height != block.header.height as u64
                {
                    return Err(protocol_error(
                        "V2 block response proposal and block contexts do not match",
                    ));
                }
                block.validate_structure(
                    &ChainContext {
                        wire_version: self.wire_version,
                        genesis_schema_version: 0,
                        protocol_version: self.protocol_version,
                        chain_id: self.chain_id,
                        genesis_hash: self.genesis_hash,
                    },
                    &crate::genesis::ProtocolResourceLimits::default(),
                )?;
                if proposal.proposer.0 == [0u8; 32]
                    || proposal.stake_snapshot_hash.0 == [0u8; 32]
                    || proposal.vrf_preout == [0u8; 32]
                    || proposal.vrf_proof == [0u8; 64]
                    || proposal.signature == [0u8; 64]
                {
                    return Err(protocol_error(
                        "V2 block response contains a zero identity or proof field",
                    ));
                }
            }
            ConsensusMessage::FinalityRequest { height } => {
                if *height == 0 {
                    return Err(protocol_error("V2 finality request has an invalid height"));
                }
            }
            ConsensusMessage::FinalityResponse { finalized } => {
                validate_finalized_v2(finalized, self)?;
            }
            ConsensusMessage::Vote(vote) => {
                validate_shared_context(
                    vote.protocol_version,
                    vote.chain_id,
                    vote.epoch,
                    vote.height,
                    vote.round,
                    vote.stake_snapshot_hash,
                    self,
                )?;
                if vote.validator.0 == [0u8; 32]
                    || vote.stake_snapshot_hash.0 == [0u8; 32]
                    || vote.signature == [0u8; 64]
                {
                    return Err(protocol_error(
                        "vote contains a zero identity, snapshot, or signature",
                    ));
                }
                if vote.block_id == Some(BlockId(Hash::default())) {
                    return Err(protocol_error("vote contains a zero block ID"));
                }
            }
            ConsensusMessage::Commit(certificate) => {
                validate_shared_context(
                    certificate.protocol_version,
                    certificate.chain_id,
                    certificate.epoch,
                    certificate.height,
                    certificate.round,
                    certificate.stake_snapshot_hash,
                    self,
                )?;
                if certificate.block_id == BlockId(Hash::default())
                    || certificate.stake_snapshot_hash.0 == [0u8; 32]
                    || certificate.precommits.is_empty()
                {
                    return Err(protocol_error(
                        "commit certificate has an invalid identity or no votes",
                    ));
                }
                if certificate.precommits.len() > MAX_CONSENSUS_CERTIFICATE_VOTES {
                    return Err(protocol_error("commit certificate exceeds wire vote limit"));
                }
                validate_precommit_order(certificate)?;
                for vote in &certificate.precommits {
                    validate_vote_context(vote, certificate)?;
                    if vote.step != VoteStep::Precommit
                        || vote.block_id != Some(certificate.block_id)
                        || vote.validator.0 == [0u8; 32]
                        || vote.signature == [0u8; 64]
                    {
                        return Err(protocol_error(
                            "commit certificate contains an invalid precommit",
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Enforce the pre-decode byte ceiling and then validate the decoded
    /// envelope against the local chain identity.
    pub fn decode_and_validate(bytes: &[u8], context: &ChainContext) -> Result<Self> {
        if bytes.is_empty() {
            return Err(protocol_error("empty consensus envelope"));
        }
        if bytes.len() > MAX_CONSENSUS_ENVELOPE_BYTES {
            return Err(protocol_error("consensus envelope exceeds wire byte limit"));
        }
        let envelope = bincode::deserialize::<Self>(bytes)
            .map_err(|e| crate::error::NornError::Serialization(e.to_string()))?;
        envelope.validate_for_context(context)?;
        let canonical = bincode::serialize(&envelope)
            .map_err(|e| crate::error::NornError::Serialization(e.to_string()))?;
        if canonical != bytes {
            return Err(protocol_error("non-canonical consensus envelope encoding"));
        }
        Ok(envelope)
    }
}

fn validate_shared_context(
    protocol_version: ProtocolVersion,
    chain_id: ChainId,
    epoch: u64,
    height: u64,
    round: u32,
    snapshot_hash: StakeSnapshotHash,
    envelope: &ConsensusEnvelope,
) -> Result<()> {
    if protocol_version != envelope.protocol_version || chain_id != envelope.chain_id {
        return Err(protocol_error("payload protocol or chain ID mismatch"));
    }
    if snapshot_hash.0 == [0u8; 32] {
        return Err(protocol_error("payload snapshot hash must be non-zero"));
    }
    if height == 0 {
        return Err(protocol_error("consensus payload height must be non-zero"));
    }
    let _ = (epoch, round);
    Ok(())
}

fn validate_finalized_v2(finalized: &FinalizedBlockV2, envelope: &ConsensusEnvelope) -> Result<()> {
    let proposal = &finalized.proposal;
    let block = &finalized.block;
    let certificate = &finalized.commit;
    let consensus_state = &finalized.consensus_state;

    validate_shared_context(
        proposal.protocol_version,
        proposal.chain_id,
        proposal.epoch,
        proposal.height,
        proposal.round,
        proposal.stake_snapshot_hash,
        envelope,
    )?;
    if proposal.block_id != BlockId(block.header.block_hash)
        || proposal.parent_block_hash != block.header.prev_block_hash
        || proposal.protocol_version != block.header.protocol_version
        || proposal.chain_id != block.header.chain_id
        || proposal.epoch != block.header.epoch
        || proposal.round != block.header.round
        || proposal.stake_snapshot_hash != block.header.stake_snapshot_hash
        || proposal.proposer != block.header.proposer
        || block.header.height < 0
        || proposal.height != block.header.height as u64
    {
        return Err(protocol_error(
            "V2 finalized record proposal and block contexts do not match",
        ));
    }
    block.validate_structure(
        &ChainContext {
            wire_version: envelope.wire_version,
            genesis_schema_version: 0,
            protocol_version: envelope.protocol_version,
            chain_id: envelope.chain_id,
            genesis_hash: envelope.genesis_hash,
        },
        &crate::genesis::ProtocolResourceLimits::default(),
    )?;
    if proposal.proposer.0 == [0u8; 32]
        || proposal.stake_snapshot_hash.0 == [0u8; 32]
        || proposal.vrf_preout == [0u8; 32]
        || proposal.vrf_proof == [0u8; 64]
        || proposal.signature == [0u8; 64]
    {
        return Err(protocol_error(
            "V2 finalized record contains a zero identity or proof field",
        ));
    }
    if proposal.valid_round.is_some() && proposal.valid_round_certificate.is_none() {
        return Err(protocol_error(
            "V2 finalized record valid round is missing its certificate",
        ));
    }
    if let Some(certificate) = &proposal.valid_round_certificate {
        validate_prevote_certificate(certificate, proposal)?;
    }

    validate_shared_context(
        certificate.protocol_version,
        certificate.chain_id,
        certificate.epoch,
        certificate.height,
        certificate.round,
        certificate.stake_snapshot_hash,
        envelope,
    )?;
    if certificate.block_id != proposal.block_id
        || certificate.height != proposal.height
        || certificate.round != proposal.round
        || certificate.precommits.is_empty()
        || certificate.precommits.len() > MAX_CONSENSUS_CERTIFICATE_VOTES
    {
        return Err(protocol_error(
            "V2 finalized record certificate does not match its proposal",
        ));
    }
    validate_precommit_order(certificate)?;
    for vote in &certificate.precommits {
        validate_vote_context(vote, certificate)?;
        if vote.step != VoteStep::Precommit
            || vote.block_id != Some(certificate.block_id)
            || vote.validator.0 == [0u8; 32]
            || vote.signature == [0u8; 64]
        {
            return Err(protocol_error(
                "V2 finalized record contains an invalid precommit",
            ));
        }
    }
    if consensus_state.height != certificate.height
        || consensus_state.finalized_block_id != certificate.block_id
        || consensus_state.commit_certificate_hash != certificate.certificate_hash()
        || consensus_state.active_stake_snapshot_hash != block.header.stake_snapshot_hash
    {
        return Err(protocol_error(
            "V2 finalized record consensus state does not match its certificate",
        ));
    }
    Ok(())
}

fn validate_vote_context(vote: &SignedVote, certificate: &CommitCertificate) -> Result<()> {
    if vote.protocol_version != certificate.protocol_version
        || vote.chain_id != certificate.chain_id
        || vote.epoch != certificate.epoch
        || vote.height != certificate.height
        || vote.round != certificate.round
        || vote.stake_snapshot_hash != certificate.stake_snapshot_hash
    {
        return Err(protocol_error("certificate vote context mismatch"));
    }
    Ok(())
}

fn validate_precommit_order(certificate: &CommitCertificate) -> Result<()> {
    for pair in certificate.precommits.windows(2) {
        if pair[0].validator >= pair[1].validator {
            return Err(protocol_error(
                "commit precommits are not canonically sorted",
            ));
        }
    }
    Ok(())
}

fn validate_prevote_certificate(
    certificate: &PrevoteCertificate,
    proposal: &Proposal,
) -> Result<()> {
    if certificate.protocol_version != proposal.protocol_version
        || certificate.chain_id != proposal.chain_id
        || certificate.epoch != proposal.epoch
        || certificate.height != proposal.height
        || certificate.block_id != proposal.block_id
        || certificate.stake_snapshot_hash != proposal.stake_snapshot_hash
        || Some(certificate.round) != proposal.valid_round
    {
        return Err(protocol_error(
            "proposal valid-round certificate context mismatch",
        ));
    }
    if certificate.prevotes.is_empty()
        || certificate.prevotes.len() > MAX_CONSENSUS_CERTIFICATE_VOTES
    {
        return Err(protocol_error(
            "proposal prevote certificate has an invalid vote count",
        ));
    }
    for pair in certificate.prevotes.windows(2) {
        if pair[0].validator >= pair[1].validator {
            return Err(protocol_error(
                "proposal prevotes are not canonically sorted",
            ));
        }
    }
    for vote in &certificate.prevotes {
        if vote.protocol_version != certificate.protocol_version
            || vote.chain_id != certificate.chain_id
            || vote.epoch != certificate.epoch
            || vote.height != certificate.height
            || vote.round != certificate.round
            || vote.step != VoteStep::Prevote
            || vote.block_id != Some(certificate.block_id)
            || vote.stake_snapshot_hash != certificate.stake_snapshot_hash
            || vote.validator.0 == [0u8; 32]
            || vote.signature == [0u8; 64]
        {
            return Err(protocol_error(
                "proposal prevote certificate contains an invalid vote",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod wire_validation_tests {
    use super::*;
    use crate::chain_context::{ChainContext, NetworkHandshake, PeerRole};

    fn context() -> ChainContext {
        ChainContext::new(
            2,
            ProtocolVersion(2),
            ChainId(Hash([3u8; 32])),
            Hash([4u8; 32]),
        )
    }

    fn proposal_envelope() -> ConsensusEnvelope {
        let context = context();
        let block_hash = Hash([9u8; 32]);
        let parent_hash = Hash([6u8; 32]);
        let snapshot_hash = StakeSnapshotHash([8u8; 32]);
        let proposer = ValidatorId([7u8; 32]);
        let mut block = Block::default();
        block.header.protocol_version = context.protocol_version;
        block.header.chain_id = context.chain_id;
        block.header.height = 1;
        block.header.epoch = 1;
        block.header.round = 0;
        block.header.prev_block_hash = parent_hash;
        block.header.block_hash = block_hash;
        block.header.stake_snapshot_hash = snapshot_hash;
        block.header.proposer = proposer;

        ConsensusEnvelope {
            wire_version: context.wire_version,
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            genesis_hash: context.genesis_hash,
            payload: ConsensusMessage::Proposal {
                proposal: Proposal {
                    protocol_version: context.protocol_version,
                    chain_id: context.chain_id,
                    epoch: 1,
                    height: 1,
                    round: 0,
                    valid_round: None,
                    valid_round_certificate: None,
                    block_id: BlockId(block_hash),
                    parent_block_hash: parent_hash,
                    stake_snapshot_hash: snapshot_hash,
                    proposer,
                    vrf_preout: [1u8; 32],
                    vrf_proof: [2u8; 64],
                    signature: [3u8; 64],
                },
                block,
            },
        }
    }

    #[test]
    fn envelope_context_and_payload_context_must_match() {
        let context = context();
        let envelope = proposal_envelope();
        assert!(envelope.validate_for_context(&context).is_ok());

        let mut wrong_genesis = envelope.clone();
        wrong_genesis.genesis_hash = Hash([5u8; 32]);
        assert!(wrong_genesis.validate_for_context(&context).is_err());

        let mut wrong_block_context = envelope.clone();
        if let ConsensusMessage::Proposal { block, .. } = &mut wrong_block_context.payload {
            block.header.chain_id = ChainId(Hash([10u8; 32]));
        }
        assert!(wrong_block_context.validate_for_context(&context).is_err());
    }

    #[test]
    fn v2_proposal_validates_block_commitments_without_legacy_fallback() {
        let context = context();
        let snapshot_hash = StakeSnapshotHash([8u8; 32]);
        let proposer = ValidatorId([7u8; 32]);
        let mut block = BlockV2::default();
        block.header.protocol_version = context.protocol_version;
        block.header.chain_id = context.chain_id;
        block.header.height = 1;
        block.header.epoch = 1;
        block.header.round = 0;
        block.header.prev_block_hash = Hash([6u8; 32]);
        block.header.stake_snapshot_hash = snapshot_hash;
        block.header.proposer = proposer;
        block.finalize_header().unwrap();

        let envelope = ConsensusEnvelope {
            wire_version: context.wire_version,
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            genesis_hash: context.genesis_hash,
            payload: ConsensusMessage::ProposalV2 {
                proposal: Proposal {
                    protocol_version: context.protocol_version,
                    chain_id: context.chain_id,
                    epoch: 1,
                    height: 1,
                    round: 0,
                    valid_round: None,
                    valid_round_certificate: None,
                    block_id: BlockId(block.header.block_hash),
                    parent_block_hash: block.header.prev_block_hash,
                    stake_snapshot_hash: snapshot_hash,
                    proposer,
                    vrf_preout: [1u8; 32],
                    vrf_proof: [2u8; 64],
                    signature: [3u8; 64],
                },
                block: block.clone(),
            },
        };
        envelope.validate_for_context(&context).unwrap();

        let mut tampered = envelope;
        if let ConsensusMessage::ProposalV2 { block, .. } = &mut tampered.payload {
            block.header.state_root = Hash([1u8; 32]);
        }
        assert!(tampered.validate_for_context(&context).is_err());
    }

    #[test]
    fn decode_rejects_oversized_and_malformed_envelopes() {
        let context = context();
        let envelope = proposal_envelope();
        let encoded = bincode::serialize(&envelope).unwrap();
        assert!(ConsensusEnvelope::decode_and_validate(&encoded, &context).is_ok());

        let oversized = vec![0u8; MAX_CONSENSUS_ENVELOPE_BYTES + 1];
        assert!(ConsensusEnvelope::decode_and_validate(&oversized, &context).is_err());
        assert!(ConsensusEnvelope::decode_and_validate(&[0u8; 3], &context).is_err());
    }

    #[test]
    fn commit_precommits_must_be_canonical_and_contextual() {
        let context = context();
        let block_id = BlockId(Hash([9u8; 32]));
        let snapshot_hash = StakeSnapshotHash([8u8; 32]);
        let vote = |validator: ValidatorId| SignedVote {
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(block_id),
            stake_snapshot_hash: snapshot_hash,
            validator,
            signature: [3u8; 64],
        };
        let mut certificate = CommitCertificate {
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            epoch: 1,
            height: 1,
            round: 0,
            block_id,
            stake_snapshot_hash: snapshot_hash,
            precommits: vec![vote(ValidatorId([2u8; 32])), vote(ValidatorId([1u8; 32]))],
        };
        let envelope = ConsensusEnvelope {
            wire_version: context.wire_version,
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            genesis_hash: context.genesis_hash,
            payload: ConsensusMessage::Commit(certificate.clone()),
        };
        assert!(envelope.validate_for_context(&context).is_err());

        certificate.precommits.reverse();
        let valid = ConsensusEnvelope {
            payload: ConsensusMessage::Commit(certificate),
            ..envelope
        };
        assert!(valid.validate_for_context(&context).is_ok());
    }

    #[test]
    fn handshake_is_bound_to_full_chain_context_and_role() {
        let context = context();
        let handshake = NetworkHandshake::new(context, PeerRole::Validator);
        assert!(handshake.validate_for_context(&context).is_ok());

        let mut wrong = handshake;
        wrong.genesis_hash = Hash([99u8; 32]);
        assert!(wrong.validate_for_context(&context).is_err());
    }

    #[test]
    fn consensus_wire_fuzz_corpus_is_panic_free() {
        let context = context();
        let valid = bincode::serialize(&proposal_envelope()).unwrap();

        // Deterministic mutation corpus: this is intentionally dependency-free
        // so it runs in every workspace test without requiring libFuzzer.
        for bit in 0..(valid.len() * 8) {
            let mut mutated = valid.clone();
            mutated[bit / 8] ^= 1u8 << (bit % 8);
            let _ = ConsensusEnvelope::decode_and_validate(&mutated, &context);
        }

        // Deterministic pseudo-random inputs exercise length prefixes and
        // enum discriminants without ever allocating beyond the wire ceiling.
        let mut state = 0x4e4f524e_u64;
        for _ in 0..2048 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let length = ((state >> 32) as usize) % 4096;
            let mut bytes = vec![0u8; length];
            for byte in &mut bytes {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                *byte = (state >> 56) as u8;
            }
            let _ = ConsensusEnvelope::decode_and_validate(&bytes, &context);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizedConsensusState {
    pub height: u64,
    pub finalized_block_id: BlockId,
    pub commit_certificate_hash: Hash,
    pub next_randomness: Hash,
    pub active_stake_snapshot_hash: StakeSnapshotHash,
    #[serde(default)]
    pub pending_validator_changes: PendingValidatorChanges,
}

/// The single canonical finalized-chain authority used by V2 production,
/// proposal validation, execution recovery, and RPC-facing tip reads.
///
/// This is intentionally richer than a height/hash pointer: the next block's
/// parent randomness and active validator snapshot are consensus inputs, while
/// the state root and base fee bind execution to the same finalized parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalFinalizedTip {
    pub height: u64,
    pub block_id: BlockId,
    pub state_root: Hash,
    pub base_fee: u64,
    pub next_randomness: Hash,
    pub active_snapshot_hash: StakeSnapshotHash,
    pub epoch: u64,
}

impl CanonicalFinalizedTip {
    pub fn from_genesis(
        genesis: &Block,
        active_snapshot_hash: StakeSnapshotHash,
        next_randomness: Hash,
    ) -> Result<Self> {
        if genesis.header.height != 0 || genesis.header.block_hash == Hash::default() {
            return Err(NornError::ConsensusError(
                "canonical Genesis tip has an invalid height or block ID".into(),
            ));
        }
        Ok(Self {
            height: 0,
            block_id: BlockId(genesis.header.block_hash),
            state_root: genesis.header.state_root,
            base_fee: genesis.header.base_fee,
            next_randomness,
            active_snapshot_hash,
            epoch: genesis.header.epoch as u64,
        })
    }

    pub fn from_finalized(finalized: &FinalizedBlockV2) -> Result<Self> {
        if finalized.block.header.height < 0
            || finalized.block.header.block_hash == Hash::default()
            || finalized.commit.block_id != BlockId(finalized.block.header.block_hash)
            || finalized.consensus_state.height != finalized.commit.height
        {
            return Err(NornError::ConsensusError(
                "canonical finalized tip does not match finalized block".into(),
            ));
        }
        Ok(Self {
            height: finalized.commit.height,
            block_id: finalized.commit.block_id,
            state_root: finalized.block.header.state_root,
            base_fee: finalized.block.header.base_fee,
            next_randomness: finalized.consensus_state.next_randomness,
            active_snapshot_hash: finalized.consensus_state.active_stake_snapshot_hash,
            epoch: finalized.block.header.epoch as u64,
        })
    }

    pub fn from_finalized_with_next_snapshot(
        finalized: &FinalizedBlockV2,
        next_snapshot: Option<&StakeSnapshot>,
    ) -> Result<Self> {
        let mut tip = Self::from_finalized(finalized)?;
        if let Some(snapshot) = next_snapshot {
            if snapshot.epoch < tip.epoch {
                return Err(NornError::ConsensusError(
                    "next validator snapshot regresses canonical tip epoch".into(),
                ));
            }
            tip.active_snapshot_hash = snapshot.snapshot_hash;
            tip.epoch = snapshot.epoch;
        }
        Ok(tip)
    }

    pub fn next_height(&self) -> Result<u64> {
        self.height
            .checked_add(1)
            .ok_or_else(|| NornError::ConsensusError("canonical tip height overflow".into()))
    }
}

impl FinalizedConsensusState {
    pub fn from_v2(
        block: &BlockV2,
        commit: &CommitCertificate,
        next_randomness: Hash,
    ) -> Result<Self> {
        if block.header.height < 0
            || block.header.block_hash == Hash::default()
            || commit.block_id != BlockId(block.header.block_hash)
            || commit.height != block.header.height as u64
            || commit.protocol_version != block.header.protocol_version
            || commit.chain_id != block.header.chain_id
            || commit.epoch != block.header.epoch
            || commit.round != block.header.round
            || commit.stake_snapshot_hash != block.header.stake_snapshot_hash
        {
            return Err(NornError::ConsensusError(
                "Finalized consensus state does not match block/certificate".into(),
            ));
        }
        Ok(Self {
            height: commit.height,
            finalized_block_id: commit.block_id,
            commit_certificate_hash: commit.certificate_hash(),
            next_randomness,
            active_stake_snapshot_hash: block.header.stake_snapshot_hash,
            pending_validator_changes: PendingValidatorChanges::default(),
        })
    }

    /// The only permitted parent randomness for the immediately following
    /// height.  Keeping this relation explicit makes recovery and replay
    /// independent of the error returned by the previous commit attempt.
    pub fn parent_randomness_for_height(&self, height: u64) -> Result<Hash> {
        if self.height.checked_add(1) != Some(height) {
            return Err(NornError::ConsensusError(
                "Finalized consensus state is not the parent of requested height".into(),
            ));
        }
        Ok(self.next_randomness)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizedBlock {
    pub block: Block,
    pub commit: CommitCertificate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizedBlockV2 {
    /// The verified proposal is durable with the finalized block so a
    /// FullNode that missed the proposal can recover the VRF proof and derive
    /// the committed next randomness from the finality record alone.
    pub proposal: Proposal,
    pub block: BlockV2,
    pub commit: CommitCertificate,
    pub consensus_state: FinalizedConsensusState,
}

/// Durable identity of one finalized transaction.  The certificate hash is
/// part of the identity so recovery can distinguish a complete finality
/// transaction from an unrelated attempt at the same height.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizeTransactionId {
    pub height: u64,
    pub block_id: BlockId,
    pub certificate_hash: Hash,
}

impl FinalizeTransactionId {
    pub fn from_v2(finalized: &FinalizedBlockV2) -> Self {
        Self {
            height: finalized.block.header.height.max(0) as u64,
            block_id: finalized.commit.block_id,
            certificate_hash: finalized.commit.certificate_hash(),
        }
    }
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
    #[serde(default)]
    pub jailed_until_epoch: Option<u64>,
    #[serde(default)]
    pub slashed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidatorChange {
    Add(ValidatorRecord),
    Remove {
        validator_id: ValidatorId,
    },
    SetVotingPower {
        validator_id: ValidatorId,
        voting_power: u64,
    },
    RotateKeys {
        validator_id: ValidatorId,
        consensus_public_key: ConsensusPublicKey,
        vrf_public_key: VrfPublicKey,
    },
    Jail {
        validator_id: ValidatorId,
        until_epoch: u64,
    },
    Unjail {
        validator_id: ValidatorId,
    },
    Slash {
        validator_id: ValidatorId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingValidatorChange {
    pub effective_epoch: u64,
    pub change: ValidatorChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PendingValidatorChanges {
    pub changes: Vec<PendingValidatorChange>,
}

impl PendingValidatorChanges {
    pub fn queue(&mut self, change: PendingValidatorChange) -> Result<()> {
        if change.effective_epoch == 0 {
            return Err(NornError::ConsensusError(
                "validator change must target a non-zero epoch".into(),
            ));
        }
        self.changes.push(change);
        self.changes.sort_by(|left, right| {
            left.effective_epoch
                .cmp(&right.effective_epoch)
                .then_with(|| {
                    bincode::serialize(&left.change)
                        .unwrap_or_default()
                        .cmp(&bincode::serialize(&right.change).unwrap_or_default())
                })
        });
        Ok(())
    }

    /// Apply all changes between the current snapshot and `epoch`, in their
    /// canonical effective-epoch/order sequence. This matters when a node
    /// recovers across more than one epoch boundary: applying only the target
    /// epoch would silently skip an earlier queued update.
    pub fn snapshot_for_epoch(&self, current: &StakeSnapshot, epoch: u64) -> Result<StakeSnapshot> {
        if epoch < current.epoch {
            return Err(NornError::ConsensusError(
                "validator snapshot cannot move backwards across epochs".into(),
            ));
        }
        let mut records = current.validators.clone();
        for pending in self.changes.iter().filter(|change| {
            change.effective_epoch > current.epoch && change.effective_epoch <= epoch
        }) {
            match &pending.change {
                ValidatorChange::Add(record) => {
                    if record.slashed {
                        return Err(NornError::ConsensusError(
                            "a slashed validator cannot be added to a snapshot".into(),
                        ));
                    }
                    if records.contains_key(&record.validator_id) {
                        return Err(NornError::ConsensusError(
                            "validator change adds a duplicate ValidatorId".into(),
                        ));
                    }
                    records.insert(record.validator_id, record.clone());
                }
                ValidatorChange::Remove { validator_id } => {
                    records.remove(validator_id);
                }
                ValidatorChange::SetVotingPower {
                    validator_id,
                    voting_power,
                } => {
                    let record = records.get_mut(validator_id).ok_or_else(|| {
                        NornError::ConsensusError(
                            "validator power update targets an unknown validator".into(),
                        )
                    })?;
                    record.voting_power = *voting_power;
                }
                ValidatorChange::RotateKeys {
                    validator_id,
                    consensus_public_key,
                    vrf_public_key,
                } => {
                    let record = records.get_mut(validator_id).ok_or_else(|| {
                        NornError::ConsensusError(
                            "validator key rotation targets an unknown validator".into(),
                        )
                    })?;
                    record.consensus_public_key = *consensus_public_key;
                    record.vrf_public_key = *vrf_public_key;
                }
                ValidatorChange::Jail {
                    validator_id,
                    until_epoch,
                } => {
                    let record = records.get_mut(validator_id).ok_or_else(|| {
                        NornError::ConsensusError("jail targets an unknown validator".into())
                    })?;
                    record.jailed_until_epoch = Some(*until_epoch);
                }
                ValidatorChange::Unjail { validator_id } => {
                    let record = records.get_mut(validator_id).ok_or_else(|| {
                        NornError::ConsensusError("unjail targets an unknown validator".into())
                    })?;
                    record.jailed_until_epoch = None;
                }
                ValidatorChange::Slash { validator_id } => {
                    let record = records.get_mut(validator_id).ok_or_else(|| {
                        NornError::ConsensusError("slash targets an unknown validator".into())
                    })?;
                    record.slashed = true;
                }
            }
        }
        let records = records.into_values().collect::<Vec<_>>();
        StakeSnapshot::from_genesis(epoch, records)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StakeSnapshot {
    pub epoch: u64,
    /// BTreeMap sorted by ValidatorId for deterministic ordering
    pub validators: BTreeMap<ValidatorId, ValidatorRecord>,
    pub snapshot_hash: StakeSnapshotHash,
}

impl StakeSnapshot {
    pub fn from_genesis(epoch: u64, records: Vec<ValidatorRecord>) -> Result<Self> {
        if records.is_empty() {
            return Err(NornError::ConsensusError(
                "Genesis validator set cannot be empty".into(),
            ));
        }
        let mut validators = BTreeMap::new();
        let mut consensus_keys = HashSet::new();
        let mut vrf_keys = HashSet::new();
        let mut total_power = 0u128;
        for record in records {
            if record.voting_power == 0 {
                return Err(NornError::ConsensusError(
                    "Validator voting power must be > 0".into(),
                ));
            }
            if record.validator_id.0 == [0u8; 32] {
                return Err(NornError::ConsensusError(
                    "Validator ID must be non-zero".into(),
                ));
            }
            if record.consensus_public_key.0 == [0u8; 33] {
                return Err(NornError::ConsensusError(
                    "Consensus public key must be non-zero".into(),
                ));
            }
            if record.vrf_public_key.0 == [0u8; 32] {
                return Err(NornError::ConsensusError(
                    "VRF public key must be non-zero".into(),
                ));
            }
            total_power = total_power
                .checked_add(record.voting_power as u128)
                .ok_or_else(|| NornError::ConsensusError("Voting power overflow".into()))?;
            if validators.contains_key(&record.validator_id) {
                return Err(NornError::ConsensusError(
                    "Duplicate validator ID in genesis".into(),
                ));
            }
            if !consensus_keys.insert(record.consensus_public_key.0) {
                return Err(NornError::ConsensusError(
                    "Duplicate consensus public key in genesis".into(),
                ));
            }
            if !vrf_keys.insert(record.vrf_public_key.0) {
                return Err(NornError::ConsensusError(
                    "Duplicate VRF public key in genesis".into(),
                ));
            }
            validators.insert(record.validator_id, record);
        }
        let mut snapshot = Self {
            epoch,
            validators,
            snapshot_hash: StakeSnapshotHash::default(),
        };
        snapshot.snapshot_hash = snapshot.compute_hash();
        Ok(snapshot)
    }

    pub fn total_voting_power(&self) -> Result<u128> {
        self.validators
            .values()
            .filter(|v| {
                !v.slashed
                    && !v
                        .jailed_until_epoch
                        .is_some_and(|until_epoch| self.epoch < until_epoch)
            })
            .try_fold(0u128, |acc, v| {
                acc.checked_add(v.voting_power as u128)
                    .ok_or_else(|| NornError::ConsensusError("Voting power overflow".into()))
            })
    }

    pub fn is_active_validator(&self, validator_id: &ValidatorId) -> bool {
        self.validators
            .get(validator_id)
            .is_some_and(|record| record.is_active_at(self.epoch))
    }

    /// Reuse the validator set at an epoch boundary while changing only the
    /// epoch domain.  The epoch is included in the snapshot hash, so the
    /// boundary cannot silently reuse a prior epoch's identity.
    pub fn for_epoch(&self, epoch: u64) -> Result<Self> {
        Self::from_genesis(epoch, self.validators.values().cloned().collect())
    }

    pub fn compute_hash(&self) -> StakeSnapshotHash {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"NORN_STAKE_SNAPSHOT_V2");
        hasher.update(&self.epoch.to_be_bytes());
        for (vid, record) in &self.validators {
            hasher.update(&vid.0);
            hasher.update(&record.consensus_public_key.0);
            hasher.update(&record.vrf_public_key.0);
            hasher.update(&record.voting_power.to_be_bytes());
            hasher.update(&record.jailed_until_epoch.unwrap_or(u64::MAX).to_be_bytes());
            hasher.update([u8::from(record.slashed)]);
        }
        let res = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&res);
        StakeSnapshotHash(arr)
    }
}

impl ValidatorRecord {
    pub fn is_active_at(&self, epoch: u64) -> bool {
        !self.slashed
            && !self
                .jailed_until_epoch
                .is_some_and(|until_epoch| epoch < until_epoch)
    }
}

#[cfg(test)]
mod finalized_consensus_state_tests {
    use super::*;

    fn record(id: u8) -> ValidatorRecord {
        ValidatorRecord {
            validator_id: ValidatorId([id; 32]),
            consensus_public_key: ConsensusPublicKey([id; 33]),
            vrf_public_key: VrfPublicKey([id; 32]),
            voting_power: u64::from(id),
            jailed_until_epoch: None,
            slashed: false,
        }
    }

    #[test]
    fn epoch_snapshot_hash_is_order_independent_and_epoch_bound() {
        let first = StakeSnapshot::from_genesis(1, vec![record(1), record(2)]).unwrap();
        let second = StakeSnapshot::from_genesis(2, vec![record(2), record(1)]).unwrap();
        assert_ne!(first.snapshot_hash, second.snapshot_hash);
        assert_eq!(first.for_epoch(2).unwrap(), second);
    }

    #[test]
    fn pending_changes_apply_intermediate_epochs_and_preserve_slashing_state() {
        let current = StakeSnapshot::from_genesis(1, vec![record(1), record(2)]).unwrap();
        let mut pending = PendingValidatorChanges::default();
        pending
            .queue(PendingValidatorChange {
                effective_epoch: 2,
                change: ValidatorChange::Jail {
                    validator_id: ValidatorId([1; 32]),
                    until_epoch: 4,
                },
            })
            .unwrap();
        pending
            .queue(PendingValidatorChange {
                effective_epoch: 3,
                change: ValidatorChange::Slash {
                    validator_id: ValidatorId([2; 32]),
                },
            })
            .unwrap();
        pending
            .queue(PendingValidatorChange {
                effective_epoch: 3,
                change: ValidatorChange::Add(record(3)),
            })
            .unwrap();

        let next = pending.snapshot_for_epoch(&current, 3).unwrap();
        assert_eq!(next.epoch, 3);
        assert!(next.validators[&ValidatorId([1; 32])].jailed_until_epoch == Some(4));
        assert!(next.validators[&ValidatorId([2; 32])].slashed);
        assert!(!next.is_active_validator(&ValidatorId([1; 32])));
        assert!(!next.is_active_validator(&ValidatorId([2; 32])));
        assert!(next.is_active_validator(&ValidatorId([3; 32])));
        assert_eq!(next.total_voting_power().unwrap(), 3);
    }

    #[test]
    fn pending_changes_reject_backward_epoch_and_duplicate_keys() {
        let current = StakeSnapshot::from_genesis(2, vec![record(1)]).unwrap();
        let pending = PendingValidatorChanges::default();
        assert!(pending.snapshot_for_epoch(&current, 1).is_err());

        let duplicate = ValidatorRecord {
            validator_id: ValidatorId([2; 32]),
            consensus_public_key: ConsensusPublicKey([1; 33]),
            vrf_public_key: VrfPublicKey([2; 32]),
            voting_power: 1,
            jailed_until_epoch: None,
            slashed: false,
        };
        let mut pending = PendingValidatorChanges::default();
        pending
            .queue(PendingValidatorChange {
                effective_epoch: 3,
                change: ValidatorChange::Add(duplicate),
            })
            .unwrap();
        assert!(pending.snapshot_for_epoch(&current, 3).is_err());
    }

    #[test]
    fn certificate_hash_is_independent_of_member_order() {
        let block_id = BlockId(Hash([9; 32]));
        let vote = |validator| SignedVote {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(Hash([1; 32])),
            epoch: 1,
            height: 1,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(block_id),
            stake_snapshot_hash: StakeSnapshotHash([8; 32]),
            validator,
            signature: [3; 64],
        };
        let mut first = CommitCertificate {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(Hash([1; 32])),
            epoch: 1,
            height: 1,
            round: 0,
            block_id,
            stake_snapshot_hash: StakeSnapshotHash([8; 32]),
            precommits: vec![vote(ValidatorId([2; 32])), vote(ValidatorId([1; 32]))],
        };
        let first_hash = first.certificate_hash();
        first.precommits.reverse();
        assert_eq!(first_hash, first.certificate_hash());
    }
}
