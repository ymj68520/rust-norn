//! Atomic, idempotent persistence for finalized protocol-v2 blocks.

use anyhow::{anyhow, bail, Result};
use norn_common::consensus_types::{FinalizeTransactionId, FinalizedBlockV2};
use norn_common::traits::DBInterface;
use norn_common::types::{BlockId, Hash, TransactionId};
use std::collections::HashSet;
use std::sync::Arc;

const HEIGHT_PREFIX: &[u8] = b"finality/v2/by-height/";
const RECORD_PREFIX: &[u8] = b"finality/v2/record/";
const BLOCK_PREFIX: &[u8] = b"block/v2/by-hash/";
const CERTIFICATE_PREFIX: &[u8] = b"finality/v2/certificate/";
const CONSENSUS_STATE_PREFIX: &[u8] = b"consensus/v2/finalized-state/";
const STATE_WRITE_COUNT_PREFIX: &[u8] = b"state/v2/write-count/";
const STATE_WRITE_PREFIX: &[u8] = b"state/v2/write/";
const TRANSACTION_PREFIX: &[u8] = b"finality/v2/transaction/";
const TIP_KEY: &[u8] = b"consensus/v2/finalized-tip";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalityCommitResult {
    Applied,
    AlreadyCommitted,
}

pub struct FinalityStore {
    db: Arc<dyn DBInterface>,
}

impl FinalityStore {
    pub fn new(db: Arc<dyn DBInterface>) -> Self {
        Self { db }
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
            if existing != id {
                bail!(
                    "height {} is already finalized by a different block or certificate",
                    id.height
                );
            }

            let required =
                self.required_keys(finalized, &transaction_ids, &id, state_write_values.len());
            let present = self.read_presence(&required).await?;
            if !present.iter().all(|present| *present) {
                bail!("finalized transaction has an incomplete durable marker set");
            }
            self.verify_state_writes(id.height, state_write_values)
                .await?;
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

        let mut keys = vec![
            height_key,
            record_key(finalized.block.header.block_hash),
            block_key(finalized.block.header.block_hash),
            certificate_key(id.certificate_hash),
            consensus_state_key(id.height),
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
        keys.push(TIP_KEY.to_vec());
        values.push(encoded_id);

        if let Some(tip_bytes) = self.db.get(TIP_KEY).await? {
            let tip: FinalizeTransactionId = decode(&tip_bytes, "finalized tip")?;
            if tip.height > id.height {
                bail!("cannot finalize a height below the durable finalized tip");
            }
        }

        self.db.batch_insert(&keys, &values).await?;
        Ok(FinalityCommitResult::Applied)
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
        let required = self.required_keys(&finalized, &transaction_ids, &id, state_write_count);
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
        Ok(Some((finalized, state_writes)))
    }

    pub async fn recover_finalized_tip(&self) -> Result<Option<FinalizedBlockV2>> {
        let Some(tip_bytes) = self.db.get(TIP_KEY).await? else {
            return Ok(None);
        };
        let tip: FinalizeTransactionId = decode(&tip_bytes, "finalized tip")?;
        self.recover_finalized_v2(tip.height).await
    }

    pub async fn recover_finalized_tip_with_state(
        &self,
    ) -> Result<Option<(FinalizedBlockV2, Vec<Vec<u8>>)>> {
        let Some(tip_bytes) = self.db.get(TIP_KEY).await? else {
            return Ok(None);
        };
        let tip: FinalizeTransactionId = decode(&tip_bytes, "finalized tip")?;
        self.recover_finalized_v2_with_state(tip.height).await
    }

    fn required_keys(
        &self,
        finalized: &FinalizedBlockV2,
        transaction_ids: &[TransactionId],
        id: &FinalizeTransactionId,
        state_write_count: usize,
    ) -> Vec<Vec<u8>> {
        let mut keys = vec![
            height_key(id.height),
            record_key(finalized.block.header.block_hash),
            block_key(finalized.block.header.block_hash),
            certificate_key(id.certificate_hash),
            consensus_state_key(id.height),
            state_write_count_key(id.height),
            TIP_KEY.to_vec(),
        ];
        keys.extend(transaction_ids.iter().map(transaction_key));
        keys.extend((0..state_write_count).map(|index| state_write_key(id.height, index)));
        keys
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

fn certificate_key(certificate_hash: Hash) -> Vec<u8> {
    key_with_bytes(CERTIFICATE_PREFIX, &certificate_hash.0)
}

fn consensus_state_key(height: u64) -> Vec<u8> {
    key_with_bytes(CONSENSUS_STATE_PREFIX, &height.to_be_bytes())
}

fn state_write_count_key(height: u64) -> Vec<u8> {
    key_with_bytes(STATE_WRITE_COUNT_PREFIX, &height.to_be_bytes())
}

fn state_write_key(height: u64, index: usize) -> Vec<u8> {
    let mut suffix = height.to_be_bytes().to_vec();
    suffix.extend_from_slice(&(index as u64).to_be_bytes());
    key_with_bytes(STATE_WRITE_PREFIX, &suffix)
}

fn transaction_key(transaction_id: &TransactionId) -> Vec<u8> {
    key_with_bytes(TRANSACTION_PREFIX, &transaction_id.0 .0)
}

fn key_with_bytes(prefix: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut key = prefix.to_vec();
    key.extend_from_slice(suffix);
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
    use norn_common::consensus_types::{CommitCertificate, FinalizedConsensusState};
    use norn_common::types::{
        BlockHeader, ChainId, Hash, ProtocolVersion, StakeSnapshotHash, ValidatorId,
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
                prev_block_hash: Hash([0; 32]),
                block_hash: block_id,
                merkle_root: Hash([4; 32]),
                state_root: Hash([5; 32]),
                proposer: ValidatorId([6; 32]),
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
        };
        FinalizedBlockV2 {
            block,
            commit,
            consensus_state,
        }
    }

    #[tokio::test]
    async fn finality_is_idempotent_and_rejects_different_block_at_height() {
        let db = Arc::new(MemoryDb::default());
        let store = FinalityStore::new(db);
        let first = finalized();
        assert_eq!(
            store.commit_finalized_transaction(&first).await.unwrap(),
            FinalityCommitResult::Applied
        );
        assert_eq!(
            store.commit_finalized_transaction(&first).await.unwrap(),
            FinalityCommitResult::AlreadyCommitted
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
}
