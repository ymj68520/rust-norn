//! Consensus Safety Store & Anti-Equivocation Persistent Integration
//!
//! Ensures that before broadcasting any Prevote or Precommit vote, the vote
//! is atomically checked for double-signing conflicts, committed to disk
//! with `sync_all()` and `flush()`, and signed fail-closed.

use super::types::ConsensusStep;
use anyhow::Result;
use norn_common::consensus_types::{PrevoteCertificate, SignedVote, VoteStep};
use norn_common::types::{BlockId, ChainId, Hash, ProtocolVersion, StakeSnapshotHash, ValidatorId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum SafetyError {
    #[error("Double-signing detected for height {height}, round {round}, step {step:?}: attempted block {attempted:?}, existing {existing:?}")]
    EquivocationDetected {
        height: u64,
        round: u32,
        step: VoteStep,
        attempted: Option<BlockId>,
        existing: Option<BlockId>,
    },

    #[error("Storage I/O failure during safety sync: {0}")]
    StorageIoError(String),

    #[error("Serialization failure during safety WAL write: {0}")]
    SerializationError(String),

    #[error("Signing failure: {0}")]
    SigningError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteSignRequest {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub step: VoteStep,
    pub block_id: Option<BlockId>,
    pub stake_snapshot_hash: StakeSnapshotHash,
    pub validator_id: ValidatorId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyRecord {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub step: VoteStep,
    pub stake_snapshot_hash: StakeSnapshotHash,
    pub validator_id: ValidatorId,
    pub block_id: Option<BlockId>,
    pub sign_bytes_hash: [u8; 32],
    /// `None` is a durable signing intent. `Some` is the completion/ack and
    /// contains the exact signature that may be replayed after restart.
    #[serde(default)]
    pub signature: Option<Vec<u8>>,
    /// A completed vote may carry the consensus safety state that became
    /// durable with that exact vote. This closes the crash window between a
    /// block Precommit and its lock/valid-round update.
    #[serde(default)]
    pub consensus_state_after: Option<DurableConsensusSafetyState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableConsensusSafetyState {
    /// Monotonic ordering across the vote WAL and the companion state WAL.
    /// The store assigns this value when the state is made durable.
    #[serde(default)]
    pub sequence: u64,
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub step: ConsensusStep,
    pub stake_snapshot_hash: StakeSnapshotHash,
    pub parent_randomness: Hash,
    pub locked_block: Option<BlockId>,
    pub locked_round: Option<u32>,
    pub valid_block: Option<BlockId>,
    pub valid_round: Option<u32>,
    pub valid_round_certificate: Option<PrevoteCertificate>,
}

fn decode_signature(signature: &Option<Vec<u8>>) -> Result<Option<[u8; 64]>, SafetyError> {
    let Some(bytes) = signature else {
        return Ok(None);
    };
    let array = bytes
        .as_slice()
        .try_into()
        .map_err(|_| SafetyError::SerializationError("invalid WAL signature length".into()))?;
    Ok(Some(array))
}

pub trait ConsensusSigner: Send + Sync {
    fn sign_canonical_bytes(&self, bytes: &[u8]) -> Result<[u8; 64]>;
}

pub trait ConsensusSafetyStore: Send + Sync {
    fn sign_vote_once(
        &self,
        request: VoteSignRequest,
        signer: &dyn ConsensusSigner,
    ) -> Result<SignedVote, SafetyError> {
        self.sign_vote_once_with_state(request, signer, None)
    }

    fn sign_vote_once_with_state(
        &self,
        request: VoteSignRequest,
        signer: &dyn ConsensusSigner,
        state_after: Option<DurableConsensusSafetyState>,
    ) -> Result<SignedVote, SafetyError>;

    fn load_consensus_state(&self) -> Result<Option<DurableConsensusSafetyState>, SafetyError>;

    fn persist_consensus_state(
        &self,
        state: DurableConsensusSafetyState,
    ) -> Result<(), SafetyError>;

    /// Return completed votes whose broadcast may have been interrupted by a
    /// process crash or a transient network failure.
    fn recover_signed_votes(&self) -> Vec<SignedVote>;
}

type SafetyIndexKey = (ChainId, ValidatorId, u64, u64, u32, VoteStep);

/// In-memory ConsensusSafetyStore
pub struct MemorySafetyStore {
    state: Mutex<HashMap<SafetyIndexKey, SafetyRecord>>,
    consensus_state: Mutex<Option<DurableConsensusSafetyState>>,
}

impl MemorySafetyStore {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            consensus_state: Mutex::new(None),
        }
    }
}

impl Default for MemorySafetyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusSafetyStore for MemorySafetyStore {
    fn sign_vote_once_with_state(
        &self,
        request: VoteSignRequest,
        signer: &dyn ConsensusSigner,
        state_after: Option<DurableConsensusSafetyState>,
    ) -> Result<SignedVote, SafetyError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;

        let key: SafetyIndexKey = (
            request.chain_id.clone(),
            request.validator_id,
            request.epoch,
            request.height,
            request.round,
            request.step,
        );

        let unsigned_vote = SignedVote {
            protocol_version: request.protocol_version.clone(),
            chain_id: request.chain_id.clone(),
            epoch: request.epoch,
            height: request.height,
            round: request.round,
            step: request.step,
            block_id: request.block_id.clone(),
            stake_snapshot_hash: request.stake_snapshot_hash.clone(),
            validator: request.validator_id,
            signature: [0u8; 64],
        };

        let sign_bytes = unsigned_vote.canonical_bytes();
        let mut hasher = Sha256::new();
        hasher.update(&sign_bytes);
        let sign_bytes_hash: [u8; 32] = hasher.finalize().into();

        if let Some(existing) = guard.get(&key) {
            if existing.sign_bytes_hash != sign_bytes_hash {
                warn!(
                    "EQUIVOCATION ATTEMPT REJECTED: height={}, round={}, step={:?}, attempted={:?}, existing={:?}",
                    request.height, request.round, request.step, request.block_id, existing.block_id
                );
                return Err(SafetyError::EquivocationDetected {
                    height: request.height,
                    round: request.round,
                    step: request.step,
                    attempted: request.block_id,
                    existing: existing.block_id,
                });
            }
        }

        if let Some(existing) = guard.get(&key) {
            if existing.sign_bytes_hash == sign_bytes_hash {
                if let Some(signature) = decode_signature(&existing.signature)? {
                    if let Some(state_after) = state_after {
                        *self
                            .consensus_state
                            .lock()
                            .map_err(|e| SafetyError::StorageIoError(e.to_string()))? =
                            Some(state_after);
                    }
                    return Ok(SignedVote {
                        protocol_version: request.protocol_version,
                        chain_id: request.chain_id,
                        epoch: request.epoch,
                        height: request.height,
                        round: request.round,
                        step: request.step,
                        block_id: request.block_id,
                        stake_snapshot_hash: request.stake_snapshot_hash,
                        validator: request.validator_id,
                        signature,
                    });
                }
            }
        }

        guard.insert(
            key,
            SafetyRecord {
                protocol_version: request.protocol_version.clone(),
                chain_id: request.chain_id.clone(),
                epoch: request.epoch,
                height: request.height,
                round: request.round,
                step: request.step,
                stake_snapshot_hash: request.stake_snapshot_hash.clone(),
                validator_id: request.validator_id,
                block_id: request.block_id.clone(),
                sign_bytes_hash,
                signature: None,
                consensus_state_after: None,
            },
        );

        let signature = signer
            .sign_canonical_bytes(&sign_bytes)
            .map_err(|e| SafetyError::SigningError(e.to_string()))?;

        let signed_vote = SignedVote {
            signature,
            ..unsigned_vote
        };
        if let Some(record) = guard.get_mut(&key) {
            record.signature = Some(signed_vote.signature.to_vec());
            record.consensus_state_after = state_after.clone();
        }
        if let Some(state_after) = state_after {
            *self
                .consensus_state
                .lock()
                .map_err(|e| SafetyError::StorageIoError(e.to_string()))? = Some(state_after);
        }
        Ok(signed_vote)
    }

    fn load_consensus_state(&self) -> Result<Option<DurableConsensusSafetyState>, SafetyError> {
        self.consensus_state
            .lock()
            .map(|state| state.clone())
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))
    }

    fn persist_consensus_state(
        &self,
        state: DurableConsensusSafetyState,
    ) -> Result<(), SafetyError> {
        *self
            .consensus_state
            .lock()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))? = Some(state);
        Ok(())
    }

    fn recover_signed_votes(&self) -> Vec<SignedVote> {
        let Ok(guard) = self.state.lock() else {
            return Vec::new();
        };
        guard
            .values()
            .filter_map(|record| {
                decode_signature(&record.signature)
                    .ok()
                    .flatten()
                    .map(|signature| SignedVote {
                        protocol_version: record.protocol_version,
                        chain_id: record.chain_id,
                        epoch: record.epoch,
                        height: record.height,
                        round: record.round,
                        step: record.step,
                        block_id: record.block_id,
                        stake_snapshot_hash: record.stake_snapshot_hash,
                        validator: record.validator_id,
                        signature,
                    })
            })
            .collect()
    }
}

/// Disk-backed Persistent ConsensusSafetyStore with mandatory `sync_all()` for crash resilience
pub struct PersistentSafetyStore {
    #[allow(dead_code)]
    file_path: PathBuf,
    state: Mutex<(HashMap<SafetyIndexKey, SafetyRecord>, BufWriter<File>)>,
    consensus_state_path: PathBuf,
    consensus_state: Mutex<(Option<DurableConsensusSafetyState>, u64)>,
    /// Serializes allocation of the monotonic companion-state sequence with
    /// vote intent/completion frames. Without this transaction boundary two
    /// concurrent writers can observe the same last sequence and persist
    /// ambiguous ordering.
    transaction_lock: Mutex<()>,
}

impl PersistentSafetyStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file_path = path.as_ref().to_path_buf();
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let consensus_state_path = file_path.with_extension("consensus_state");
        let mut map = HashMap::new();
        let mut wal_consensus_state = None;
        let mut valid_wal_len = 0u64;

        if file_path.exists() {
            let f = File::open(&file_path)?;
            let mut reader = BufReader::new(f);
            let mut len_bytes = [0u8; 4];
            loop {
                let frame_start = reader.stream_position()?;
                match reader.read_exact(&mut len_bytes) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(err) => return Err(err.into()),
                }
                let len = u32::from_le_bytes(len_bytes) as usize;
                if len > 1024 * 1024 {
                    return Err(anyhow::anyhow!(
                        "Safety WAL record exceeds max length threshold"
                    ));
                }
                let mut buf = vec![0u8; len];
                match reader.read_exact(&mut buf) {
                    Ok(()) => {}
                    // A crash may leave a torn final frame. Earlier synced
                    // frames remain authoritative and can be replayed.
                    Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(err) => return Err(err.into()),
                }
                if let Ok(rec) = bincode::deserialize::<SafetyRecord>(&buf) {
                    if rec
                        .signature
                        .as_ref()
                        .is_some_and(|signature| signature.len() != 64)
                    {
                        return Err(anyhow::anyhow!(
                            "Safety WAL completion has invalid signature length"
                        ));
                    }
                    let key: SafetyIndexKey = (
                        rec.chain_id,
                        rec.validator_id,
                        rec.epoch,
                        rec.height,
                        rec.round,
                        rec.step,
                    );
                    if rec.consensus_state_after.as_ref().is_some_and(|state| {
                        wal_consensus_state.as_ref().is_none_or(
                            |current: &DurableConsensusSafetyState| {
                                state.sequence >= current.sequence
                            },
                        )
                    }) {
                        wal_consensus_state = rec.consensus_state_after.clone();
                    }
                    map.insert(key, rec);
                } else {
                    return Err(anyhow::anyhow!("Corrupted Safety WAL record encountered"));
                }
                valid_wal_len = reader.stream_position()?.max(frame_start);
            }
            info!(
                "Recovered {} consensus safety lock records from {:?}",
                map.len(),
                file_path
            );
        }

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&file_path)?;
        if file.metadata()?.len() > valid_wal_len {
            file.set_len(valid_wal_len)?;
            file.sync_all()?;
        }
        file.seek(SeekFrom::End(0))?;
        let writer = BufWriter::new(file);

        let mut companion_state = None;
        let mut valid_state_len = 0u64;
        if consensus_state_path.exists() {
            let f = File::open(&consensus_state_path)?;
            let mut reader = BufReader::new(f);
            let mut len_bytes = [0u8; 4];
            loop {
                let frame_start = reader.stream_position()?;
                match reader.read_exact(&mut len_bytes) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(err) => return Err(err.into()),
                }
                let len = u32::from_le_bytes(len_bytes) as usize;
                if len > 1024 * 1024 {
                    return Err(anyhow::anyhow!(
                        "Consensus safety state exceeds max length threshold"
                    ));
                }
                let mut buf = vec![0u8; len];
                match reader.read_exact(&mut buf) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(err) => return Err(err.into()),
                }
                let state = bincode::deserialize::<DurableConsensusSafetyState>(&buf)
                    .map_err(|err| anyhow::anyhow!("Corrupted consensus safety state: {err}"))?;
                if companion_state
                    .as_ref()
                    .is_none_or(|current: &DurableConsensusSafetyState| {
                        state.sequence >= current.sequence
                    })
                {
                    companion_state = Some(state);
                }
                valid_state_len = reader.stream_position()?.max(frame_start);
            }
        }
        if consensus_state_path.exists() {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&consensus_state_path)?;
            if file.metadata()?.len() > valid_state_len {
                file.set_len(valid_state_len)?;
                file.sync_all()?;
            }
        }
        let durable_state = match (wal_consensus_state, companion_state) {
            (Some(wal), Some(companion)) if companion.sequence > wal.sequence => Some(companion),
            (Some(wal), _) => Some(wal),
            (None, companion) => companion,
        };
        let state_sequence = durable_state.as_ref().map_or(0, |state| state.sequence);

        Ok(Self {
            file_path,
            state: Mutex::new((map, writer)),
            consensus_state_path,
            consensus_state: Mutex::new((durable_state, state_sequence)),
            transaction_lock: Mutex::new(()),
        })
    }
}

impl ConsensusSafetyStore for PersistentSafetyStore {
    fn sign_vote_once_with_state(
        &self,
        request: VoteSignRequest,
        signer: &dyn ConsensusSigner,
        mut state_after: Option<DurableConsensusSafetyState>,
    ) -> Result<SignedVote, SafetyError> {
        let _transaction_guard = self
            .transaction_lock
            .lock()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        if let Some(state) = state_after.as_mut() {
            let guard = self
                .consensus_state
                .lock()
                .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
            state.sequence = guard.1.saturating_add(1);
        }
        let mut guard = self
            .state
            .lock()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;

        let (ref mut map, ref mut writer) = *guard;
        let key: SafetyIndexKey = (
            request.chain_id.clone(),
            request.validator_id,
            request.epoch,
            request.height,
            request.round,
            request.step,
        );

        let unsigned_vote = SignedVote {
            protocol_version: request.protocol_version.clone(),
            chain_id: request.chain_id.clone(),
            epoch: request.epoch,
            height: request.height,
            round: request.round,
            step: request.step,
            block_id: request.block_id.clone(),
            stake_snapshot_hash: request.stake_snapshot_hash.clone(),
            validator: request.validator_id,
            signature: [0u8; 64],
        };

        let sign_bytes = unsigned_vote.canonical_bytes();
        let mut hasher = Sha256::new();
        hasher.update(&sign_bytes);
        let sign_bytes_hash: [u8; 32] = hasher.finalize().into();

        if let Some(existing) = map.get(&key) {
            if existing.sign_bytes_hash != sign_bytes_hash {
                warn!(
                    "PERSISTENT EQUIVOCATION ATTEMPT REJECTED: height={}, round={}, step={:?}, attempted={:?}, existing={:?}",
                    request.height, request.round, request.step, request.block_id, existing.block_id
                );
                return Err(SafetyError::EquivocationDetected {
                    height: request.height,
                    round: request.round,
                    step: request.step,
                    attempted: request.block_id,
                    existing: existing.block_id,
                });
            }
            if let Some(signature) = decode_signature(&existing.signature)? {
                return Ok(SignedVote {
                    protocol_version: request.protocol_version,
                    chain_id: request.chain_id,
                    epoch: request.epoch,
                    height: request.height,
                    round: request.round,
                    step: request.step,
                    block_id: request.block_id,
                    stake_snapshot_hash: request.stake_snapshot_hash,
                    validator: request.validator_id,
                    signature,
                });
            }
        }

        let record = SafetyRecord {
            protocol_version: request.protocol_version.clone(),
            chain_id: request.chain_id.clone(),
            epoch: request.epoch,
            height: request.height,
            round: request.round,
            step: request.step,
            stake_snapshot_hash: request.stake_snapshot_hash.clone(),
            validator_id: request.validator_id,
            block_id: request.block_id.clone(),
            sign_bytes_hash,
            signature: None,
            consensus_state_after: None,
        };

        let encoded = bincode::serialize(&record)
            .map_err(|e| SafetyError::SerializationError(e.to_string()))?;

        let len = encoded.len() as u32;
        writer
            .write_all(&len.to_le_bytes())
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        writer
            .write_all(&encoded)
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        writer
            .flush()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;

        map.insert(key, record);

        let signature = signer
            .sign_canonical_bytes(&sign_bytes)
            .map_err(|e| SafetyError::SigningError(e.to_string()))?;

        let signed_vote = SignedVote {
            signature,
            ..unsigned_vote
        };

        let completion = SafetyRecord {
            protocol_version: request.protocol_version,
            chain_id: request.chain_id,
            epoch: request.epoch,
            height: request.height,
            round: request.round,
            step: request.step,
            stake_snapshot_hash: request.stake_snapshot_hash,
            validator_id: request.validator_id,
            block_id: request.block_id,
            sign_bytes_hash,
            signature: Some(signed_vote.signature.to_vec()),
            consensus_state_after: state_after.clone(),
        };
        let completion_encoded = bincode::serialize(&completion)
            .map_err(|e| SafetyError::SerializationError(e.to_string()))?;
        let completion_len = completion_encoded.len() as u32;
        writer
            .write_all(&completion_len.to_le_bytes())
            .and_then(|_| writer.write_all(&completion_encoded))
            .and_then(|_| writer.flush())
            .and_then(|_| writer.get_ref().sync_all())
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        map.insert(key, completion);

        if let Some(state_after) = state_after {
            let mut state_guard = self
                .consensus_state
                .lock()
                .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
            *state_guard = (Some(state_after.clone()), state_after.sequence);
        }

        Ok(signed_vote)
    }

    fn load_consensus_state(&self) -> Result<Option<DurableConsensusSafetyState>, SafetyError> {
        self.consensus_state
            .lock()
            .map(|state| state.0.clone())
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))
    }

    fn persist_consensus_state(
        &self,
        mut state: DurableConsensusSafetyState,
    ) -> Result<(), SafetyError> {
        let _transaction_guard = self
            .transaction_lock
            .lock()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        let mut state_guard = self
            .consensus_state
            .lock()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        state.sequence = state_guard.1.saturating_add(1);
        let encoded = bincode::serialize(&state)
            .map_err(|e| SafetyError::SerializationError(e.to_string()))?;
        let len = encoded.len() as u32;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.consensus_state_path)
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        file.write_all(&len.to_le_bytes())
            .and_then(|_| file.write_all(&encoded))
            .and_then(|_| file.flush())
            .and_then(|_| file.sync_all())
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;
        *state_guard = (Some(state.clone()), state.sequence);
        Ok(())
    }

    fn recover_signed_votes(&self) -> Vec<SignedVote> {
        let Ok(guard) = self.state.lock() else {
            return Vec::new();
        };
        guard
            .0
            .values()
            .filter_map(|record| {
                decode_signature(&record.signature)
                    .ok()
                    .flatten()
                    .map(|signature| SignedVote {
                        protocol_version: record.protocol_version,
                        chain_id: record.chain_id,
                        epoch: record.epoch,
                        height: record.height,
                        round: record.round,
                        step: record.step,
                        block_id: record.block_id,
                        stake_snapshot_hash: record.stake_snapshot_hash,
                        validator: record.validator_id,
                        signature,
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct DummySigner;
    impl ConsensusSigner for DummySigner {
        fn sign_canonical_bytes(&self, _bytes: &[u8]) -> Result<[u8; 64]> {
            Ok([7u8; 64])
        }
    }

    struct FailingSigner;
    impl ConsensusSigner for FailingSigner {
        fn sign_canonical_bytes(&self, _bytes: &[u8]) -> Result<[u8; 64]> {
            Err(anyhow::anyhow!("injected signer failure"))
        }
    }

    #[test]
    fn test_prevent_double_signing_persistent() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("safety.log");
        let signer = DummySigner;

        let store = PersistentSafetyStore::open(&path).unwrap();

        let req1 = VoteSignRequest {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(norn_common::types::Hash([1u8; 32])),
            epoch: 1,
            height: 10,
            round: 0,
            step: VoteStep::Prevote,
            block_id: Some(BlockId(norn_common::types::Hash([1u8; 32]))),
            stake_snapshot_hash: StakeSnapshotHash([1u8; 32]),
            validator_id: ValidatorId([0u8; 32]),
        };

        let v1 = store.sign_vote_once(req1.clone(), &signer);
        assert!(v1.is_ok());
        let persisted = store.recover_signed_votes();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0], v1.as_ref().unwrap().clone());

        // Reopen store from disk to verify crash recovery
        drop(store);
        // Simulate a crash while writing the next frame length.
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[1u8, 2u8])
            .unwrap();
        let recovered_store = PersistentSafetyStore::open(&path).unwrap();
        assert_eq!(recovered_store.recover_signed_votes(), persisted);

        // Replaying the same request returns the exact durable signature and
        // does not invoke the signer again.
        assert_eq!(
            recovered_store
                .sign_vote_once(req1.clone(), &FailingSigner)
                .unwrap(),
            persisted[0]
        );

        let req_conflicting = VoteSignRequest {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(norn_common::types::Hash([1u8; 32])),
            epoch: 1,
            height: 10,
            round: 0,
            step: VoteStep::Prevote,
            block_id: Some(BlockId(norn_common::types::Hash([2u8; 32]))),
            stake_snapshot_hash: StakeSnapshotHash([1u8; 32]),
            validator_id: ValidatorId([0u8; 32]),
        };

        let err = recovered_store
            .sign_vote_once(req_conflicting, &signer)
            .unwrap_err();
        assert!(matches!(err, SafetyError::EquivocationDetected { .. }));
    }

    #[test]
    fn signer_failure_leaves_intent_without_a_recoverable_vote() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("safety-intent.log");
        let store = PersistentSafetyStore::open(&path).unwrap();
        let request = VoteSignRequest {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(norn_common::types::Hash([4u8; 32])),
            epoch: 1,
            height: 11,
            round: 0,
            step: VoteStep::Precommit,
            block_id: Some(BlockId(norn_common::types::Hash([4u8; 32]))),
            stake_snapshot_hash: StakeSnapshotHash([4u8; 32]),
            validator_id: ValidatorId([4u8; 32]),
        };

        assert!(matches!(
            store.sign_vote_once(request.clone(), &FailingSigner),
            Err(SafetyError::SigningError(_))
        ));
        assert!(store.recover_signed_votes().is_empty());

        let conflicting = VoteSignRequest {
            block_id: Some(BlockId(norn_common::types::Hash([5u8; 32]))),
            ..request
        };
        assert!(matches!(
            store.sign_vote_once(conflicting, &DummySigner),
            Err(SafetyError::EquivocationDetected { .. })
        ));
    }

    #[test]
    fn consensus_safety_state_survives_restart_and_torn_companion_frame() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("safety-state.log");
        let store = PersistentSafetyStore::open(&path).unwrap();
        let state = DurableConsensusSafetyState {
            sequence: 0,
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(norn_common::types::Hash([9u8; 32])),
            epoch: 1,
            height: 10,
            round: 2,
            step: ConsensusStep::PrecommitWait,
            stake_snapshot_hash: StakeSnapshotHash([8u8; 32]),
            parent_randomness: Hash([7u8; 32]),
            locked_block: Some(BlockId(Hash([6u8; 32]))),
            locked_round: Some(1),
            valid_block: None,
            valid_round: None,
            valid_round_certificate: None,
        };
        store.persist_consensus_state(state.clone()).unwrap();
        let persisted = store.load_consensus_state().unwrap().unwrap();
        assert_eq!(persisted.sequence, 1);
        drop(store);

        let companion_path = path.with_extension("consensus_state");
        OpenOptions::new()
            .append(true)
            .open(&companion_path)
            .unwrap()
            .write_all(&[1u8, 2u8])
            .unwrap();
        let recovered = PersistentSafetyStore::open(&path).unwrap();
        let recovered_state = recovered.load_consensus_state().unwrap().unwrap();
        assert_eq!(recovered_state, persisted);
    }

    #[test]
    fn persistent_sequence_allocator_is_serialized_across_threads() {
        use std::sync::Arc;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("safety-concurrent.log");
        let store = Arc::new(PersistentSafetyStore::open(&path).unwrap());
        let mut workers = Vec::new();
        for index in 0..16u64 {
            let store = Arc::clone(&store);
            workers.push(std::thread::spawn(move || {
                store
                    .persist_consensus_state(DurableConsensusSafetyState {
                        sequence: 0,
                        protocol_version: ProtocolVersion(2),
                        chain_id: ChainId(Hash([12; 32])),
                        epoch: 1,
                        height: 20,
                        round: index as u32,
                        step: ConsensusStep::PrecommitWait,
                        stake_snapshot_hash: StakeSnapshotHash([13; 32]),
                        parent_randomness: Hash([14; 32]),
                        locked_block: None,
                        locked_round: None,
                        valid_block: None,
                        valid_round: None,
                        valid_round_certificate: None,
                    })
                    .unwrap();
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(store.load_consensus_state().unwrap().unwrap().sequence, 16);
        drop(store);
        let recovered = PersistentSafetyStore::open(&path).unwrap();
        assert_eq!(
            recovered.load_consensus_state().unwrap().unwrap().sequence,
            16
        );
    }
}
