use norn_common::types::{Block, BlockHeader, Hash, Transaction};
use norn_common::traits::DBInterface;
use norn_common::error::{NornError, Result};
use norn_core::blockchain::Blockchain;
use norn_core::consensus::povf::PoVFEngine;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use async_trait::async_trait;

/// 同步状态
#[derive(Debug, Clone, PartialEq)]
pub enum SyncState {
    /// 空闲状态
    Idle,
    
    /// 请求区块头
    RequestingHeaders,
    
    /// 下载区块
    DownloadingBlocks,
    
    /// 验证区块
    VerifyingBlocks,
    
    /// 应用区块
    ApplyingBlocks,
    
    /// 同步完成
    Synced,
    
    /// 同步错误
    Error(String),
}

/// 同步配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// 最大并发连接数
    pub max_peers: usize,
    
    /// 批量请求大小
    pub batch_size: usize,
    
    /// 同步超时时间
    pub sync_timeout: Duration,
    
    /// 最大区块大小
    pub max_block_size: usize,
    
    /// 是否启用快速同步
    pub enable_fast_sync: bool,
    
    /// 快速同步检查点间隔
    pub fast_sync_checkpoint_interval: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            max_peers: 10,
            batch_size: 100,
            sync_timeout: Duration::from_secs(30),
            max_block_size: 10 * 1024 * 1024, // 10MB
            enable_fast_sync: true,
            fast_sync_checkpoint_interval: 1000,
        }
    }
}

/// 同步进度
#[derive(Debug, Clone)]
pub struct SyncProgress {
    pub current_height: u64,
    pub target_height: u64,
    pub downloaded_blocks: u64,
    pub verified_blocks: u64,
    pub applied_blocks: u64,
    pub sync_start_time: Instant,
    pub estimated_time_remaining: Option<Duration>,
}

/// 区块请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRequest {
    pub start_height: u64,
    pub end_height: u64,
    pub max_blocks: usize,
    pub include_transactions: bool,
}

/// 区块响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockResponse {
    pub blocks: Vec<Block>,
    pub start_height: u64,
    pub has_more: bool,
    pub total_blocks: u64,
}

/// 区块同步器
pub struct BlockSyncer {
    /// 区块链实例
    blockchain: Arc<Blockchain>,
    
    /// 数据库接口
    db: Arc<dyn DBInterface>,
    
    /// 共识引擎
    consensus: Arc<PoVFEngine>,
    
    /// 同步配置
    config: SyncConfig,
    
    /// 当前状态
    current_state: Arc<RwLock<SyncState>>,
    
    /// 同步进度
    progress: Arc<RwLock<SyncProgress>>,
    
    /// 活跃的同步任务
    active_tasks: Arc<RwLock<HashMap<u64, SyncTask>>>,
    
    /// 已知的区块哈希
    known_blocks: Arc<RwLock<HashMap<Hash, u64>>>,
    
    /// 同步队列
    sync_queue: Arc<RwLock<Vec<SyncTask>>>,
}

/// 同步任务
#[derive(Debug, Clone)]
pub struct SyncTask {
    pub id: u64,
    pub task_type: SyncTaskType,
    pub start_height: u64,
    pub target_height: u64,
    pub priority: SyncPriority,
    pub created_at: SystemTime,
}

/// 同步任务类型
#[derive(Debug, Clone, PartialEq)]
pub enum SyncTaskType {
    /// 完整同步
    FullSync,
    
    /// 快速同步
    FastSync,
    
    /// 头部同步
    HeaderSync,
    
    /// 区块请求
    BlockRequest,
}

/// 同步优先级
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SyncPriority {
    High = 3,
    Medium = 2,
    Low = 1,
}

impl BlockSyncer {
    /// 创建新的区块同步器
    pub fn new(
        blockchain: Arc<Blockchain>,
        db: Arc<dyn DBInterface>,
        consensus: Arc<PoVFEngine>,
        config: SyncConfig,
    ) -> Self {
        Self {
            blockchain,
            db,
            consensus,
            config,
            current_state: Arc::new(RwLock::new(SyncState::Idle)),
            progress: Arc::new(RwLock::new(SyncProgress {
                current_height: 0,
                target_height: 0,
                downloaded_blocks: 0,
                verified_blocks: 0,
                applied_blocks: 0,
                sync_start_time: Instant::now(),
                estimated_time_remaining: None,
            })),
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            known_blocks: Arc::new(RwLock::new(HashMap::new())),
            sync_queue: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 启动同步
    pub async fn start_sync(&self, target_height: u64) -> Result<()> {
        info!("Starting blockchain sync to height {}", target_height);
        
        // 1. 检查当前状态
        {
            let mut state = self.current_state.write().await;
            if !matches!(*state, SyncState::Idle) {
                return Err(NornError::SyncError("Sync already in progress".to_string()));
            }
            *state = SyncState::RequestingHeaders;
        }

        // 2. 获取当前高度
        let current_height = self.blockchain.get_latest_height().await?;
        
        // 3. 如果已经是最新的，直接返回
        if current_height >= target_height {
            info!("Already synced to height {}", current_height);
            return Ok(());
        }

        // 4. 更新进度
        {
            let mut progress = self.progress.write().await;
            progress.current_height = current_height;
            progress.target_height = target_height;
            progress.sync_start_time = Instant::now();
            progress.estimated_time_remaining = None;
        }

        // 5. 创建同步任务
        let task = SyncTask {
            id: self.generate_task_id(),
            task_type: if self.config.enable_fast_sync {
                SyncTaskType::FastSync
            } else {
                SyncTaskType::FullSync
            },
            start_height: current_height,
            target_height,
            priority: SyncPriority::High,
            created_at: SystemTime::now(),
        };

        // 6. 添加到任务队列
        {
            let mut queue = self.sync_queue.write().await;
            queue.push(task);
        }

        // 7. 开始处理同步任务
        self.process_sync_tasks().await
    }

    /// 处理同步任务
    async fn process_sync_tasks(&self) -> Result<()> {
        loop {
            // 1. 获取下一个任务
            let task = {
                let mut queue = self.sync_queue.write().await;
                if queue.is_empty() {
                    break;
                }
                queue.remove(0)
            };

            let task = match task {
                Some(t) => t,
                None => break,
            };

            // 2. 执行任务
            match task.task_type {
                SyncTaskType::FastSync => self.execute_fast_sync(&task).await?,
                SyncTaskType::FullSync => self.execute_full_sync(&task).await?,
                SyncTaskType::HeaderSync => self.execute_header_sync(&task).await?,
                SyncTaskType::BlockRequest => self.execute_block_request(&task).await?,
            }

            // 3. 更新任务状态
            self.complete_task(&task).await;

            // 4. 让出控制权
            tokio::task::yield_now().await;
        }

        // 5. 设置为空闲状态
        {
            let mut state = self.current_state.write().await;
            *state = SyncState::Idle;
        }

        info!("Sync tasks completed");
        Ok(())
    }

    /// 执行快速同步
    async fn execute_fast_sync(&self, task: &SyncTask) -> Result<()> {
        info!("Executing fast sync from {} to {}", task.start_height, task.target_height);
        
        {
            let mut state = self.current_state.write().await;
            *state = SyncState::DownloadingBlocks;
        }

        // 1. 下载区块头
        let headers = self.download_block_headers(task.start_height, task.target_height).await?;
        
        // 2. 下载区块体
        let blocks = self.download_block_bodies(&headers).await?;
        
        // 3. 验证区块
        {
            let mut state = self.current_state.write().await;
            *state = SyncState::VerifyingBlocks;
        }
        
        let verified_blocks = self.verify_blocks(&blocks).await?;
        
        // 4. 应用区块
        {
            let mut state = self.current_state.write().await;
            *state = SyncState::ApplyingBlocks;
        }
        
        self.apply_blocks(&verified_blocks).await?;
        
        info!("Fast sync completed: {} blocks processed", verified_blocks.len());
        Ok(())
    }

    /// 执行完整同步
    async fn execute_full_sync(&self, task: &SyncTask) -> Result<()> {
        info!("Executing full sync from {} to {}", task.start_height, task.target_height);
        
        let mut current_height = task.start_height;
        
        while current_height < task.target_height {
            // 1. 请求下一批区块
            let batch_end = std::cmp::min(current_height + self.config.batch_size as u64, task.target_height);
            let blocks = self.request_blocks(current_height, batch_end).await?;
            
            // 2. 验证和应用区块
            for block in &blocks {
                if !self.validate_and_apply_block(block).await? {
                    warn!("Failed to apply block: {}", block.header.block_hash);
                    continue;
                }
                
                current_height = block.header.height;
                self.update_progress(block.header.height).await;
            }

            // 3. 更新已知区块
            self.update_known_blocks(&blocks).await;

            // 4. 让出控制权
            tokio::task::yield_now().await;
        }

        info!("Full sync completed to height {}", current_height);
        Ok(())
    }

    /// 执行头部同步
    async fn execute_header_sync(&self, task: &SyncTask) -> Result<()> {
        info!("Executing header sync from {} to {}", task.start_height, task.target_height);
        
        // 1. 下载区块头
        let headers = self.download_block_headers(task.start_height, task.target_height).await?;
        
        // 2. 验证头部
        for header in &headers {
            if !self.validate_block_header(header).await? {
                warn!("Invalid block header: {}", header.block_hash);
                continue;
            }
        }

        // 3. 更新已知区块
        let hashes: Vec<Hash> = headers.iter().map(|h| h.block_hash).collect();
        self.update_known_block_hashes(&hashes).await;

        info!("Header sync completed: {} headers", headers.len());
        Ok(())
    }

    /// 执行区块请求
    async fn execute_block_request(&self, task: &SyncTask) -> Result<()> {
        info!("Executing block request from {} to {}", task.start_height, task.target_height);
        
        // 1. 请求特定区块
        let blocks = self.request_blocks(task.start_height, task.target_height).await?;
        
        // 2. 验证和应用
        for block in &blocks {
            if !self.validate_and_apply_block(block).await? {
                warn!("Failed to apply requested block: {}", block.header.block_hash);
                continue;
            }
        }

        info!("Block request completed: {} blocks", blocks.len());
        Ok(())
    }

    /// 下载区块头
    async fn download_block_headers(&self, start_height: u64, end_height: u64) -> Result<Vec<BlockHeader>> {
        debug!("Downloading block headers from {} to {}", start_height, end_height);
        
        // TODO: 实现网络请求
        // 这里返回模拟数据
        let mut headers = Vec::new();
        for height in start_height..=end_height {
            let header = BlockHeader {
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| NornError::NetworkError(format!("Time error: {}", e)))?
                    .as_secs(),
                prev_block_hash: Hash::default(),
                block_hash: Hash([height as u8; 32]),
                merkle_root: Hash::default(),
                height,
                public_key: norn_common::types::PublicKey::default(),
                params: vec![],
                gas_limit: 1000000,
            };
            headers.push(header);
        }

        Ok(headers)
    }

    /// 下载区块体
    async fn download_block_bodies(&self, headers: &[BlockHeader]) -> Result<Vec<Block>> {
        debug!("Downloading block bodies for {} headers", headers.len());
        
        let mut blocks = Vec::new();
        
        for header in headers {
            // TODO: 从网络下载完整区块
            // 这里创建模拟区块
            let block = Block {
                header: header.clone(),
                transactions: vec![], // TODO: 下载实际交易
            };
            blocks.push(block);
        }

        Ok(blocks)
    }

    /// 请求区块
    async fn request_blocks(&self, start_height: u64, end_height: u64) -> Result<Vec<Block>> {
        debug!("Requesting blocks from {} to {}", start_height, end_height);
        
        // TODO: 实现网络请求
        // 这里返回模拟数据
        let mut blocks = Vec::new();
        for height in start_height..=end_height {
            let block = Block {
                header: BlockHeader {
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| NornError::NetworkError(format!("Time error: {}", e)))?
                        .as_secs(),
                    prev_block_hash: Hash::default(),
                    block_hash: Hash([height as u8; 32]),
                    merkle_root: Hash::default(),
                    height,
                    public_key: norn_common::types::PublicKey::default(),
                    params: vec![],
                    gas_limit: 1000000,
                },
                transactions: vec![],
            };
            blocks.push(block);
        }

        Ok(blocks)
    }

    /// 验证区块
    async fn verify_blocks(&self, blocks: &[Block]) -> Result<Vec<Block>> {
        debug!("Verifying {} blocks", blocks.len());
        
        let mut verified_blocks = Vec::new();
        
        for block in blocks {
            // 1. 验证区块头
            if !self.validate_block_header(&block.header).await? {
                warn!("Invalid block header: {}", block.header.block_hash);
                continue;
            }

            // 2. 验证交易
            let mut all_tx_valid = true;
            for tx in &block.transactions {
                if !self.validate_transaction(tx).await? {
                    warn!("Invalid transaction in block: {:?}", tx);
                    all_tx_valid = false;
                    break;
                }
            }

            if all_tx_valid {
                verified_blocks.push(block.clone());
            }
        }

        Ok(verified_blocks)
    }

    /// 验证区块头
    async fn validate_block_header(&self, header: &BlockHeader) -> Result<bool> {
        // 1. 验证时间戳
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| NornError::ValidationError(format!("Time error: {}", e)))?
            .as_secs();
        
        if header.timestamp > now + 300 { // 允许 5 分钟时钟偏差
            warn!("Block timestamp too far in future: {}", header.timestamp);
            return Ok(false);
        }

        // 2. 验证 Gas 限制
        if header.gas_limit <= 0 || header.gas_limit > 100_000_000 {
            warn!("Invalid gas limit: {}", header.gas_limit);
            return Ok(false);
        }

        // 3. 验证参数
        if header.params.is_empty() {
            warn!("Empty block params");
            return Ok(false);
        }

        // 4. 验证前一个区块哈希（除了创世块）
        if header.height > 0 && header.prev_block_hash == Hash::default() {
            warn!("Invalid prev block hash for height {}", header.height);
            return Ok(false);
        }

        Ok(true)
    }

    /// 验证交易
    async fn validate_transaction(&self, _tx: &Transaction) -> Result<bool> {
        // TODO: 实现交易验证逻辑
        Ok(true)
    }

    /// 应用区块
    async fn apply_blocks(&self, blocks: &[Block]) -> Result<()> {
        debug!("Applying {} blocks", blocks.len());
        
        for block in blocks {
            self.apply_block(block).await?;
            self.update_progress(block.header.height).await;
        }

        Ok(())
    }

    /// 应用单个区块
    async fn apply_block(&self, block: &Block) -> Result<()> {
        debug!("Applying block {} at height {}", block.header.block_hash, block.header.height);
        
        // 1. 验证区块
        if !self.validate_block_header(&block.header).await? {
            return Err(NornError::ValidationError("Invalid block header".to_string()));
        }

        // 2. 验证交易
        for tx in &block.transactions {
            if !self.validate_transaction(tx).await? {
                return Err(NornError::ValidationError("Invalid transaction".to_string()));
            }
        }

        // 3. 应用到区块链
        self.blockchain.add_block(block.clone()).await?;
        
        // 4. 更新已知区块
        self.update_known_blocks(&[block.clone()]).await;

        Ok(())
    }

    /// 验证并应用区块
    async fn validate_and_apply_block(&self, block: &Block) -> Result<bool> {
        match self.apply_block(block).await {
            Ok(()) => Ok(true),
            Err(e) => {
                error!("Failed to apply block {}: {}", block.header.block_hash, e);
                Ok(false)
            }
        }
    }

    /// 更新同步进度
    async fn update_progress(&self, current_height: u64) {
        let mut progress = self.progress.write().await;
        progress.current_height = current_height;
        progress.applied_blocks += 1;
        
        // 估算剩余时间
        let elapsed = progress.sync_start_time.elapsed();
        let remaining_heights = progress.target_height.saturating_sub(current_height);
        if remaining_heights > 0 && progress.applied_blocks > 0 {
            let avg_time_per_block = elapsed / progress.applied_blocks as u32;
            let estimated_remaining = avg_time_per_block * remaining_heights as u32;
            progress.estimated_time_remaining = Some(estimated_remaining);
        }
        
        debug!("Sync progress: {}/{}, applied: {}", current_height, progress.target_height);
    }

    /// 更新已知区块
    async fn update_known_blocks(&self, blocks: &[Block]) {
        let mut known_blocks = self.known_blocks.write().await;
        for block in blocks {
            known_blocks.insert(block.header.block_hash, block.header.height);
        }
    }

    /// 更新已知区块哈希
    async fn update_known_block_hashes(&self, hashes: &[Hash]) {
        let mut known_blocks = self.known_blocks.write().await;
        for hash in hashes {
            if !known_blocks.contains_key(hash) {
                // 估算高度
                let estimated_height = self.estimate_block_height(hash);
                known_blocks.insert(*hash, estimated_height);
            }
        }
    }

    /// 估算区块高度
    fn estimate_block_height(&self, hash: &Hash) -> u64 {
        // 简化实现：基于哈希值估算
        let hash_bytes = &hash.0;
        let mut height = 0u64;
        
        for (i, &byte) in hash_bytes.iter().enumerate() {
            height += (*byte as u64) << (i * 8);
        }
        
        height
    }

    /// 完成任务
    async fn complete_task(&self, task: &SyncTask) {
        let mut active_tasks = self.active_tasks.write().await;
        active_tasks.remove(&task.id);
    }

    /// 生成任务 ID
    fn generate_task_id(&self) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        COUNTER.fetch_add(1, Ordering::SeqCst)
    }

    /// 获取当前状态
    pub async fn get_state(&self) -> (SyncState, SyncProgress) {
        let state = self.current_state.read().await.clone();
        let progress = self.progress.read().await.clone();
        (state, progress)
    }

    /// 获取同步统计
    pub async fn get_sync_stats(&self) -> SyncStats {
        let progress = self.progress.read().await;
        let active_tasks = self.active_tasks.read().await;
        let known_blocks = self.known_blocks.read().await;
        
        SyncStats {
            current_height: progress.current_height,
            target_height: progress.target_height,
            downloaded_blocks: progress.downloaded_blocks,
            verified_blocks: progress.verified_blocks,
            applied_blocks: progress.applied_blocks,
            sync_duration: progress.sync_start_time.elapsed(),
            active_tasks: active_tasks.len(),
            known_blocks: known_blocks.len(),
            estimated_time_remaining: progress.estimated_time_remaining,
        }
    }

    /// 检查是否需要同步
    pub async fn needs_sync(&self, target_height: u64) -> bool {
        let current_height = self.blockchain.get_latest_height().await.unwrap_or(0);
        current_height < target_height
    }

    /// 强制重新同步
    pub async fn force_resync(&self) -> Result<()> {
        info!("Forcing blockchain resync");
        
        // 1. 清理缓存
        {
            let mut known_blocks = self.known_blocks.write().await;
            known_blocks.clear();
        }
        
        // 2. 重置状态
        {
            let mut state = self.current_state.write().await;
            *state = SyncState::Idle;
        }
        
        // 3. 重置进度
        {
            let mut progress = self.progress.write().await;
            progress.current_height = 0;
            progress.target_height = 0;
            progress.downloaded_blocks = 0;
            progress.verified_blocks = 0;
            progress.applied_blocks = 0;
            progress.sync_start_time = Instant::now();
            progress.estimated_time_remaining = None;
        }
        
        // 4. 取消所有活跃任务
        {
            let mut active_tasks = self.active_tasks.write().await;
            active_tasks.clear();
        }
        
        // 5. 清理同步队列
        {
            let mut queue = self.sync_queue.write().await;
            queue.clear();
        }
        
        Ok(())
    }
}

/// 同步统计信息
#[derive(Debug, Clone)]
pub struct SyncStats {
    pub current_height: u64,
    pub target_height: u64,
    pub downloaded_blocks: u64,
    pub verified_blocks: u64,
    pub applied_blocks: u64,
    pub sync_duration: Duration,
    pub active_tasks: usize,
    pub known_blocks: usize,
    pub estimated_time_remaining: Option<Duration>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_core::blockchain::Blockchain;
    use norn_crypto::vdf::SimpleVDF;
    use norn_crypto::vrf::VRFKeyPair;
    use norn_core::consensus::povf::PoVFConfig;

    #[tokio::test]
    async fn test_block_syncer_creation() {
        let blockchain = Arc::new(Blockchain::new_with_fixed_genesis(
            Arc::new(crate::mocks::MockDB::new())
        ).await);
        
        let db = Arc::new(crate::mocks::MockDB::new());
        let consensus = Arc::new(PoVFEngine::new(
            PoVFConfig::default(),
            Arc::new(SimpleVDF::new()),
            VRFKeyPair::default(),
        ));
        
        let config = SyncConfig::default();
        let syncer = BlockSyncer::new(blockchain, db, consensus, config);
        
        let (state, progress) = syncer.get_state().await;
        assert_eq!(state, SyncState::Idle);
        assert_eq!(progress.current_height, 0);
    }

    #[tokio::test]
    async fn test_sync_progress() {
        let blockchain = Arc::new(Blockchain::new_with_fixed_genesis(
            Arc::new(crate::mocks::MockDB::new())
        ).await);
        
        let db = Arc::new(crate::mocks::MockDB::new());
        let consensus = Arc::new(PoVFEngine::new(
            PoVFConfig::default(),
            Arc::new(SimpleVDF::new()),
            VRFKeyPair::default(),
        ));
        
        let config = SyncConfig::default();
        let syncer = BlockSyncer::new(blockchain, db, consensus, config);
        
        // 测试进度更新
        syncer.update_progress(5).await;
        syncer.update_progress(10).await;
        
        let progress = syncer.progress.read().await;
        assert_eq!(progress.current_height, 10);
        assert_eq!(progress.applied_blocks, 2);
    }

    #[tokio::test]
    async fn test_needs_sync() {
        let blockchain = Arc::new(Blockchain::new_with_fixed_genesis(
            Arc::new(crate::mocks::MockDB::new())
        ).await);
        
        let db = Arc::new(crate::mocks::MockDB::new());
        let consensus = Arc::new(PoVFEngine::new(
            PoVFConfig::default(),
            Arc::new(SimpleVDF::new()),
            VRFKeyPair::default(),
        ));
        
        let config = SyncConfig::default();
        let syncer = BlockSyncer::new(blockchain, db, consensus, config);
        
        // 测试需要同步
        assert!(syncer.needs_sync(10).await);
        assert!(!syncer.needs_sync(0).await);
    }
}