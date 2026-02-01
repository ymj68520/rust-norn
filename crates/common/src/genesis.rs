use crate::types::{Block, BlockHeader, Hash, GenesisParams, PublicKey};

/// 获取固定的创世块
///
/// 确保所有节点使用相同的创世块，这对于网络同步至关重要
pub fn get_genesis_block() -> Block {
    let header = BlockHeader {
        timestamp: GENESIS_TIMESTAMP,
        prev_block_hash: Hash::default(), // 创世块的前一个区块哈希为全零
        block_hash: GENESIS_BLOCK_HASH,
        merkle_root: Hash::default(),     // 没有交易，Merkle根为全零
        state_root: Hash::default(),      // 创世块状态根为全零
        height: 0,                        // 创世块高度为0
        public_key: PublicKey::default(),
        params: serialize_genesis_params(),
        gas_limit: GENESIS_GAS_LIMIT,
        base_fee: GENESIS_BASE_FEE,       // EIP-1559: 初始基础费用
    };

    Block {
        header,
        transactions: vec![], // 创世块不包含交易
    }
}

/// 创世块的固定哈希
/// 使用预计算的哈希值，确保所有节点一致
pub const GENESIS_BLOCK_HASH: Hash = Hash([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
]);

/// 创世时间戳 (Unix timestamp)
pub const GENESIS_TIMESTAMP: i64 = 1700000000; // 2023-11-14 00:53:20 UTC

/// 创世块的Gas限制
pub const GENESIS_GAS_LIMIT: i64 = 10_000_000;

/// 创世块的EIP-1559基础费用 (1 Gwei)
pub const GENESIS_BASE_FEE: u64 = 1_000_000_000;

/// 获取创世块参数
pub fn get_genesis_params() -> GenesisParams {
    GenesisParams {
        order: GENESIS_ORDER,
        time_param: GENESIS_TIME_PARAM,
        seed: GENESIS_SEED,
        verify_param: GENESIS_VERIFY_PARAM,
    }
}

/// 序列化创世块参数
fn serialize_genesis_params() -> Vec<u8> {
    let params = get_genesis_params();
    crate::utils::codec::serialize(&params).unwrap_or_default()
}

/// 创世块的VDF参数 - 大数阶（128字节）
pub const GENESIS_ORDER: [u8; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// 创世块的VDF时间参数
pub const GENESIS_TIME_PARAM: i64 = 10_000_000; // 10 million iterations

/// 创世块的VRF/VDF种子
pub const GENESIS_SEED: Hash = Hash([
    0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42,
    0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42,
    0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42,
    0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42,
]);

/// 创世块的VDF验证参数
pub const GENESIS_VERIFY_PARAM: Hash = Hash([
    0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43,
    0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43,
    0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43,
    0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43,
]);

/// 验证是否为有效的创世块
pub fn is_valid_genesis_block(block: &Block) -> bool {
    block.header.height == 0
        && block.header.prev_block_hash == Hash::default()
        && block.header.block_hash == GENESIS_BLOCK_HASH
        && block.transactions.is_empty()
}

/// 检查区块链是否从正确的创世块开始
pub async fn validate_genesis_start<F, Fut>(
    _db: &F,
    get_block: F,
) -> Result<bool, Box<dyn std::error::Error>>
where
    F: Fn(Hash) -> Fut,
    Fut: std::future::Future<Output = Result<Option<Block>, Box<dyn std::error::Error>>>,
{
    // 尝试获取高度为0的区块
    match get_block(GENESIS_BLOCK_HASH).await {
        Ok(Some(block)) => Ok(is_valid_genesis_block(&block)),
        Ok(None) => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_block_constants() {
        let genesis = get_genesis_block();

        assert_eq!(genesis.header.height, 0);
        assert_eq!(genesis.header.prev_block_hash, Hash::default());
        assert_eq!(genesis.header.block_hash, GENESIS_BLOCK_HASH);
        assert_eq!(genesis.header.timestamp, GENESIS_TIMESTAMP);
        assert_eq!(genesis.header.gas_limit, GENESIS_GAS_LIMIT);
        assert!(genesis.transactions.is_empty());
    }

    #[test]
    fn test_genesis_validation() {
        let valid_genesis = get_genesis_block();
        assert!(is_valid_genesis_block(&valid_genesis));

        let mut invalid_genesis = valid_genesis.clone();
        invalid_genesis.header.height = 1;
        assert!(!is_valid_genesis_block(&invalid_genesis));

        invalid_genesis = valid_genesis.clone();
        invalid_genesis.transactions.push(crate::types::Transaction::default());
        assert!(!is_valid_genesis_block(&invalid_genesis));
    }

    #[test]
    fn test_genesis_consistency() {
        let genesis1 = get_genesis_block();
        let genesis2 = get_genesis_block();

        // 两次调用应该返回相同的创世块
        assert_eq!(genesis1.header.block_hash, genesis2.header.block_hash);
        assert_eq!(genesis1.header.timestamp, genesis2.header.timestamp);
        assert_eq!(genesis1.header.params, genesis2.header.params);
    }
}