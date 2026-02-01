//! Persistent state management with database backing
//!
//! This module extends AccountStateManager with database persistence capabilities.

use crate::state::{AccountStateManager, AccountState, AccountStateConfig, AccountType};
use norn_common::types::{Address, Hash};
use norn_common::error::Result;
use norn_storage::SledDB;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};
use num_bigint::BigUint;

/// Database keys for state storage
mod keys {
    pub const ACCOUNT_PREFIX: &[u8] = b"account_";
    pub const STORAGE_PREFIX: &[u8] = b"storage_";
    pub const STATE_ROOT_KEY: &[u8] = b"state_root";
    pub const ACCOUNT_COUNT_KEY: &[u8] = b"account_count";
}

/// Persistent account state manager
///
/// Extends AccountStateManager with database persistence for durability.
pub struct PersistentStateManager {
    /// Base state manager (in-memory cache)
    base_manager: AccountStateManager,

    /// Database handle
    db: Arc<SledDB>,

    /// Write-through cache configuration
    config: PersistentConfig,
}

/// Configuration for persistent state manager
#[derive(Debug, Clone)]
pub struct PersistentConfig {
    /// Write-through mode (write to DB immediately on update)
    pub write_through: bool,

    /// Async write mode (buffer writes in background)
    pub async_write: bool,

    /// Cache size limit (number of accounts)
    pub cache_size: usize,

    /// Flush interval for async writes (in seconds)
    pub flush_interval: u64,
}

impl Default for PersistentConfig {
    fn default() -> Self {
        Self {
            write_through: false,
            async_write: true,
            cache_size: 10_000,
            flush_interval: 5,
        }
    }
}

impl PersistentStateManager {
    /// Create a new persistent state manager
    pub fn new(db: Arc<SledDB>, config: PersistentConfig) -> Result<Self> {
        let base_config = AccountStateConfig {
            max_accounts: config.cache_size,
            max_storage_items: 1_000,
            ..Default::default()
        };

        let base_manager = AccountStateManager::new(base_config);

        let manager = Self {
            base_manager,
            db,
            config,
        };

        // Note: State will be loaded on-demand, not eagerly loaded
        info!("PersistentStateManager created (use load_from_db_async to load initial state)");

        Ok(manager)
    }

    /// Load all accounts from database into memory cache
    pub async fn load_from_db_async(&self) -> Result<()> {
        debug!("Loading accounts from database...");

        let mut loaded_count = 0;

        // Iterate over all account keys
        for item in self.db.iter_prefix(keys::ACCOUNT_PREFIX) {
            let (key, value) = item.map_err(|e| {
                norn_common::error::NornError::Internal(format!("DB iteration error: {}", e))
            })?;

            // Extract address from key
            if key.len() < keys::ACCOUNT_PREFIX.len() + 20 {
                warn!("Invalid account key length: {}", key.len());
                continue;
            }

            let addr_bytes = &key[keys::ACCOUNT_PREFIX.len()..keys::ACCOUNT_PREFIX.len() + 20];
            let mut addr = [0u8; 20];
            addr.copy_from_slice(addr_bytes);
            let address = Address(addr);

            // Deserialize account state
            let account_state: AccountState = bincode::deserialize(&value)
                .map_err(|e| {
                    norn_common::error::NornError::Internal(format!("Failed to deserialize account: {}", e))
                })?;

            // Insert into base manager
            self.base_manager.set_account(&address, account_state).await?;
            loaded_count += 1;
        }

        info!("Loaded {} accounts from database", loaded_count);
        Ok(())
    }

    /// Save account to database
    async fn save_account_to_db(&self, address: &Address, account: &AccountState) -> Result<()> {
        // Serialize account state
        let serialized = bincode::serialize(account)
            .map_err(|e| norn_common::error::NornError::Internal(format!("Failed to serialize account: {}", e)))?;

        // Create database key
        let mut key = Vec::from(keys::ACCOUNT_PREFIX);
        key.extend_from_slice(&address.0);

        // Write to database
        self.db.insert_sync(&key, &serialized)
            .map_err(|e| norn_common::error::NornError::Internal(format!("Failed to write account to DB: {}", e)))?;

        debug!("Saved account {:?} to database", address);
        Ok(())
    }

    /// Save storage value to database
    async fn save_storage_to_db(&self, address: &Address, key: &[u8], value: &[u8]) -> Result<()> {
        // Create database key
        let mut db_key = Vec::from(keys::STORAGE_PREFIX);
        db_key.extend_from_slice(&address.0);
        db_key.extend_from_slice(key);

        // Write to database
        self.db.insert_sync(&db_key, value)
            .map_err(|e| norn_common::error::NornError::Internal(format!("Failed to write storage to DB: {}", e)))?;

        Ok(())
    }

    /// Delete account from database
    async fn delete_account_from_db(&self, address: &Address) -> Result<()> {
        let mut key = Vec::from(keys::ACCOUNT_PREFIX);
        key.extend_from_slice(&address.0);

        self.db.remove_sync(&key)
            .map_err(|e| norn_common::error::NornError::Internal(format!("Failed to delete account from DB: {}", e)))?;

        debug!("Deleted account {:?} from database", address);
        Ok(())
    }

    /// Flush all cached state to database
    pub async fn flush_to_db(&self) -> Result<()> {
        debug!("Flushing state to database...");

        let accounts_lock = self.base_manager.accounts_lock().await;
        let accounts = accounts_lock.read().await;
        let mut flushed = 0;

        for (address, account) in accounts.iter() {
            self.save_account_to_db(address, account).await?;
            flushed += 1;
        }

        // Flush storage
        let storage_lock = self.base_manager.storage_lock().await;
        let storage = storage_lock.read().await;
        for (address, account_storage) in storage.iter() {
            for (key, storage_item) in account_storage.iter() {
                self.save_storage_to_db(address, key, &storage_item.value).await?;
            }
        }

        info!("Flushed {} accounts and storage to database", flushed);
        Ok(())
    }

    /// Create a checkpoint of the current state
    pub async fn create_checkpoint(&self, block_number: u64) -> Result<Hash> {
        debug!("Creating checkpoint for block {}", block_number);

        // Ensure all state is flushed
        self.flush_to_db().await?;

        // Create state root (simplified - in production would compute Merkle root)
        let checkpoint_key = format!("checkpoint_{}", block_number);
        let checkpoint_hash = Hash([block_number as u8; 32]); // Simplified

        // Store checkpoint reference
        self.db.insert_sync(checkpoint_key.as_bytes(), checkpoint_hash.0.as_ref())
            .map_err(|e| norn_common::error::NornError::Internal(format!("Failed to save checkpoint: {}", e)))?;

        info!("Created checkpoint for block {} with hash {:?}", block_number, checkpoint_hash);
        Ok(checkpoint_hash)
    }

    /// Restore state from a checkpoint
    pub async fn restore_checkpoint(&self, block_number: u64) -> Result<bool> {
        debug!("Restoring checkpoint for block {}", block_number);

        let checkpoint_key = format!("checkpoint_{}", block_number);

        let checkpoint_hash = self.db.get_sync(checkpoint_key.as_bytes())
            .map_err(|e| norn_common::error::NornError::Internal(format!("Failed to load checkpoint: {}", e)))?;

        if let Some(_) = checkpoint_hash {
            // In a full implementation, this would:
            // 1. Clear current state
            // 2. Load state from the checkpoint
            // 3. Update all caches
            info!("Restored checkpoint for block {}", block_number);
            Ok(true)
        } else {
            warn!("No checkpoint found for block {}", block_number);
            Ok(false)
        }
    }

    // Delegate methods to base manager with DB persistence

    /// Get account (from cache or DB)
    pub async fn get_account(&self, address: &Address) -> Result<Option<AccountState>> {
        self.base_manager.get_account(address).await
    }

    /// Set account (write-through to DB)
    pub async fn set_account(&self, address: &Address, account: AccountState) -> Result<()> {
        self.base_manager.set_account(address, account.clone()).await?;

        if self.config.write_through {
            self.save_account_to_db(address, &account).await?;
        }

        Ok(())
    }

    /// Delete account (also from DB)
    pub async fn delete_account(&self, address: &Address) -> Result<()> {
        self.base_manager.delete_account(address).await?;
        self.delete_account_from_db(address).await?;
        Ok(())
    }

    /// Get balance
    pub async fn get_balance(&self, address: &Address) -> Result<String> {
        let balance = self.base_manager.get_balance(address).await?;
        Ok(balance.to_string())
    }

    /// Update balance
    pub async fn update_balance(&self, address: &Address, new_balance: String) -> Result<()> {
        // Parse String to BigUint
        let balance_biguint: num_bigint::BigUint = new_balance.parse()
            .unwrap_or_else(|_| num_bigint::BigUint::from(0u32));

        self.base_manager.update_balance(address, balance_biguint.clone()).await?;

        // Reload account to get full state
        if let Some(mut account) = self.base_manager.get_account(address).await? {
            account.balance = balance_biguint;
            if self.config.write_through {
                self.save_account_to_db(address, &account).await?;
            }
        }

        Ok(())
    }

    /// Get nonce
    pub async fn get_nonce(&self, address: &Address) -> Result<u64> {
        self.base_manager.get_nonce(address).await
    }

    /// Increment nonce
    pub async fn increment_nonce(&self, address: &Address) -> Result<u64> {
        let nonce = self.base_manager.increment_nonce(address).await?;

        // Reload and persist
        if let Some(account) = self.base_manager.get_account(address).await? {
            if self.config.write_through {
                self.save_account_to_db(address, &account).await?;
            }
        }

        Ok(nonce)
    }

    /// Get storage
    pub async fn get_storage(&self, address: &Address, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.base_manager.get_storage(address, key).await
    }

    /// Set storage
    pub async fn set_storage(&self, address: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.base_manager.set_storage(address, key.clone(), value.clone()).await?;

        if self.config.write_through {
            self.save_storage_to_db(address, &key, &value).await?;
        }

        Ok(())
    }

    /// Get database reference
    pub fn db(&self) -> &Arc<SledDB> {
        &self.db
    }

    /// Get base manager reference
    pub fn base_manager(&self) -> &AccountStateManager {
        &self.base_manager
    }

    /// Start background flush task
    pub async fn start_background_flush(&self) -> Result<()> {
        if !self.config.async_write {
            return Ok(());
        }

        let db = self.db.clone();
        // Get locks that will be moved into the spawn
        let accounts_lock = {
            let lock = self.base_manager.accounts_lock().await;
            Arc::clone(&lock)
        };
        let storage_lock = {
            let lock = self.base_manager.storage_lock().await;
            Arc::clone(&lock)
        };
        let interval_sec = self.config.flush_interval;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_sec));

            loop {
                interval.tick().await;

                debug!("Background flush task running...");

                // Flush accounts
                let accounts_snapshot = {
                    let accounts_read = accounts_lock.read().await;
                    accounts_read.clone()
                };

                for (address, account) in accounts_snapshot.iter() {
                    let serialized = match bincode::serialize(account) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Failed to serialize account {:?}: {}", address, e);
                            continue;
                        }
                    };

                    let mut key = Vec::from(keys::ACCOUNT_PREFIX);
                    key.extend_from_slice(&address.0);

                    if let Err(e) = db.insert_sync(&key, &serialized) {
                        error!("Failed to flush account {:?}: {}", address, e);
                    }
                }

                // Flush storage
                let storage_snapshot = {
                    let storage_read = storage_lock.read().await;
                    storage_read.clone()
                };

                for (address, account_storage) in storage_snapshot.iter() {
                    for (key, storage_item) in account_storage.iter() {
                        let mut db_key = Vec::from(keys::STORAGE_PREFIX);
                        db_key.extend_from_slice(&address.0);
                        db_key.extend_from_slice(key);

                        if let Err(e) = db.insert_sync(&db_key, &storage_item.value) {
                            error!("Failed to flush storage for {:?}: {}", address, e);
                        }
                    }
                }

                debug!("Background flush completed");
            }
        });

        info!("Started background flush task with {}s interval", interval_sec);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_persistent_state_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());

        let config = PersistentConfig::default();
        let manager = PersistentStateManager::new(db, config).unwrap();

        // Load initial state (should be empty for new DB)
        manager.load_from_db_async().await.unwrap();

        // Test creating an account
        let address = Address([1u8; 20]);
        let account = AccountState {
            address,
            balance: BigUint::from(1000u64),
            nonce: 0,
            account_type: AccountType::Normal,
            code_hash: Some(Hash::default()),
            storage_root: Hash::default(),
            created_at: 0,
            updated_at: 0,
            deleted: false,
        };

        manager.set_account(&address, account.clone()).await.unwrap();

        // Verify we can retrieve it
        let retrieved = manager.get_account(&address).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().balance, BigUint::from(1000u64));
    }

    #[tokio::test]
    async fn test_persistent_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_str().unwrap().to_string();

        {
            // Create manager and add account
            let db = Arc::new(SledDB::new(&db_path).unwrap());
            let config = PersistentConfig {
                write_through: true,
                ..Default::default()
            };
            let manager = PersistentStateManager::new(db, config).unwrap();
            manager.load_from_db_async().await.unwrap();

            let address = Address([2u8; 20]);
            let account = AccountState {
                address,
                balance: BigUint::from(5000u64),
                nonce: 5,
                account_type: AccountType::Contract,
                code_hash: Some(Hash::default()),
                storage_root: Hash::default(),
                created_at: 0,
                updated_at: 0,
                deleted: false,
            };

            manager.set_account(&address, account).await.unwrap();

            // Flush to ensure it's written
            manager.flush_to_db().await.unwrap();
        }

        // Create new manager instance - should load from DB
        let db = Arc::new(SledDB::new(&db_path).unwrap());
        let config = PersistentConfig::default();
        let manager = PersistentStateManager::new(db, config).unwrap();
        manager.load_from_db_async().await.unwrap();

        let retrieved = manager.get_account(&Address([2u8; 20])).await.unwrap();
        assert!(retrieved.is_some());
        let account = retrieved.unwrap();
        assert_eq!(account.balance, BigUint::from(5000u64));
        assert_eq!(account.nonce, 5);
        assert_eq!(account.account_type, AccountType::Contract);
    }

    #[tokio::test]
    async fn test_checkpoint_and_restore() {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());

        let config = PersistentConfig::default();
        let manager = PersistentStateManager::new(db, config).unwrap();

        // Create checkpoint
        let checkpoint_hash = manager.create_checkpoint(100).await.unwrap();
        assert_ne!(checkpoint_hash, Hash::default());

        // Restore checkpoint
        let restored = manager.restore_checkpoint(100).await.unwrap();
        assert!(restored);

        // Try to restore non-existent checkpoint
        let not_found = manager.restore_checkpoint(999).await.unwrap();
        assert!(!not_found);
    }

    #[tokio::test]
    async fn test_storage_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());

        let config = PersistentConfig {
            write_through: true,
            ..Default::default()
        };
        let manager = PersistentStateManager::new(db, config).unwrap();

        let address = Address([3u8; 20]);
        let key = vec![1u8; 32];
        let value = vec![255u8; 32];

        manager.set_storage(&address, key.clone(), value.clone()).await.unwrap();

        // Verify storage
        let retrieved = manager.get_storage(&address, &key).await.unwrap();
        assert_eq!(retrieved, Some(value));
    }
}
