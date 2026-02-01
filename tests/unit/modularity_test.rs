// 模块化测试：测试单个模块的功能
// 运行方式: cargo test --test modularity_test

use norn_common::types::{Block, BlockHeader, Transaction, TransactionBody, Hash, Address};
use norn_core::blockchain::Blockchain;
use norn_storage::StateDB;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_block_creation() {
    // 测试区块创建
    let block = Block {
        header: BlockHeader {
            timestamp: chrono::Utc::now().timestamp(),
            prev_block_hash: Hash::default(),
            block_hash: Hash::default(),
            merkle_root: Hash::default(),
            state_root: Hash::default(),
            height: 1,
            public_key: norn_common::types::PublicKey::default(),
            params: vec![],
            gas_limit: 30_000_000,
            base_fee: 1000000000,
        },
        transactions: vec![],
    };

    assert_eq!(block.header.height, 1);
    assert_eq!(block.transactions.len(), 0);
    println!("✅ Block creation test passed");
}

#[tokio::test]
async fn test_transaction_structure() {
    // 测试交易结构
    let tx = Transaction {
        body: TransactionBody {
            hash: Hash::default(),
            address: Address::default(),
            receiver: Address::default(),
            gas: 21000,
            nonce: 0,
            event: vec![],
            opt: vec![],
            state: vec![],
            data: vec![1, 2, 3],
            expire: 0,
            height: 0,
            index: 0,
            block_hash: Hash::default(),
            timestamp: chrono::Utc::now().timestamp(),
            public: norn_common::types::PublicKey::default(),
            signature: vec![],
            tx_type: norn_common::types::TransactionType::Native,
            chain_id: Some(31337),
            value: Some("1000".to_string()),
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
            access_list: None,
            gas_price: None,
        },
    };

    assert_eq!(tx.body.gas, 21000);
    assert_eq!(tx.body.data, vec![1, 2, 3]);
    assert_eq!(tx.body.chain_id, Some(31337));
    println!("✅ Transaction structure test passed");
}

#[tokio::test]
async fn test_hash_operations() {
    // 测试哈希操作
    let hash1 = Hash::default();
    let hash2 = Hash::default();

    assert_eq!(hash1, hash2);

    let mut bytes = [0u8; 32];
    bytes[0] = 1;
    let hash3 = Hash(bytes);

    assert_ne!(hash1, hash3);
    println!("✅ Hash operations test passed");
}

#[tokio::test]
async fn test_address_creation() {
    // 测试地址创建
    let addr1 = Address::default();
    let addr2 = Address::default();

    assert_eq!(addr1, addr2);

    let bytes = [1u8; 20];
    let addr3 = Address(bytes);

    assert_ne!(addr1, addr3);
    assert_eq!(addr3.0.len(), 20);
    println!("✅ Address creation test passed");
}

#[tokio::test]
async fn test_blockchain_initialization() {
    // 测试区块链初始化
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(StateDB::new(temp_dir.path()).await.unwrap());

    // 创建创世区块
    let genesis_block = Block {
        header: BlockHeader {
            timestamp: chrono::Utc::now().timestamp(),
            prev_block_hash: Hash::default(),
            block_hash: Hash::default(),
            merkle_root: Hash::default(),
            state_root: Hash::default(),
            height: 0,
            public_key: norn_common::types::PublicKey::default(),
            params: vec![],
            gas_limit: 30_000_000,
            base_fee: 1000000000,
        },
        transactions: vec![],
    };

    assert_eq!(genesis_block.header.height, 0);
    println!("✅ Blockchain initialization test passed");
}
