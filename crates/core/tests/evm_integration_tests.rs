//! EVM Integration Tests
//!
//! Comprehensive tests for EVM functionality including:
//! - ABI encoding/decoding
//! - Smart contract deployment
//! - Contract execution
//! - Event emission
//! - State root calculation

use norn_core::evm::*;
use norn_core::state::{AccountStateManager, AccountState, AccountStateConfig, AccountType};
use norn_common::types::{Address, Hash, Transaction, TransactionBody, TransactionType};
use num_bigint::BigUint;
use std::sync::Arc;

/// Helper to create a test address
fn test_address(byte: u8) -> Address {
    Address([byte; 20])
}

/// Helper to create a test hash
fn test_hash(byte: u8) -> Hash {
    Hash([byte; 32])
}

#[tokio::test]
async fn test_abi_function_encoding() {
    // Test encoding a transfer function call
    // transfer(address,uint256)

    let to = test_address(1);
    let amount = ABIParam::new(ABIValue::Uint(1_000_000_000_000_000_000, 256)); // 1 ETH

    let encoded = ABI::encode_function_call(
        "transfer(address,uint256)",
        &[
            ABIParam::new(ABIValue::Address(to)),
            amount,
        ],
    ).unwrap();

    // Should have 4-byte selector + parameters
    assert!(encoded.len() >= 68); // 4 + 32 + 32
}

#[tokio::test]
async fn test_abi_event_encoding() {
    // Test encoding a Transfer event
    // event Transfer(address indexed from, address indexed to, uint256 value)

    let from = test_address(1);
    let to = test_address(2);
    let value = ABIParam::new(ABIValue::Uint(500, 256));

    let (topics, data) = ABI::encode_event(
        "Transfer(address,address,uint256)",
        &[
            ABIParam::new(ABIValue::Address(from)),
            ABIParam::new(ABIValue::Address(to)),
        ],
        &[value],
    ).unwrap();

    // Should have 3 topics (signature + 2 indexed params)
    assert_eq!(topics.len(), 3);

    // Should have data for non-indexed params
    assert!(!data.is_empty());
}

#[tokio::test]
async fn test_abi_parse_human_readable() {
    let _hrabi = HumanReadableABI::new();

    // Test function parsing
    let func = "function balanceOf(address owner) view returns (uint256)";
    let item = HumanReadableABI::parse_item(func).unwrap();

    match item {
        ABIItem::Function { name, inputs, outputs } => {
            assert_eq!(name, "balanceOf");
            assert_eq!(inputs.len(), 1);
            assert_eq!(outputs.len(), 1);
        }
        _ => panic!("Expected function"),
    }

    // Test event parsing
    let event = "event Transfer(address indexed from, address indexed to, uint256 value)";
    let item = HumanReadableABI::parse_item(event).unwrap();

    match item {
        ABIItem::Event { name, inputs } => {
            assert_eq!(name, "Transfer");
            assert_eq!(inputs.len(), 3);
            assert!(inputs[0].indexed);
            assert!(inputs[1].indexed);
            assert!(!inputs[2].indexed);
        }
        _ => panic!("Expected event"),
    }
}

#[tokio::test]
async fn test_evm_executor_creation() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();

    let executor = EVMExecutor::new(state_manager, config);

    assert_eq!(executor.config().chain_id, 31337);
    assert_eq!(executor.config().max_contract_size, 24_576);
}

#[tokio::test]
async fn test_simple_eth_transfer() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    // Setup accounts
    let from = test_address(1);
    let to = test_address(2);

    state_manager.update_balance(&from, BigUint::from(2_000_000_000_000_000_000u128)).await.unwrap();
    state_manager.update_balance(&to, BigUint::from(0u128)).await.unwrap();

    // Create transfer transaction
    let tx = Transaction {
        body: TransactionBody {
            hash: Hash::default(),
            address: from,
            receiver: to,
            gas: 21_000,
            nonce: 0,
            value: Some("1000000000000000000".to_string()), // 1 ETH
            data: Vec::new(),
            tx_type: TransactionType::EVM,
            ..Default::default()
        },
    };

    let ctx = EVMContext::default();
    let result = executor.execute(&tx, &ctx).await.unwrap();

    assert!(result.success);
    assert_eq!(result.gas_used, 21_000);
    assert!(result.logs.is_empty());

    // Verify balances
    let from_balance = state_manager.get_balance(&from).await.unwrap();
    let to_balance = state_manager.get_balance(&to).await.unwrap();

    assert_eq!(from_balance, BigUint::from(1_000_000_000_000_000_000u128)); // 1 ETH remaining
    assert_eq!(to_balance, BigUint::from(1_000_000_000_000_000_000u128)); // 1 ETH received
}

#[tokio::test]
async fn test_contract_deployment() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    let deployer = test_address(1);
    let init_code = vec![0x60, 0x60, 0x60]; // Simple bytecode

    // Deploy contract
    let (contract_address, result) = executor.create_contract(
        deployer,
        0, // nonce
        init_code.clone(),
        0, // value
        100_000, // gas limit
    ).await.unwrap();

    assert!(result.success);
    assert!(executor.code_storage().is_contract(&contract_address).await);

    let stored_code = executor.code_storage().get_code_by_address(&contract_address).await.unwrap();
    assert_eq!(stored_code, Some(init_code));
}

#[tokio::test]
async fn test_eip170_contract_size_limit() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default(); // 24KB limit
    let executor = EVMExecutor::new(state_manager, config);

    let deployer = test_address(1);

    // Contract at size limit should succeed
    let valid_code = vec![0x60; 24_576];
    let result = executor.create_contract(
        deployer,
        0,
        valid_code,
        0,
        100_000,
    ).await;

    assert!(result.is_ok());

    // Contract exceeding limit should fail
    let oversized_code = vec![0x60; 24_577];
    let result = executor.create_contract(
        deployer,
        1,
        oversized_code,
        0,
        100_000,
    ).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_event_log_emission() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    let contract = test_address(1);
    let topic0 = test_hash(10);
    let topic1 = test_hash(11);
    let data = vec![0x01, 0x02, 0x03];

    // Emit LOG3
    executor.emit_log3(contract, topic0, topic1, test_hash(12), data.clone()).await.unwrap();

    let logs = executor.get_logs().await;

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].address, contract);
    assert_eq!(logs[0].topics.len(), 3);
    assert_eq!(logs[0].topics[0], topic0);
    assert_eq!(logs[0].topics[1], topic1);
    assert_eq!(logs[0].data, data);
}

#[tokio::test]
async fn test_transaction_receipt() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    let from = test_address(1);
    let tx_hash = test_hash(1);
    let block_hash = test_hash(2);

    // Emit a log
    executor.emit_log1(from, test_hash(10), vec![0x01]).await.unwrap();

    // Create execution result
    let exec_result = EVMExecutionResult {
        success: true,
        gas_used: 21_000,
        output: vec![0x02],
        error: None,
        logs: vec![],
    };

    // Create receipt
    let receipt = executor.create_receipt(
        tx_hash,
        block_hash,
        100,
        0,
        from,
        None,
        &exec_result,
        None,
        21_000,
    ).await;

    assert_eq!(receipt.tx_hash, tx_hash);
    assert_eq!(receipt.block_number, 100);
    assert_eq!(receipt.gas_used, 21_000);
    assert_eq!(receipt.status, true);
    assert_eq!(receipt.logs.len(), 1);

    // Store and retrieve receipt
    executor.receipt_db().put_receipt(receipt.clone()).await.unwrap();

    let retrieved = executor.receipt_db().get_receipt(&tx_hash).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().tx_hash, tx_hash);
}

#[tokio::test]
async fn test_gas_estimation() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    // Simple ETH transfer
    let mut tx = create_test_transaction();
    let gas = executor.estimate_gas(&tx).await.unwrap();
    assert_eq!(gas, 21_000);

    // Contract creation
    tx.body.data = vec![0x60, 0x60];
    tx.body.receiver = Address::default();
    let gas = executor.estimate_gas(&tx).await.unwrap();
    assert_eq!(gas, 53_000);

    // Contract call
    tx.body.receiver = test_address(5);
    let gas = executor.estimate_gas(&tx).await.unwrap();
    assert_eq!(gas, 26_000 + 2 * 16);
}

#[tokio::test]
async fn test_contract_call() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    // First deploy a contract
    let deployer = test_address(1);
    let init_code = vec![0x60, 0x60, 0x60];

    // Set up deployer account with sufficient balance for gas
    state_manager.update_balance(&deployer, BigUint::from(100_000_000_000_000u128)).await.unwrap();

    let (contract_address, _) = executor.create_contract(
        deployer,
        0,
        init_code.clone(),
        0,
        100_000,
    ).await.unwrap();

    // Now call the contract
    let caller = test_address(2);
    let input_data = vec![0x01, 0x02, 0x03];

    // Set up caller account with sufficient balance for gas and value transfer
    state_manager.update_balance(&caller, BigUint::from(100_000_000_000_000u128)).await.unwrap();

    let result = executor.call_contract(
        caller,
        contract_address,
        100, // value
        input_data,
        50_000, // gas limit
    ).await.unwrap();

    assert!(result.success);
}

#[tokio::test]
async fn test_static_call() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    // Deploy contract
    let deployer = test_address(1);
    let init_code = vec![0x60, 0x60];

    let (contract_address, _) = executor.create_contract(
        deployer,
        0,
        init_code,
        0,
        100_000,
    ).await.unwrap();

    // Static call
    let caller = test_address(2);
    let input_data = vec![0x01];

    let result = executor.static_call(
        caller,
        contract_address,
        input_data,
        50_000,
    ).await.unwrap();

    assert!(result.success);
}

#[tokio::test]
async fn test_delegate_call() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    // Deploy contract with code
    let deployer = test_address(1);
    let init_code = vec![0x60, 0x60, 0x60];

    let (code_address, _) = executor.create_contract(
        deployer,
        0,
        init_code,
        0,
        100_000,
    ).await.unwrap();

    // Delegate call
    let caller = test_address(2);
    let input_data = vec![0x01, 0x02, 0x03, 0x04];

    let result = executor.delegate_call(
        caller,
        code_address,
        input_data,
        50_000,
    ).await.unwrap();

    assert!(result.success);
}

#[tokio::test]
async fn test_bloom_filter() {
    let mut bloom = Bloom::new();

    let address = test_address(1);
    let topic = test_hash(10);

    bloom.add_address(&address);
    bloom.add_topic(&topic);

    // The bloom filter should contain the added values
    // (may have false positives, but no false negatives)
    assert!(bloom.might_contain(&address.0));
}

#[tokio::test]
async fn test_receipt_filtering() {
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(state_manager.clone(), config);

    let address1 = test_address(1);
    let address2 = test_address(2);
    let block_hash = test_hash(10);

    // Create receipts with logs
    let receipt1 = Receipt::new(test_hash(1), block_hash, 100, 0)
        .with_log(ReceiptLog {
            log_index: 0,
            tx_hash: test_hash(1),
            block_hash,
            block_number: 100,
            address: address1,
            topics: vec![test_hash(10)],
            data: vec![0x01],
        });

    let receipt2 = Receipt::new(test_hash(2), block_hash, 100, 1)
        .with_log(ReceiptLog {
            log_index: 0,
            tx_hash: test_hash(2),
            block_hash,
            block_number: 100,
            address: address2,
            topics: vec![test_hash(11)],
            data: vec![0x02],
        });

    executor.receipt_db().put_receipt(receipt1).await.unwrap();
    executor.receipt_db().put_receipt(receipt2).await.unwrap();

    // Filter by address
    let filtered = executor.receipt_db().get_receipts_by_address(&address1).await.unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].logs[0].address, address1);

    // Filter by topic
    let filtered = executor.receipt_db().get_receipts_by_topic(&test_hash(10)).await.unwrap();
    assert_eq!(filtered.len(), 1);
}

#[tokio::test]
async fn test_state_root_calculation() {
    use norn_core::state::merkle::StateRootCalculator;

    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let calculator = StateRootCalculator::default();

    // Add some accounts
    for i in 1u64..=3 {
        let address = Address([i as u8; 20]);
        let account = AccountState {
            address,
            balance: BigUint::from(i * 1000u64),
            nonce: i,
            account_type: AccountType::Normal,
            code_hash: None,
            storage_root: Hash::default(),
            created_at: 0,
            updated_at: 0,
            deleted: false,
        };

        state_manager.set_account(&address, account).await.unwrap();
    }

    let root = calculator.calculate_from_manager(&state_manager).await.unwrap();

    // State root should be non-zero
    assert_ne!(root, Hash::default());

    // Modify an account
    let address = Address([1u8; 20]);
    state_manager.update_balance(&address, BigUint::from(9999u128)).await.unwrap();

    let root2 = calculator.calculate_from_manager(&state_manager).await.unwrap();

    // State root should change
    assert_ne!(root, root2);
}

// Helper function to create a test transaction
fn create_test_transaction() -> Transaction {
    Transaction {
        body: TransactionBody {
            hash: Hash::default(),
            address: test_address(1),
            receiver: test_address(2),
            gas: 100_000,
            nonce: 0,
            event: Vec::new(),
            opt: Vec::new(),
            state: Vec::new(),
            data: Vec::new(),
            expire: 0,
            height: 0,
            index: 0,
            block_hash: Hash::default(),
            timestamp: 0,
            public: Default::default(),
            signature: Vec::new(),
            tx_type: TransactionType::EVM,
            chain_id: Some(31337),
            value: Some("1000000000000000000".to_string()),
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
            access_list: None,
            gas_price: None,
        },
    }
}
