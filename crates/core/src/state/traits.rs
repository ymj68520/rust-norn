//! Account state manager trait
//!
//! This module defines a unified trait for account state management,
//! allowing different implementations to be used interchangeably.

use crate::state::account::{AccountState, AccountType, AccountStateManager as FullAccountStateManager};
use norn_common::types::{Address, Hash};
use norn_common::error::Result;
use async_trait::async_trait;
use num_bigint::BigUint;
use std::sync::Arc;

/// Unified trait for account state management
///
/// This trait defines the common interface that all account state managers
/// must implement, allowing different implementations to be used
/// interchangeably throughout the codebase.
#[async_trait]
pub trait AccountStateManagerTrait: Send + Sync {
    /// Get account state for an address
    async fn get_account(&self, address: &Address) -> Result<Option<AccountState>>;

    /// Set account state for an address
    async fn set_account(&self, address: &Address, account: AccountState) -> Result<()>;

    /// Delete an account
    async fn delete_account(&self, address: &Address) -> Result<()>;

    /// Get account balance
    async fn get_balance(&self, address: &Address) -> Result<BigUint>;

    /// Update account balance
    async fn update_balance(&self, address: &Address, new_balance: BigUint) -> Result<()>;

    /// Add to account balance
    async fn add_balance(&self, address: &Address, amount: &BigUint) -> Result<()>;

    /// Subtract from account balance
    async fn subtract_balance(&self, address: &Address, amount: &BigUint) -> Result<()>;

    /// Get account nonce
    async fn get_nonce(&self, address: &Address) -> Result<u64>;

    /// Increment account nonce
    async fn increment_nonce(&self, address: &Address) -> Result<u64>;

    /// Get storage value
    async fn get_storage(&self, address: &Address, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Set storage value
    async fn set_storage(&self, address: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()>;

    /// Delete storage value
    async fn delete_storage(&self, address: &Address, key: &[u8]) -> Result<()>;

    /// Get state root hash
    async fn get_state_root(&self) -> Result<Hash>;

    /// Get total account count
    async fn account_count(&self) -> usize;
}

/// Type alias for the full account state manager
pub type AccountStateManager = FullAccountStateManager;

/// Implement the trait for the full account state manager
#[async_trait]
impl AccountStateManagerTrait for AccountStateManager {
    async fn get_account(&self, address: &Address) -> Result<Option<AccountState>> {
        self.get_account(address).await
    }

    async fn set_account(&self, address: &Address, account: AccountState) -> Result<()> {
        self.set_account(address, account).await
    }

    async fn delete_account(&self, address: &Address) -> Result<()> {
        self.delete_account(address).await
    }

    async fn get_balance(&self, address: &Address) -> Result<BigUint> {
        let account = self.get_account(address).await?;
        Ok(account.map(|a| a.balance).unwrap_or_else(|| BigUint::from(0u32)))
    }

    async fn update_balance(&self, address: &Address, new_balance: BigUint) -> Result<()> {
        self.update_balance(address, new_balance).await
    }

    async fn add_balance(&self, address: &Address, amount: &BigUint) -> Result<()> {
        self.add_balance(address, amount).await
    }

    async fn subtract_balance(&self, address: &Address, amount: &BigUint) -> Result<()> {
        self.subtract_balance(address, amount).await
    }

    async fn get_nonce(&self, address: &Address) -> Result<u64> {
        self.get_nonce(address).await
    }

    async fn increment_nonce(&self, address: &Address) -> Result<u64> {
        self.increment_nonce(address).await
    }

    async fn get_storage(&self, address: &Address, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.get_storage(address, key).await
    }

    async fn set_storage(&self, address: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.set_storage(address, key, value).await
    }

    async fn delete_storage(&self, address: &Address, key: &[u8]) -> Result<()> {
        self.delete_storage(address, key).await
    }

    async fn get_state_root(&self) -> Result<Hash> {
        self.get_state_root().await
    }

    async fn account_count(&self) -> usize {
        self.account_count().await
    }
}

/// Shared account state manager reference
pub type SharedAccountStateManager = Arc<AccountStateManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::account::AccountStateConfig;

    #[tokio::test]
    async fn test_trait_abstraction() {
        let config = AccountStateConfig::default();
        let manager = Arc::new(AccountStateManager::new(config));
        let address = Address([1u8; 20]);

        // Test get_account (should return None for non-existent account)
        let account = manager.get_account(&address).await.unwrap();
        assert!(account.is_none());

        // Test get_balance (should return 0 for non-existent account)
        let balance = manager.get_balance(&address).await.unwrap();
        assert_eq!(balance, BigUint::from(0u32));

        // Test get_nonce (should return 0 for non-existent account)
        let nonce = manager.get_nonce(&address).await.unwrap();
        assert_eq!(nonce, 0);
    }
}
