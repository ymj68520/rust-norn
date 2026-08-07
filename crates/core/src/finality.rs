//! Atomic, idempotent persistence for finalized protocol-v2 blocks.

use crate::execution::overlay::{CanonicalStateCheckpoint, OverlayWrite};
use anyhow::{anyhow, bail, Result};
use norn_common::consensus_types::{
    CanonicalFinalizedTip, FinalizeTransactionId, FinalizedBlockV2, Proposal, StakeSnapshot,
};
use norn_common::genesis::ProtocolResourceLimits;
use norn_common::traits::DBInterface;
use norn_common::types::{
    Block, BlockConsensusData, BlockHeader, BlockId, BlockV2, Hash, StakeSnapshotHash,
    TransactionId,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::Arc;

const HEIGHT_PREFIX: &[u8] = b"finality/v2/by-height/";
const RECORD_PREFIX: &[u8] = b"finality/v2/record/";
const CERTIFICATE_PREFIX: &[u8] = b"finality/v2/certificate/";
const STATE_WRITE_COUNT_PREFIX: &[u8] = b"state/v2/write-count/";
const STATE_WRITE_PREFIX: &[u8] = b"state/v2/write/";
const TIP_KEY: &[u8] = b"finality/v2/tip";
const LEGACY_TIP_KEY: &[u8] = b"consensus/v2/finalized-tip";
const STATE_ROOT_KEY: &[u8] = b"state/v2/root";
const STATE_ROOT_PREFIX: &[u8] = b"state/v2/root/";
const STATE_CHECKPOINT_KEY: &[u8] = b"state/v2/checkpoint";
const STATE_CHECKPOINT_PREFIX: &[u8] = b"state/v2/checkpoint/";
const STATE_CHECKPOINT_CONTENT_PREFIX: &[u8] = b"state/v2/checkpoint-content/";
const CONSENSUS_STATE_KEY: &[u8] = b"finality/v2/consensus-state";
const STATE_ACCOUNT_PREFIX: &[u8] = b"state/v2/account/";
const STATE_STORAGE_PREFIX: &[u8] = b"state/v2/storage/";
const STATE_CODE_PREFIX: &[u8] = b"state/v2/code/";
const STATE_TOMBSTONE: &[u8] = b"NORN_STATE_TOMBSTONE_V2";
const SNAPSHOT_PREFIX: &[u8] = b"finality/v2/snapshot/";
const PENDING_PROPOSAL_PREFIX: &[u8] = b"consensus/v2/pending-proposal/";
const SAFETY_CANDIDATE_PREFIX: &[u8] = b"consensus/v2/safety-candidate/";
const SAFETY_BLOCK_CANDIDATE_PREFIX: &[u8] = b"consensus/v2/safety-candidate-block/";
const SAFETY_ATTEMPT_PREFIX: &[u8] = b"consensus/v2/safety-proposal-attempt/";
const SAFETY_HEIGHT_INDEX_PREFIX: &[u8] = b"consensus/v2/safety-proposal-height-index/";
const SAFETY_RECORD_FORMAT_VERSION: u16 = 1;
const COMPRESSED_FINALITY_MAGIC: &[u8] = b"NORN_FINALITY_ZSTD_V1";
const COMPRESSED_FINALITY_LENGTH_BYTES: usize = std::mem::size_of::<u64>();
// This is a corruption/allocation guard for durable data, not a consensus
// limit. Genesis currently caps the encoded block itself at 8 MiB, while the
// serde representation of a complete finality record can be larger because
// byte fields use a human-readable-compatible encoding.
const MAX_DURABLE_FINALITY_RECORD_BYTES: usize = 64 * 1024 * 1024;

/// Complete validated candidate material required to recover a durable
/// Tendermint valid/locked reference after a process restart.  Persisting
/// only the block ID is insufficient: a producer must be able to re-propose
/// the exact bytes and preserve the original block builder identity.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DurableSafetyCandidate {
    pub proposal: Proposal,
    pub block: BlockV2,
    pub derived_randomness: Hash,
}

/// Durable immutable block material. Proposal attempts are intentionally
/// stored separately because the same block body may be proposed in multiple
/// rounds with different proposer/VRF metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableBlockCandidate {
    pub format_version: u16,
    pub height: u64,
    pub block_id: BlockId,
    pub block: BlockV2,
    pub attempt_rounds: Vec<u32>,
    pub payload_hash: Hash,
}

/// Crash-safety only needs the immutable block commitment, not thousands of
/// transaction bodies or IDs that are already committed by the header's
/// Merkle root and block hash.  Keeping the durable record constant-sized is
/// essential on SD cards: a pre-vote fsync must not grow with block traffic.
/// The full body remains available in the bounded live candidate cache for
/// normal finality and re-proposal and can be rehydrated by block ID after a
/// restart.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct DurableCompactBlock {
    header: BlockHeader,
    consensus_data: BlockConsensusData,
}

impl DurableCompactBlock {
    fn from_block(block: &BlockV2) -> Self {
        Self {
            header: block.header.clone(),
            consensus_data: block.consensus_data.clone(),
        }
    }

    fn matches(&self, block: &BlockV2) -> bool {
        self.header == block.header && self.consensus_data == block.consensus_data
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct DurableBlockCandidateRecord {
    format_version: u16,
    height: u64,
    block_id: BlockId,
    block: DurableCompactBlock,
    attempt_rounds: Vec<u32>,
    payload_hash: Hash,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DurableProposalAttempt {
    pub format_version: u16,
    pub height: u64,
    pub round: u32,
    pub block_id: BlockId,
    pub proposal: Proposal,
    pub derived_randomness: Hash,
    pub payload_hash: Hash,
}

/// Durable per-height index for every proposal attempt. The block record's
/// `attempt_rounds` field is per block, so it cannot enforce a protocol limit
/// that applies across multiple competing block IDs at one height.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct DurableHeightAttemptIndex {
    format_version: u16,
    height: u64,
    attempts: Vec<DurableAttemptRef>,
    payload_hash: Hash,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct DurableAttemptRef {
    round: u32,
    block_id: BlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalityCommitResult {
    Applied,
    AlreadyCommitted,
}

/// Result of inspecting durable markers after a DB operation returned an
/// error. `Indeterminate` is deliberately distinct from `NotApplied`: the
/// caller must fail-stop when the marker set cannot prove a complete old or
/// complete new transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableCommitOutcome {
    Applied,
    AlreadyApplied,
    NotApplied,
    Indeterminate,
}

pub struct FinalityStore {
    db: Arc<dyn DBInterface>,
    resource_limits: ProtocolResourceLimits,
    /// Serializes durable block-record read-modify-write and finality cleanup
    /// so `attempt_rounds` cannot lose a concurrent writer's index update.
    candidate_write_lock: Arc<tokio::sync::Mutex<()>>,
    candidate_bodies: std::sync::Mutex<std::collections::HashMap<(u64, BlockId), BlockV2>>,
}

impl FinalityStore {
    pub fn new(db: Arc<dyn DBInterface>) -> Self {
        Self::new_with_limits(db, ProtocolResourceLimits::default())
    }

    pub fn new_with_limits(
        db: Arc<dyn DBInterface>,
        resource_limits: ProtocolResourceLimits,
    ) -> Self {
        Self {
            db,
            resource_limits,
            candidate_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            candidate_bodies: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Initialize the canonical V2 tip for a fresh database. Existing state is
    /// never guessed or rewritten: it must decode as the new tip schema.
    pub async fn initialize_genesis_tip(
        &self,
        genesis: &Block,
        active_snapshot_hash: StakeSnapshotHash,
        next_randomness: Hash,
    ) -> Result<CanonicalFinalizedTip> {
        if self.db.get(LEGACY_TIP_KEY).await?.is_some() {
            bail!(
                "legacy finalized tip schema is present; explicit migration is required before V2 startup"
            );
        }
        if let Some(bytes) = self.db.get(TIP_KEY).await? {
            return decode(&bytes, "canonical finalized tip");
        }
        let tip =
            CanonicalFinalizedTip::from_genesis(genesis, active_snapshot_hash, next_randomness)?;
        self.db
            .batch_insert(&[TIP_KEY.to_vec()], &[encode(&tip)?])
            .await?;
        Ok(tip)
    }

    pub async fn recover_canonical_tip(&self) -> Result<Option<CanonicalFinalizedTip>> {
        self.db
            .get(TIP_KEY)
            .await?
            .map(|bytes| decode(&bytes, "canonical finalized tip"))
            .transpose()
    }

    pub async fn recover_snapshot(&self, epoch: u64) -> Result<Option<StakeSnapshot>> {
        self.db
            .get(&snapshot_key(epoch))
            .await?
            .map(|bytes| decode(&bytes, "finalized validator snapshot"))
            .transpose()
    }

    fn remember_candidate_body(
        &self,
        height: u64,
        block_id: BlockId,
        block: &BlockV2,
    ) -> Result<()> {
        let mut bodies = self
            .candidate_bodies
            .lock()
            .map_err(|_| anyhow!("candidate body cache lock is poisoned"))?;
        if let Some(existing) = bodies.get(&(height, block_id)) {
            if existing != block {
                bail!("candidate body conflicts with the immutable block ID");
            }
        } else {
            bodies.insert((height, block_id), block.clone());
        }
        Ok(())
    }

    fn candidate_body(
        &self,
        height: u64,
        block_id: BlockId,
        compact: &DurableCompactBlock,
    ) -> Result<Option<BlockV2>> {
        let bodies = self
            .candidate_bodies
            .lock()
            .map_err(|_| anyhow!("candidate body cache lock is poisoned"))?;
        let Some(block) = bodies.get(&(height, block_id)) else {
            return Ok(None);
        };
        if !compact.matches(block) {
            bail!("candidate body does not match its durable compact commitment");
        }
        Ok(Some(block.clone()))
    }

    /// Persist the exact V2 proposal/block pair before its local vote is
    /// signed. This lets a restarted validator recover the proposal that its
    /// safety WAL already authorized instead of constructing a different block
    /// for the same height/round.
    pub async fn persist_pending_proposal(
        &self,
        proposal: &Proposal,
        block: &BlockV2,
    ) -> Result<()> {
        if proposal.height != block.header.height as u64
            || proposal.block_id != BlockId(block.header.block_hash)
            || block.header.block_builder.0 == [0u8; 32]
            || (proposal.protocol_version.0 < 3
                && (proposal.valid_round.is_some() || proposal.valid_round_certificate.is_some()))
            || match (
                proposal.valid_round,
                proposal.valid_round_certificate.as_ref(),
            ) {
                (None, None) => {
                    proposal.proposer != block.header.block_builder
                        || block.header.round != proposal.round
                }
                (Some(valid_round), Some(certificate)) => {
                    valid_round >= proposal.round
                        || certificate.round != valid_round
                        || certificate.block_id != proposal.block_id
                        || block.header.round > valid_round
                }
                _ => true,
            }
        {
            bail!("pending V2 proposal does not match its block identity");
        }
        self.remember_candidate_body(proposal.height, proposal.block_id, block)?;
        let key = pending_proposal_key(proposal.height, proposal.round);
        self.db
            .batch_insert(
                &[key],
                &[encode(&(proposal, DurableCompactBlock::from_block(block)))?],
            )
            .await
    }

    pub async fn recover_pending_proposal(
        &self,
        height: u64,
        round: u32,
    ) -> Result<Option<(Proposal, BlockV2)>> {
        let Some(bytes) = self.db.get(&pending_proposal_key(height, round)).await? else {
            return Ok(None);
        };
        let (proposal, compact): (Proposal, DurableCompactBlock) =
            decode(&bytes, "pending V2 proposal")?;
        let block = self
            .candidate_body(height, proposal.block_id, &compact)?
            .ok_or_else(|| anyhow!("pending proposal body must be rehydrated from a peer"))?;
        Ok(Some((proposal, block)))
    }

    pub async fn clear_pending_proposal(&self, height: u64, round: u32) -> Result<()> {
        self.db
            .batch_delete(&[pending_proposal_key(height, round)])
            .await
    }

    pub async fn persist_safety_candidate(
        &self,
        proposal: &Proposal,
        block: &BlockV2,
        derived_randomness: Hash,
    ) -> Result<()> {
        let _candidate_guard = self.candidate_write_lock.lock().await;
        if block.header.height < 0
            || proposal.height != block.header.height as u64
            || proposal.block_id != BlockId(block.header.block_hash)
            || block.header.block_builder.0 == [0u8; 32]
            || (proposal.protocol_version.0 < 3
                && (proposal.valid_round.is_some() || proposal.valid_round_certificate.is_some()))
            || match (
                proposal.valid_round,
                proposal.valid_round_certificate.as_ref(),
            ) {
                (None, None) => {
                    proposal.proposer != block.header.block_builder
                        || proposal.round != block.header.round
                }
                (Some(valid_round), Some(certificate)) => {
                    valid_round >= proposal.round
                        || certificate.round != valid_round
                        || certificate.block_id != proposal.block_id
                        || block.header.round > valid_round
                }
                _ => true,
            }
        {
            bail!("safety candidate identity does not match its block");
        }
        self.remember_candidate_body(proposal.height, proposal.block_id, block)?;
        let block_id = proposal.block_id;
        if proposal.round > self.resource_limits.max_consensus_round {
            bail!("proposal round exceeds the protocol consensus round bound");
        }
        if u64::from(proposal.round)
            >= u64::from(self.resource_limits.max_durable_attempts_per_height)
        {
            bail!("proposal round exceeds the durable attempt budget for this height");
        }
        let block_key = safety_block_candidate_key(proposal.height, block_id);
        let attempt_key = safety_proposal_attempt_key(proposal.height, proposal.round, block_id);
        let height_index_key = safety_height_attempt_index_key(proposal.height);
        let existing_block_bytes = self.db.get(&block_key).await?;
        let existing_block_present = existing_block_bytes.is_some();
        let mut block_record = if let Some(bytes) = existing_block_bytes.as_ref() {
            let record: DurableBlockCandidateRecord =
                decode(&bytes, "durable safety block candidate")?;
            validate_durable_block_candidate(&record)?;
            if record.height != proposal.height
                || record.block_id != block_id
                || !record.block.matches(block)
            {
                bail!("durable safety block candidate conflicts with immutable block");
            }
            record
        } else {
            DurableBlockCandidateRecord {
                format_version: SAFETY_RECORD_FORMAT_VERSION,
                height: proposal.height,
                block_id,
                block: DurableCompactBlock::from_block(block),
                attempt_rounds: Vec::new(),
                payload_hash: Hash::default(),
            }
        };
        let mut height_index = if let Some(bytes) = self.db.get(&height_index_key).await? {
            let index: DurableHeightAttemptIndex =
                decode(&bytes, "durable safety proposal height index")?;
            validate_durable_height_attempt_index(&index)?;
            if index.height != proposal.height {
                bail!("durable safety proposal height index key does not match payload");
            }
            index
        } else if existing_block_present {
            bail!("durable safety block candidate exists without its per-height attempt index");
        } else {
            DurableHeightAttemptIndex {
                format_version: SAFETY_RECORD_FORMAT_VERSION,
                height: proposal.height,
                attempts: Vec::new(),
                payload_hash: Hash::default(),
            }
        };
        let attempt = DurableProposalAttempt {
            format_version: SAFETY_RECORD_FORMAT_VERSION,
            height: proposal.height,
            round: proposal.round,
            block_id,
            proposal: proposal.clone(),
            derived_randomness,
            payload_hash: Hash::default(),
        };
        if let Some(bytes) = self.db.get(&attempt_key).await? {
            let existing: DurableProposalAttempt =
                decode(&bytes, "durable safety proposal attempt")?;
            validate_durable_proposal_attempt(&existing)?;
            if existing != attempt_with_hash(&attempt)? {
                bail!("durable safety proposal attempt conflicts with existing attempt");
            }
        }
        let current_attempt_ref = DurableAttemptRef {
            round: proposal.round,
            block_id,
        };
        let has_indexed_attempt = height_index.attempts.contains(&current_attempt_ref);
        if has_indexed_attempt != self.db.get(&attempt_key).await?.is_some() {
            bail!("durable safety proposal attempt index disagrees with its record");
        }
        if !has_indexed_attempt {
            height_index.attempts.push(current_attempt_ref.clone());
            height_index.attempts.sort_by(compare_durable_attempt_refs);
        }
        let is_new_attempt = !has_indexed_attempt;
        let is_new_round = !block_record.attempt_rounds.contains(&proposal.round);
        if is_new_round {
            let max_rounds = usize::try_from(self.resource_limits.max_durable_attempts_per_height)
                .map_err(|_| anyhow!("durable proposal attempt bound exceeds platform limits"))?;
            if block_record.attempt_rounds.len() >= max_rounds {
                bail!("durable proposal attempt count exceeds the protocol bound");
            }
        }
        let max_attempts = usize::try_from(self.resource_limits.max_durable_attempts_per_height)
            .map_err(|_| anyhow!("durable proposal attempt bound exceeds platform limits"))?;
        if is_new_attempt && height_index.attempts.len() > max_attempts {
            bail!("durable proposal attempt count exceeds the protocol bound");
        }
        if is_new_round {
            block_record.attempt_rounds.push(proposal.round);
            block_record.attempt_rounds.sort_unstable();
        }
        block_record.payload_hash = durable_block_payload_hash(&block_record)?;
        let attempt = attempt_with_hash(&attempt)?;
        let encoded_block_record = encode(&block_record)?;
        let encoded_attempt = encode(&attempt)?;
        if is_new_attempt {
            let mut durable_bytes = encoded_block_record.len();
            let mut indexed_block_ids = HashSet::new();
            indexed_block_ids.insert(block_id);
            for attempt_ref in &height_index.attempts {
                indexed_block_ids.insert(attempt_ref.block_id);
            }
            for indexed_block_id in indexed_block_ids {
                if indexed_block_id == block_id {
                    continue;
                }
                let existing_block = self
                    .db
                    .get(&safety_block_candidate_key(
                        proposal.height,
                        indexed_block_id,
                    ))
                    .await?
                    .ok_or_else(|| anyhow!("durable attempt index references a missing block"))?;
                let existing_block: DurableBlockCandidateRecord =
                    decode(&existing_block, "durable safety block candidate")?;
                validate_durable_block_candidate(&existing_block)?;
                durable_bytes = durable_bytes
                    .checked_add(encode(&existing_block)?.len())
                    .ok_or_else(|| anyhow!("durable attempt byte count overflow"))?;
            }
            for attempt_ref in &height_index.attempts {
                let attempt_bytes = if attempt_ref == &current_attempt_ref {
                    encoded_attempt.clone()
                } else {
                    self.db
                        .get(&safety_proposal_attempt_key(
                            proposal.height,
                            attempt_ref.round,
                            attempt_ref.block_id,
                        ))
                        .await?
                        .ok_or_else(|| {
                            anyhow!("durable attempt index references a missing record")
                        })?
                };
                let existing_attempt: DurableProposalAttempt =
                    decode(&attempt_bytes, "durable safety proposal attempt")?;
                validate_durable_proposal_attempt(&existing_attempt)?;
                if existing_attempt.height != proposal.height
                    || existing_attempt.round != attempt_ref.round
                    || existing_attempt.block_id != attempt_ref.block_id
                {
                    bail!("durable attempt index references a mismatched record");
                }
                durable_bytes = durable_bytes
                    .checked_add(attempt_bytes.len())
                    .ok_or_else(|| anyhow!("durable attempt byte count overflow"))?;
            }
            let max_bytes =
                usize::try_from(self.resource_limits.max_durable_attempt_bytes_per_height)
                    .map_err(|_| anyhow!("durable attempt byte bound exceeds platform limits"))?;
            if durable_bytes > max_bytes {
                bail!("durable proposal attempt bytes exceed the protocol bound");
            }
        }
        if !is_new_attempt {
            return Ok(());
        }
        height_index.payload_hash = durable_height_attempt_index_payload_hash(&height_index)?;
        self.db
            .batch_insert(
                &[block_key, attempt_key, height_index_key],
                &[
                    encoded_block_record,
                    encoded_attempt,
                    encode(&height_index)?,
                ],
            )
            .await
    }

    pub async fn recover_safety_candidate(
        &self,
        height: u64,
        block_id: BlockId,
    ) -> Result<Option<DurableSafetyCandidate>> {
        Ok(self
            .recover_safety_candidates(height, block_id)
            .await?
            .into_iter()
            .max_by_key(|candidate| candidate.proposal.round))
    }

    pub async fn recover_safety_candidates(
        &self,
        height: u64,
        block_id: BlockId,
    ) -> Result<Vec<DurableSafetyCandidate>> {
        let Some(block_record) = self.recover_safety_block(height, block_id).await? else {
            return Ok(Vec::new());
        };
        let Some(index_bytes) = self
            .db
            .get(&safety_height_attempt_index_key(height))
            .await?
        else {
            bail!("durable safety block candidate has no per-height attempt index");
        };
        let index: DurableHeightAttemptIndex =
            decode(&index_bytes, "durable safety proposal height index")?;
        validate_durable_height_attempt_index(&index)?;
        if index.height != height
            || index.attempts.len()
                > usize::try_from(self.resource_limits.max_durable_attempts_per_height).map_err(
                    |_| anyhow!("durable proposal attempt bound exceeds platform limits"),
                )?
        {
            bail!("durable safety proposal height attempt bound is exceeded");
        }
        let mut indexed_rounds = index
            .attempts
            .iter()
            .filter(|attempt| attempt.block_id == block_id)
            .map(|attempt| attempt.round)
            .collect::<Vec<_>>();
        indexed_rounds.sort_unstable();
        if indexed_rounds != block_record.attempt_rounds {
            bail!("durable block candidate disagrees with its height attempt index");
        }
        let mut candidates = Vec::with_capacity(block_record.attempt_rounds.len());
        for round in block_record.attempt_rounds {
            let Some(attempt) = self
                .recover_safety_candidate_attempt(height, round, block_id)
                .await?
            else {
                bail!("durable safety block candidate references a missing proposal attempt");
            };
            candidates.push(DurableSafetyCandidate {
                proposal: attempt.proposal,
                block: block_record.block.clone(),
                derived_randomness: attempt.derived_randomness,
            });
        }
        Ok(candidates)
    }

    pub async fn recover_safety_candidate_attempt(
        &self,
        height: u64,
        round: u32,
        block_id: BlockId,
    ) -> Result<Option<DurableProposalAttempt>> {
        let Some(bytes) = self
            .db
            .get(&safety_proposal_attempt_key(height, round, block_id))
            .await?
        else {
            return Ok(None);
        };
        let attempt: DurableProposalAttempt = decode(&bytes, "durable safety proposal attempt")?;
        validate_durable_proposal_attempt(&attempt)?;
        if attempt.height != height || attempt.round != round || attempt.block_id != block_id {
            bail!("durable safety proposal attempt key does not match payload");
        }
        Ok(Some(attempt))
    }

    pub async fn recover_safety_block(
        &self,
        height: u64,
        block_id: BlockId,
    ) -> Result<Option<DurableBlockCandidate>> {
        let Some(record) = self.recover_safety_block_record(height, block_id).await? else {
            return Ok(None);
        };
        let block = self
            .candidate_body(height, block_id, &record.block)?
            .ok_or_else(|| anyhow!("durable candidate body must be rehydrated from a peer"))?;
        Ok(Some(DurableBlockCandidate {
            format_version: record.format_version,
            height: record.height,
            block_id: record.block_id,
            block,
            attempt_rounds: record.attempt_rounds,
            payload_hash: record.payload_hash,
        }))
    }

    async fn recover_safety_block_record(
        &self,
        height: u64,
        block_id: BlockId,
    ) -> Result<Option<DurableBlockCandidateRecord>> {
        let Some(bytes) = self
            .db
            .get(&safety_block_candidate_key(height, block_id))
            .await?
        else {
            return Ok(None);
        };
        let record: DurableBlockCandidateRecord = decode(&bytes, "durable safety block candidate")?;
        validate_durable_block_candidate(&record)?;
        if record.height != height || record.block_id != block_id {
            bail!("durable safety block candidate key does not match payload");
        }
        Ok(Some(record))
    }

    /// Candidate cleanup is intentionally only allowed after the finalized
    /// transaction has been durably committed.
    pub async fn clear_safety_candidate(&self, height: u64, block_id: BlockId) -> Result<()> {
        let _candidate_guard = self.candidate_write_lock.lock().await;
        let mut keys = vec![
            safety_block_candidate_key(height, block_id),
            safety_candidate_key(height, block_id),
        ];
        if let Some(record) = self.recover_safety_block_record(height, block_id).await? {
            keys.extend(
                record
                    .attempt_rounds
                    .into_iter()
                    .map(|round| safety_proposal_attempt_key(height, round, block_id)),
            );
        }
        let index_key = safety_height_attempt_index_key(height);
        let mut insert_keys = Vec::new();
        let mut insert_values = Vec::new();
        if let Some(index_bytes) = self.db.get(&index_key).await? {
            let mut index: DurableHeightAttemptIndex =
                decode(&index_bytes, "durable safety proposal height index")?;
            validate_durable_height_attempt_index(&index)?;
            if index.height != height {
                bail!("durable safety proposal height index key does not match payload");
            }
            index
                .attempts
                .retain(|attempt| attempt.block_id != block_id);
            if index.attempts.is_empty() {
                keys.push(index_key);
            } else {
                index.payload_hash = durable_height_attempt_index_payload_hash(&index)?;
                insert_keys.push(index_key);
                insert_values.push(encode(&index)?);
            }
        } else if self
            .db
            .get(&safety_block_candidate_key(height, block_id))
            .await?
            .is_some()
        {
            bail!("durable safety block candidate exists without its per-height attempt index");
        }
        self.db
            .batch_write(&insert_keys, &insert_values, &keys)
            .await?;
        self.candidate_bodies
            .lock()
            .map_err(|_| anyhow!("candidate body cache lock is poisoned"))?
            .remove(&(height, block_id));
        Ok(())
    }

    /// Commit a finalized block and all of its finality markers atomically.
    ///
    /// The DB implementation must provide one-tree batch semantics. If the
    /// call returns an error after the underlying apply/flush boundary, this
    /// method deliberately does not update any in-memory state. A retry or a
    /// restart resolves the ambiguity by observing the durable markers.
    pub async fn commit_finalized_transaction(
        &self,
        finalized: &FinalizedBlockV2,
    ) -> Result<FinalityCommitResult> {
        self.commit_finalized_transaction_with_state(finalized, &[])
            .await
    }

    pub async fn commit_finalized_transaction_with_state(
        &self,
        finalized: &FinalizedBlockV2,
        state_write_values: &[Vec<u8>],
    ) -> Result<FinalityCommitResult> {
        self.commit_finalized_transaction_with_state_and_checkpoint(
            finalized,
            state_write_values,
            None,
        )
        .await
    }

    pub async fn commit_finalized_transaction_with_state_and_checkpoint(
        &self,
        finalized: &FinalizedBlockV2,
        state_write_values: &[Vec<u8>],
        checkpoint: Option<&CanonicalStateCheckpoint>,
    ) -> Result<FinalityCommitResult> {
        self.commit_finalized_transaction_with_state_and_checkpoint_and_snapshot(
            finalized,
            state_write_values,
            checkpoint,
            None,
        )
        .await
    }

    pub async fn commit_finalized_transaction_with_state_and_checkpoint_and_snapshot(
        &self,
        finalized: &FinalizedBlockV2,
        state_write_values: &[Vec<u8>],
        checkpoint: Option<&CanonicalStateCheckpoint>,
        next_snapshot: Option<&StakeSnapshot>,
    ) -> Result<FinalityCommitResult> {
        let _candidate_guard = self.candidate_write_lock.lock().await;
        let id = FinalizeTransactionId::from_v2(finalized);
        if finalized.block.header.height < 0
            || finalized.block.header.height as u64 != id.height
            || finalized.commit.block_id != BlockId(finalized.block.header.block_hash)
            || finalized.commit.height != id.height
        {
            bail!("finalized transaction identity does not match block/certificate");
        }

        let transaction_ids = finalized
            .block
            .transactions
            .iter()
            .map(|tx| tx.transaction_id)
            .collect::<Vec<TransactionId>>();
        let unique_transaction_ids = transaction_ids.iter().copied().collect::<HashSet<_>>();
        if unique_transaction_ids.len() != transaction_ids.len() {
            bail!("finalized block contains duplicate transaction IDs");
        }

        let height_key = height_key(id.height);
        if let Some(existing_bytes) = self.db.get(&height_key).await? {
            let existing: FinalizeTransactionId = decode(&existing_bytes, "height marker")?;
            if existing.height != id.height || existing.block_id != id.block_id {
                bail!(
                    "height {} is already finalized by a different block or certificate",
                    id.height
                );
            }

            // More than one valid quorum certificate can be formed for the
            // same block when different validators observe the quorum at
            // slightly different times.  The durable record keeps the first
            // certificate, while a later certificate for the same block is a
            // harmless equivalent finality result.  A different block at the
            // same height remains a fail-closed conflict above.
            if existing != id {
                let persisted = self
                    .recover_finalized_v2(id.height)
                    .await?
                    .ok_or_else(|| anyhow!("finalized height marker has no complete record"))?;
                if persisted.commit.block_id != id.block_id {
                    bail!(
                        "height {} is already finalized by a different block or certificate",
                        id.height
                    );
                }
                return Ok(FinalityCommitResult::AlreadyCommitted);
            }

            let required = self.required_keys(
                finalized,
                &transaction_ids,
                &id,
                state_write_values.len(),
                checkpoint.is_some(),
            )?;
            let present = self.read_presence(&required).await?;
            if !present.iter().all(|present| *present) {
                bail!("finalized transaction has an incomplete durable marker set");
            }
            self.verify_state_writes(id.height, state_write_values)
                .await?;
            if let Some(checkpoint) = checkpoint {
                self.verify_checkpoint(id.height, checkpoint).await?;
            }
            if let Some(snapshot) = next_snapshot {
                let stored_snapshot = self
                    .recover_snapshot(snapshot.epoch)
                    .await?
                    .ok_or_else(|| anyhow!("durable next validator snapshot is missing"))?;
                if stored_snapshot != *snapshot {
                    bail!("durable next validator snapshot conflicts with retry");
                }
            }
            let expected_tip =
                CanonicalFinalizedTip::from_finalized_with_next_snapshot(finalized, next_snapshot)
                    .map_err(|error| anyhow!(error.to_string()))?;
            let actual_tip = self
                .recover_canonical_tip()
                .await?
                .ok_or_else(|| anyhow!("canonical finalized tip disappeared"))?;
            if actual_tip != expected_tip {
                bail!("canonical finalized tip conflicts with durable finality record");
            }
            let persisted = decode_finalized_record(
                &self
                    .db
                    .get(&record_key(finalized.block.header.block_hash))
                    .await?
                    .ok_or_else(|| anyhow!("finalized record marker disappeared"))?,
                "finalized record",
            )?;
            if persisted != *finalized {
                bail!("finalized transaction payload conflicts with durable record");
            }
            return Ok(FinalityCommitResult::AlreadyCommitted);
        }

        let current_tip = self
            .recover_canonical_tip()
            .await?
            .ok_or_else(|| anyhow!("canonical finalized tip is not initialized"))?;
        let expected_height = current_tip
            .next_height()
            .map_err(|error| anyhow!(error.to_string()))?;
        if id.height != expected_height {
            bail!(
                "finalized height {} is not the direct successor of canonical tip {}",
                id.height,
                current_tip.height
            );
        }
        if finalized.block.header.prev_block_hash != current_tip.block_id.0 {
            bail!("finalized block does not point to the canonical finalized tip");
        }
        if finalized.block.header.protocol_version.0 >= 5 {
            if finalized.block.header.timestamp < 0 {
                bail!("finalized block timestamp must be non-negative");
            }
            if finalized.block.header.timestamp <= current_tip.timestamp {
                bail!("finalized block timestamp must be greater than the canonical parent");
            }
            let max_step = i64::try_from(self.resource_limits.max_block_timestamp_step)
                .map_err(|_| anyhow!("block timestamp step exceeds i64 range"))?;
            let max_timestamp = current_tip
                .timestamp
                .checked_add(max_step)
                .ok_or_else(|| anyhow!("finalized timestamp upper bound overflow"))?;
            if finalized.block.header.timestamp > max_timestamp {
                bail!("finalized block timestamp exceeds the protocol parent-relative bound");
            }
        }
        if finalized.block.header.parent_randomness != current_tip.next_randomness {
            bail!("finalized block parent randomness does not match canonical tip");
        }
        if (finalized.block.header.epoch as u64) < current_tip.epoch {
            bail!("finalized block epoch regresses canonical tip epoch");
        }

        let encoded_id = encode(&id)?;
        // Transaction signatures, IDs and chain fields dominate a full block
        // record and are highly redundant in their serde/bincode form. Sled
        // stores values of this size as external blobs; writing the raw value
        // caused roughly 6 MiB per 8192-transfer block and periodic multi-
        // second blob compaction on SD cards. Level-1 zstd keeps the durable
        // atomic record intact while materially reducing write amplification.
        let encoded_finalized = encode_finalized_record(finalized)?;
        let encoded_certificate = encode(&finalized.commit)?;
        let encoded_consensus_state = encode(&finalized.consensus_state)?;
        let encoded_state_write_count = encode(&state_write_values.len())?;
        let canonical_entries = if checkpoint.is_some() {
            canonical_state_entries(state_write_values)?
        } else {
            Vec::new()
        };

        let mut keys = vec![
            height_key,
            record_key(finalized.block.header.block_hash),
            certificate_key(id.height),
            CONSENSUS_STATE_KEY.to_vec(),
            state_write_count_key(id.height),
        ];
        let mut values = vec![
            encoded_id.clone(),
            encoded_finalized,
            encoded_certificate,
            encoded_consensus_state,
            encoded_state_write_count,
        ];
        for (index, value) in state_write_values.iter().enumerate() {
            keys.push(state_write_key(id.height, index));
            values.push(value.clone());
        }
        for (key, value) in canonical_entries {
            keys.push(key);
            values.push(value);
        }
        if let Some(checkpoint) = checkpoint {
            let encoded_checkpoint = encode(checkpoint)?;
            let checkpoint_content_key = checkpoint_content_key(&checkpoint.state_root);
            match self.db.get(&checkpoint_content_key).await? {
                Some(existing) if existing != encoded_checkpoint => {
                    bail!("canonical checkpoint content conflicts with its state root")
                }
                Some(_) => {}
                None => {
                    keys.push(checkpoint_content_key);
                    values.push(encoded_checkpoint);
                }
            }
            keys.push(STATE_ROOT_KEY.to_vec());
            values.push(encode(&checkpoint.state_root)?);
            keys.push(state_root_key(id.height));
            values.push(encode(&checkpoint.state_root)?);
            keys.push(STATE_CHECKPOINT_KEY.to_vec());
            values.push(encode(&checkpoint.state_root)?);
            keys.push(checkpoint_key(id.height));
            values.push(encode(&checkpoint.state_root)?);
        }
        if let Some(snapshot) = next_snapshot {
            keys.push(snapshot_key(snapshot.epoch));
            values.push(encode(snapshot)?);
        }
        let new_tip =
            CanonicalFinalizedTip::from_finalized_with_next_snapshot(finalized, next_snapshot)
                .map_err(|error| anyhow!(error.to_string()))?;
        keys.push(TIP_KEY.to_vec());
        values.push(encode(&new_tip)?);

        // Candidate cleanup is part of the same mixed single-tree batch as
        // finality. A crash therefore leaves either the old candidate set plus
        // no finalized marker, or the complete finalized transaction with all
        // candidate material for the finalized height removed.
        let mut safety_delete_keys = Vec::new();
        let height_index_key = safety_height_attempt_index_key(id.height);
        if let Some(index_bytes) = self.db.get(&height_index_key).await? {
            let index: DurableHeightAttemptIndex =
                decode(&index_bytes, "durable safety proposal height index")?;
            validate_durable_height_attempt_index(&index)?;
            if index.height != id.height {
                bail!("durable safety proposal height index key does not match payload");
            }
            let mut indexed_block_ids = HashSet::new();
            for attempt_ref in &index.attempts {
                indexed_block_ids.insert(attempt_ref.block_id);
                safety_delete_keys.push(safety_proposal_attempt_key(
                    id.height,
                    attempt_ref.round,
                    attempt_ref.block_id,
                ));
            }
            for indexed_block_id in indexed_block_ids {
                let record = self
                    .recover_safety_block_record(id.height, indexed_block_id)
                    .await?
                    .ok_or_else(|| anyhow!("durable attempt index references a missing block"))?;
                let mut indexed_rounds = index
                    .attempts
                    .iter()
                    .filter(|attempt| attempt.block_id == indexed_block_id)
                    .map(|attempt| attempt.round)
                    .collect::<Vec<_>>();
                indexed_rounds.sort_unstable();
                if record.attempt_rounds != indexed_rounds {
                    bail!("durable block candidate disagrees with its height attempt index");
                }
                safety_delete_keys.push(safety_candidate_key(id.height, indexed_block_id));
                safety_delete_keys.push(safety_block_candidate_key(id.height, indexed_block_id));
            }
            safety_delete_keys.push(height_index_key);
        } else if self
            .db
            .get(&safety_block_candidate_key(id.height, id.block_id))
            .await?
            .is_some()
        {
            bail!("durable safety block candidate exists without its per-height attempt index");
        } else {
            safety_delete_keys.push(safety_candidate_key(id.height, id.block_id));
        }
        self.db
            .batch_write(&keys, &values, &safety_delete_keys)
            .await?;
        self.candidate_bodies
            .lock()
            .map_err(|_| anyhow!("candidate body cache lock is poisoned"))?
            .retain(|(height, _), _| *height != id.height);
        Ok(FinalityCommitResult::Applied)
    }

    /// Reconcile a failed finality write by inspecting the durable marker
    /// set. This method never infers persistence from the original error and
    /// never mutates memory. It is safe to call after a crash or an ambiguous
    /// flush result with the exact same finalized payload.
    pub async fn reconcile_finalized_transaction(
        &self,
        finalized: &FinalizedBlockV2,
        state_write_values: &[Vec<u8>],
        checkpoint: Option<&CanonicalStateCheckpoint>,
        next_snapshot: Option<&StakeSnapshot>,
    ) -> Result<DurableCommitOutcome> {
        let id = FinalizeTransactionId::from_v2(finalized);
        let transaction_ids = finalized
            .block
            .transactions
            .iter()
            .map(|tx| tx.transaction_id)
            .collect::<Vec<_>>();
        let required = self.required_keys(
            finalized,
            &transaction_ids,
            &id,
            state_write_values.len(),
            checkpoint.is_some(),
        )?;
        let mut probe_keys = required.clone();
        if let Some(snapshot) = next_snapshot {
            probe_keys.push(snapshot_key(snapshot.epoch));
        }

        let height_marker = self.db.get(&height_key(id.height)).await?;
        let Some(height_bytes) = height_marker else {
            // TIP_KEY is always present after Genesis initialization and is
            // therefore excluded from this partial-write probe.
            let mut any_non_tip = false;
            for key in probe_keys.iter().filter(|key| key.as_slice() != TIP_KEY) {
                if self.db.get(key).await?.is_some() {
                    any_non_tip = true;
                    break;
                }
            }
            return Ok(if any_non_tip {
                DurableCommitOutcome::Indeterminate
            } else {
                DurableCommitOutcome::NotApplied
            });
        };

        let stored_id: FinalizeTransactionId = match decode(&height_bytes, "height marker") {
            Ok(id) => id,
            Err(_) => return Ok(DurableCommitOutcome::Indeterminate),
        };
        if stored_id != id {
            if stored_id.height == id.height && stored_id.block_id == id.block_id {
                let persisted = self.recover_finalized_v2(id.height).await?;
                if persisted
                    .as_ref()
                    .is_some_and(|finalized| finalized.commit.block_id == id.block_id)
                {
                    return Ok(DurableCommitOutcome::AlreadyApplied);
                }
            }
            return Ok(DurableCommitOutcome::Indeterminate);
        }
        if !self
            .read_presence(&probe_keys)
            .await?
            .iter()
            .all(|present| *present)
        {
            return Ok(DurableCommitOutcome::Indeterminate);
        }
        if self
            .verify_state_writes(id.height, state_write_values)
            .await
            .is_err()
        {
            return Ok(DurableCommitOutcome::Indeterminate);
        }
        if let Some(checkpoint) = checkpoint {
            if self.verify_checkpoint(id.height, checkpoint).await.is_err() {
                return Ok(DurableCommitOutcome::Indeterminate);
            }
        }
        if let Some(snapshot) = next_snapshot {
            let stored = self
                .recover_snapshot(snapshot.epoch)
                .await?
                .ok_or_else(|| anyhow!("durable next validator snapshot is missing"))?;
            if stored != *snapshot {
                return Ok(DurableCommitOutcome::Indeterminate);
            }
        }
        let expected_tip =
            CanonicalFinalizedTip::from_finalized_with_next_snapshot(finalized, next_snapshot)
                .map_err(|error| anyhow!(error.to_string()))?;
        if self.recover_canonical_tip().await? != Some(expected_tip) {
            return Ok(DurableCommitOutcome::Indeterminate);
        }
        let Some(record_bytes) = self.db.get(&record_key(id.block_id.0)).await? else {
            return Ok(DurableCommitOutcome::Indeterminate);
        };
        let persisted = match decode_finalized_record(&record_bytes, "finalized record") {
            Ok(record) => record,
            Err(_) => return Ok(DurableCommitOutcome::Indeterminate),
        };
        if persisted != *finalized {
            return Ok(DurableCommitOutcome::Indeterminate);
        }
        Ok(DurableCommitOutcome::Applied)
    }

    /// Recover a complete finalized V2 record. A height marker without its
    /// record is corruption and fails closed instead of guessing the result
    /// from a previous error value.
    pub async fn recover_finalized_v2(&self, height: u64) -> Result<Option<FinalizedBlockV2>> {
        Ok(self
            .recover_finalized_v2_with_state(height)
            .await?
            .map(|(finalized, _)| finalized))
    }

    pub async fn recover_finalized_v2_with_state(
        &self,
        height: u64,
    ) -> Result<Option<(FinalizedBlockV2, Vec<Vec<u8>>)>> {
        Ok(self
            .recover_finalized_v2_with_state_and_checkpoint(height)
            .await?
            .map(|(finalized, writes, _)| (finalized, writes)))
    }

    pub async fn recover_finalized_v2_with_state_and_checkpoint(
        &self,
        height: u64,
    ) -> Result<
        Option<(
            FinalizedBlockV2,
            Vec<Vec<u8>>,
            Option<CanonicalStateCheckpoint>,
        )>,
    > {
        let Some(id_bytes) = self.db.get(&height_key(height)).await? else {
            return Ok(None);
        };
        let id: FinalizeTransactionId = decode(&id_bytes, "height marker")?;
        if id.height != height {
            bail!("finalized height marker contains a different height");
        }
        let Some(record_bytes) = self.db.get(&record_key(id.block_id.0)).await? else {
            bail!("finalized height marker has no finalized record");
        };
        let finalized = decode_finalized_record(&record_bytes, "finalized record")?;
        if FinalizeTransactionId::from_v2(&finalized) != id {
            bail!("finalized record does not match its durable identity");
        }
        let transaction_ids = finalized
            .block
            .transactions
            .iter()
            .map(|tx| tx.transaction_id)
            .collect::<Vec<_>>();
        let state_write_count: usize = decode(
            &self
                .db
                .get(&state_write_count_key(id.height))
                .await?
                .ok_or_else(|| anyhow!("finalized state write count is missing"))?,
            "state write count",
        )?;
        let checkpoint: Option<CanonicalStateCheckpoint> =
            if let Some(bytes) = self.db.get(&checkpoint_key(id.height)).await? {
                let checkpoint_root: Hash = decode(&bytes, "canonical state checkpoint reference")?;
                let checkpoint_bytes = self
                    .db
                    .get(&checkpoint_content_key(&checkpoint_root))
                    .await?
                    .ok_or_else(|| anyhow!("canonical state checkpoint content is missing"))?;
                let checkpoint: CanonicalStateCheckpoint =
                    decode(&checkpoint_bytes, "canonical state checkpoint content")?;
                if checkpoint.state_root != checkpoint_root {
                    bail!("canonical state checkpoint content does not match its reference");
                }
                Some(checkpoint)
            } else {
                None
            };
        if let Some(ref checkpoint) = checkpoint {
            let stored_root: Hash = decode(
                &self
                    .db
                    .get(&state_root_key(id.height))
                    .await?
                    .ok_or_else(|| anyhow!("canonical state root is missing"))?,
                "canonical state root",
            )?;
            if stored_root != checkpoint.state_root {
                bail!("canonical state root does not match its checkpoint");
            }
        }
        let required = self.required_keys(
            &finalized,
            &transaction_ids,
            &id,
            state_write_count,
            checkpoint.is_some(),
        )?;
        if !self
            .read_presence(&required)
            .await?
            .iter()
            .all(|present| *present)
        {
            bail!("durable finalized state has an incomplete marker set");
        }
        let mut state_writes = Vec::with_capacity(state_write_count);
        for index in 0..state_write_count {
            state_writes.push(
                self.db
                    .get(&state_write_key(height, index))
                    .await?
                    .ok_or_else(|| anyhow!("finalized state write is missing"))?,
            );
        }
        if let Some(ref checkpoint) = checkpoint {
            if checkpoint.state_root != finalized.block.header.state_root {
                bail!("canonical state checkpoint root does not match finalized block");
            }
        }
        Ok(Some((finalized, state_writes, checkpoint)))
    }

    pub async fn recover_finalized_tip(&self) -> Result<Option<FinalizedBlockV2>> {
        let Some(tip) = self.recover_canonical_tip().await? else {
            return Ok(None);
        };
        self.recover_finalized_v2(tip.height).await
    }

    pub async fn recover_finalized_tip_with_state(
        &self,
    ) -> Result<Option<(FinalizedBlockV2, Vec<Vec<u8>>)>> {
        let Some(tip) = self.recover_canonical_tip().await? else {
            return Ok(None);
        };
        self.recover_finalized_v2_with_state(tip.height).await
    }

    pub async fn recover_finalized_tip_with_state_and_checkpoint(
        &self,
    ) -> Result<
        Option<(
            FinalizedBlockV2,
            Vec<Vec<u8>>,
            Option<CanonicalStateCheckpoint>,
        )>,
    > {
        let Some(tip) = self.recover_canonical_tip().await? else {
            return Ok(None);
        };
        self.recover_finalized_v2_with_state_and_checkpoint(tip.height)
            .await
    }

    fn required_keys(
        &self,
        finalized: &FinalizedBlockV2,
        _transaction_ids: &[TransactionId],
        id: &FinalizeTransactionId,
        state_write_count: usize,
        has_checkpoint: bool,
    ) -> Result<Vec<Vec<u8>>> {
        let mut keys = vec![
            height_key(id.height),
            record_key(finalized.block.header.block_hash),
            certificate_key(id.height),
            CONSENSUS_STATE_KEY.to_vec(),
            state_write_count_key(id.height),
            TIP_KEY.to_vec(),
        ];
        keys.extend((0..state_write_count).map(|index| state_write_key(id.height, index)));
        if has_checkpoint {
            keys.push(STATE_ROOT_KEY.to_vec());
            keys.push(state_root_key(id.height));
            keys.push(STATE_CHECKPOINT_KEY.to_vec());
            keys.push(checkpoint_key(id.height));
            keys.push(checkpoint_content_key(&finalized.block.header.state_root));
        }
        Ok(keys)
    }

    async fn verify_checkpoint(
        &self,
        height: u64,
        expected: &CanonicalStateCheckpoint,
    ) -> Result<()> {
        let stored = self
            .db
            .get(&checkpoint_key(height))
            .await?
            .ok_or_else(|| anyhow!("canonical state checkpoint is missing"))?;
        let checkpoint_root: Hash = decode(&stored, "canonical state checkpoint reference")?;
        if checkpoint_root != expected.state_root {
            bail!("canonical state checkpoint reference conflicts with durable state");
        }
        let checkpoint_bytes = self
            .db
            .get(&checkpoint_content_key(&checkpoint_root))
            .await?
            .ok_or_else(|| anyhow!("canonical state checkpoint content is missing"))?;
        let actual: CanonicalStateCheckpoint =
            decode(&checkpoint_bytes, "canonical state checkpoint content")?;
        if actual != *expected {
            bail!("canonical state checkpoint conflicts with durable state");
        }
        let stored_root: Hash = decode(
            &self
                .db
                .get(&state_root_key(height))
                .await?
                .ok_or_else(|| anyhow!("canonical state root is missing"))?,
            "canonical state root",
        )?;
        if stored_root != expected.state_root {
            bail!("canonical state root conflicts with durable checkpoint");
        }
        Ok(())
    }

    async fn verify_state_writes(&self, height: u64, expected: &[Vec<u8>]) -> Result<()> {
        let count: usize = decode(
            &self
                .db
                .get(&state_write_count_key(height))
                .await?
                .ok_or_else(|| anyhow!("finalized state write count is missing"))?,
            "state write count",
        )?;
        if count != expected.len() {
            bail!("finalized transaction state write count conflicts with durable record");
        }
        for (index, value) in expected.iter().enumerate() {
            let stored = self
                .db
                .get(&state_write_key(height, index))
                .await?
                .ok_or_else(|| anyhow!("finalized state write is missing"))?;
            if stored != *value {
                bail!("finalized transaction state write conflicts with durable record");
            }
        }
        Ok(())
    }

    async fn read_presence(&self, keys: &[Vec<u8>]) -> Result<Vec<bool>> {
        Ok(self
            .db
            .batch_get(keys)
            .await?
            .into_iter()
            .map(|value| value.is_some())
            .collect())
    }
}

fn height_key(height: u64) -> Vec<u8> {
    key_with_bytes(HEIGHT_PREFIX, &height.to_be_bytes())
}

fn record_key(block_id: Hash) -> Vec<u8> {
    key_with_bytes(RECORD_PREFIX, &block_id.0)
}

fn certificate_key(height: u64) -> Vec<u8> {
    key_with_bytes(CERTIFICATE_PREFIX, &height.to_be_bytes())
}

fn state_write_count_key(height: u64) -> Vec<u8> {
    key_with_bytes(STATE_WRITE_COUNT_PREFIX, &height.to_be_bytes())
}

fn state_write_key(height: u64, index: usize) -> Vec<u8> {
    let mut suffix = height.to_be_bytes().to_vec();
    suffix.extend_from_slice(&(index as u64).to_be_bytes());
    key_with_bytes(STATE_WRITE_PREFIX, &suffix)
}

fn checkpoint_key(height: u64) -> Vec<u8> {
    key_with_bytes(STATE_CHECKPOINT_PREFIX, &height.to_be_bytes())
}

fn checkpoint_content_key(state_root: &Hash) -> Vec<u8> {
    key_with_bytes(STATE_CHECKPOINT_CONTENT_PREFIX, &state_root.0)
}

fn state_root_key(height: u64) -> Vec<u8> {
    key_with_bytes(STATE_ROOT_PREFIX, &height.to_be_bytes())
}

fn state_account_key(address: &norn_common::types::Address) -> Vec<u8> {
    key_with_bytes(STATE_ACCOUNT_PREFIX, &address.0)
}

fn state_storage_key(address: &norn_common::types::Address, slot: &[u8]) -> Vec<u8> {
    let mut suffix = address.0.to_vec();
    suffix.push(b'/');
    suffix.extend_from_slice(slot);
    key_with_bytes(STATE_STORAGE_PREFIX, &suffix)
}

fn state_code_key(hash: &Hash) -> Vec<u8> {
    key_with_bytes(STATE_CODE_PREFIX, &hash.0)
}

fn snapshot_key(epoch: u64) -> Vec<u8> {
    key_with_bytes(SNAPSHOT_PREFIX, &epoch.to_be_bytes())
}

fn canonical_state_entries(state_write_values: &[Vec<u8>]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut entries = std::collections::BTreeMap::<Vec<u8>, Vec<u8>>::new();
    // The content-addressed checkpoint is the authoritative recovery image.
    // The granular namespace only needs this block's delta; rewriting every
    // account on every (including empty) block caused severe Sled write
    // amplification and multi-second SD-card stalls.
    for encoded in state_write_values {
        let write: OverlayWrite = decode(encoded, "canonical overlay write")?;
        match write {
            OverlayWrite::Account {
                address, new_state, ..
            } => {
                entries.insert(state_account_key(&address), encode(&new_state)?);
            }
            OverlayWrite::Storage {
                address,
                key,
                new_value,
                ..
            } => {
                entries.insert(
                    state_storage_key(&address, &key),
                    if new_value.is_empty() {
                        STATE_TOMBSTONE.to_vec()
                    } else {
                        new_value
                    },
                );
            }
            OverlayWrite::Code {
                new_hash,
                code,
                deleted,
                ..
            } => {
                if !deleted {
                    entries.insert(state_code_key(&new_hash), code);
                }
            }
        }
    }
    Ok(entries.into_iter().collect())
}

fn key_with_bytes(prefix: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut key = prefix.to_vec();
    key.extend_from_slice(suffix);
    key
}

fn pending_proposal_key(height: u64, round: u32) -> Vec<u8> {
    let mut key = PENDING_PROPOSAL_PREFIX.to_vec();
    key.extend_from_slice(&height.to_be_bytes());
    key.extend_from_slice(&round.to_be_bytes());
    key
}

fn safety_candidate_key(height: u64, block_id: BlockId) -> Vec<u8> {
    let mut key = SAFETY_CANDIDATE_PREFIX.to_vec();
    key.extend_from_slice(&height.to_be_bytes());
    key.extend_from_slice(&block_id.0 .0);
    key
}

fn safety_block_candidate_key(height: u64, block_id: BlockId) -> Vec<u8> {
    let mut key = SAFETY_BLOCK_CANDIDATE_PREFIX.to_vec();
    key.extend_from_slice(&height.to_be_bytes());
    key.extend_from_slice(&block_id.0 .0);
    key
}

fn safety_proposal_attempt_key(height: u64, round: u32, block_id: BlockId) -> Vec<u8> {
    let mut key = SAFETY_ATTEMPT_PREFIX.to_vec();
    key.extend_from_slice(&height.to_be_bytes());
    key.extend_from_slice(&round.to_be_bytes());
    key.extend_from_slice(&block_id.0 .0);
    key
}

fn safety_height_attempt_index_key(height: u64) -> Vec<u8> {
    let mut key = SAFETY_HEIGHT_INDEX_PREFIX.to_vec();
    key.extend_from_slice(&height.to_be_bytes());
    key
}

fn durable_payload_hash<T: serde::Serialize>(payload: &T) -> Result<Hash> {
    let bytes = encode(payload)?;
    Ok(Hash(Sha256::digest(bytes).into()))
}

fn durable_block_payload_hash(record: &DurableBlockCandidateRecord) -> Result<Hash> {
    durable_payload_hash(&(
        record.format_version,
        record.height,
        record.block_id,
        &record.block,
        &record.attempt_rounds,
    ))
}

fn durable_attempt_payload_hash(record: &DurableProposalAttempt) -> Result<Hash> {
    durable_payload_hash(&(
        record.format_version,
        record.height,
        record.round,
        record.block_id,
        &record.proposal,
        record.derived_randomness,
    ))
}

fn durable_height_attempt_index_payload_hash(index: &DurableHeightAttemptIndex) -> Result<Hash> {
    durable_payload_hash(&(index.format_version, index.height, &index.attempts))
}

fn compare_durable_attempt_refs(
    left: &DurableAttemptRef,
    right: &DurableAttemptRef,
) -> std::cmp::Ordering {
    left.block_id
        .0
         .0
        .cmp(&right.block_id.0 .0)
        .then(left.round.cmp(&right.round))
}

fn validate_durable_block_candidate(record: &DurableBlockCandidateRecord) -> Result<()> {
    if record.format_version != SAFETY_RECORD_FORMAT_VERSION
        || record.block_id != BlockId(record.block.header.block_hash)
        || record.block.header.height < 0
        || record.height != record.block.header.height as u64
        || record
            .attempt_rounds
            .windows(2)
            .any(|rounds| rounds[0] >= rounds[1])
        || record.payload_hash != durable_block_payload_hash(record)?
    {
        bail!("invalid durable safety block candidate record");
    }
    Ok(())
}

fn validate_durable_proposal_attempt(record: &DurableProposalAttempt) -> Result<()> {
    if record.format_version != SAFETY_RECORD_FORMAT_VERSION
        || record.height != record.proposal.height
        || record.round != record.proposal.round
        || record.block_id != record.proposal.block_id
        || record.payload_hash != durable_attempt_payload_hash(record)?
    {
        bail!("invalid durable safety proposal attempt record");
    }
    Ok(())
}

fn validate_durable_height_attempt_index(index: &DurableHeightAttemptIndex) -> Result<()> {
    if index.format_version != SAFETY_RECORD_FORMAT_VERSION
        || index.attempts.windows(2).any(|attempts| {
            compare_durable_attempt_refs(&attempts[0], &attempts[1]) != std::cmp::Ordering::Less
        })
        || index.payload_hash != durable_height_attempt_index_payload_hash(index)?
    {
        bail!("invalid durable safety proposal height index");
    }
    Ok(())
}

fn attempt_with_hash(record: &DurableProposalAttempt) -> Result<DurableProposalAttempt> {
    let mut record = record.clone();
    record.payload_hash = durable_attempt_payload_hash(&record)?;
    Ok(record)
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    norn_common::utils::codec::serialize(value)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8], name: &str) -> Result<T> {
    norn_common::utils::codec::deserialize(bytes)
        .map_err(|error| anyhow!("invalid {}: {}", name, error))
}

fn encode_finalized_record(finalized: &FinalizedBlockV2) -> Result<Vec<u8>> {
    let raw = encode(finalized)?;
    if raw.len() > MAX_DURABLE_FINALITY_RECORD_BYTES {
        bail!("finalized record exceeds the durable encoding limit");
    }
    let compressed = zstd::bulk::compress(&raw, 1)
        .map_err(|error| anyhow!("failed to compress finalized record: {error}"))?;
    let framed_len = COMPRESSED_FINALITY_MAGIC
        .len()
        .checked_add(COMPRESSED_FINALITY_LENGTH_BYTES)
        .and_then(|len| len.checked_add(compressed.len()))
        .ok_or_else(|| anyhow!("compressed finalized record length overflow"))?;
    if framed_len >= raw.len() {
        return Ok(raw);
    }
    let mut framed = Vec::with_capacity(framed_len);
    framed.extend_from_slice(COMPRESSED_FINALITY_MAGIC);
    framed.extend_from_slice(&(raw.len() as u64).to_be_bytes());
    framed.extend_from_slice(&compressed);
    Ok(framed)
}

fn decode_finalized_record(bytes: &[u8], name: &str) -> Result<FinalizedBlockV2> {
    if !bytes.starts_with(COMPRESSED_FINALITY_MAGIC) {
        return decode(bytes, name);
    }
    let length_start = COMPRESSED_FINALITY_MAGIC.len();
    let payload_start = length_start
        .checked_add(COMPRESSED_FINALITY_LENGTH_BYTES)
        .ok_or_else(|| anyhow!("invalid {name}: compressed frame length overflow"))?;
    let declared_len = u64::from_be_bytes(
        bytes
            .get(length_start..payload_start)
            .ok_or_else(|| anyhow!("invalid {name}: truncated compressed frame"))?
            .try_into()
            .map_err(|_| anyhow!("invalid {name}: malformed compressed length"))?,
    );
    let declared_len = usize::try_from(declared_len)
        .map_err(|_| anyhow!("invalid {name}: compressed length exceeds this platform"))?;
    if declared_len == 0 || declared_len > MAX_DURABLE_FINALITY_RECORD_BYTES {
        bail!("invalid {name}: compressed length is outside the durable encoding limit");
    }
    let compressed = bytes
        .get(payload_start..)
        .ok_or_else(|| anyhow!("invalid {name}: compressed payload is missing"))?;
    let raw = zstd::bulk::decompress(compressed, declared_len)
        .map_err(|error| anyhow!("invalid {name}: zstd decompression failed: {error}"))?;
    if raw.len() != declared_len {
        bail!("invalid {name}: decompressed length does not match its frame");
    }
    decode(&raw, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evm::CodeStorageCheckpoint;
    use async_trait::async_trait;
    use norn_common::consensus_types::{
        CommitCertificate, FinalizedConsensusState, PendingValidatorChange,
        PendingValidatorChanges, PrevoteCertificate, ValidatorChange, ValidatorRecord,
    };
    use norn_common::types::{
        BlockHeader, ChainId, ConsensusPublicKey, Hash, ProtocolVersion, StakeSnapshotHash,
        ValidatorId, VrfPublicKey,
    };
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU8, Ordering};
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MemoryDb {
        values: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
        failure_mode: AtomicU8,
    }

    impl MemoryDb {
        fn apply_then_fail(&self) {
            self.failure_mode.store(2, Ordering::SeqCst);
        }

        fn clear_failure(&self) {
            self.failure_mode.store(0, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl DBInterface for MemoryDb {
        async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
            Ok(self.values.lock().await.get(key).cloned())
        }

        async fn insert(&self, key: &[u8], value: &[u8]) -> Result<()> {
            self.values
                .lock()
                .await
                .insert(key.to_vec(), value.to_vec());
            Ok(())
        }

        async fn remove(&self, key: &[u8]) -> Result<()> {
            self.values.lock().await.remove(key);
            Ok(())
        }

        async fn batch_insert(&self, keys: &[Vec<u8>], values: &[Vec<u8>]) -> Result<()> {
            if keys.len() != values.len() {
                bail!("mock batch length mismatch");
            }
            if self.failure_mode.load(Ordering::SeqCst) == 1 {
                bail!("injected pre-apply failure");
            }
            let mut guard = self.values.lock().await;
            for (key, value) in keys.iter().zip(values.iter()) {
                guard.insert(key.clone(), value.clone());
            }
            if self.failure_mode.swap(0, Ordering::SeqCst) == 2 {
                bail!("injected post-apply flush ambiguity");
            }
            Ok(())
        }

        async fn batch_delete(&self, keys: &[Vec<u8>]) -> Result<()> {
            let mut guard = self.values.lock().await;
            for key in keys {
                guard.remove(key);
            }
            Ok(())
        }
    }

    fn finalized() -> FinalizedBlockV2 {
        let protocol_version = ProtocolVersion(3);
        let chain_id = ChainId(Hash([1; 32]));
        let block_id = Hash([2; 32]);
        let snapshot_hash = StakeSnapshotHash([3; 32]);
        let block = norn_common::types::BlockV2 {
            header: BlockHeader {
                protocol_version,
                chain_id,
                height: 1,
                epoch: 0,
                round: 0,
                timestamp: 1,
                prev_block_hash: Hash([8; 32]),
                block_hash: block_id,
                merkle_root: Hash([4; 32]),
                state_root: Hash([5; 32]),
                block_builder: ValidatorId([6; 32]),
                stake_snapshot_hash: snapshot_hash,
                parent_randomness: Hash([7; 32]),
                gas_limit: 10,
                base_fee: 1,
                consensus_data_hash: Hash([8; 32]),
            },
            transactions: Vec::new(),
            consensus_data: norn_common::types::BlockConsensusData::default(),
        };
        let commit = CommitCertificate {
            protocol_version,
            chain_id,
            epoch: 0,
            height: 1,
            round: 0,
            block_id: BlockId(block_id),
            stake_snapshot_hash: snapshot_hash,
            precommits: Vec::new(),
        };
        let consensus_state = FinalizedConsensusState {
            height: 1,
            finalized_block_id: BlockId(block_id),
            state_root: Hash([5; 32]),
            timestamp: 1,
            next_randomness: Hash([9; 32]),
            active_stake_snapshot_hash: snapshot_hash,
            epoch: 0,
            pending_validator_changes: Default::default(),
        };
        FinalizedBlockV2 {
            proposal: Proposal {
                protocol_version,
                chain_id,
                epoch: 0,
                height: 1,
                round: 0,
                valid_round: None,
                valid_round_certificate: None,
                block_id: BlockId(block_id),
                parent_block_hash: Hash([8; 32]),
                stake_snapshot_hash: snapshot_hash,
                proposer: ValidatorId([6; 32]),
                vrf_preout: [10; 32],
                vrf_proof: [11; 64],
                signature: [12; 64],
            },
            block,
            commit,
            consensus_state,
        }
    }

    #[tokio::test]
    async fn durable_safety_candidate_round_trips_full_block_material() {
        let db = Arc::new(MemoryDb::default());
        let store = FinalityStore::new(db);
        let finalized = finalized();
        let derived = Hash([42; 32]);

        store
            .persist_safety_candidate(&finalized.proposal, &finalized.block, derived)
            .await
            .unwrap();
        let recovered = store
            .recover_safety_candidate(1, finalized.commit.block_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recovered.proposal, finalized.proposal);
        assert_eq!(recovered.block, finalized.block);
        assert_eq!(recovered.derived_randomness, derived);

        let protocol_version = finalized.proposal.protocol_version;
        let chain_id = finalized.proposal.chain_id;
        let snapshot_hash = finalized.proposal.stake_snapshot_hash;
        let mut reproposal = finalized.proposal.clone();
        reproposal.round = 1;
        reproposal.valid_round = Some(0);
        reproposal.valid_round_certificate = Some(PrevoteCertificate {
            protocol_version,
            chain_id,
            epoch: 0,
            height: 1,
            round: 0,
            block_id: finalized.commit.block_id,
            stake_snapshot_hash: snapshot_hash,
            prevotes: Vec::new(),
        });
        reproposal.proposer = ValidatorId([7; 32]);
        store
            .persist_safety_candidate(&reproposal, &finalized.block, Hash([43; 32]))
            .await
            .unwrap();
        let mut later_reproposal = reproposal.clone();
        later_reproposal.round = 2;
        later_reproposal.valid_round = Some(1);
        later_reproposal
            .valid_round_certificate
            .as_mut()
            .unwrap()
            .round = 1;
        store
            .persist_safety_candidate(&later_reproposal, &finalized.block, Hash([44; 32]))
            .await
            .unwrap();
        store
            .persist_pending_proposal(&later_reproposal, &finalized.block)
            .await
            .unwrap();
        let pending = store.recover_pending_proposal(1, 2).await.unwrap().unwrap();
        assert_eq!(pending.0, later_reproposal);
        assert_eq!(pending.1, finalized.block);
        let original_attempt = store
            .recover_safety_candidate_attempt(1, 0, finalized.commit.block_id)
            .await
            .unwrap()
            .unwrap();
        let later_attempt = store
            .recover_safety_candidate_attempt(1, 1, finalized.commit.block_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(original_attempt.proposal.round, 0);
        assert_eq!(later_attempt.proposal.round, 1);
        assert_eq!(original_attempt.derived_randomness, derived);
        assert_eq!(later_attempt.derived_randomness, Hash([43; 32]));

        store
            .clear_safety_candidate(1, finalized.commit.block_id)
            .await
            .unwrap();
        assert!(store
            .recover_safety_candidate(1, finalized.commit.block_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn concurrent_attempt_persistence_keeps_the_complete_round_index() {
        let db = Arc::new(MemoryDb::default());
        let store = Arc::new(FinalityStore::new(db));
        let finalized = finalized();
        let mut round_one = finalized.proposal.clone();
        round_one.round = 1;
        round_one.valid_round = Some(0);
        round_one.valid_round_certificate = Some(PrevoteCertificate {
            protocol_version: round_one.protocol_version,
            chain_id: round_one.chain_id,
            epoch: round_one.epoch,
            height: round_one.height,
            round: 0,
            block_id: round_one.block_id,
            stake_snapshot_hash: round_one.stake_snapshot_hash,
            prevotes: Vec::new(),
        });
        round_one.proposer = ValidatorId([9; 32]);

        let (first, second) = tokio::join!(
            store.persist_safety_candidate(&finalized.proposal, &finalized.block, Hash([1; 32])),
            store.persist_safety_candidate(&round_one, &finalized.block, Hash([2; 32])),
        );
        first.unwrap();
        second.unwrap();

        let candidates = store
            .recover_safety_candidates(1, finalized.commit.block_id)
            .await
            .unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.proposal.round)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[tokio::test]
    async fn durable_safety_attempts_fail_closed_at_protocol_bound() {
        let db = Arc::new(MemoryDb::default());
        let mut limits = ProtocolResourceLimits::default();
        limits.max_durable_attempts_per_height = 1;
        let store = FinalityStore::new_with_limits(db, limits);
        let finalized = finalized();
        store
            .persist_safety_candidate(&finalized.proposal, &finalized.block, Hash([1; 32]))
            .await
            .unwrap();

        let mut second_round = finalized.proposal.clone();
        second_round.round = 1;
        second_round.proposer = ValidatorId([9; 32]);
        assert!(store
            .persist_safety_candidate(&second_round, &finalized.block, Hash([2; 32]))
            .await
            .is_err());

        // A competing block at the same height cannot bypass the same
        // per-height limit by using a different block ID.
        let mut competing = finalized.clone();
        competing.block.header.block_hash = Hash([13; 32]);
        competing.proposal.block_id = BlockId(competing.block.header.block_hash);
        assert!(store
            .persist_safety_candidate(&competing.proposal, &competing.block, Hash([3; 32]),)
            .await
            .is_err());

        // An exact replay is idempotent and does not consume another slot.
        store
            .persist_safety_candidate(&finalized.proposal, &finalized.block, Hash([1; 32]))
            .await
            .unwrap();
    }

    fn genesis_block() -> Block {
        Block {
            header: BlockHeader {
                protocol_version: ProtocolVersion(2),
                chain_id: ChainId(Hash([1; 32])),
                height: 0,
                epoch: 0,
                round: 0,
                timestamp: 0,
                prev_block_hash: Hash::default(),
                block_hash: Hash([8; 32]),
                merkle_root: Hash::default(),
                state_root: Hash::default(),
                block_builder: ValidatorId([0; 32]),
                stake_snapshot_hash: StakeSnapshotHash([3; 32]),
                parent_randomness: Hash::default(),
                gas_limit: 0,
                base_fee: 0,
                consensus_data_hash: Hash::default(),
            },
            transactions: Vec::new(),
        }
    }

    #[tokio::test]
    async fn finality_is_idempotent_and_rejects_different_block_at_height() {
        let db = Arc::new(MemoryDb::default());
        let store = FinalityStore::new(db);
        store
            .initialize_genesis_tip(&genesis_block(), StakeSnapshotHash([3; 32]), Hash([7; 32]))
            .await
            .unwrap();
        let first = finalized();
        store
            .persist_safety_candidate(&first.proposal, &first.block, Hash([21; 32]))
            .await
            .unwrap();
        let mut competing_candidate = first.clone();
        competing_candidate.block.header.block_hash = Hash([22; 32]);
        competing_candidate.proposal.block_id =
            BlockId(competing_candidate.block.header.block_hash);
        competing_candidate.commit.block_id = competing_candidate.proposal.block_id;
        store
            .persist_safety_candidate(
                &competing_candidate.proposal,
                &competing_candidate.block,
                Hash([23; 32]),
            )
            .await
            .unwrap();
        assert_eq!(
            store.commit_finalized_transaction(&first).await.unwrap(),
            FinalityCommitResult::Applied
        );
        assert!(store
            .recover_safety_block(1, first.commit.block_id)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .recover_safety_block(1, competing_candidate.commit.block_id)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store.commit_finalized_transaction(&first).await.unwrap(),
            FinalityCommitResult::AlreadyCommitted
        );

        // A different quorum certificate may be formed for the same block;
        // it must not be treated as a conflicting height.
        let mut equivalent_certificate = first.clone();
        equivalent_certificate
            .commit
            .precommits
            .push(norn_common::consensus_types::SignedVote {
                protocol_version: equivalent_certificate.commit.protocol_version,
                chain_id: equivalent_certificate.commit.chain_id,
                epoch: equivalent_certificate.commit.epoch,
                height: equivalent_certificate.commit.height,
                round: equivalent_certificate.commit.round,
                step: norn_common::consensus_types::VoteStep::Precommit,
                block_id: Some(equivalent_certificate.commit.block_id),
                stake_snapshot_hash: equivalent_certificate.commit.stake_snapshot_hash,
                validator: norn_common::types::ValidatorId([9; 32]),
                signature: [13; 64],
            });
        assert_ne!(
            FinalizeTransactionId::from_v2(&first).certificate_hash,
            FinalizeTransactionId::from_v2(&equivalent_certificate).certificate_hash
        );
        assert_eq!(
            store
                .commit_finalized_transaction(&equivalent_certificate)
                .await
                .unwrap(),
            FinalityCommitResult::AlreadyCommitted
        );
        assert_eq!(
            store
                .reconcile_finalized_transaction(&equivalent_certificate, &[], None, None)
                .await
                .unwrap(),
            DurableCommitOutcome::AlreadyApplied
        );

        let mut conflicting = finalized();
        conflicting.block.header.block_hash = Hash([10; 32]);
        conflicting.commit.block_id = BlockId(Hash([10; 32]));
        conflicting.consensus_state.finalized_block_id = conflicting.commit.block_id;
        assert!(store
            .commit_finalized_transaction(&conflicting)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn post_apply_failure_is_resolved_by_durable_retry() {
        let db = Arc::new(MemoryDb::default());
        let store = FinalityStore::new(db.clone());
        store
            .initialize_genesis_tip(&genesis_block(), StakeSnapshotHash([3; 32]), Hash([7; 32]))
            .await
            .unwrap();
        let block = finalized();
        db.apply_then_fail();
        assert!(store.commit_finalized_transaction(&block).await.is_err());
        db.clear_failure();
        assert_eq!(
            store.commit_finalized_transaction(&block).await.unwrap(),
            FinalityCommitResult::AlreadyCommitted
        );
        assert_eq!(store.recover_finalized_v2(1).await.unwrap(), Some(block));
    }

    #[tokio::test]
    async fn pending_validator_changes_are_recovered_with_finalized_record() {
        let db = Arc::new(MemoryDb::default());
        let store = FinalityStore::new(db);
        store
            .initialize_genesis_tip(&genesis_block(), StakeSnapshotHash([3; 32]), Hash([7; 32]))
            .await
            .unwrap();

        let mut block = finalized();
        let validator_id = ValidatorId([6; 32]);
        block.consensus_state.pending_validator_changes = PendingValidatorChanges {
            changes: vec![PendingValidatorChange {
                effective_epoch: 3,
                change: ValidatorChange::SetVotingPower {
                    validator_id,
                    voting_power: 7,
                },
            }],
        };
        let next_snapshot = norn_common::consensus_types::StakeSnapshot::from_genesis(
            1,
            vec![ValidatorRecord {
                validator_id,
                consensus_public_key: ConsensusPublicKey([1; 33]),
                vrf_public_key: VrfPublicKey([2; 32]),
                voting_power: 1,
                jailed_until_epoch: None,
                slashed: false,
            }],
        )
        .unwrap();

        assert_eq!(
            store
                .commit_finalized_transaction_with_state_and_checkpoint_and_snapshot(
                    &block,
                    &[],
                    None,
                    Some(&next_snapshot),
                )
                .await
                .unwrap(),
            FinalityCommitResult::Applied
        );

        let recovered = store.recover_finalized_v2(1).await.unwrap().unwrap();
        assert_eq!(
            recovered.consensus_state.pending_validator_changes,
            block.consensus_state.pending_validator_changes
        );
        assert_eq!(
            store.recover_snapshot(1).await.unwrap(),
            Some(next_snapshot.clone())
        );

        let mut conflicting = block.clone();
        conflicting
            .consensus_state
            .pending_validator_changes
            .changes[0]
            .effective_epoch = 4;
        assert!(store
            .commit_finalized_transaction_with_state_and_checkpoint_and_snapshot(
                &conflicting,
                &[],
                None,
                Some(&next_snapshot),
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn checkpoints_are_content_addressed_and_empty_blocks_do_not_rewrite_full_state() {
        let db = Arc::new(MemoryDb::default());
        let store = FinalityStore::new(db.clone());
        store
            .initialize_genesis_tip(&genesis_block(), StakeSnapshotHash([3; 32]), Hash([7; 32]))
            .await
            .unwrap();

        let checkpoint = CanonicalStateCheckpoint {
            state_root: Hash([5; 32]),
            accounts: Vec::new(),
            storage: Vec::new(),
            code: CodeStorageCheckpoint {
                codes: Vec::new(),
                address_to_code: Vec::new(),
            },
        };
        let first = finalized();
        store
            .commit_finalized_transaction_with_state_and_checkpoint(&first, &[], Some(&checkpoint))
            .await
            .unwrap();

        let mut second = first.clone();
        second.block.header.height = 2;
        second.block.header.prev_block_hash = first.block.header.block_hash;
        second.block.header.parent_randomness = first.consensus_state.next_randomness;
        second.block.header.block_hash = Hash([20; 32]);
        second.proposal.height = 2;
        second.proposal.block_id = BlockId(Hash([20; 32]));
        second.proposal.parent_block_hash = first.block.header.block_hash;
        second.commit.height = 2;
        second.commit.block_id = BlockId(Hash([20; 32]));
        second.consensus_state.height = 2;
        second.consensus_state.finalized_block_id = BlockId(Hash([20; 32]));
        store
            .commit_finalized_transaction_with_state_and_checkpoint(&second, &[], Some(&checkpoint))
            .await
            .unwrap();

        let recovered = store
            .recover_finalized_v2_with_state_and_checkpoint(2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recovered.2, Some(checkpoint));
        let values = db.values.lock().await;
        assert_eq!(
            values
                .keys()
                .filter(|key| key.starts_with(STATE_CHECKPOINT_CONTENT_PREFIX))
                .count(),
            1
        );
        assert_eq!(
            values
                .keys()
                .filter(|key| key.starts_with(STATE_ACCOUNT_PREFIX))
                .count(),
            0
        );
    }
}
