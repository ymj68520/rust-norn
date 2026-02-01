//! WAL recovery manager
//!
//! This module provides recovery functionality using the WAL,
//! allowing the database to recover to a consistent state after a crash.

use crate::wal::{WAL, WALEntry, WALConfig};
use norn_common::error::{NornError, Result};
use norn_common::types::Hash;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use std::collections::HashMap;

use crate::SledDB;

/// Recovery status
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryStatus {
    /// No recovery needed (clean shutdown)
    Clean,

    /// Recovery completed successfully
    Recovered {
        entries_applied: usize,
        checkpoint_block: Option<u64>,
    },

    /// Recovery failed
    Failed {
        reason: String,
    },
}

/// WAL recovery manager
pub struct WALRecoveryManager {
    /// WAL instance
    wal: Arc<WAL>,

    /// Database
    db: Arc<SledDB>,
}

impl WALRecoveryManager {
    /// Create a new recovery manager
    pub fn new(wal: Arc<WAL>, db: Arc<SledDB>) -> Self {
        Self { wal, db }
    }

    /// Recover from crash using WAL
    pub async fn recover(&self) -> Result<RecoveryStatus> {
        info!("Starting WAL recovery");

        // Read all WAL entries
        let entries = self.wal.read_all()?;

        if entries.is_empty() {
            info!("No WAL entries found, clean shutdown");
            return Ok(RecoveryStatus::Clean);
        }

        let mut entries_applied = 0;
        let mut checkpoint_block = None;

        // Group entries by transactions
        let mut current_transaction: Option<(u64, Vec<WALEntry>)> = None;
        let mut committed_transactions = Vec::new();

        for entry in entries {
            match entry {
                WALEntry::TransactionBegin { id } => {
                    if current_transaction.is_some() {
                        warn!("Nested transaction detected, rolling back previous");
                        // Previous transaction was not committed, discard it
                    }
                    current_transaction = Some((id, Vec::new()));
                }

                WALEntry::TransactionCommit { id } => {
                    if let Some((tid, entries)) = current_transaction.take() {
                        if tid == id {
                            committed_transactions.push((tid, entries));
                        } else {
                            warn!("Transaction ID mismatch: expected {}, got {}", tid, id);
                        }
                    }
                }

                WALEntry::TransactionRollback { id } => {
                    if let Some((tid, _)) = current_transaction.take() {
                        if tid == id {
                            debug!("Rolled back transaction {}", id);
                        } else {
                            warn!("Transaction rollback ID mismatch: expected {}, got {}", tid, id);
                        }
                    }
                }

                WALEntry::Checkpoint { block_number, block_hash: _ } => {
                    checkpoint_block = Some(block_number);
                    info!("Found checkpoint at block {}", block_number);
                }

                entry => {
                    // Add to current transaction or apply directly
                    if let Some((_, ref mut entries)) = current_transaction {
                        entries.push(entry);
                    } else {
                        // Not in a transaction, apply directly
                        if let Err(e) = self.apply_entry(&entry).await {
                            error!("Failed to apply WAL entry: {:?}", e);
                            return Ok(RecoveryStatus::Failed {
                                reason: format!("Failed to apply entry: {}", e),
                            });
                        }
                        entries_applied += 1;
                    }
                }
            }
        }

        // Apply committed transactions
        for (_id, entries) in committed_transactions {
            for entry in entries {
                if let Err(e) = self.apply_entry(&entry).await {
                    error!("Failed to apply transaction entry: {:?}", e);
                    return Ok(RecoveryStatus::Failed {
                        reason: format!("Failed to apply transaction: {}", e),
                    });
                }
                entries_applied += 1;
            }
        }

        info!("WAL recovery completed: {} entries applied, checkpoint at {:?}",
              entries_applied, checkpoint_block);

        Ok(RecoveryStatus::Recovered {
            entries_applied,
            checkpoint_block,
        })
    }

    /// Apply a single WAL entry to the database
    async fn apply_entry(&self, entry: &WALEntry) -> Result<()> {
        match entry {
            WALEntry::CreateAccount { address, data } => {
                let key = format!("account_{}", hex::encode(address));
                self.db.insert_sync(key.as_bytes(), data)
                    .map_err(|e| NornError::Internal(format!("Failed to insert account: {}", e)))?;
                debug!("Recovered account {}", hex::encode(address));
            }

            WALEntry::UpdateAccount { address, data } => {
                let key = format!("account_{}", hex::encode(address));
                self.db.insert_sync(key.as_bytes(), data)
                    .map_err(|e| NornError::Internal(format!("Failed to update account: {}", e)))?;
                debug!("Updated account {}", hex::encode(address));
            }

            WALEntry::DeleteAccount { address } => {
                let key = format!("account_{}", hex::encode(address));
                self.db.remove_sync(key.as_bytes())
                    .map_err(|e| NornError::Internal(format!("Failed to delete account: {}", e)))?;
                debug!("Deleted account {}", hex::encode(address));
            }

            WALEntry::WriteStorage { address, key, value } => {
                let storage_key = format!("storage_{}_{}",
                    hex::encode(address),
                    hex::encode(key)
                );
                self.db.insert_sync(storage_key.as_bytes(), value)
                    .map_err(|e| NornError::Internal(format!("Failed to write storage: {}", e)))?;
                debug!("Recovered storage for {}", hex::encode(address));
            }

            WALEntry::DeleteStorage { address, key } => {
                let storage_key = format!("storage_{}_{}",
                    hex::encode(address),
                    hex::encode(key)
                );
                self.db.remove_sync(storage_key.as_bytes())
                    .map_err(|e| NornError::Internal(format!("Failed to delete storage: {}", e)))?;
                debug!("Deleted storage for {}", hex::encode(address));
            }

            WALEntry::Checkpoint { .. } => {
                // Checkpoint markers don't need to be applied
            }

            _ => {
                warn!("Skipping WAL entry during recovery: {:?}", entry);
            }
        }

        Ok(())
    }
}

/// Helper to integrate WAL with state manager
pub struct WALStateManager {
    /// Recovery manager
    recovery: Arc<WALRecoveryManager>,

    /// WAL instance
    wal: Arc<WAL>,
}

impl WALStateManager {
    /// Create a new WAL state manager
    pub fn new(wal_dir: impl AsRef<Path>, db: Arc<SledDB>) -> Result<Self> {
        let config = WALConfig::default();
        let wal = Arc::new(WAL::new(wal_dir, config)?);
        let recovery = Arc::new(WALRecoveryManager::new(wal.clone(), db));

        Ok(Self { recovery, wal })
    }

    /// Perform recovery on startup
    pub async fn recover(&self) -> Result<RecoveryStatus> {
        self.recovery.recover().await
    }

    /// Get WAL instance for writing
    pub fn wal(&self) -> &WAL {
        &self.wal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_recovery_clean_shutdown() {
        let temp_dir = TempDir::new().unwrap();
        let db_dir = temp_dir.path().join("db");
        std::fs::create_dir(&db_dir).unwrap();

        let db = Arc::new(SledDB::new(&db_dir).unwrap());
        let wal = Arc::new(WAL::new(temp_dir.path().join("wal"), WALConfig::default()).unwrap());
        let recovery = WALRecoveryManager::new(wal, db);

        let status = recovery.recover().await.unwrap();
        assert_eq!(status, RecoveryStatus::Clean);
    }

    #[tokio::test]
    async fn test_recovery_with_entries() {
        let temp_dir = TempDir::new().unwrap();
        let db_dir = temp_dir.path().join("db");
        std::fs::create_dir(&db_dir).unwrap();

        let db = Arc::new(SledDB::new(&db_dir).unwrap());
        let wal = Arc::new(WAL::new(temp_dir.path().join("wal"), WALConfig::default()).unwrap());

        // Write some WAL entries
        wal.write(WALEntry::CreateAccount {
            address: [1u8; 20],
            data: vec![2, 3, 4],
        }).unwrap();

        wal.checkpoint(100, [5u8; 32]).unwrap();
        wal.sync().unwrap();

        // Create new recovery manager (simulating restart)
        let recovery = WALRecoveryManager::new(wal.clone(), db.clone());
        let status = recovery.recover().await.unwrap();

        match status {
            RecoveryStatus::Recovered { entries_applied, checkpoint_block } => {
                assert_eq!(entries_applied, 1);
                assert_eq!(checkpoint_block, Some(100));
            }
            _ => panic!("Expected Recovered status"),
        }

        // Verify data was recovered
        let key = format!("account_{}", hex::encode([1u8; 20]));
        let data = db.get_sync(key.as_bytes()).unwrap().unwrap();
        assert_eq!(data, vec![2, 3, 4]);
    }
}
