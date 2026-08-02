//! Consensus Safety Store & Anti-Equivocation Persistent Integration
//! 
//! Ensures that before broadcasting any Prevote or Precommit vote, the vote
//! is atomically checked for double-signing conflicts and written to disk
//! with `sync_all()` to ensure crash-safe anti-equivocation.

use anyhow::Result;
use norn_common::consensus_types::{SignedVote, VoteStep};
use norn_common::types::{BlockId, ChainId, ProtocolVersion, ValidatorId};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
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

    #[error("Signing failure: {0}")]
    SigningError(String),
}

#[derive(Debug, Clone)]
pub struct VoteSignRequest {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub step: VoteStep,
    pub block_id: Option<BlockId>,
    pub validator_id: ValidatorId,
}

pub trait ConsensusSigner: Send + Sync {
    fn sign_canonical_bytes(&self, bytes: &[u8]) -> Result<[u8; 64]>;
}

pub trait ConsensusSafetyStore: Send + Sync {
    fn sign_vote_once(
        &self,
        request: VoteSignRequest,
        signer: &dyn ConsensusSigner,
    ) -> Result<SignedVote, SafetyError>;
}

/// In-memory ConsensusSafetyStore
pub struct MemorySafetyStore {
    state: Mutex<HashMap<(u64, u32, VoteStep), Option<BlockId>>>,
}

impl MemorySafetyStore {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemorySafetyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusSafetyStore for MemorySafetyStore {
    fn sign_vote_once(
        &self,
        request: VoteSignRequest,
        signer: &dyn ConsensusSigner,
    ) -> Result<SignedVote, SafetyError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;

        let key = (request.height, request.round, request.step);

        if let Some(existing_block_id) = guard.get(&key) {
            if *existing_block_id != request.block_id {
                warn!(
                    "EQUIVOCATION ATTEMPT REJECTED: height={}, round={}, step={:?}, attempted={:?}, existing={:?}",
                    request.height, request.round, request.step, request.block_id, existing_block_id
                );
                return Err(SafetyError::EquivocationDetected {
                    height: request.height,
                    round: request.round,
                    step: request.step,
                    attempted: request.block_id,
                    existing: *existing_block_id,
                });
            }
        }

        guard.insert(key, request.block_id);

        let unsigned_vote = SignedVote {
            protocol_version: request.protocol_version,
            chain_id: request.chain_id,
            epoch: request.epoch,
            height: request.height,
            round: request.round,
            step: request.step,
            block_id: request.block_id,
            validator: request.validator_id,
            signature: [0u8; 64],
        };

        let bytes = unsigned_vote.canonical_bytes();
        let signature = signer
            .sign_canonical_bytes(&bytes)
            .map_err(|e| SafetyError::SigningError(e.to_string()))?;

        Ok(SignedVote {
            signature,
            ..unsigned_vote
        })
    }
}

/// Disk-backed Persistent ConsensusSafetyStore with mandatory `sync_all()` for crash resilience
pub struct PersistentSafetyStore {
    file_path: PathBuf,
    state: Mutex<(HashMap<(u64, u32, VoteStep), Option<BlockId>>, BufWriter<File>)>,
}

impl PersistentSafetyStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file_path = path.as_ref().to_path_buf();
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut map = HashMap::new();

        if file_path.exists() {
            let f = File::open(&file_path)?;
            let mut reader = BufReader::new(f);
            let mut len_bytes = [0u8; 4];
            while reader.read_exact(&mut len_bytes).is_ok() {
                let len = u32::from_le_bytes(len_bytes) as usize;
                let mut buf = vec![0u8; len];
                reader.read_exact(&mut buf)?;
                if let Ok((height, round, step_byte, block_id_opt)) =
                    bincode::deserialize::<(u64, u32, u8, Option<[u8; 32]>) >(&buf)
                {
                    let step = match step_byte {
                        1 => VoteStep::Prevote,
                        _ => VoteStep::Precommit,
                    };
                    let bid = block_id_opt.map(|arr| BlockId(norn_common::types::Hash(arr)));
                    map.insert((height, round, step), bid);
                }
            }
            info!("Recovered {} consensus safety lock records from {:?}", map.len(), file_path);
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        let writer = BufWriter::new(file);

        Ok(Self {
            file_path,
            state: Mutex::new((map, writer)),
        })
    }
}

impl ConsensusSafetyStore for PersistentSafetyStore {
    fn sign_vote_once(
        &self,
        request: VoteSignRequest,
        signer: &dyn ConsensusSigner,
    ) -> Result<SignedVote, SafetyError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|e| SafetyError::StorageIoError(e.to_string()))?;

        let (ref mut map, ref mut writer) = *guard;
        let key = (request.height, request.round, request.step);

        if let Some(existing_block_id) = map.get(&key) {
            if *existing_block_id != request.block_id {
                warn!(
                    "PERSISTENT EQUIVOCATION ATTEMPT REJECTED: height={}, round={}, step={:?}, attempted={:?}, existing={:?}",
                    request.height, request.round, request.step, request.block_id, existing_block_id
                );
                return Err(SafetyError::EquivocationDetected {
                    height: request.height,
                    round: request.round,
                    step: request.step,
                    attempted: request.block_id,
                    existing: *existing_block_id,
                });
            }
        }

        // Persist to disk before signing and flush + sync_all
        let step_byte = match request.step {
            VoteStep::Prevote => 1u8,
            VoteStep::Precommit => 2u8,
        };
        let block_id_bytes = request.block_id.map(|b| b.0.0);
        let record = (request.height, request.round, step_byte, block_id_bytes);

        if let Ok(encoded) = bincode::serialize(&record) {
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
        }

        map.insert(key, request.block_id);

        let unsigned_vote = SignedVote {
            protocol_version: request.protocol_version,
            chain_id: request.chain_id,
            epoch: request.epoch,
            height: request.height,
            round: request.round,
            step: request.step,
            block_id: request.block_id,
            validator: request.validator_id,
            signature: [0u8; 64],
        };

        let bytes = unsigned_vote.canonical_bytes();
        let signature = signer
            .sign_canonical_bytes(&bytes)
            .map_err(|e| SafetyError::SigningError(e.to_string()))?;

        Ok(SignedVote {
            signature,
            ..unsigned_vote
        })
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
            validator_id: ValidatorId([0u8; 32]),
        };

        let v1 = store.sign_vote_once(req1.clone(), &signer);
        assert!(v1.is_ok());

        // Reopen store from disk to verify crash recovery
        drop(store);
        let recovered_store = PersistentSafetyStore::open(&path).unwrap();

        let req_conflicting = VoteSignRequest {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(norn_common::types::Hash([1u8; 32])),
            epoch: 1,
            height: 10,
            round: 0,
            step: VoteStep::Prevote,
            block_id: Some(BlockId(norn_common::types::Hash([2u8; 32]))),
            validator_id: ValidatorId([0u8; 32]),
        };

        let err = recovered_store.sign_vote_once(req_conflicting, &signer).unwrap_err();
        assert!(matches!(err, SafetyError::EquivocationDetected { .. }));
    }
}
