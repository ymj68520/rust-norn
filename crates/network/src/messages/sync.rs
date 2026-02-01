use norn_common::types::{Block, BlockHeader, Hash, Transaction};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Re-export compression utilities
use crate::compression::{Compressor, CompressionConfig, CompressionAlgorithm};

/// 网络消息类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NetworkMessage {
    /// 基础消息
    Basic(BasicMessage),
    
    /// 同步消息
    Sync(SyncMessage),
    
    /// 共识消息
    Consensus(ConsensusMessage),
    
    /// 交易消息
    Transaction(TransactionMessage),
    
    /// 状态消息
    State(StateMessage),
}

/// 基础消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BasicMessage {
    /// 握手
    Handshake(HandshakeMessage),
    
    /// Ping
    Ping(PingMessage),
    
    /// Pong
    Pong(PongMessage),
    
    /// 断开连接
    Disconnect(DisconnectMessage),
}

/// 同步消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SyncMessage {
    /// 区块请求
    BlockRequest(BlockRequestMessage),
    
    /// 区块响应
    BlockResponse(BlockResponseMessage),
    
    /// 区块头请求
    HeaderRequest(HeaderRequestMessage),
    
    /// 区块头响应
    HeaderResponse(HeaderResponseMessage),
    
    /// 同步状态
    SyncStatus(SyncStatusMessage),
    
    /// 链请求
    ChainRequest(ChainRequestMessage),
    
    /// 链响应
    ChainResponse(ChainResponseMessage),
}

/// 共识消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConsensusMessage {
    /// 区块提议
    BlockProposal(BlockProposalMessage),
    
    /// 投票
    Vote(VoteMessage),
    
    /// VDF 完成
    VDFComplete(VDFCompleteMessage),
    
    /// 共识状态
    ConsensusStatus(ConsensusStatusMessage),
}

/// 交易消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionMessage {
    /// 交易广播
    TransactionBroadcast(TransactionBroadcastMessage),
    
    /// 交易请求
    TransactionRequest(TransactionRequestMessage),
    
    /// 交易响应
    TransactionResponse(TransactionResponseMessage),
    
    /// 交易池状态
    TransactionPoolStatus(TransactionPoolStatusMessage),
}

/// 状态消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StateMessage {
    /// 状态请求
    StateRequest(StateRequestMessage),
    
    /// 状态响应
    StateResponse(StateResponseMessage),
    
    /// 账户状态
    AccountState(AccountStateMessage),
    
    /// 存储状态
    StorageState(StorageStateMessage),
}

/// 握手消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandshakeMessage {
    /// 协议版本
    pub version: String,
    
    /// 节点 ID
    pub node_id: Vec<u8>,
    
    /// 支持的功能
    pub capabilities: Vec<String>,
    
    /// 当前高度
    pub height: u64,
    
    /// 最新区块哈希
    pub latest_hash: Hash,
    
    /// 时间戳
    pub timestamp: u64,
}

/// Ping 消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PingMessage {
    /// 随机数
    pub nonce: u64,
    
    /// 时间戳
    pub timestamp: u64,
}

/// Pong 消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PongMessage {
    /// 对应的 ping nonce
    pub nonce: u64,
    
    /// 时间戳
    pub timestamp: u64,
}

/// 断开连接消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisconnectMessage {
    /// 断开原因
    pub reason: DisconnectReason,
    
    /// 消息
    pub message: Option<String>,
}

/// 断开原因
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DisconnectReason {
    /// 正常断开
    Normal,
    
    /// 协议错误
    ProtocolError,
    
    /// 超时
    Timeout,
    
    /// 被拒绝
    Rejected,
    
    /// 其他
    Other(String),
}

/// 区块请求消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockRequestMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 起始高度
    pub start_height: u64,
    
    /// 结束高度
    pub end_height: u64,
    
    /// 最大区块数
    pub max_blocks: u32,
    
    /// 是否包含交易
    pub include_transactions: bool,
    
    /// 哈希列表（可选）
    pub hashes: Option<Vec<Hash>>,
}

/// 区块响应消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockResponseMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 区块列表
    pub blocks: Vec<Block>,
    
    /// 起始高度
    pub start_height: u64,
    
    /// 是否有更多
    pub has_more: bool,
    
    /// 总区块数
    pub total_blocks: u64,
}

/// 区块头请求消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeaderRequestMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 起始高度
    pub start_height: u64,
    
    /// 结束高度
    pub end_height: u64,
    
    /// 最大头数
    pub max_headers: u32,
    
    /// 哈希列表（可选）
    pub hashes: Option<Vec<Hash>>,
}

/// 区块头响应消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeaderResponseMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 区块头列表
    pub headers: Vec<BlockHeader>,
    
    /// 起始高度
    pub start_height: u64,
    
    /// 是否有更多
    pub has_more: bool,
    
    /// 总头数
    pub total_headers: u64,
}

/// 同步状态消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncStatusMessage {
    /// 当前高度
    pub current_height: u64,
    
    /// 最新哈希
    pub latest_hash: Hash,
    
    /// 同步状态
    pub sync_state: String,
    
    /// 目标高度
    pub target_height: Option<u64>,
    
    /// 已下载区块数
    pub downloaded_blocks: u64,
    
    /// 已验证区块数
    pub verified_blocks: u64,
    
    /// 估算剩余时间
    pub estimated_time_remaining: Option<u64>,
}

/// 链请求消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChainRequestMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 起始哈希
    pub start_hash: Hash,
    
    /// 最大长度
    pub max_length: u32,
    
    /// 是否包含交易
    pub include_transactions: bool,
}

/// 链响应消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChainResponseMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 区块链
    pub blocks: Vec<Block>,
    
    /// 起始哈希
    pub start_hash: Hash,
    
    /// 是否有更多
    pub has_more: bool,
    
    /// 总长度
    pub total_length: u64,
}

/// 区块提议消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockProposalMessage {
    /// 提议者
    pub proposer: Vec<u8>,
    
    /// 区块
    pub block: Block,
    
    /// VRF 输出
    pub vrf_output: Vec<u8>,
    
    /// 轮次
    pub round: u64,
    
    /// 签名
    pub signature: Vec<u8>,
}

/// 投票消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoteMessage {
    /// 投票者
    pub voter: Vec<u8>,
    
    /// 区块哈希
    pub block_hash: Hash,
    
    /// 轮次
    pub round: u64,
    
    /// 投票类型
    pub vote_type: String,
    
    /// 签名
    pub signature: Vec<u8>,
}

/// VDF 完成消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VDFCompleteMessage {
    /// 区块哈希
    pub block_hash: Hash,
    
    /// VDF 输出
    pub vdf_output: Vec<u8>,
    
    /// 轮次
    pub round: u64,
    
    /// 证明
    pub proof: Vec<u8>,
}

/// 共识状态消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsensusStatusMessage {
    /// 当前轮次
    pub current_round: u64,
    
    /// 共识状态
    pub consensus_state: String,
    
    /// 当前高度
    pub current_height: u64,
    
    /// 验证者数量
    pub validator_count: u32,
    
    /// 活跃验证者数量
    pub active_validators: u32,
}

/// 交易广播消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransactionBroadcastMessage {
    /// 交易
    pub transaction: Transaction,
    
    /// 来源节点
    pub source_node: Option<Vec<u8>>,
    
    /// 广播时间
    pub timestamp: u64,
}

/// 交易请求消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransactionRequestMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 交易哈希
    pub tx_hash: Hash,
}

/// 交易响应消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransactionResponseMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 交易
    pub transaction: Option<Transaction>,
    
    /// 是否找到
    pub found: bool,
}

/// 交易池状态消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransactionPoolStatusMessage {
    /// 池中交易数量
    pub pool_size: u64,
    
    /// 待处理交易数量
    pub pending_count: u64,
    
    /// 已验证交易数量
    pub verified_count: u64,
    
    /// 池容量
    pub pool_capacity: u64,
}

/// 状态请求消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateRequestMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 状态哈希
    pub state_hash: Hash,
    
    /// 请求类型
    pub request_type: StateRequestType,
}

/// 状态请求类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StateRequestType {
    /// 完整状态
    Full,
    
    /// 账户状态
    Account(Vec<u8>),
    
    /// 存储状态
    Storage(Vec<u8>, Vec<u8>),
    
    /// 证明
    Proof(Vec<u8>),
}

/// 状态响应消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateResponseMessage {
    /// 请求 ID
    pub request_id: u64,
    
    /// 状态数据
    pub state_data: Vec<u8>,
    
    /// 状态哈希
    pub state_hash: Hash,
    
    /// 是否找到
    pub found: bool,
}

/// 账户状态消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountStateMessage {
    /// 账户地址
    pub address: Vec<u8>,
    
    /// 账户数据
    pub account_data: Vec<u8>,
    
    /// 状态哈希
    pub state_hash: Hash,
    
    /// 证明
    pub proof: Option<Vec<u8>>,
}

/// 存储状态消息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageStateMessage {
    /// 账户地址
    pub address: Vec<u8>,
    
    /// 存储键
    pub storage_key: Vec<u8>,
    
    /// 存储值
    pub storage_value: Vec<u8>,
    
    /// 状态哈希
    pub state_hash: Hash,
    
    /// 证明
    pub proof: Option<Vec<u8>>,
}

/// 网络消息配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMessageConfig {
    /// 最大消息大小
    pub max_message_size: usize,
    
    /// 消息超时时间
    pub message_timeout: Duration,
    
    /// 重试次数
    pub max_retries: u32,
    
    /// 批量大小
    pub batch_size: usize,
    
    /// 压缩阈值
    pub compression_threshold: usize,
}

impl Default for NetworkMessageConfig {
    fn default() -> Self {
        Self {
            max_message_size: 10 * 1024 * 1024, // 10MB
            message_timeout: Duration::from_secs(30),
            max_retries: 3,
            batch_size: 100,
            compression_threshold: 1024, // 1KB
        }
    }
}

/// 消息编码器
pub struct MessageEncoder {
    config: NetworkMessageConfig,
    compressor: Compressor,
}

// Compression magic bytes: [0xFF, 0xCF, ALGORITHM]
// 0xFF 0xCF identifies this as compressed data
// ALGORITHM: 0x00 = None, 0x01 = Zstd, 0x02 = Snappy
const COMPRESSED_MAGIC_PREFIX: &[u8] = &[0xFF, 0xCF];

impl MessageEncoder {
    /// 创建新的消息编码器
    pub fn new(config: NetworkMessageConfig) -> Self {
        let compressor = Compressor::with_config(crate::compression::CompressionConfig {
            algorithm: crate::compression::CompressionAlgorithm::Zstd,
            level: crate::compression::CompressionLevel::Default,
            min_size: config.compression_threshold,
            adaptive: true,
        });
        Self { config, compressor }
    }

    /// 创建带有自定义压缩器的编码器
    pub fn with_compressor(config: NetworkMessageConfig, compressor: Compressor) -> Self {
        Self { config, compressor }
    }

    /// 编码消息
    pub fn encode(&self, message: &NetworkMessage) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = serde_json::to_vec(message)?;
        
        // 检查消息大小
        if data.len() > self.config.max_message_size {
            return Err("Message too large".into());
        }

        // 压缩大消息
        let final_data = if data.len() > self.config.compression_threshold {
            self.compress(&data)?
        } else {
            data
        };

        Ok(final_data)
    }

    /// 解码消息
    pub fn decode(&self, data: &[u8]) -> Result<NetworkMessage, Box<dyn std::error::Error>> {
        // 尝试解压缩
        let decompressed_data = self.try_decompress(data)?;
        
        serde_json::from_slice(&decompressed_data).map_err(Into::into)
    }

    /// 压缩数据
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Skip compression if data is too small (Compressor handles this internally)
        let compressed = self.compressor.compress(data)?;

        // Add compression magic prefix
        // Format: [0xFF, 0xCF, ALGORITHM, ...compressed_data...]
        let algorithm_byte = match self.compressor.config().algorithm {
            crate::compression::CompressionAlgorithm::None => 0x00u8,
            crate::compression::CompressionAlgorithm::Zstd => 0x01u8,
            crate::compression::CompressionAlgorithm::Snappy => 0x02u8,
        };

        let mut result = Vec::with_capacity(3 + compressed.len());
        result.extend_from_slice(COMPRESSED_MAGIC_PREFIX);
        result.push(algorithm_byte);
        result.extend_from_slice(&compressed);

        Ok(result)
    }

    /// 尝试解压缩
    fn try_decompress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Check if data has compression magic prefix
        if data.len() < 3 || !data.starts_with(COMPRESSED_MAGIC_PREFIX) {
            // Data is not compressed
            return Ok(data.to_vec());
        }

        // Extract algorithm byte
        let algorithm_byte = data[2];
        let compressed_data = &data[3..];

        let algorithm = match algorithm_byte {
            0x00 => crate::compression::CompressionAlgorithm::None,
            0x01 => crate::compression::CompressionAlgorithm::Zstd,
            0x02 => crate::compression::CompressionAlgorithm::Snappy,
            _ => return Err(format!("Unknown compression algorithm: {}", algorithm_byte).into()),
        };

        // Decompress using the appropriate algorithm
        self.compressor.decompress(compressed_data, algorithm)
            .map_err(|e| format!("Decompression failed: {:?}", e).into())
    }
}

/// 消息验证器
pub struct MessageValidator {
    config: NetworkMessageConfig,
}

impl MessageValidator {
    /// 创建新的消息验证器
    pub fn new(config: NetworkMessageConfig) -> Self {
        Self { config }
    }

    /// 验证消息
    pub fn validate(&self, message: &NetworkMessage) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            NetworkMessage::Basic(basic) => self.validate_basic_message(basic)?,
            NetworkMessage::Sync(sync) => self.validate_sync_message(sync)?,
            NetworkMessage::Consensus(consensus) => self.validate_consensus_message(consensus)?,
            NetworkMessage::Transaction(tx) => self.validate_transaction_message(tx)?,
            NetworkMessage::State(state) => self.validate_state_message(state)?,
        }
        Ok(())
    }

    /// 验证基础消息
    fn validate_basic_message(&self, message: &BasicMessage) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            BasicMessage::Handshake(handshake) => {
                if handshake.version.is_empty() {
                    return Err("Empty version".into());
                }
                if handshake.node_id.is_empty() {
                    return Err("Empty node ID".into());
                }
            }
            BasicMessage::Ping(ping) => {
                if ping.timestamp == 0 {
                    return Err("Invalid timestamp".into());
                }
            }
            BasicMessage::Pong(pong) => {
                if pong.timestamp == 0 {
                    return Err("Invalid timestamp".into());
                }
            }
            BasicMessage::Disconnect(_) => {
                // 断开连接消息总是有效
            }
        }
        Ok(())
    }

    /// 验证同步消息
    fn validate_sync_message(&self, message: &SyncMessage) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            SyncMessage::BlockRequest(req) => {
                if req.start_height > req.end_height {
                    return Err("Invalid height range".into());
                }
                if req.max_blocks == 0 {
                    return Err("Invalid max blocks".into());
                }
            }
            SyncMessage::BlockResponse(resp) => {
                if resp.blocks.is_empty() && resp.has_more {
                    return Err("Empty response with more flag".into());
                }
            }
            SyncMessage::HeaderRequest(req) => {
                if req.start_height > req.end_height {
                    return Err("Invalid height range".into());
                }
                if req.max_headers == 0 {
                    return Err("Invalid max headers".into());
                }
            }
            SyncMessage::HeaderResponse(resp) => {
                if resp.headers.is_empty() && resp.has_more {
                    return Err("Empty response with more flag".into());
                }
            }
            _ => {
                // 其他同步消息的验证
            }
        }
        Ok(())
    }

    /// 验证共识消息
    fn validate_consensus_message(&self, message: &ConsensusMessage) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            ConsensusMessage::BlockProposal(proposal) => {
                if proposal.proposer.is_empty() {
                    return Err("Empty proposer".into());
                }
                if proposal.signature.is_empty() {
                    return Err("Empty signature".into());
                }
            }
            ConsensusMessage::Vote(vote) => {
                if vote.voter.is_empty() {
                    return Err("Empty voter".into());
                }
                if vote.signature.is_empty() {
                    return Err("Empty signature".into());
                }
            }
            _ => {
                // 其他共识消息的验证
            }
        }
        Ok(())
    }

    /// 验证交易消息
    fn validate_transaction_message(&self, message: &TransactionMessage) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            TransactionMessage::TransactionBroadcast(broadcast) => {
                if broadcast.transaction.body.signature.is_empty() {
                    return Err("Empty transaction signature".into());
                }
            }
            TransactionMessage::TransactionRequest(_) => {
                // 交易请求总是有效
            }
            TransactionMessage::TransactionResponse(_) => {
                // 交易响应总是有效
            }
            TransactionMessage::TransactionPoolStatus(_) => {
                // 交易池状态总是有效
            }
        }
        Ok(())
    }

    /// 验证状态消息
    fn validate_state_message(&self, message: &StateMessage) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            StateMessage::StateRequest(req) => {
                if req.state_hash == Hash::default() {
                    return Err("Invalid state hash".into());
                }
            }
            StateMessage::StateResponse(resp) => {
                if resp.state_hash == Hash::default() && resp.found {
                    return Err("Invalid state hash for found response".into());
                }
            }
            StateMessage::AccountState(account) => {
                if account.address.is_empty() {
                    return Err("Empty account address".into());
                }
            }
            StateMessage::StorageState(storage) => {
                if storage.address.is_empty() {
                    return Err("Empty account address".into());
                }
                if storage.storage_key.is_empty() {
                    return Err("Empty storage key".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_encoding_decoding() {
        let config = NetworkMessageConfig::default();
        let encoder = MessageEncoder::new(config.clone());
        
        let message = NetworkMessage::Basic(BasicMessage::Ping(PingMessage {
            nonce: 12345,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }));

        let encoded = encoder.encode(&message).unwrap();
        let decoded = encoder.decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_message_validation() {
        let config = NetworkMessageConfig::default();
        let validator = MessageValidator::new(config);
        
        // 测试有效消息
        let valid_message = NetworkMessage::Basic(BasicMessage::Ping(PingMessage {
            nonce: 12345,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }));
        
        assert!(validator.validate(&valid_message).is_ok());

        // 测试无效消息
        let invalid_message = NetworkMessage::Basic(BasicMessage::Ping(PingMessage {
            nonce: 12345,
            timestamp: 0, // 无效时间戳
        }));
        
        assert!(validator.validate(&invalid_message).is_err());
    }

    #[test]
    fn test_block_request_message() {
        let message = SyncMessage::BlockRequest(BlockRequestMessage {
            request_id: 1,
            start_height: 100,
            end_height: 200,
            max_blocks: 50,
            include_transactions: true,
            hashes: None,
        });

        assert_eq!(message, message);
    }

    #[test]
    fn test_handshake_message() {
        let message = BasicMessage::Handshake(HandshakeMessage {
            version: "1.0.0".to_string(),
            node_id: vec![1, 2, 3, 4],
            capabilities: vec!["sync".to_string(), "consensus".to_string()],
            height: 100,
            latest_hash: Hash([1u8; 32]),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        });

        assert_eq!(message, message);
    }
}

/// Compression utilities for network messages
///
/// This module provides helper functions to compress and decompress
/// network messages, reducing bandwidth usage during synchronization.
impl NetworkMessage {
    /// Compress the message if beneficial
    pub fn compress(&self) -> anyhow::Result<CompressedMessage> {
        // Serialize the message
        let serialized = bincode::serialize(self)?;

        // Create compression config with adaptive compression
        let config = CompressionConfig {
            algorithm: CompressionAlgorithm::Zstd,
            level: crate::compression::CompressionLevel::Default,
            min_size: 256,
            adaptive: true,
        };

        CompressedMessage::compress(&serialized, &config)
    }

    /// Decompress a compressed message
    pub fn decompress(compressed: &CompressedMessage) -> anyhow::Result<Self> {
        let data = compressed.decompress()?;
        let message: Self = bincode::deserialize(&data)?;
        Ok(message)
    }
}

// Re-export CompressedMessage
pub use super::compression::CompressedMessage;