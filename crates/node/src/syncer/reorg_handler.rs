use norn_common::types::{Block, BlockHeader, Hash, Transaction};
use norn_common::traits::DBInterface;
use norn_common::error::{NornError, Result};
use norn_core::blockchain::Blockchain;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use async_trait::async_trait;

/// 链重组状态
#[derive(Debug, Clone, PartialEq)]
pub enum ReorgState {
    /// 无重组
    None,
    
    /// 检测到分叉
    ForkDetected,
    
    /// 正在回滚
    RollingBack,
    
    /// 正在应用新区块
    ApplyingNewChain,
    
    /// 重组完成
    Completed,
    
    /// 重组错误
    Error(String),
}

/// 分叉信息
#[derive(Debug, Clone)]
pub struct ForkInfo {
    /// 分叉点高度
    pub fork_height: u64,
    
    /// 分叉点哈希
    pub fork_hash: Hash,
    
    /// 当前链哈希
    pub current_chain_hash: Hash,
    
    /// 新链哈希
    pub new_chain_hash: Hash,
    
    /// 当前链高度
    pub current_height: u64,
    
    /// 新链高度
    pub new_height: u64,
    
    /// 需要回滚的区块数量
    pub rollback_count: u64,
    
    /// 需要应用的区块数量
    pub apply_count: u64,
    
    /// 检测时间
    pub detected_at: SystemTime,
}

/// 重组配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgConfig {
    /// 最大重组深度
    pub max_reorg_depth: u64,
    
    /// 重组超时时间
    pub reorg_timeout: Duration,
    
    /// 是否启用自动重组
    pub enable_auto_reorg: bool,
    
    /// 重组检查间隔
    pub check_interval: Duration,
    
    /// 最小确认数
    pub min_confirmations: u64,
}

impl Default for ReorgConfig {
    fn default() -> Self {
        Self {
            max_reorg_depth: 1000, // 最大重组 1000 个区块
            reorg_timeout: Duration::from_secs(300), // 5 分钟超时
            enable_auto_reorg: true,
            check_interval: Duration::from_secs(10), // 10 秒检查一次
            min_confirmations: 6, // 6 个确认
        }
    }
}

/// 链重组处理器
pub struct ReorgHandler {
    /// 区块链实例
    blockchain: Arc<Blockchain>,
    
    /// 数据库接口
    db: Arc<dyn DBInterface>,
    
    /// 重组配置
    config: ReorgConfig,
    
    /// 当前状态
    current_state: Arc<RwLock<ReorgState>>,
    
    /// 活跃的重组任务
    active_reorgs: Arc<RwLock<HashMap<Hash, ReorgTask>>>,
    
    /// 分叉历史
    fork_history: Arc<RwLock<Vec<ForkInfo>>>,
    
    /// 重组统计
    reorg_stats: Arc<RwLock<ReorgStats>>,
}

/// 重组任务
#[derive(Debug, Clone)]
pub struct ReorgTask {
    /// 任务 ID
    pub id: Hash,
    
    /// 分叉信息
    pub fork_info: ForkInfo,
    
    /// 新链区块
    pub new_chain: Vec<Block>,
    
    /// 当前状态
    pub state: ReorgState,
    
    /// 开始时间
    pub start_time: SystemTime,
    
    /// 已回滚区块数
    pub rolled_back_blocks: u64,
    
    /// 已应用区块数
    pub applied_blocks: u64,
}

/// 重组统计
#[derive(Debug, Clone, Default)]
pub struct ReorgStats {
    /// 总重组次数
    pub total_reorgs: u64,
    
    /// 成功重组次数
    pub successful_reorgs: u64,
    
    /// 失败重组次数
    pub failed_reorgs: u64,
    
    /// 最大重组深度
    pub max_reorg_depth: u64,
    
    /// 平均重组时间
    pub avg_reorg_time: Duration,
    
    /// 最后重组时间
    pub last_reorg_time: Option<SystemTime>,
    
    /// 总回滚区块数
    pub total_rolled_back: u64,
    
    /// 总应用区块数
    pub total_applied: u64,
}

impl ReorgHandler {
    /// 创建新的重组处理器
    pub fn new(
        blockchain: Arc<Blockchain>,
        db: Arc<dyn DBInterface>,
        config: ReorgConfig,
    ) -> Self {
        Self {
            blockchain,
            db,
            config,
            current_state: Arc::new(RwLock::new(ReorgState::None)),
            active_reorgs: Arc::new(RwLock::new(HashMap::new())),
            fork_history: Arc::new(RwLock::new(Vec::new())),
            reorg_stats: Arc::new(RwLock::new(ReorgStats::default())),
        }
    }

    /// 检测并处理链重组
    pub async fn check_and_handle_reorg(&self, new_block: &Block) -> Result<bool> {
        debug!("Checking for chain reorg with new block: {}", new_block.header.block_hash);
        
        // 1. 获取当前链状态
        let current_height = self.blockchain.get_latest_height().await?;
        let current_hash = self.blockchain.get_latest_hash().await?;
        
        // 2. 检查是否需要重组
        let fork_info = self.detect_fork(new_block, current_height, current_hash).await?;
        
        if fork_info.is_none() {
            debug!("No fork detected");
            return Ok(false);
        }

        let fork_info = fork_info.unwrap();
        info!("Fork detected: {:?}", fork_info);

        // 3. 验证重组是否可行
        if !self.validate_reorg(&fork_info).await? {
            warn!("Reorg validation failed for fork: {:?}", fork_info);
            return Ok(false);
        }

        // 4. 执行重组
        let reorg_result = self.execute_reorg(&fork_info, new_block).await?;
        
        // 5. 更新统计
        self.update_reorg_stats(&fork_info, &reorg_result).await;

        Ok(reorg_result.success)
    }

    /// 检测分叉
    async fn detect_fork(
        &self,
        new_block: &Block,
        current_height: u64,
        current_hash: Hash,
    ) -> Result<Option<ForkInfo>> {
        // 1. 检查新区块是否连接到当前链
        if new_block.header.prev_block_hash == current_hash {
            // 直接连接，无需重组
            return Ok(None);
        }

        // 2. 查找共同祖先
        let (fork_height, fork_hash) = self.find_common_ancestor(new_block, current_height).await?;
        
        // 3. 计算重组信息
        let rollback_count = current_height - fork_height;
        let apply_count = new_block.header.height - fork_height;

        // 4. 检查重组深度
        if rollback_count > self.config.max_reorg_depth {
            warn!("Reorg depth {} exceeds maximum {}", rollback_count, self.config.max_reorg_depth);
            return Ok(None);
        }

        let fork_info = ForkInfo {
            fork_height,
            fork_hash,
            current_chain_hash: current_hash,
            new_chain_hash: new_block.header.block_hash,
            current_height,
            new_height: new_block.header.height,
            rollback_count,
            apply_count,
            detected_at: SystemTime::now(),
        };

        Ok(Some(fork_info))
    }

    /// 查找共同祖先
    async fn find_common_ancestor(
        &self,
        new_block: &Block,
        current_height: u64,
    ) -> Result<(u64, Hash)> {
        let mut current_hash = new_block.header.prev_block_hash;
        let mut height = new_block.header.height - 1;

        // 从新区块向后查找
        while height > 0 {
            // 检查当前链是否有这个区块
            if let Ok(existing_hash) = self.blockchain.get_block_hash(height).await {
                if existing_hash == current_hash {
                    return Ok((height, current_hash));
                }
            }

            // 获取前一个区块哈希
            if let Ok(block) = self.db.get_block(&current_hash).await {
                current_hash = block.header.prev_block_hash;
                height -= 1;
            } else {
                break;
            }
        }

        // 如果没找到，返回创世块
        Ok((0, Hash::default()))
    }

    /// 验证重组
    async fn validate_reorg(&self, fork_info: &ForkInfo) -> Result<bool> {
        // 1. 检查重组深度
        if fork_info.rollback_count > self.config.max_reorg_depth {
            warn!("Reorg depth {} exceeds maximum {}", 
                   fork_info.rollback_count, self.config.max_reorg_depth);
            return Ok(false);
        }

        // 2. 检查新链的有效性
        if !self.validate_new_chain(fork_info).await? {
            warn!("New chain validation failed");
            return Ok(false);
        }

        // 3. 检查确认数
        if fork_info.rollback_count > 0 && 
           fork_info.rollback_count < self.config.min_confirmations {
            warn!("Insufficient confirmations for reorg: {}", fork_info.rollback_count);
            return Ok(false);
        }

        Ok(true)
    }

    /// 验证新链
    async fn validate_new_chain(&self, fork_info: &ForkInfo) -> Result<bool> {
        // TODO: 实现新链验证逻辑
        // 1. 验证区块头
        // 2. 验证交易
        // 3. 验证共识规则
        // 4. 验证状态转换
        
        // 简化实现：总是返回 true
        Ok(true)
    }

    /// 执行重组
    async fn execute_reorg(&self, fork_info: &ForkInfo, new_block: &Block) -> Result<ReorgResult> {
        info!("Executing reorg: {:?}", fork_info);
        
        let start_time = SystemTime::now();
        
        // 1. 创建重组任务
        let task_id = self.generate_task_id();
        let task = ReorgTask {
            id: task_id,
            fork_info: fork_info.clone(),
            new_chain: vec![new_block.clone()], // TODO: 获取完整的新链
            state: ReorgState::RollingBack,
            start_time,
            rolled_back_blocks: 0,
            applied_blocks: 0,
        };

        // 2. 添加到活跃任务
        {
            let mut active_reorgs = self.active_reorgs.write().await;
            active_reorgs.insert(task_id, task.clone());
        }

        // 3. 更新状态
        {
            let mut current_state = self.current_state.write().await;
            *current_state = ReorgState::RollingBack;
        }

        // 4. 执行回滚
        let rollback_result = self.rollback_blocks(fork_info.rollback_count).await?;
        if !rollback_result.success {
            return Ok(ReorgResult {
                success: false,
                rolled_back_blocks: rollback_result.rolled_back,
                applied_blocks: 0,
                duration: start_time.elapsed().unwrap_or(Duration::ZERO),
                error: rollback_result.error,
            });
        }

        // 5. 应用新区块
        let apply_result = self.apply_new_blocks(&task.new_chain).await?;
        if !apply_result.success {
            // 尝试回滚应用
            let _ = self.rollback_applied_blocks(apply_result.applied).await;
            return Ok(ReorgResult {
                success: false,
                rolled_back_blocks: rollback_result.rolled_back,
                applied_blocks: apply_result.applied,
                duration: start_time.elapsed().unwrap_or(Duration::ZERO),
                error: apply_result.error,
            });
        }

        // 6. 更新任务状态
        {
            let mut active_reorgs = self.active_reorgs.write().await;
            if let Some(task) = active_reorgs.get_mut(&task_id) {
                task.state = ReorgState::Completed;
                task.rolled_back_blocks = rollback_result.rolled_back;
                task.applied_blocks = apply_result.applied;
            }
        }

        // 7. 更新全局状态
        {
            let mut current_state = self.current_state.write().await;
            *current_state = ReorgState::Completed;
        }

        // 8. 记录分叉历史
        {
            let mut fork_history = self.fork_history.write().await;
            fork_history.push(fork_info.clone());
            // 保留最近 100 次分叉记录
            if fork_history.len() > 100 {
                fork_history.remove(0);
            }
        }

        // 9. 清理任务
        {
            let mut active_reorgs = self.active_reorgs.write().await;
            active_reorgs.remove(&task_id);
        }

        let duration = start_time.elapsed().unwrap_or(Duration::ZERO);
        info!("Reorg completed successfully in {:?}", duration);

        Ok(ReorgResult {
            success: true,
            rolled_back_blocks: rollback_result.rolled_back,
            applied_blocks: apply_result.applied,
            duration,
            error: None,
        })
    }

    /// 回滚区块
    async fn rollback_blocks(&self, count: u64) -> Result<RollbackResult> {
        debug!("Rolling back {} blocks", count);
        
        let mut rolled_back = 0u64;
        
        for _ in 0..count {
            // 1. 获取最新区块
            let latest_hash = self.blockchain.get_latest_hash().await?;
            let latest_block = self.db.get_block(&latest_hash).await?;
            
            // 2. 回滚状态
            if let Err(e) = self.rollback_block_state(&latest_block).await {
                error!("Failed to rollback block {}: {}", latest_hash, e);
                return Ok(RollbackResult {
                    success: false,
                    rolled_back,
                    error: Some(format!("Rollback failed: {}", e)),
                });
            }
            
            // 3. 从区块链移除
            if let Err(e) = self.blockchain.remove_latest_block().await {
                error!("Failed to remove block from blockchain: {}", e);
                return Ok(RollbackResult {
                    success: false,
                    rolled_back,
                    error: Some(format!("Remove failed: {}", e)),
                });
            }
            
            rolled_back += 1;
        }

        Ok(RollbackResult {
            success: true,
            rolled_back,
            error: None,
        })
    }

    /// 回滚单个区块状态
    async fn rollback_block_state(&self, block: &Block) -> Result<()> {
        // TODO: 实现状态回滚逻辑
        // 1. 回滚交易状态
        // 2. 回滚账户状态
        // 3. 回滚存储状态
        // 4. 更新统计
        
        debug!("Rolling back state for block: {}", block.header.block_hash);
        Ok(())
    }

    /// 应用新区块
    async fn apply_new_blocks(&self, blocks: &[Block]) -> Result<ApplyResult> {
        debug!("Applying {} new blocks", blocks.len());
        
        let mut applied = 0u64;
        
        for block in blocks {
            // 1. 验证区块
            if !self.validate_block_for_reorg(block).await? {
                error!("Invalid block for reorg: {}", block.header.block_hash);
                return Ok(ApplyResult {
                    success: false,
                    applied,
                    error: Some("Invalid block".to_string()),
                });
            }
            
            // 2. 应用区块
            if let Err(e) = self.apply_block_for_reorg(block).await {
                error!("Failed to apply block {}: {}", block.header.block_hash, e);
                return Ok(ApplyResult {
                    success: false,
                    applied,
                    error: Some(format!("Apply failed: {}", e)),
                });
            }
            
            applied += 1;
        }

        Ok(ApplyResult {
            success: true,
            applied,
            error: None,
        })
    }

    /// 验证重组中的区块
    async fn validate_block_for_reorg(&self, block: &Block) -> Result<bool> {
        // TODO: 实现重组中的区块验证
        // 1. 验证区块头
        // 2. 验证交易
        // 3. 验证共识
        // 4. 验证状态转换
        
        Ok(true)
    }

    /// 应用重组中的区块
    async fn apply_block_for_reorg(&self, block: &Block) -> Result<()> {
        // TODO: 实现重组中的区块应用
        // 1. 应用交易
        // 2. 更新状态
        // 3. 更新统计
        
        debug!("Applying block for reorg: {}", block.header.block_hash);
        self.blockchain.add_block(block.clone()).await?;
        Ok(())
    }

    /// 回滚已应用的区块
    async fn rollback_applied_blocks(&self, count: u64) -> Result<()> {
        debug!("Rolling back {} applied blocks", count);
        
        for _ in 0..count {
            if let Err(e) = self.blockchain.remove_latest_block().await {
                warn!("Failed to rollback applied block: {}", e);
                break;
            }
        }

        Ok(())
    }

    /// 更新重组统计
    async fn update_reorg_stats(&self, fork_info: &ForkInfo, result: &ReorgResult) {
        let mut stats = self.reorg_stats.write().await;
        
        stats.total_reorgs += 1;
        
        if result.success {
            stats.successful_reorgs += 1;
            stats.max_reorg_depth = stats.max_reorg_depth.max(fork_info.rollback_count);
            stats.total_rolled_back += result.rolled_back_blocks;
            stats.total_applied += result.applied_blocks;
            stats.last_reorg_time = Some(SystemTime::now());
            
            // 更新平均重组时间
            let total_time = stats.avg_reorg_time * (stats.successful_reorgs - 1) as u32 + result.duration;
            stats.avg_reorg_time = total_time / stats.successful_reorgs as u32;
        } else {
            stats.failed_reorgs += 1;
        }
    }

    /// 生成任务 ID
    fn generate_task_id(&self) -> Hash {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        
        let mut hash = [0u8; 32];
        hash[0..8].copy_from_slice(&id.to_le_bytes());
        Hash(hash)
    }

    /// 获取当前状态
    pub async fn get_state(&self) -> (ReorgState, Vec<ReorgTask>) {
        let state = self.current_state.read().await.clone();
        let active_tasks = self.active_reorgs.read().await;
        let tasks = active_tasks.values().cloned().collect();
        (state, tasks)
    }

    /// 获取重组统计
    pub async fn get_reorg_stats(&self) -> ReorgStats {
        self.reorg_stats.read().await.clone()
    }

    /// 获取分叉历史
    pub async fn get_fork_history(&self) -> Vec<ForkInfo> {
        self.fork_history.read().await.clone()
    }

    /// 强制重组到指定区块
    pub async fn force_reorg_to_block(&self, target_block: &Block) -> Result<bool> {
        info!("Forcing reorg to block: {}", target_block.header.block_hash);
        
        let current_height = self.blockchain.get_latest_height().await?;
        let current_hash = self.blockchain.get_latest_hash().await?;
        
        // 创建人工分叉信息
        let fork_info = ForkInfo {
            fork_height: 0, // 假设从创世块分叉
            fork_hash: Hash::default(),
            current_chain_hash: current_hash,
            new_chain_hash: target_block.header.block_hash,
            current_height,
            new_height: target_block.header.height,
            rollback_count: current_height,
            apply_count: target_block.header.height,
            detected_at: SystemTime::now(),
        };

        let result = self.execute_reorg(&fork_info, target_block).await?;
        Ok(result.success)
    }

    /// 清理旧的重组记录
    pub async fn cleanup_old_records(&self, max_age: Duration) {
        let mut fork_history = self.fork_history.write().await;
        let now = SystemTime::now();
        
        fork_history.retain(|fork| {
            now.duration_since(fork.detected_at).unwrap_or(Duration::MAX) < max_age
        });
    }
}

/// 重组结果
#[derive(Debug, Clone)]
pub struct ReorgResult {
    pub success: bool,
    pub rolled_back_blocks: u64,
    pub applied_blocks: u64,
    pub duration: Duration,
    pub error: Option<String>,
}

/// 回滚结果
#[derive(Debug, Clone)]
pub struct RollbackResult {
    pub success: bool,
    pub rolled_back: u64,
    pub error: Option<String>,
}

/// 应用结果
#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub success: bool,
    pub applied: u64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_core::blockchain::Blockchain;

    #[tokio::test]
    async fn test_reorg_handler_creation() {
        let blockchain = Arc::new(Blockchain::new_with_fixed_genesis(
            Arc::new(crate::mocks::MockDB::new())
        ).await);
        
        let db = Arc::new(crate::mocks::MockDB::new());
        let config = ReorgConfig::default();
        
        let handler = ReorgHandler::new(blockchain, db, config);
        
        let (state, tasks) = handler.get_state().await;
        assert_eq!(state, ReorgState::None);
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn test_fork_detection() {
        let blockchain = Arc::new(Blockchain::new_with_fixed_genesis(
            Arc::new(crate::mocks::MockDB::new())
        ).await);
        
        let db = Arc::new(crate::mocks::MockDB::new());
        let config = ReorgConfig::default();
        let handler = ReorgHandler::new(blockchain, db, config);
        
        // 创建测试区块
        let new_block = Block {
            header: norn_common::types::BlockHeader {
                timestamp: 1234567890,
                prev_block_hash: Hash([1u8; 32]), // 不同的前一个区块哈希
                block_hash: Hash([2u8; 32]),
                merkle_root: Hash::default(),
                height: 1,
                public_key: norn_common::types::PublicKey::default(),
                params: vec![],
                gas_limit: 1000000,
            },
            transactions: vec![],
        };

        let fork_info = handler.detect_fork(&new_block, 0, Hash::default()).await;
        assert!(fork_info.is_ok());
    }

    #[tokio::test]
    async fn test_reorg_stats() {
        let blockchain = Arc::new(Blockchain::new_with_fixed_genesis(
            Arc::new(crate::mocks::MockDB::new())
        ).await);
        
        let db = Arc::new(crate::mocks::MockDB::new());
        let config = ReorgConfig::default();
        let handler = ReorgHandler::new(blockchain, db, config);
        
        let stats = handler.get_reorg_stats().await;
        assert_eq!(stats.total_reorgs, 0);
        assert_eq!(stats.successful_reorgs, 0);
        assert_eq!(stats.failed_reorgs, 0);
    }
}