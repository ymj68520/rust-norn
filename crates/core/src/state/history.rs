//! State change history tracking
//!
//! This module provides time-travel queries for blockchain state,
//! allowing queries of historical account states and storage values.

use crate::state::{AccountState, AccountType};
use norn_common::types::{Address, Hash};
use norn_common::error::{NornError, Result};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use std::time::{SystemTime, UNIX_EPOCH};

/// State change record
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateChangeRecord {
    /// Block number when this change occurred
    pub block_number: u64,

    /// Timestamp of the change
    pub timestamp: u64,

    /// Address that was changed
    pub address: Address,

    /// Type of change
    pub change_type: StateChangeType,

    /// Previous state (if applicable)
    pub old_state: Option<AccountState>,

    /// New state
    pub new_state: AccountState,
}

/// Type of state change
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StateChangeType {
    /// Account created
    AccountCreated,

    /// Account updated
    AccountUpdated {
        fields_changed: Vec<String>,
    },

    /// Account deleted
    AccountDeleted,

    /// Storage modified
    StorageModified {
        key: Vec<u8>,
    },
}

/// Snapshot of the entire state at a specific block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Block number
    pub block_number: u64,

    /// Block hash
    pub block_hash: Hash,

    /// Timestamp
    pub timestamp: u64,

    /// Account states
    pub accounts: HashMap<Address, AccountState>,

    /// State root hash
    pub state_root: Hash,
}

/// State history manager
pub struct StateHistory {
    /// Maximum number of snapshots to keep
    max_snapshots: usize,

    /// Snapshots indexed by block number
    snapshots: Arc<RwLock<HashMap<u64, StateSnapshot>>>,

    /// State changes indexed by block number
    changes: Arc<RwLock<HashMap<u64, Vec<StateChangeRecord>>>>,

    /// Current block number
    current_block: Arc<RwLock<u64>>,
}

impl StateHistory {
    /// Create a new state history manager
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            max_snapshots,
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            changes: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(0)),
        }
    }

    /// Record a state change
    pub async fn record_change(&self, change: StateChangeRecord) -> Result<()> {
        debug!("Recording state change at block {}", change.block_number);

        let mut changes = self.changes.write().await;
        changes.entry(change.block_number)
            .or_insert_with(Vec::new)
            .push(change);

        Ok(())
    }

    /// Create a snapshot at the current block
    pub async fn create_snapshot(
        &self,
        block_number: u64,
        block_hash: Hash,
        accounts: HashMap<Address, AccountState>,
        state_root: Hash,
    ) -> Result<StateSnapshot> {
        info!("Creating state snapshot at block {}", block_number);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let snapshot = StateSnapshot {
            block_number,
            block_hash,
            timestamp,
            accounts,
            state_root,
        };

        // Store snapshot
        let mut snapshots = self.snapshots.write().await;
        snapshots.insert(block_number, snapshot.clone());

        // Update current block
        let mut current = self.current_block.write().await;
        *current = block_number;

        // Prune old snapshots if necessary
        if snapshots.len() > self.max_snapshots {
            let to_remove = snapshots.len() - self.max_snapshots;
            let mut keys: Vec<u64> = snapshots.keys().copied().collect();
            keys.sort();

            for key in keys.iter().take(to_remove) {
                snapshots.remove(key);
                debug!("Pruned old snapshot at block {}", key);
            }
        }

        Ok(snapshot)
    }

    /// Get account state at a specific block (time-travel query)
    pub async fn get_account_at_block(
        &self,
        address: &Address,
        block_number: u64,
    ) -> Result<Option<AccountState>> {
        debug!("Querying account state at block {}", block_number);

        let snapshots = self.snapshots.read().await;

        // Find the most recent snapshot at or before the requested block
        let mut best_block = None;
        for &snap_block in snapshots.keys() {
            if snap_block <= block_number {
                if best_block.is_none() || snap_block > best_block.unwrap() {
                    best_block = Some(snap_block);
                }
            }
        }

        if let Some(snap_block) = best_block {
            let snapshot = snapshots.get(&snap_block).unwrap();
            let account = snapshot.accounts.get(address).cloned();

            // Now apply changes from snapshots after the found one
            if let Some(mut account_state) = account {
                let changes = self.changes.read().await;

                for block in (snap_block + 1)..=block_number {
                    if let Some(block_changes) = changes.get(&block) {
                        for change in block_changes {
                            if &change.address == address {
                                match &change.change_type {
                                    StateChangeType::AccountCreated => {
                                        account_state = change.new_state.clone();
                                    }
                                    StateChangeType::AccountUpdated { .. } => {
                                        account_state = change.new_state.clone();
                                    }
                                    StateChangeType::AccountDeleted => {
                                        return Ok(None);
                                    }
                                    StateChangeType::StorageModified { .. } => {
                                        // Storage changes don't affect account state directly
                                    }
                                }
                            }
                        }
                    }
                }

                return Ok(Some(account_state));
            }
        }

        // No snapshot found, account doesn't exist at this block
        Ok(None)
    }

    /// Get storage value at a specific block (time-travel query)
    pub async fn get_storage_at_block(
        &self,
        address: &Address,
        key: &[u8],
        block_number: u64,
    ) -> Result<Option<Vec<u8>>> {
        debug!("Querying storage at block {} for key {:?}", block_number, key);

        // Get account state first
        if let Some(account) = self.get_account_at_block(address, block_number).await? {
            // Account storage would need to be tracked separately
            // For now, return None as storage history is not implemented
            Ok(None)
        } else {
            Ok(None)
        }
    }

    /// Get all changes in a block
    pub async fn get_changes_at_block(&self, block_number: u64) -> Result<Vec<StateChangeRecord>> {
        let changes = self.changes.read().await;
        Ok(changes.get(&block_number)
            .cloned()
            .unwrap_or_default())
    }

    /// Get current block number
    pub async fn current_block(&self) -> u64 {
        *self.current_block.read().await
    }

    /// Get all snapshots
    pub async fn get_all_snapshots(&self) -> Vec<StateSnapshot> {
        let snapshots = self.snapshots.read().await;
        snapshots.values().cloned().collect()
    }

    /// Clear all history (for testing or pruning)
    pub async fn clear(&self) -> Result<()> {
        info!("Clearing state history");

        let mut snapshots = self.snapshots.write().await;
        snapshots.clear();

        let mut changes = self.changes.write().await;
        changes.clear();

        let mut current = self.current_block.write().await;
        *current = 0;

        Ok(())
    }

    /// Prune a snapshot at a specific block
    /// Returns true if the snapshot was found and removed
    pub async fn prune_snapshot(&self, block_number: u64) -> Result<bool> {
        debug!("Pruning snapshot at block {}", block_number);

        let mut snapshots = self.snapshots.write().await;
        let removed = snapshots.remove(&block_number).is_some();

        if removed {
            debug!("Successfully pruned snapshot at block {}", block_number);
        } else {
            debug!("No snapshot found at block {}", block_number);
        }

        Ok(removed)
    }

    /// Prune state changes at a specific block
    /// Returns the number of change records removed
    pub async fn prune_changes(&self, block_number: u64) -> Result<usize> {
        debug!("Pruning changes at block {}", block_number);

        let mut changes = self.changes.write().await;
        let removed = changes.remove(&block_number)
            .map(|v| v.len())
            .unwrap_or(0);

        if removed > 0 {
            debug!("Pruned {} change(s) at block {}", removed, block_number);
        }

        Ok(removed)
    }

    /// Prune all snapshots and changes before a cutoff block
    /// Returns the number of snapshots and changes pruned
    pub async fn prune_before(&self, cutoff_block: u64) -> Result<(usize, usize)> {
        info!("Pruning all history before block {}", cutoff_block);

        let mut snapshots_pruned = 0;
        let mut changes_pruned = 0;

        // Prune old snapshots
        {
            let mut snapshots = self.snapshots.write().await;
            let mut keys_to_remove = Vec::new();

            for &block in snapshots.keys() {
                if block < cutoff_block {
                    keys_to_remove.push(block);
                }
            }

            for block in keys_to_remove {
                snapshots.remove(&block);
                snapshots_pruned += 1;
            }
        }

        // Prune old changes
        {
            let mut changes = self.changes.write().await;
            let mut keys_to_remove = Vec::new();

            for &block in changes.keys() {
                if block < cutoff_block {
                    keys_to_remove.push(block);
                }
            }

            for block in keys_to_remove {
                let count = changes.remove(&block).unwrap().len();
                changes_pruned += count;
            }
        }

        info!(
            "Pruned {} snapshots and {} changes before block {}",
            snapshots_pruned, changes_pruned, cutoff_block
        );

        Ok((snapshots_pruned, changes_pruned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigUint;

    #[tokio::test]
    async fn test_state_history_creation() {
        let history = StateHistory::new(10);

        assert_eq!(history.current_block().await, 0);
        assert_eq!(history.get_all_snapshots().await.len(), 0);
    }

    #[tokio::test]
    async fn test_snapshot_creation() {
        let history = StateHistory::new(10);

        let address = Address([1u8; 20]);
        let mut accounts = HashMap::new();

        accounts.insert(address, AccountState {
            address,
            balance: BigUint::from(1000u64),
            nonce: 0,
            code_hash: None,
            storage_root: Hash::default(),
            account_type: AccountType::Normal,
            created_at: 0,
            updated_at: 0,
            deleted: false,
        });

        let snapshot = history.create_snapshot(
            100,
            Hash([2u8; 32]),
            accounts,
            Hash([3u8; 32]),
        ).await.unwrap();

        assert_eq!(snapshot.block_number, 100);
        assert_eq!(snapshot.accounts.len(), 1);
        assert_eq!(history.current_block().await, 100);
    }

    #[tokio::test]
    async fn test_time_travel_query() {
        let history = StateHistory::new(10);

        let address = Address([1u8; 20]);

        // Create snapshot at block 100
        let mut accounts = HashMap::new();
        accounts.insert(address, AccountState {
            address,
            balance: BigUint::from(1000u64),
            nonce: 0,
            code_hash: None,
            storage_root: Hash::default(),
            account_type: AccountType::Normal,
            created_at: 0,
            updated_at: 0,
            deleted: false,
        });

        history.create_snapshot(
            100,
            Hash([1u8; 32]),
            accounts,
            Hash([2u8; 32]),
        ).await.unwrap();

        // Query account at block 100
        let account = history.get_account_at_block(&address, 100).await.unwrap();
        assert!(account.is_some());
        assert_eq!(account.unwrap().balance, BigUint::from(1000u64));

        // Query account at block 50 (should return None - before creation)
        let account = history.get_account_at_block(&address, 50).await.unwrap();
        assert!(account.is_none());
    }

    #[tokio::test]
    async fn test_state_change_recording() {
        let history = StateHistory::new(10);

        let address = Address([1u8; 20]);
        let new_state = AccountState {
            address,
            balance: BigUint::from(2000u64),
            nonce: 1,
            code_hash: None,
            storage_root: Hash::default(),
            account_type: AccountType::Normal,
            created_at: 0,
            updated_at: 0,
            deleted: false,
        };

        let change = StateChangeRecord {
            block_number: 100,
            timestamp: 1234567890,
            address,
            change_type: StateChangeType::AccountUpdated {
                fields_changed: vec!["balance".to_string(), "nonce".to_string()],
            },
            old_state: None,
            new_state: new_state.clone(),
        };

        history.record_change(change).await.unwrap();

        let changes = history.get_changes_at_block(100).await.unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].block_number, 100);
    }
}
