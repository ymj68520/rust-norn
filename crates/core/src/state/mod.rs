//! State management module
//! 
//! Provides state management for blockchain accounts and storage.

use norn_common::types::{Hash, Address};
use norn_common::error::{NornError, Result};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Account state - simplified version
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AccountState {
    /// Account address
    pub address: Address,
    /// Account balance (as string for serialization simplicity)
    pub balance: String,
    /// Account nonce
    pub nonce: u64,
    /// Account type
    pub account_type: AccountType,
    /// Code hash (for contract accounts)
    pub code_hash: Hash,
    /// Storage root hash
    pub storage_root: Hash,
}

/// Account type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum AccountType {
    #[default]
    Normal,
    Contract,
    Validator,
    System,
}

/// State change record
#[derive(Debug, Clone)]
pub enum StateChange {
    AccountCreated { address: Address, account: AccountState },
    AccountUpdated { address: Address, old: AccountState, new: AccountState },
    AccountDeleted { address: Address },
    BalanceChanged { address: Address, old_balance: String, new_balance: String },
}

/// Account state configuration
#[derive(Debug, Clone)]
pub struct AccountStateConfig {
    pub max_accounts: usize,
    pub max_storage_per_account: usize,
}

impl Default for AccountStateConfig {
    fn default() -> Self {
        Self {
            max_accounts: 1_000_000,
            max_storage_per_account: 10_000,
        }
    }
}

/// Account state manager
pub struct AccountStateManager {
    accounts: Arc<RwLock<HashMap<Address, AccountState>>>,
    storage: Arc<RwLock<HashMap<Address, HashMap<Vec<u8>, Vec<u8>>>>>,
    config: AccountStateConfig,
}

impl AccountStateManager {
    /// Create new account state manager
    pub fn new(config: AccountStateConfig) -> Self {
        Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            storage: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Get account state
    pub async fn get_account(&self, address: &Address) -> Result<Option<AccountState>> {
        let accounts = self.accounts.read().await;
        Ok(accounts.get(address).cloned())
    }

    /// Set account state
    pub async fn set_account(&self, address: &Address, account: AccountState) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        if accounts.len() >= self.config.max_accounts && !accounts.contains_key(address) {
            return Err(NornError::Internal("Maximum account limit reached".to_string()));
        }
        accounts.insert(*address, account);
        debug!("Account updated: {:?}", address);
        Ok(())
    }

    /// Delete account
    pub async fn delete_account(&self, address: &Address) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        accounts.remove(address);
        
        // Also remove storage
        let mut storage = self.storage.write().await;
        storage.remove(address);
        
        debug!("Account deleted: {:?}", address);
        Ok(())
    }

    /// Get account balance
    pub async fn get_balance(&self, address: &Address) -> Result<String> {
        let accounts = self.accounts.read().await;
        match accounts.get(address) {
            Some(account) => Ok(account.balance.clone()),
            None => Ok("0".to_string()),
        }
    }

    /// Update account balance
    pub async fn update_balance(&self, address: &Address, new_balance: String) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        if let Some(account) = accounts.get_mut(address) {
            account.balance = new_balance;
            Ok(())
        } else {
            // Create new account with balance
            let account = AccountState {
                address: *address,
                balance: new_balance,
                nonce: 0,
                account_type: AccountType::Normal,
                code_hash: Hash::default(),
                storage_root: Hash::default(),
            };
            accounts.insert(*address, account);
            Ok(())
        }
    }

    /// Get account nonce
    pub async fn get_nonce(&self, address: &Address) -> Result<u64> {
        let accounts = self.accounts.read().await;
        match accounts.get(address) {
            Some(account) => Ok(account.nonce),
            None => Ok(0),
        }
    }

    /// Increment account nonce
    pub async fn increment_nonce(&self, address: &Address) -> Result<u64> {
        let mut accounts = self.accounts.write().await;
        if let Some(account) = accounts.get_mut(address) {
            account.nonce += 1;
            Ok(account.nonce)
        } else {
            Err(NornError::Internal("Account not found".to_string()))
        }
    }

    /// Get storage value
    pub async fn get_storage(&self, address: &Address, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let storage = self.storage.read().await;
        if let Some(account_storage) = storage.get(address) {
            Ok(account_storage.get(key).cloned())
        } else {
            Ok(None)
        }
    }

    /// Set storage value
    pub async fn set_storage(&self, address: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let mut storage = self.storage.write().await;
        let account_storage = storage.entry(*address).or_insert_with(HashMap::new);
        
        if account_storage.len() >= self.config.max_storage_per_account && !account_storage.contains_key(&key) {
            return Err(NornError::Internal("Maximum storage limit reached".to_string()));
        }
        
        account_storage.insert(key, value);
        Ok(())
    }

    /// Delete storage value
    pub async fn delete_storage(&self, address: &Address, key: &[u8]) -> Result<()> {
        let mut storage = self.storage.write().await;
        if let Some(account_storage) = storage.get_mut(address) {
            account_storage.remove(key);
        }
        Ok(())
    }

    /// Get state root hash (simplified)
    pub async fn get_state_root(&self) -> Result<Hash> {
        use sha2::{Sha256, Digest};
        
        let accounts = self.accounts.read().await;
        let mut hasher = Sha256::new();
        
        // Simple state root calculation
        for (address, account) in accounts.iter() {
            hasher.update(&address.0);
            hasher.update(account.balance.as_bytes());
            hasher.update(account.nonce.to_le_bytes());
        }
        
        let result = hasher.finalize();
        let mut hash = Hash::default();
        hash.0.copy_from_slice(&result);
        Ok(hash)
    }

    /// Get total account count
    pub async fn account_count(&self) -> usize {
        let accounts = self.accounts.read().await;
        accounts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_account_operations() {
        let manager = AccountStateManager::new(AccountStateConfig::default());
        let address = Address([1u8; 20]);
        
        // Create account
        let account = AccountState {
            address,
            balance: "1000".to_string(),
            nonce: 0,
            account_type: AccountType::Normal,
            code_hash: Hash::default(),
            storage_root: Hash::default(),
        };
        
        manager.set_account(&address, account).await.unwrap();
        
        // Get account
        let retrieved = manager.get_account(&address).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().balance, "1000");
        
        // Update balance
        manager.update_balance(&address, "2000".to_string()).await.unwrap();
        let balance = manager.get_balance(&address).await.unwrap();
        assert_eq!(balance, "2000");
        
        // Increment nonce
        let nonce = manager.increment_nonce(&address).await.unwrap();
        assert_eq!(nonce, 1);
    }

    #[tokio::test]
    async fn test_storage_operations() {
        let manager = AccountStateManager::new(AccountStateConfig::default());
        let address = Address([1u8; 20]);
        
        // Set storage
        manager.set_storage(&address, b"key1".to_vec(), b"value1".to_vec()).await.unwrap();
        
        // Get storage
        let value = manager.get_storage(&address, b"key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
        
        // Delete storage
        manager.delete_storage(&address, b"key1").await.unwrap();
        let value = manager.get_storage(&address, b"key1").await.unwrap();
        assert!(value.is_none());
    }
}