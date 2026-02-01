//! State management module
//!
//! Provides state management for blockchain accounts and storage.

pub mod merkle;
pub mod persistent;
pub mod account;  // Comprehensive account state implementation
pub mod cache;    // Synchronous cache for async/sync bridging
pub mod traits;   // Unified trait for account state management
pub mod history;  // State change history tracking
pub mod pruning;  // State pruning for storage optimization

// Re-export the comprehensive account state manager and trait
pub use account::{AccountState, AccountType, AccountStateConfig, AccountStateManager};
pub use traits::{AccountStateManagerTrait, SharedAccountStateManager};
pub use history::{StateHistory, StateChangeRecord, StateChangeType, StateSnapshot};
pub use persistent::{PersistentStateManager, PersistentConfig};
pub use pruning::{PruningConfig, PruningStats, StatePruningManager, PruningResult};

use norn_common::types::{Hash, Address};
use norn_common::error::{NornError, Result};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use num_bigint::BigUint;

/// State change record
#[derive(Debug, Clone)]
pub enum StateChange {
    AccountCreated { address: Address, account: AccountState },
    AccountUpdated { address: Address, old: AccountState, new: AccountState },
    AccountDeleted { address: Address },
    BalanceChanged { address: Address, old_balance: String, new_balance: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_manager_reexport() {
        // Test that the re-exported AccountStateManager works
        let config = AccountStateConfig::default();
        let manager = AccountStateManager::new(config);
        let address = Address([1u8; 20]);

        // Create account
        let account = AccountState {
            address,
            balance: BigUint::from(1000u64),
            nonce: 0,
            code_hash: None,
            storage_root: Hash::default(),
            account_type: AccountType::Normal,
            created_at: 0,
            updated_at: 0,
            deleted: false,
        };

        manager.set_account(&address, account).await.unwrap();

        // Get account
        let retrieved = manager.get_account(&address).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().balance, BigUint::from(1000u64));
    }
}

