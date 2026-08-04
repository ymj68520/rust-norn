//! Atomic, idempotent persistence for finalized protocol-v2 blocks.

use crate::execution::overlay::{CanonicalStateCheckpoint, OverlayWrite};
use anyhow::{anyhow, bail, Result};
use norn_common::consensus_types::{
    CanonicalFinalizedTip, FinalizeTransactionId, FinalizedBlockV2, Proposal, StakeSnapshot,
};
use norn_common::traits::DBInterface;
use norn_common::types::{Block, BlockId, BlockV2, Hash, StakeSnapshotHash, TransactionId};
use std::collections::HashSet;
use std::sync::Arc;

const HEIGHT_PREFIX: &[u8] = b"finality/v2/by-height/";
const RECORD_PREFIX: &[u8] = b"finality/v2/record/";
const BLOCK_PREFIX: &[u8] = b"finality/v2/block/";
const CERTIFICATE_PREFIX: &[u8] = b"finality/v2/certificate/";
const STATE_WRITE_COUNT_PREFIX: &[u8] = b"state/v2/write-count/";
const STATE_WRITE_PREFIX: &[u8] = b"state/v2/write/";
const TRANSACTION_PREFIX: &[u8] = b"finality/v2/transaction/";
const TIP_KEY: &[u8] = b"finality/v2/tip";
const LEGACY_TIP_KEY: &[u8] = b"consensus/v2/finalized-tip";
const STATE_ROOT_KEY: &[u8] = b"state/v2/root";
const STATE_ROOT_PREFIX: &[u8] = b"state/v2/root/";
const STATE_CHECKPOINT_KEY: &[u8] = b"state/v2/checkpoint";
const STATE_CHECKPOINT_PREFIX: &[u8] = b"state/v2/checkpoint/";
const CONSENSUS_STATE_KEY: &[u8] = b"finality/v2/consensus-state";
const STATE_ACCOUNT_PREFIX: &[u8] = b"state/v2/account/";
const STATE_STORAGE_PREFIX: &[u8] = b"state/v2/storage/";
const STATE_CODE_PREFIX: &[u8] = b"state/v2/code/";
const STATE_TOMBSTONE: &[u8] = b"NORN_STATE_TOMBSTONE_V2";
const SNAPSHOT_PREFIX: &[u8] = b"finality/v2/snapshot/";
const PENDING_PROPOSAL_PREFIX: &[u8] = b"consensus/v2/pending-proposal/";
const SAFETY_CANDIDATE_PREFIX: &[u8] = b"consensus/v2/safety-candidate/";

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
}

impl FinalityStore {
    pub fn new(db: Arc<dyn DBInterface>) -> Self {
        Self { db }
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
                        || block.header.round != valid_round
                }
                _ => true,
            }
        {
            bail!("pending V2 proposal does not match its block identity");
        }
        let key = pending_proposal_key(proposal.height, proposal.round);
        self.db
            .batch_insert(&[key], &[encode(&(proposal, block))?])
            .await
    }

    pub async fn recover_pending_proposal(
        &self,
        height: u64,
        round: u32,
    ) -> Result<Option<(Proposal, BlockV2)>> {
        self.db
            .get(&pending_proposal_key(height, round))
            .await?
            .map(|bytes| decode(&bytes, "pending V2 proposal"))
            .transpose()
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
        if block.header.height < 0
            || proposal.height != block.header.height as u64
            || proposal.block_id != BlockId(block.header.block_hash)
            || block.header.block_builder.0 == [0u8; 32]
        {
            bail!("safety candidate identity does not match its block");
        }
        let candidate = DurableSafetyCandidate {
            proposal: proposal.clone(),
            block: block.clone(),
            derived_randomness,
        };
        self.db
            .batch_insert(
                &[safety_candidate_key(proposal.height, proposal.block_id)],
                &[encode(&candidate)?],
            )
            .await
    }

    pub async fn recover_safety_candidate(
        &self,
        height: u64,
        block_id: BlockId,
    ) -> Result<Option<DurableSafetyCandidate>> {
        self.db
            .get(&safety_candidate_key(height, block_id))
            .await?
            .map(|bytes| decode(&bytes, "durable safety candidate"))
            .transpose()
    }

    /// Candidate cleanup is intentionally only allowed after the finalized
    /// transaction has been durably committed.
    pub async fn clear_safety_candidate(&self, height: u64, block_id: BlockId) -> Result<()> {
        self.db
            .batch_delete(&[safety_candidate_key(height, block_id)])
            .await
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
            let persisted: FinalizedBlockV2 = decode(
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
        if finalized.block.header.parent_randomness != current_tip.next_randomness {
            bail!("finalized block parent randomness does not match canonical tip");
        }
        if (finalized.block.header.epoch as u64) < current_tip.epoch {
            bail!("finalized block epoch regresses canonical tip epoch");
        }

        for transaction_id in &transaction_ids {
            let key = transaction_key(transaction_id);
            if let Some(existing_bytes) = self.db.get(&key).await? {
                let existing: FinalizeTransactionId =
                    decode(&existing_bytes, "transaction marker")?;
                if existing != id {
                    bail!(
                        "transaction {:?} is already finalized under a different transaction",
                        transaction_id
                    );
                }
                bail!("transaction marker exists without its finalized height marker");
            }
        }

        let encoded_id = encode(&id)?;
        let encoded_finalized = encode(finalized)?;
        let encoded_block = encode(&finalized.block)?;
        let encoded_certificate = encode(&finalized.commit)?;
        let encoded_consensus_state = encode(&finalized.consensus_state)?;
        let encoded_state_write_count = encode(&state_write_values.len())?;
        let canonical_entries = if let Some(checkpoint) = checkpoint {
            canonical_state_entries(checkpoint, state_write_values)?
        } else {
            Vec::new()
        };

        let mut keys = vec![
            height_key,
            record_key(finalized.block.header.block_hash),
            block_key(finalized.block.header.block_hash),
            certificate_key(id.height),
            CONSENSUS_STATE_KEY.to_vec(),
            state_write_count_key(id.height),
        ];
        let mut values = vec![
            encoded_id.clone(),
            encoded_finalized,
            encoded_block,
            encoded_certificate,
            encoded_consensus_state,
            encoded_state_write_count,
        ];
        for (index, value) in state_write_values.iter().enumerate() {
            keys.push(state_write_key(id.height, index));
            values.push(value.clone());
        }
        for transaction_id in &transaction_ids {
            keys.push(transaction_key(transaction_id));
            values.push(encoded_id.clone());
        }
        for (key, value) in canonical_entries {
            keys.push(key);
            values.push(value);
        }
        if let Some(checkpoint) = checkpoint {
            keys.push(STATE_ROOT_KEY.to_vec());
            values.push(encode(&checkpoint.state_root)?);
            keys.push(state_root_key(id.height));
            values.push(encode(&checkpoint.state_root)?);
            keys.push(STATE_CHECKPOINT_KEY.to_vec());
            values.push(encode(checkpoint)?);
            keys.push(checkpoint_key(id.height));
            values.push(encode(checkpoint)?);
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

        self.db.batch_insert(&keys, &values).await?;
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
        let persisted: FinalizedBlockV2 = match decode(&record_bytes, "finalized record") {
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
        let finalized: FinalizedBlockV2 = decode(&record_bytes, "finalized record")?;
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
        let checkpoint: Option<CanonicalStateCheckpoint> = self
            .db
            .get(&checkpoint_key(id.height))
            .await?
            .map(|bytes| decode(&bytes, "canonical state checkpoint"))
            .transpose()?;
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
        transaction_ids: &[TransactionId],
        id: &FinalizeTransactionId,
        state_write_count: usize,
        has_checkpoint: bool,
    ) -> Result<Vec<Vec<u8>>> {
        let mut keys = vec![
            height_key(id.height),
            record_key(finalized.block.header.block_hash),
            block_key(finalized.block.header.block_hash),
            certificate_key(id.height),
            CONSENSUS_STATE_KEY.to_vec(),
            state_write_count_key(id.height),
            TIP_KEY.to_vec(),
        ];
        keys.extend(transaction_ids.iter().map(transaction_key));
        keys.extend((0..state_write_count).map(|index| state_write_key(id.height, index)));
        if has_checkpoint {
            keys.push(STATE_ROOT_KEY.to_vec());
            keys.push(state_root_key(id.height));
            keys.push(STATE_CHECKPOINT_KEY.to_vec());
            keys.push(checkpoint_key(id.height));
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
        let actual: CanonicalStateCheckpoint = decode(&stored, "canonical state checkpoint")?;
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
        let mut present = Vec::with_capacity(keys.len());
        for key in keys {
            present.push(self.db.get(key).await?.is_some());
        }
        Ok(present)
    }
}

fn height_key(height: u64) -> Vec<u8> {
    key_with_bytes(HEIGHT_PREFIX, &height.to_be_bytes())
}

fn record_key(block_id: Hash) -> Vec<u8> {
    key_with_bytes(RECORD_PREFIX, &block_id.0)
}

fn block_key(block_id: Hash) -> Vec<u8> {
    key_with_bytes(BLOCK_PREFIX, &block_id.0)
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

fn state_root_key(height: u64) -> Vec<u8> {
    key_with_bytes(STATE_ROOT_PREFIX, &height.to_be_bytes())
}

fn transaction_key(transaction_id: &TransactionId) -> Vec<u8> {
    key_with_bytes(TRANSACTION_PREFIX, &transaction_id.0 .0)
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

fn canonical_state_entries(
    checkpoint: &CanonicalStateCheckpoint,
    state_write_values: &[Vec<u8>],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut entries = std::collections::BTreeMap::<Vec<u8>, Vec<u8>>::new();
    for account in &checkpoint.accounts {
        entries.insert(state_account_key(&account.address), encode(account)?);
    }
    for (address, items) in &checkpoint.storage {
        for item in items {
            entries.insert(state_storage_key(address, &item.key), item.value.clone());
        }
    }
    for (hash, code) in &checkpoint.code.codes {
        entries.insert(state_code_key(hash), code.clone());
    }

    // Overlay writes also emit tombstones for keys deleted by this block. The
    // checkpoint remains the authoritative recovery image; these entries make
    // the granular state namespace reflect the same update/delete operation.
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

fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    norn_common::utils::codec::serialize(value)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8], name: &str) -> Result<T> {
    norn_common::utils::codec::deserialize(bytes)
        .map_err(|error| anyhow!("invalid {}: {}", name, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use norn_common::consensus_types::{
        CommitCertificate, FinalizedConsensusState, PendingValidatorChange,
        PendingValidatorChanges, ValidatorChange, ValidatorRecord,
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
        let protocol_version = ProtocolVersion(2);
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
            commit_certificate_hash: commit.certificate_hash(),
            next_randomness: Hash([9; 32]),
            active_stake_snapshot_hash: snapshot_hash,
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
        assert_eq!(
            store.commit_finalized_transaction(&first).await.unwrap(),
            FinalityCommitResult::Applied
        );
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
        equivalent_certificate
            .consensus_state
            .commit_certificate_hash = equivalent_certificate.commit.certificate_hash();
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
        conflicting.consensus_state.commit_certificate_hash = conflicting.commit.certificate_hash();
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
}
