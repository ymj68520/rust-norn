use norn_common::error::{NornError, Result};
use norn_common::types::{Address, Hash, PublicKey};
use num_bigint::BigUint;
use num_traits::{One, Zero};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// 账户状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountState {
    /// 账户地址
    pub address: Address,

    /// 账户余额
    pub balance: BigUint,

    /// Nonce（交易序号）
    pub nonce: u64,

    /// 代码哈希（合约账户）
    pub code_hash: Option<Hash>,

    /// 存储根哈希
    pub storage_root: Hash,

    /// 账户类型
    pub account_type: AccountType,

    /// 创建时间
    pub created_at: u64,

    /// 最后更新时间
    pub updated_at: u64,

    /// 是否被删除
    pub deleted: bool,
}

/// 账户类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountType {
    /// 普通账户
    Normal,

    /// 智能合约账户
    Contract,

    /// 验证者账户
    Validator,

    /// 系统账户
    System,
}

/// 存储项
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageItem {
    /// 存储键
    pub key: Vec<u8>,

    /// 存储值
    pub value: Vec<u8>,

    /// 创建时间
    pub created_at: u64,

    /// 最后更新时间
    pub updated_at: u64,
}

use dashmap::DashMap;

/// 账户状态管理器
#[derive(Clone)]
pub struct AccountStateManager {
    /// 账户状态存储
    pub accounts: DashMap<Address, AccountState>,

    /// 存储状态存储
    pub storage: DashMap<Address, HashMap<Vec<u8>, StorageItem>>,

    /// 状态根哈希
    state_root: Arc<RwLock<Hash>>,

    /// 配置
    config: AccountStateConfig,
}

/// 账户状态配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStateConfig {
    /// 是否启用缓存
    pub enable_cache: bool,

    /// 缓存大小
    pub cache_size: usize,

    /// 最大账户数量
    pub max_accounts: usize,

    /// 最大存储项数量
    pub max_storage_items: usize,

    /// 是否启用快照
    pub enable_snapshots: bool,

    /// 快照间隔
    pub snapshot_interval: u64,
}

impl Default for AccountStateConfig {
    fn default() -> Self {
        Self {
            enable_cache: true,
            cache_size: 10000,
            max_accounts: 1000000,
            max_storage_items: 10000000,
            enable_snapshots: true,
            snapshot_interval: 1000,
        }
    }
}

/// 状态变更
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateChange {
    /// 账户创建
    AccountCreated {
        address: Address,
        account: AccountState,
    },

    /// 账户更新
    AccountUpdated {
        address: Address,
        old_account: AccountState,
        new_account: AccountState,
    },

    /// 账户删除
    AccountDeleted {
        address: Address,
        old_account: AccountState,
    },

    /// 余额变更
    BalanceChanged {
        address: Address,
        old_balance: String,
        new_balance: String,
    },

    /// 存储设置
    StorageSet {
        address: Address,
        key: Vec<u8>,
        old_value: Option<Vec<u8>>,
        new_value: Vec<u8>,
    },

    /// 存储删除
    StorageDeleted {
        address: Address,
        key: Vec<u8>,
        old_value: Vec<u8>,
    },
}

/// 状态快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// 快照 ID
    pub id: u64,

    /// 快照时间
    pub timestamp: u64,

    /// 状态根哈希
    pub state_root: Hash,

    /// 账户状态
    pub accounts: HashMap<Address, AccountState>,

    /// 存储状态
    pub storage: HashMap<Address, HashMap<Vec<u8>, StorageItem>>,

    /// 变更历史
    pub changes: Vec<StateChange>,
}

impl AccountStateManager {
    /// 创建新的账户状态管理器
    pub fn new(config: AccountStateConfig) -> Self {
        Self {
            accounts: DashMap::new(),
            storage: DashMap::new(),
            state_root: Arc::new(RwLock::new(Hash::default())),
            config,
        }
    }

    /// 获取账户状态
    pub async fn get_account(&self, address: &Address) -> Result<Option<AccountState>> {
        debug!("Getting account state for address: {:?}", address);
        let account = self.accounts.get(address).map(|r| r.value().clone());
        debug!("Account state for {:?}: {:?}", address, account.is_some());
        Ok(account)
    }

    /// 设置账户状态
    pub async fn set_account(&self, address: &Address, account: AccountState) -> Result<()> {
        debug!("Setting account state for address: {:?}", address);

        let old_account = self.accounts.get(address).map(|r| r.value().clone());

        // 检查账户数量限制
        if old_account.is_none() && self.accounts.len() >= self.config.max_accounts {
            return Err(NornError::Internal(
                "Maximum account limit reached".to_string(),
            ));
        }

        let change = if old_account.is_none() {
            StateChange::AccountCreated {
                address: *address,
                account: account.clone(),
            }
        } else {
            StateChange::AccountUpdated {
                address: *address,
                old_account: old_account.unwrap(),
                new_account: account.clone(),
            }
        };

        self.accounts.insert(*address, account);

        // 记录变更
        self.record_change(change).await;

        debug!("Account state set for address: {:?}", address);
        Ok(())
    }

    /// 删除账户
    pub async fn delete_account(&self, address: &Address) -> Result<()> {
        debug!("Deleting account: {:?}", address);

        let old_account = self.accounts.remove(address).map(|(_, v)| v);

        if let Some(account) = old_account {
            let change = StateChange::AccountDeleted {
                address: *address,
                old_account: account,
            };

            // 记录变更
            self.record_change(change).await;

            // 删除相关存储
            self.storage.remove(address);

            debug!("Account deleted: {:?}", address);
        } else {
            warn!("Attempted to delete non-existent account: {:?}", address);
        }

        Ok(())
    }

    /// 获取存储值
    pub async fn get_storage(&self, address: &Address, key: &[u8]) -> Result<Option<Vec<u8>>> {
        debug!("Getting storage for address: {:?}, key: {:?}", address, key);
        let value = self
            .storage
            .get(address)
            .and_then(|account_storage| account_storage.get(key).map(|item| item.value.clone()));
        debug!(
            "Storage value for {:?}/{:?}: {:?}",
            address,
            key,
            value.is_some()
        );
        Ok(value)
    }

    /// 设置存储值
    pub async fn set_storage(&self, address: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        debug!("Setting storage for address: {:?}, key: {:?}", address, key);

        let mut account_storage = self.storage.entry(*address).or_default();

        // 检查存储项数量限制
        if !account_storage.contains_key(&key)
            && account_storage.len() >= self.config.max_storage_items
        {
            return Err(NornError::Internal(
                "Maximum storage limit reached".to_string(),
            ));
        }

        let old_value = account_storage.get(&key).map(|item| item.value.clone());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| NornError::Internal(format!("Time error: {}", e)))?
            .as_secs();

        let change = StateChange::StorageSet {
            address: *address,
            key: key.clone(),
            old_value,
            new_value: value.clone(),
        };

        let storage_item = StorageItem {
            key: key.clone(),
            value,
            created_at: now,
            updated_at: now,
        };

        account_storage.insert(key, storage_item);
        drop(account_storage);

        // 记录变更
        self.record_change(change).await;

        debug!("Storage set for address: {:?}", address);
        Ok(())
    }

    /// 删除存储值
    pub async fn delete_storage(&self, address: &Address, key: &[u8]) -> Result<()> {
        debug!(
            "Deleting storage for address: {:?}, key: {:?}",
            address, key
        );

        if let Some(mut account_storage) = self.storage.get_mut(address) {
            if let Some(item) = account_storage.remove(key) {
                let change = StateChange::StorageDeleted {
                    address: *address,
                    key: key.to_vec(),
                    old_value: item.value,
                };
                drop(account_storage);

                // 记录变更
                self.record_change(change).await;

                debug!("Storage deleted for address: {:?}, key: {:?}", address, key);
            } else {
                warn!(
                    "Attempted to delete non-existent storage: {:?}/{:?}",
                    address, key
                );
            }
        }

        Ok(())
    }

    /// 更新账户余额
    pub async fn update_balance(&self, address: &Address, new_balance: BigUint) -> Result<()> {
        debug!(
            "Updating balance for address: {:?}, new balance: {}",
            address, new_balance
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut entry = self
            .accounts
            .entry(*address)
            .or_insert_with(|| AccountState {
                address: *address,
                balance: BigUint::zero(),
                nonce: 0,
                code_hash: None,
                storage_root: Hash::default(),
                account_type: AccountType::Normal,
                created_at: now,
                updated_at: now,
                deleted: false,
            });

        let old_balance = entry.balance.clone();
        entry.balance = new_balance.clone();
        entry.updated_at = now;

        let old_account = entry.value().clone();
        drop(entry);

        let change = StateChange::AccountUpdated {
            address: *address,
            old_account: AccountState {
                balance: old_balance,
                ..old_account.clone()
            },
            new_account: old_account,
        };

        self.record_change(change).await;

        debug!("Balance updated for address: {:?}", address);
        Ok(())
    }

    /// 增加账户余额
    pub async fn add_balance(&self, address: &Address, amount: &BigUint) -> Result<()> {
        let account = self.get_account(address).await?;
        let current_balance = account
            .map(|a| a.balance)
            .unwrap_or_else(|| BigUint::zero());
        let new_balance = current_balance + amount;
        self.update_balance(address, new_balance).await
    }

    /// 减少账户余额
    pub async fn subtract_balance(&self, address: &Address, amount: &BigUint) -> Result<()> {
        let account = self.get_account(address).await?;
        let current_balance = account
            .map(|a| a.balance)
            .unwrap_or_else(|| BigUint::zero());

        if current_balance < *amount {
            return Err(NornError::Internal("Insufficient balance".to_string()));
        }

        let new_balance = current_balance - amount;
        self.update_balance(address, new_balance).await
    }

    /// 增加 Nonce
    pub async fn increment_nonce(&self, address: &Address) -> Result<u64> {
        debug!("Incrementing nonce for address: {:?}", address);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut entry = self
            .accounts
            .entry(*address)
            .or_insert_with(|| AccountState {
                address: *address,
                balance: BigUint::zero(),
                nonce: 0,
                code_hash: None,
                storage_root: Hash::default(),
                account_type: AccountType::Normal,
                created_at: now,
                updated_at: now,
                deleted: false,
            });

        entry.nonce += 1;
        entry.updated_at = now;
        let new_nonce = entry.nonce;
        drop(entry);

        debug!(
            "Nonce incremented for address: {:?}, new nonce: {}",
            address, new_nonce
        );
        Ok(new_nonce)
    }

    /// 获取账户 Nonce
    pub async fn get_nonce(&self, address: &Address) -> Result<u64> {
        let account = self.get_account(address).await?;
        Ok(account.map(|a| a.nonce).unwrap_or(0))
    }

    /// 验证账户余额
    pub async fn validate_balance(
        &self,
        address: &Address,
        required_balance: &BigUint,
    ) -> Result<bool> {
        let account = self.get_account(address).await?;
        let current_balance = account
            .map(|a| a.balance)
            .unwrap_or_else(|| BigUint::zero());
        Ok(current_balance >= *required_balance)
    }

    /// 验证账户 Nonce
    pub async fn validate_nonce(&self, address: &Address, expected_nonce: u64) -> Result<bool> {
        let current_nonce = self.get_nonce(address).await?;
        Ok(current_nonce == expected_nonce)
    }

    /// 获取状态根哈希
    pub async fn get_state_root(&self) -> Result<Hash> {
        let state_root = self.state_root.read().await;
        Ok(*state_root)
    }

    /// 计算状态根哈希
    pub async fn compute_state_root(&self) -> Result<Hash> {
        debug!("Computing state root hash");

        // 创建包含所有账户数据的映射
        let mut state_data = HashMap::new();

        for entry in self.accounts.iter() {
            let address = entry.key();
            let account = entry.value();
            if !account.deleted {
                // 序列化账户状态
                let account_data = serde_json::to_vec(account)?;
                state_data.insert(address.0.to_vec(), account_data);

                // 添加存储数据
                if let Some(account_storage) = self.storage.get(address) {
                    for (key, item) in account_storage.iter() {
                        let storage_key = [&address.0[..], key].concat();
                        state_data.insert(storage_key, item.value.clone());
                    }
                }
            }
        }

        // 计算哈希
        let mut hasher = sha2::Sha256::new();
        let mut sorted_keys: Vec<_> = state_data.keys().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            hasher.update(key);
            hasher.update(&state_data[key]);
        }

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);

        debug!("State root hash computed: {:?}", Hash(result));
        Ok(Hash(result))
    }

    /// 更新状态根哈希
    pub async fn update_state_root(&self) -> Result<()> {
        let new_root = self.compute_state_root().await?;
        let mut state_root = self.state_root.write().await;
        *state_root = new_root;
        Ok(())
    }

    /// 创建状态快照
    pub async fn create_snapshot(&self, snapshot_id: u64) -> Result<StateSnapshot> {
        debug!("Creating state snapshot: {}", snapshot_id);

        let accounts_map: HashMap<Address, AccountState> = self
            .accounts
            .iter()
            .map(|r| (*r.key(), r.value().clone()))
            .collect();
        let storage_map: HashMap<Address, HashMap<Vec<u8>, StorageItem>> = self
            .storage
            .iter()
            .map(|r| (*r.key(), r.value().clone()))
            .collect();
        let state_root = self.state_root.read().await;

        let snapshot = StateSnapshot {
            id: snapshot_id,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            state_root: *state_root,
            accounts: accounts_map,
            storage: storage_map,
            changes: Vec::new(),
        };

        debug!("State snapshot created: {}", snapshot_id);
        Ok(snapshot)
    }

    /// 恢复状态快照
    pub async fn restore_snapshot(&self, snapshot: &StateSnapshot) -> Result<()> {
        debug!("Restoring state snapshot: {}", snapshot.id);

        self.accounts.clear();
        for (k, v) in &snapshot.accounts {
            self.accounts.insert(*k, v.clone());
        }

        self.storage.clear();
        for (k, v) in &snapshot.storage {
            self.storage.insert(*k, v.clone());
        }

        {
            let mut state_root = self.state_root.write().await;
            *state_root = snapshot.state_root;
        }

        debug!("State snapshot restored: {}", snapshot.id);
        Ok(())
    }

    /// Restore the canonical V2 account/storage image after its durable
    /// checkpoint has been verified. This is intentionally separate from the
    /// historical snapshot API so consensus recovery cannot accidentally use
    /// an older legacy snapshot format.
    pub async fn restore_canonical_state(
        &self,
        accounts: &[AccountState],
        storage: &[(Address, Vec<StorageItem>)],
        state_root: Hash,
    ) -> Result<()> {
        self.accounts.clear();
        for account in accounts {
            self.accounts.insert(account.address, account.clone());
        }
        self.storage.clear();
        for (address, items) in storage {
            let values = items
                .iter()
                .cloned()
                .map(|item| (item.key.clone(), item))
                .collect::<HashMap<_, _>>();
            self.storage.insert(*address, values);
        }
        *self.state_root.write().await = state_root;
        Ok(())
    }

    /// 清理已删除的账户
    pub async fn cleanup_deleted_accounts(&self) -> Result<usize> {
        debug!("Cleaning up deleted accounts");

        let addresses_to_remove: Vec<Address> = self
            .accounts
            .iter()
            .filter(|r| r.value().deleted)
            .map(|r| *r.key())
            .collect();

        let mut deleted_count = 0;
        for address in addresses_to_remove {
            self.accounts.remove(&address);
            self.storage.remove(&address);
            deleted_count += 1;
        }

        debug!("Cleaned up {} deleted accounts", deleted_count);
        Ok(deleted_count)
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> AccountStateStats {
        let mut stats = AccountStateStats::default();

        for entry in self.accounts.iter() {
            let account = entry.value();
            stats.total_accounts += 1;

            if account.deleted {
                stats.deleted_accounts += 1;
            } else {
                match account.account_type {
                    AccountType::Normal => stats.normal_accounts += 1,
                    AccountType::Contract => stats.contract_accounts += 1,
                    AccountType::Validator => stats.validator_accounts += 1,
                    AccountType::System => stats.system_accounts += 1,
                }

                stats.total_balance += &account.balance;
            }
        }

        for entry in self.storage.iter() {
            stats.total_storage_items += entry.value().len() as u64;
        }

        stats
    }

    /// 记录状态变更
    async fn record_change(&self, change: StateChange) {
        debug!("Recording state change: {:?}", change);

        // 发送到tracing系统作为日志
        match &change {
            StateChange::AccountCreated { address, .. } => {
                info!("Account created: {:?}", hex::encode(address));
            }
            StateChange::AccountUpdated {
                address,
                old_account,
                new_account,
            } => {
                info!(
                    "Account updated: {:?}, old_nonce={}, new_nonce={}",
                    hex::encode(address),
                    old_account.nonce,
                    new_account.nonce
                );
            }
            StateChange::AccountDeleted {
                address,
                old_account,
            } => {
                info!("Account deleted: {:?}", hex::encode(address));
                let _ = old_account;
            }
            StateChange::BalanceChanged {
                address,
                old_balance,
                new_balance,
            } => {
                info!(
                    "Balance changed: {:?}, {} -> {}",
                    hex::encode(address),
                    old_balance,
                    new_balance
                );
            }
            StateChange::StorageSet { address, key, .. } => {
                info!(
                    "Storage set: {:?}, key={}",
                    hex::encode(address),
                    hex::encode(key)
                );
            }
            StateChange::StorageDeleted { address, key, .. } => {
                info!(
                    "Storage deleted: {:?}, key={}",
                    hex::encode(address),
                    hex::encode(key)
                );
            }
        }
    }

    // ========== Additional methods for compatibility ==========

    /// Get balance as BigUint (convenience method)
    pub async fn get_balance(&self, address: &Address) -> Result<BigUint> {
        let account = self.get_account(address).await?;
        Ok(account
            .map(|a| a.balance)
            .unwrap_or_else(|| BigUint::from(0u32)))
    }

    /// Get accounts map reference
    pub fn accounts_map(&self) -> DashMap<Address, AccountState> {
        self.accounts.clone()
    }

    /// Get storage map reference
    pub fn storage_map(&self) -> DashMap<Address, HashMap<Vec<u8>, StorageItem>> {
        self.storage.clone()
    }
}

impl Default for AccountStateManager {
    fn default() -> Self {
        Self::new(AccountStateConfig::default())
    }
}

/// 账户状态统计信息
#[derive(Debug, Clone, Default)]
pub struct AccountStateStats {
    pub total_accounts: u64,
    pub normal_accounts: u64,
    pub contract_accounts: u64,
    pub validator_accounts: u64,
    pub system_accounts: u64,
    pub deleted_accounts: u64,
    pub total_balance: BigUint,
    pub total_storage_items: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigUint;

    #[tokio::test]
    async fn test_account_creation() {
        let config = AccountStateConfig::default();
        let manager = AccountStateManager::new(config);

        let address = Address::default();
        let account = AccountState {
            address,
            balance: BigUint::from(1000u64),
            nonce: 0,
            code_hash: None,
            storage_root: Hash::default(),
            account_type: AccountType::Normal,
            created_at: 1234567890,
            updated_at: 1234567890,
            deleted: false,
        };

        manager
            .set_account(&address, account.clone())
            .await
            .unwrap();
        let retrieved = manager.get_account(&address).await.unwrap();

        assert_eq!(retrieved, Some(account));
    }

    #[tokio::test]
    async fn test_balance_operations() {
        let config = AccountStateConfig::default();
        let manager = AccountStateManager::new(config);

        let address = Address::default();

        // 初始余额为 0
        assert!(!manager
            .validate_balance(&address, &BigUint::from(100u64))
            .await
            .unwrap());

        // 增加余额
        manager
            .add_balance(&address, &BigUint::from(1000u64))
            .await
            .unwrap();
        assert!(manager
            .validate_balance(&address, &BigUint::from(100u64))
            .await
            .unwrap());

        // 减少余额
        manager
            .subtract_balance(&address, &BigUint::from(500u64))
            .await
            .unwrap();
        assert!(manager
            .validate_balance(&address, &BigUint::from(500u64))
            .await
            .unwrap());
        assert!(!manager
            .validate_balance(&address, &BigUint::from(600u64))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_nonce_operations() {
        let config = AccountStateConfig::default();
        let manager = AccountStateManager::new(config);

        let address = Address::default();

        // 初始 nonce 为 0
        assert_eq!(manager.get_nonce(&address).await.unwrap(), 0);
        assert!(manager.validate_nonce(&address, 0).await.unwrap());
        assert!(!manager.validate_nonce(&address, 1).await.unwrap());

        // 增加 nonce
        let new_nonce = manager.increment_nonce(&address).await.unwrap();
        assert_eq!(new_nonce, 1);
        assert_eq!(manager.get_nonce(&address).await.unwrap(), 1);
        assert!(manager.validate_nonce(&address, 1).await.unwrap());
        assert!(!manager.validate_nonce(&address, 0).await.unwrap());
    }

    #[tokio::test]
    async fn test_storage_operations() {
        let config = AccountStateConfig::default();
        let manager = AccountStateManager::new(config);

        let address = Address::default();
        let key = b"test_key";
        let value = b"test_value";

        // 设置存储
        manager
            .set_storage(&address, key.to_vec(), value.to_vec())
            .await
            .unwrap();
        let retrieved = manager.get_storage(&address, key).await.unwrap();
        assert_eq!(retrieved, Some(value.to_vec()));

        // 删除存储
        manager.delete_storage(&address, key).await.unwrap();
        let deleted = manager.get_storage(&address, key).await.unwrap();
        assert_eq!(deleted, None);
    }

    #[tokio::test]
    async fn test_state_root() {
        let config = AccountStateConfig::default();
        let manager = AccountStateManager::new(config);

        let address = Address::default();
        let account = AccountState {
            address,
            balance: BigUint::from(1000u64),
            nonce: 0,
            code_hash: None,
            storage_root: Hash::default(),
            account_type: AccountType::Normal,
            created_at: 1234567890,
            updated_at: 1234567890,
            deleted: false,
        };

        manager.set_account(&address, account).await.unwrap();
        let state_root = manager.compute_state_root().await.unwrap();

        // 状态根哈希应该不为默认值
        assert_ne!(state_root, Hash::default());
    }

    #[tokio::test]
    async fn test_snapshot() {
        let config = AccountStateConfig::default();
        let manager = AccountStateManager::new(config);

        let address = Address::default();
        let account = AccountState {
            address,
            balance: BigUint::from(1000u64),
            nonce: 0,
            code_hash: None,
            storage_root: Hash::default(),
            account_type: AccountType::Normal,
            created_at: 1234567890,
            updated_at: 1234567890,
            deleted: false,
        };

        manager
            .set_account(&address, account.clone())
            .await
            .unwrap();

        // 创建快照
        let snapshot = manager.create_snapshot(1).await.unwrap();
        assert_eq!(snapshot.id, 1);
        assert_eq!(snapshot.accounts.get(&address), Some(&account));

        // 删除账户
        manager.delete_account(&address).await.unwrap();
        assert!(manager.get_account(&address).await.unwrap().is_none());

        // 恢复快照
        manager.restore_snapshot(&snapshot).await.unwrap();
        let restored = manager.get_account(&address).await.unwrap();
        assert_eq!(restored, Some(account));
    }

    #[tokio::test]
    async fn test_account_stats() {
        let config = AccountStateConfig::default();
        let manager = AccountStateManager::new(config);

        // 创建不同类型的账户
        let normal_account = AccountState {
            address: Address::default(),
            balance: BigUint::from(1000u64),
            nonce: 0,
            code_hash: None,
            storage_root: Hash::default(),
            account_type: AccountType::Normal,
            created_at: 1234567890,
            updated_at: 1234567890,
            deleted: false,
        };

        let contract_account = AccountState {
            address: Address([1u8; 20]),
            balance: BigUint::from(2000u64),
            nonce: 0,
            code_hash: Some(Hash([2u8; 32])),
            storage_root: Hash::default(),
            account_type: AccountType::Contract,
            created_at: 1234567890,
            updated_at: 1234567890,
            deleted: false,
        };

        let normal_addr = normal_account.address;
        manager
            .set_account(&normal_addr, normal_account)
            .await
            .unwrap();
        let contract_addr = contract_account.address;
        manager
            .set_account(&contract_addr, contract_account)
            .await
            .unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.total_accounts, 2);
        assert_eq!(stats.normal_accounts, 1);
        assert_eq!(stats.contract_accounts, 1);
        assert_eq!(stats.total_balance, BigUint::from(3000u64));
    }
}
