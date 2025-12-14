// Nonce 管理器
// 
// 负责管理账户的交易序号，防止重放攻击

use crate::types::{Address, Transaction, H256};
use crate::state::{StateDB, AccountStateManager, AccountState};
use anyhow::{Result, anyhow};
use std::collections::{HashMap, BTreeMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};

/// Nonce 管理器
pub struct NonceManager {
    state_db: Arc<RwLock<StateDB>>,
    account_manager: Arc<RwLock<AccountStateManager>>,
    // 内存中的 Nonce 缓存
    nonce_cache: Arc<RwLock<HashMap<Address, u64>>>,
    // 待处理的 Nonce 映射（用于交易池）
    pending_nonces: Arc<RwLock<HashMap<Address, BTreeMap<u64, H256>>>>,
    // 配置
    config: NonceConfig,
}

/// Nonce 管理器配置
#[derive(Debug, Clone)]
pub struct NonceConfig {
    /// 最大缓存大小
    pub max_cache_size: usize,
    /// 缓存过期时间（秒）
    pub cache_ttl: u64,
    /// 每个账户最大待处理交易数
    pub max_pending_per_account: usize,
}

impl Default for NonceConfig {
    fn default() -> Self {
        Self {
            max_cache_size: 10000,
            cache_ttl: 300, // 5 分钟
            max_pending_per_account: 16,
        }
    }
}

/// Nonce 状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonceStatus {
    /// 已执行
    Executed,
    /// 在交易池中待处理
    Pending,
    /// 未来（跳跃的 nonce）
    Future,
    /// 已过期（低于当前 nonce）
    Stale,
}

/// Nonce 信息
#[derive(Debug, Clone)]
pub struct NonceInfo {
    /// Nonce 值
    pub nonce: u64,
    /// 状态
    pub status: NonceStatus,
    /// 交易哈希（如果待处理）
    pub tx_hash: Option<H256>,
    /// 时间戳
    pub timestamp: u64,
}

impl NonceManager {
    pub fn new(
        state_db: Arc<RwLock<StateDB>>,
        account_manager: Arc<RwLock<AccountStateManager>>,
        config: NonceConfig,
    ) -> Self {
        Self {
            state_db,
            account_manager,
            nonce_cache: Arc::new(RwLock::new(HashMap::new())),
            pending_nonces: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// 获取账户的当前 nonce
    pub async fn get_account_nonce(&self, address: &Address) -> Result<u64> {
        // 首先检查缓存
        {
            let cache = self.nonce_cache.read().await;
            if let Some(&nonce) = cache.get(address) {
                debug!("从缓存获取 nonce: {} -> {}", address, nonce);
                return Ok(nonce);
            }
        }

        // 从状态数据库获取
        let account_state = self
            .account_manager
            .read()
            .await
            .get_account_state(address)
            .await?;

        let nonce = account_state.map_or(0, |state| state.nonce);

        // 更新缓存
        {
            let mut cache = self.nonce_cache.write().await;
            if cache.len() >= self.config.max_cache_size {
                // 清理最旧的条目
                self.cleanup_cache(&mut cache).await;
            }
            cache.insert(*address, nonce);
        }

        debug!("获取账户 nonce: {} -> {}", address, nonce);
        Ok(nonce)
    }

    /// 获取账户的下一个 nonce
    pub async fn get_next_nonce(&self, address: &Address) -> Result<u64> {
        let current_nonce = self.get_account_nonce(address).await?;
        let pending_count = self.get_pending_nonce_count(address).await?;
        Ok(current_nonce + pending_count)
    }

    /// 获取待处理的 nonce 数量
    pub async fn get_pending_nonce_count(&self, address: &Address) -> u64 {
        let pending = self.pending_nonces.read().await;
        pending
            .get(address)
            .map_or(0, |nonces| nonces.len() as u64)
    }

    /// 验证交易 nonce
    pub async fn validate_transaction_nonce(&self, tx: &Transaction) -> Result<NonceInfo> {
        let current_nonce = self.get_account_nonce(&tx.from).await?;
        let pending_count = self.get_pending_nonce_count(&tx.from).await();
        let next_expected = current_nonce + pending_count;

        let status = if tx.nonce < current_nonce {
            NonceStatus::Stale
        } else if tx.nonce == current_nonce {
            NonceStatus::Executed
        } else if tx.nonce <= next_expected {
            NonceStatus::Pending
        } else {
            NonceStatus::Future
        };

        let nonce_info = NonceInfo {
            nonce: tx.nonce,
            status,
            tx_hash: Some(crate::crypto::hash::hash_transaction(tx)),
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        debug!("验证交易 nonce: {} -> {:?}", tx.from, nonce_info);
        Ok(nonce_info)
    }

    /// 添加待处理的 nonce
    pub async fn add_pending_nonce(&self, tx: &Transaction) -> Result<()> {
        let address = tx.from;
        let nonce = tx.nonce;
        let tx_hash = crate::crypto::hash::hash_transaction(tx);

        // 验证 nonce
        let nonce_info = self.validate_transaction_nonce(tx).await?;
        match nonce_info.status {
            NonceStatus::Stale => {
                return Err(anyhow!("Nonce {} 已过期", nonce));
            }
            NonceStatus::Executed => {
                // 可以立即执行，不需要添加到待处理
                return Ok(());
            }
            NonceStatus::Pending | NonceStatus::Future => {
                // 可以添加到待处理
            }
        }

        // 检查待处理数量限制
        {
            let pending = self.pending_nonces.read().await;
            if let Some(nonces) = pending.get(&address) {
                if nonces.len() >= self.config.max_pending_per_account {
                    return Err(anyhow!(
                        "账户 {} 待处理交易数量超过限制 {}",
                        address,
                        self.config.max_pending_per_account
                    ));
                }
            }
        }

        // 添加到待处理
        {
            let mut pending = self.pending_nonces.write().await;
            let nonces = pending.entry(address).or_insert_with(BTreeMap::new);
            
            if nonces.contains_key(&nonce) {
                return Err(anyhow!("Nonce {} 已存在", nonce));
            }
            
            nonces.insert(nonce, tx_hash);
        }

        info!("添加待处理 nonce: {} -> {}", address, nonce);
        Ok(())
    }

    /// 移除待处理的 nonce（交易执行后调用）
    pub async fn remove_pending_nonce(&self, address: &Address, nonce: u64) -> Result<()> {
        let mut pending = self.pending_nonces.write().await;
        
        if let Some(nonces) = pending.get_mut(address) {
            nonces.remove(&nonce);
            
            // 如果没有待处理的 nonce 了，移除整个条目
            if nonces.is_empty() {
                pending.remove(address);
            }
        }

        // 更新账户的实际 nonce
        self.update_account_nonce(address, nonce + 1).await?;

        info!("移除待处理 nonce: {} -> {}", address, nonce);
        Ok(())
    }

    /// 更新账户 nonce
    pub async fn update_account_nonce(&self, address: &Address, new_nonce: u64) -> Result<()> {
        // 更新状态数据库
        {
            let mut account_manager = self.account_manager.write().await;
            let mut state = account_manager
                .get_account_state(address)
                .await?
                .unwrap_or_default();
            
            if new_nonce > state.nonce {
                state.nonce = new_nonce;
                account_manager.update_account_state(*address, state).await?;
            }
        }

        // 更新缓存
        {
            let mut cache = self.nonce_cache.write().await;
            cache.insert(*address, new_nonce);
        }

        debug!("更新账户 nonce: {} -> {}", address, new_nonce);
        Ok(())
    }

    /// 获取账户的待处理交易列表
    pub async fn get_pending_transactions(&self, address: &Address) -> Result<Vec<H256>> {
        let pending = self.pending_nonces.read().await;
        Ok(pending
            .get(address)
            .map_or(Vec::new(), |nonces| nonces.values().copied().collect()))
    }

    /// 获取下一个可执行的 nonce
    pub async fn get_next_executable_nonce(&self, address: &Address) -> Result<Option<u64>> {
        let current_nonce = self.get_account_nonce(address).await?;
        let pending = self.pending_nonces.read().await;
        
        if let Some(nonces) = pending.get(address) {
            // 查找最小的 nonce
            for (&nonce, _) in nonces {
                if nonce >= current_nonce {
                    return Ok(Some(nonce));
                }
            }
        }
        
        Ok(None)
    }

    /// 清理过期的缓存条目
    async fn cleanup_cache(&self, cache: &mut HashMap<Address, u64>) {
        let current_time = chrono::Utc::now().timestamp() as u64;
        let mut to_remove = Vec::new();

        // 这里简化处理，实际应该记录每个条目的时间戳
        // 暂时移除一半的条目
        if cache.len() > self.config.max_cache_size / 2 {
            let keys_to_remove: Vec<_> = cache.keys().take(cache.len() / 2).copied().collect();
            for key in keys_to_remove {
                to_remove.push(key);
            }
        }

        for key in to_remove {
            cache.remove(&key);
        }

        debug!("清理了 {} 个缓存条目", to_remove.len());
    }

    /// 重置账户的 nonce 状态（用于链重组）
    pub async fn reset_account_nonce(&self, address: &Address, new_nonce: u64) -> Result<()> {
        // 清理待处理的 nonce
        {
            let mut pending = self.pending_nonces.write().await;
            pending.remove(address);
        }

        // 更新账户 nonce
        self.update_account_nonce(address, new_nonce).await?;

        info!("重置账户 nonce: {} -> {}", address, new_nonce);
        Ok(())
    }

    /// 获取 Nonce 统计信息
    pub async fn get_nonce_stats(&self) -> Result<NonceStats> {
        let cache_size = self.nonce_cache.read().await.len();
        let pending_count = self.pending_nonces.read().await.len();
        let total_pending_txs = self
            .pending_nonces
            .read()
            .await
            .values()
            .map(|nonces| nonces.len())
            .sum();

        Ok(NonceStats {
            cached_accounts: cache_size,
            pending_accounts: pending_count,
            total_pending_transactions: total_pending_txs,
        })
    }

    /// 批量验证交易 nonce
    pub async fn validate_transaction_nonces(&self, transactions: &[Transaction]) -> Result<Vec<NonceInfo>> {
        let mut results = Vec::new();
        
        for tx in transactions {
            let nonce_info = self.validate_transaction_nonce(tx).await?;
            results.push(nonce_info);
        }
        
        Ok(results)
    }

    /// 检查是否有重复的交易
    pub async fn check_duplicate_transaction(&self, tx: &Transaction) -> Result<bool> {
        let tx_hash = crate::crypto::hash::hash_transaction(tx);
        
        // 检查待处理的交易
        let pending = self.pending_nonces.read().await;
        if let Some(nonces) = pending.get(&tx.from) {
            if nonces.values().any(|&hash| hash == tx_hash) {
                return Ok(true);
            }
        }
        
        // 这里还可以检查已执行的交易历史
        // 暂时只检查待处理的
        
        Ok(false)
    }
}

/// Nonce 统计信息
#[derive(Debug, Clone)]
pub struct NonceStats {
    /// 缓存的账户数量
    pub cached_accounts: usize,
    /// 有待处理交易的账户数量
    pub pending_accounts: usize,
    /// 总待处理交易数量
    pub total_pending_transactions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Transaction;
    use std::str::FromStr;

    async fn create_test_nonce_manager() -> NonceManager {
        let state_db = Arc::new(RwLock::new(StateDB::new(":memory:").await.unwrap()));
        let account_manager = Arc::new(RwLock::new(AccountStateManager::new(state_db.clone())));
        let config = NonceConfig::default();
        
        NonceManager::new(state_db, account_manager, config)
    }

    #[tokio::test]
    async fn test_get_account_nonce() {
        let manager = create_test_nonce_manager().await;
        let address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        // 初始 nonce 应该是 0
        let nonce = manager.get_account_nonce(&address).await.unwrap();
        assert_eq!(nonce, 0);

        // 更新 nonce
        manager.update_account_nonce(&address, 5).await.unwrap();
        let nonce = manager.get_account_nonce(&address).await.unwrap();
        assert_eq!(nonce, 5);
    }

    #[tokio::test]
    async fn test_validate_transaction_nonce() {
        let manager = create_test_nonce_manager().await;
        let address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        // 设置当前 nonce 为 5
        manager.update_account_nonce(&address, 5).await.unwrap();

        // 测试不同的 nonce
        let test_cases = vec![
            (3, NonceStatus::Stale),    // 过期
            (5, NonceStatus::Executed), // 当前
            (6, NonceStatus::Pending),  // 下一个
            (7, NonceStatus::Future),   // 未来
        ];

        for (nonce, expected_status) in test_cases {
            let tx = Transaction {
                from: address,
                to: Some(Address::default()),
                value: crate::types::Wei::zero(),
                gas_limit: 21000,
                gas_price: crate::types::Wei::from(1_000_000_000u64),
                nonce,
                data: vec![],
                signature: vec![],
            };

            let nonce_info = manager.validate_transaction_nonce(&tx).await.unwrap();
            assert_eq!(nonce_info.status, expected_status);
            assert_eq!(nonce_info.nonce, nonce);
        }
    }

    #[tokio::test]
    async fn test_add_pending_nonce() {
        let manager = create_test_nonce_manager().await;
        let address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        // 添加待处理的 nonce
        let tx = Transaction {
            from: address,
            to: Some(Address::default()),
            value: crate::types::Wei::zero(),
            gas_limit: 21000,
            gas_price: crate::types::Wei::from(1_000_000_000u64),
            nonce: 5, // 未来的 nonce
            data: vec![],
            signature: vec![],
        };

        assert!(manager.add_pending_nonce(&tx).await.is_ok());

        // 检查待处理数量
        let count = manager.get_pending_nonce_count(&address).await;
        assert_eq!(count, 1);

        // 获取待处理交易
        let pending_txs = manager.get_pending_transactions(&address).await.unwrap();
        assert_eq!(pending_txs.len(), 1);
    }

    #[tokio::test]
    async fn test_remove_pending_nonce() {
        let manager = create_test_nonce_manager().await;
        let address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        // 添加待处理的 nonce
        let tx = Transaction {
            from: address,
            to: Some(Address::default()),
            value: crate::types::Wei::zero(),
            gas_limit: 21000,
            gas_price: crate::types::Wei::from(1_000_000_000u64),
            nonce: 5,
            data: vec![],
            signature: vec![],
        };

        manager.add_pending_nonce(&tx).await.unwrap();
        assert_eq!(manager.get_pending_nonce_count(&address).await, 1);

        // 移除待处理的 nonce
        manager.remove_pending_nonce(&address, 5).await.unwrap();
        assert_eq!(manager.get_pending_nonce_count(&address).await, 0);

        // 检查账户 nonce 是否更新
        let nonce = manager.get_account_nonce(&address).await.unwrap();
        assert_eq!(nonce, 6); // 5 + 1
    }

    #[tokio::test]
    async fn test_get_next_nonce() {
        let manager = create_test_nonce_manager().await;
        let address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        // 初始下一个 nonce 应该是 0
        let next_nonce = manager.get_next_nonce(&address).await.unwrap();
        assert_eq!(next_nonce, 0);

        // 添加待处理的 nonce
        let tx = Transaction {
            from: address,
            to: Some(Address::default()),
            value: crate::types::Wei::zero(),
            gas_limit: 21000,
            gas_price: crate::types::Wei::from(1_000_000_000u64),
            nonce: 0,
            data: vec![],
            signature: vec![],
        };

        manager.add_pending_nonce(&tx).await.unwrap();

        // 下一个 nonce 应该是 1
        let next_nonce = manager.get_next_nonce(&address).await.unwrap();
        assert_eq!(next_nonce, 1);
    }

    #[tokio::test]
    async fn test_nonce_stats() {
        let manager = create_test_nonce_manager().await;
        let address1 = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let address2 = Address::from_str("0x9876543210987654321098765432109876543210").unwrap();

        // 获取初始统计
        let stats = manager.get_nonce_stats().await.unwrap();
        assert_eq!(stats.cached_accounts, 0);
        assert_eq!(stats.pending_accounts, 0);
        assert_eq!(stats.total_pending_transactions, 0);

        // 添加一些 nonce
        manager.update_account_nonce(&address1, 5).await.unwrap();
        manager.update_account_nonce(&address2, 3).await.unwrap();

        let tx1 = Transaction {
            from: address1,
            to: Some(Address::default()),
            value: crate::types::Wei::zero(),
            gas_limit: 21000,
            gas_price: crate::types::Wei::from(1_000_000_000u64),
            nonce: 5,
            data: vec![],
            signature: vec![],
        };

        let tx2 = Transaction {
            from: address2,
            to: Some(Address::default()),
            value: crate::types::Wei::zero(),
            gas_limit: 21000,
            gas_price: crate::types::Wei::from(1_000_000_000u64),
            nonce: 3,
            data: vec![],
            signature: vec![],
        };

        manager.add_pending_nonce(&tx1).await.unwrap();
        manager.add_pending_nonce(&tx2).await.unwrap();

        // 检查统计
        let stats = manager.get_nonce_stats().await.unwrap();
        assert_eq!(stats.cached_accounts, 2);
        assert_eq!(stats.pending_accounts, 2);
        assert_eq!(stats.total_pending_transactions, 2);
    }

    #[tokio::test]
    async fn test_reset_account_nonce() {
        let manager = create_test_nonce_manager().await;
        let address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        // 设置初始状态
        manager.update_account_nonce(&address, 10).await.unwrap();

        let tx = Transaction {
            from: address,
            to: Some(Address::default()),
            value: crate::types::Wei::zero(),
            gas_limit: 21000,
            gas_price: crate::types::Wei::from(1_000_000_000u64),
            nonce: 10,
            data: vec![],
            signature: vec![],
        };

        manager.add_pending_nonce(&tx).await.unwrap();
        assert_eq!(manager.get_pending_nonce_count(&address).await, 1);

        // 重置账户 nonce
        manager.reset_account_nonce(&address, 5).await.unwrap();

        // 检查状态
        assert_eq!(manager.get_account_nonce(&address).await.unwrap(), 5);
        assert_eq!(manager.get_pending_nonce_count(&address).await, 0);
    }

    #[tokio::test]
    async fn test_check_duplicate_transaction() {
        let manager = create_test_nonce_manager().await;
        let address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        let tx = Transaction {
            from: address,
            to: Some(Address::default()),
            value: crate::types::Wei::zero(),
            gas_limit: 21000,
            gas_price: crate::types::Wei::from(1_000_000_000u64),
            nonce: 0,
            data: vec![],
            signature: vec![],
        };

        // 初始应该没有重复
        assert!(!manager.check_duplicate_transaction(&tx).await.unwrap());

        // 添加到待处理
        manager.add_pending_nonce(&tx).await.unwrap();

        // 现在应该检测到重复
        assert!(manager.check_duplicate_transaction(&tx).await.unwrap());
    }
}