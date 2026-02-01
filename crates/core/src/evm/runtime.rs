//! EVM runtime integration with revm
//!
//! This module provides the bridge between revm's Database trait and norn's state management.
//! It uses the SyncStateManager to provide synchronous access to async state operations.

use crate::state::cache::SyncStateManager;
use crate::evm::CodeStorage;
use norn_common::types::Address;
use revm::{
    primitives::{
        AccountInfo, Address as RevmAddress, Bytecode, Bytes, HashMap, B256, U256,
        KECCAK_EMPTY,
    },
    Database,
};
use revm::DatabaseCommit;
use revm::primitives::HashMap as RevmHashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{debug, warn, error};

/// Norn database adapter for revm
///
/// This implements revm's Database trait to bridge between revm's synchronous interface
/// and norn's async state management via the SyncStateManager cache layer.
pub struct NornDatabaseAdapter {
    /// Synchronous state manager (cache layer)
    state: SyncStateManager,

    /// Code storage for contract bytecode
    code_storage: Arc<CodeStorage>,

    /// Block hash cache for BLOCKHASH opcode
    block_hashes: HashMap<u64, B256>,

    /// Current block number
    block_number: u64,
}

impl NornDatabaseAdapter {
    /// Create a new database adapter
    pub fn new(state: SyncStateManager, block_number: u64) -> Self {
        Self {
            state,
            code_storage: Arc::new(CodeStorage::new()),
            block_hashes: HashMap::default(),
            block_number,
        }
    }

    /// Create a new database adapter with custom code storage
    pub fn with_code_storage(
        state: SyncStateManager,
        code_storage: Arc<CodeStorage>,
        block_number: u64,
    ) -> Self {
        Self {
            state,
            code_storage,
            block_hashes: HashMap::default(),
            block_number,
        }
    }

    /// Get reference to code storage
    pub fn code_storage(&self) -> &Arc<CodeStorage> {
        &self.code_storage
    }

    /// Insert a block hash for BLOCKHASH opcode
    pub fn insert_block_hash(&mut self, number: u64, hash: B256) {
        self.block_hashes.insert(number, hash);
    }

    /// Get basic account information (balance, nonce, code hash, storage root)
    ///
    /// This is the core method that revm calls to access account state.
    /// It uses the SyncStateManager to bridge async state operations.
    pub fn get_account_basic(
        &mut self,
        address: RevmAddress,
    ) -> Result<Option<AccountInfo>, Infallible> {
        debug!("Getting account basic for address: {:?}", address);

        // Convert revm address to norn address
        let addr_bytes: [u8; 20] = address.as_slice().try_into().unwrap_or([0u8; 20]);
        let norn_address = Address(addr_bytes);

        // Get balance
        let balance_str = self.state.get_balance(&norn_address)
            .unwrap_or_else(|e| {
                warn!("Failed to get balance for {:?}: {}", address, e);
                "0".to_string()
            });

        // Parse balance from string to U256
        let balance = {
            let balance_u128: u128 = balance_str.parse()
                .unwrap_or_else(|e| {
                    warn!("Failed to parse balance '{}' for {:?}: {}", balance_str, address, e);
                    0
                });
            U256::from(balance_u128)
        };

        // Get nonce
        let nonce = self.state.get_nonce(&norn_address)
            .unwrap_or_else(|e| {
                warn!("Failed to get nonce for {:?}: {}", address, e);
                0
            });

        // Get code hash
        let code_hash = self.state.get_code_hash(&norn_address)
            .unwrap_or_else(|e| {
                warn!("Failed to get code hash for {:?}: {}", address, e);
                B256::default()
            });

        // Get code if code_hash is not empty
        let code = if code_hash != KECCAK_EMPTY {
            // Try to load bytecode from CodeStorage
            // Use a separate thread with its own runtime to avoid nested runtime issues
            let code_storage_clone = Arc::clone(&self.code_storage);
            let norn_hash = norn_common::types::Hash(code_hash.0);

            match std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new()
                    .expect("Failed to create runtime");

                rt.block_on(async move {
                    code_storage_clone.get_code(&norn_hash).await
                })
            })
            .join()
            {
                Ok(Ok(Some(bytecode))) => {
                    debug!("Loaded bytecode: {} bytes", bytecode.len());
                    // Convert Vec<u8> to revm Bytes
                    use revm::primitives::Bytes;
                    Some(Bytecode::new_raw(Bytes::from(bytecode)))
                }
                Ok(Ok(None)) => {
                    debug!("No bytecode found for hash: {}", hex::encode(code_hash.as_slice()));
                    None
                }
                Ok(Err(_)) | Err(_) => {
                    warn!("Failed to load bytecode or thread error");
                    None
                }
            }
        } else {
            None
        };

        let account_info = AccountInfo {
            balance,
            nonce,
            code_hash,
            code,
        };

        debug!(
            "Account info for {:?}: balance={}, nonce={}, code_hash={}",
            address,
            balance,
            nonce,
            hex::encode(code_hash.as_slice())
        );

        Ok(Some(account_info))
    }

    /// Get account code by code hash
    ///
    /// Retrieves contract bytecode from CodeStorage.
    pub fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Infallible> {
        debug!("Getting code by hash: {}", hex::encode(code_hash.as_slice()));

        // Convert B256 to norn Hash
        let norn_hash = norn_common::types::Hash(code_hash.0);

        // Try to get code from storage
        // Use a separate thread with its own runtime to avoid nested runtime issues
        let code_storage_clone = Arc::clone(&self.code_storage);

        match std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()
                .expect("Failed to create runtime");

            rt.block_on(async move {
                code_storage_clone.get_code(&norn_hash).await
            })
        })
        .join()
        {
            Ok(Ok(Some(bytecode))) => {
                debug!("Found code: {} bytes", bytecode.len());
                use revm::primitives::Bytes;
                Ok(Bytecode::new_raw(Bytes::from(bytecode)))
            }
            Ok(Ok(None)) => {
                debug!("Code not found for hash: {}", hex::encode(code_hash.as_slice()));
                Ok(Bytecode::default())
            }
            Ok(Err(_)) | Err(_) => {
                warn!("Failed to get code");
                Ok(Bytecode::default())
            }
        }
    }

    /// Get storage value
    pub fn storage(
        &mut self,
        address: RevmAddress,
        index: U256,
    ) -> Result<U256, Infallible> {
        debug!("Getting storage for address: {:?}, index: {}", address, index);

        let addr_bytes: [u8; 20] = address.as_slice().try_into().unwrap_or([0u8; 20]);
        let norn_address = Address(addr_bytes);

        // Convert U256 index to Vec<u8> key
        let key: Vec<u8> = {
            let bytes = index.to_be_bytes_vec();
            // Trim leading zeros
            bytes.iter().skip_while(|&&b| b == 0).copied().collect()
        };

        // Get storage value
        let value = self.state.get_storage(&norn_address, &key)
            .unwrap_or_else(|e| {
                warn!("Failed to get storage for {:?}: {}", address, e);
                None
            });

        // Convert Vec<u8> to U256
        let u256_value = if let Some(v) = value {
            // Try to parse as U256
            if v.len() <= 32 {
                let mut array = [0u8; 32];
                array[(32 - v.len())..].copy_from_slice(&v);
                U256::from_be_bytes(array)
            } else {
                warn!("Storage value too long: {} bytes", v.len());
                U256::ZERO
            }
        } else {
            U256::ZERO
        };

        debug!("Storage value for {:?}[{}]: {}", address, index, u256_value);
        Ok(u256_value)
    }

    /// Get block hash by block number
    pub fn block_hash(&mut self, number: u64) -> Result<B256, Infallible> {
        debug!("Getting block hash for block number: {}", number);

        // Only allow recent block hashes (last 256 blocks per Ethereum spec)
        let current_block = self.block_number;
        if number >= current_block || current_block - number > 256 {
            warn!(
                "Block hash request out of range: requested={}, current={}",
                number, current_block
            );
            return Ok(B256::default());
        }

        // Look up in cache
        if let Some(hash) = self.block_hashes.get(&number) {
            debug!("Found block hash for {}: {}", number, hex::encode(hash));
            return Ok(*hash);
        }

        warn!("Block hash not found for block number: {}", number);
        Ok(B256::default())
    }
}

/// Error type for Norn database adapter
///
/// Currently uses Infallible as we don't have specific errors yet.
/// This may need to be extended with proper error types.
pub type NornDBError = Infallible;

impl Database for NornDatabaseAdapter {
    type Error = NornDBError;

    fn basic(
        &mut self,
        address: RevmAddress,
    ) -> Result<Option<AccountInfo>, Self::Error> {
        self.get_account_basic(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.code_by_hash(code_hash)
    }

    fn storage(
        &mut self,
        address: RevmAddress,
        index: U256,
    ) -> Result<U256, Self::Error> {
        self.storage(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.block_hash(number)
    }
}

impl DatabaseCommit for NornDatabaseAdapter {
    /// Commit state changes
    ///
    /// This is called by revm after transaction execution to persist state changes.
    fn commit(&mut self, _changes: revm::primitives::HashMap<RevmAddress, revm::primitives::Account>) {
        debug!("Committing state changes");

        // Flush the sync state manager to persist dirty state to async backend
        if let Err(e) = self.state.flush() {
            error!("Failed to flush state changes: {}", e);
        }

        debug!("State changes committed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::account::AccountStateManager;
    use crate::state::account::AccountStateConfig;
    use crate::state::cache::SyncCacheConfig;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_database_adapter_basic() {
        // Create async state manager
        let async_config = AccountStateConfig::default();
        let async_manager = Arc::new(AccountStateManager::new(async_config));

        // Create sync cache
        let sync_config = SyncCacheConfig::default();
        let sync_state = SyncStateManager::new(async_manager, sync_config);

        // Create database adapter
        let mut db = NornDatabaseAdapter::new(sync_state, 100);

        // Test getting account info
        let address = RevmAddress::from([1u8; 20]);
        let account_info = db.basic(address).unwrap();

        // Should return Some even for non-existent accounts (with zero values)
        assert!(account_info.is_some());
        let info = account_info.unwrap();
        assert_eq!(info.balance, U256::ZERO);
        assert_eq!(info.nonce, 0);
    }

    #[tokio::test]
    async fn test_database_adapter_with_balance() {
        let async_config = AccountStateConfig::default();
        let async_manager = Arc::new(AccountStateManager::new(async_config));
        let sync_config = SyncCacheConfig::default();
        let sync_state = SyncStateManager::new(async_manager, sync_config);

        let mut db = NornDatabaseAdapter::new(sync_state, 100);

        let norn_address = Address([1u8; 20]);
        let revm_address = RevmAddress::from(norn_address.0);

        // Set balance via sync state
        db.state.set_balance(&norn_address, "1000000000000000000".to_string()).unwrap();

        // Get account info
        let account_info = db.basic(revm_address).unwrap();
        assert!(account_info.is_some());

        let info = account_info.unwrap();
        assert_eq!(info.balance, U256::from(1_000_000_000_000_000_000u128));
    }

    #[tokio::test]
    async fn test_database_adapter_storage() {
        let async_config = AccountStateConfig::default();
        let async_manager = Arc::new(AccountStateManager::new(async_config));
        let sync_config = SyncCacheConfig::default();
        let sync_state = SyncStateManager::new(async_manager, sync_config);

        let mut db = NornDatabaseAdapter::new(sync_state, 100);

        let norn_address = Address([2u8; 20]);
        let revm_address = RevmAddress::from(norn_address.0);
        let key = U256::from(42);

        // Set storage via sync state
        let key_bytes: Vec<u8> = {
            let bytes = key.to_be_bytes_vec();
            bytes.iter().skip_while(|&&b| b == 0).copied().collect()
        };
        let value = vec![0xDE, 0xAD, 0xBE, 0xEF];
        db.state.set_storage(&norn_address, key_bytes, value.clone()).unwrap();

        // Get storage
        let storage_value = db.storage(revm_address, key).unwrap();
        assert_eq!(storage_value, U256::from(0xDEADBEEFu32));
    }

    #[tokio::test]
    async fn test_database_adapter_block_hash() {
        let async_config = AccountStateConfig::default();
        let async_manager = Arc::new(AccountStateManager::new(async_config));
        let sync_config = SyncCacheConfig::default();
        let sync_state = SyncStateManager::new(async_manager, sync_config);

        let mut db = NornDatabaseAdapter::new(sync_state, 100);

        // Insert block hash
        let block_number = 99;
        let block_hash = B256::from([0xAA; 32]);
        db.insert_block_hash(block_number, block_hash);

        // Get block hash
        let retrieved_hash = db.block_hash(block_number).unwrap();
        assert_eq!(retrieved_hash, block_hash);

        // Test out of range
        let out_of_range = db.block_hash(50).unwrap();
        assert_eq!(out_of_range, B256::default());

        let future_block = db.block_hash(200).unwrap();
        assert_eq!(future_block, B256::default());
    }
}
