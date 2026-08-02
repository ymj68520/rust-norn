//! DeFi Vault Smart Contract Integration Test for Norn EVM (revm)
//! 
//! Tests deployment and execution of a multi-feature Solidity contract (`DeFiVault.sol`)
//! featuring ERC-20 token accounting, Synthetix-style staking rewards, and
//! constant-product AMM swaps (x * y = k with 0.3% fees).

use norn_core::evm::{EVMExecutor, EVMConfig, EVMContext};
use norn_core::state::account::{AccountStateManager, AccountStateConfig};
use norn_common::types::Address;
use std::sync::Arc;
use std::fs;
use std::path::Path;
use num_bigint::BigUint;
use tiny_keccak::{Hasher, Keccak};

/// Helper function to compute 4-byte EVM function selector
fn function_selector(signature: &str) -> Vec<u8> {
    let mut keccak = Keccak::v256();
    keccak.update(signature.as_bytes());
    let mut res = [0u8; 32];
    keccak.finalize(&mut res);
    res[0..4].to_vec()
}

/// Helper function to encode uint256 parameter into 32-byte EVM word
fn encode_uint256(val: u128) -> Vec<u8> {
    let mut word = vec![0u8; 32];
    let bytes = val.to_be_bytes();
    word[32 - bytes.len()..].copy_from_slice(&bytes);
    word
}

/// Helper function to encode bool parameter into 32-byte EVM word
fn encode_bool(b: bool) -> Vec<u8> {
    let mut word = vec![0u8; 32];
    if b {
        word[31] = 1;
    }
    word
}

#[tokio::test]
async fn test_defi_vault_solidity_contract_execution() {
    println!("=== Testing DeFiVault Solidity Smart Contract on revm ===");

    // 1. Load Compiled Bytecode from solc Artifact
    let bin_path = if Path::new("tests/contracts/build/DeFiVault.bin").exists() {
        Path::new("tests/contracts/build/DeFiVault.bin").to_path_buf()
    } else {
        Path::new("d:/Programs/blockchain/rust-norn/tests/contracts/build/DeFiVault.bin").to_path_buf()
    };
    let hex_bytecode = fs::read_to_string(bin_path)
        .expect("DeFiVault.bin artifact missing. Ensure solc compiled tests/contracts/DeFiVault.sol");
    let init_bytecode = hex::decode(hex_bytecode.trim())
        .expect("Failed to hex-decode contract bytecode");

    println!("  📄 Loaded DeFiVault Init Bytecode: {} bytes", init_bytecode.len());

    // 2. Initialize EVM State Manager & Executor
    let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
    let config = EVMConfig::default();
    let executor = EVMExecutor::new(Arc::clone(&state_manager), config);

    // Fund Deployer Account
    let deployer = Address([0x11u8; 20]);
    let initial_balance = BigUint::from(1_000_000_000_000_000_000_000_000u128); // 1,000,000 ETH
    state_manager.update_balance(&deployer, initial_balance).await.unwrap();

    let ctx = EVMContext::default();

    // 3. Deploy DeFiVault Contract (constructor argument: initialSupply = 1,000,000 * 1e18)
    let initial_supply = 1_000_000u128 * 1_000_000_000_000_000_000u128;
    let mut deployment_data = init_bytecode.clone();
    deployment_data.extend_from_slice(&encode_uint256(initial_supply));

    let (contract_address, deploy_result) = executor.create_contract(
        deployer,
        0,    // nonce = 0
        deployment_data,
        0,    // 0 ETH value
        5_000_000, // 5M Gas Limit
    ).await.expect("DeFiVault Contract deployment failed");

    assert!(deploy_result.success, "Contract deployment execution reported failure");

    println!("  ✅ DeFiVault Deployed Successfully to Address: {:?}", contract_address);
    println!("     Gas Used for Deployment: {}", deploy_result.gas_used);

    // 4. Test Call 1: addLiquidity(100_000, 200_000)
    let mut add_liq_calldata = function_selector("addLiquidity(uint256,uint256)");
    add_liq_calldata.extend_from_slice(&encode_uint256(100_000));
    add_liq_calldata.extend_from_slice(&encode_uint256(200_000));

    let add_liq_result = executor.execute_with_revm(
        deployer,
        Some(contract_address),
        0,
        add_liq_calldata,
        500_000,
        &ctx,
    ).await.expect("addLiquidity call execution failed");

    assert!(add_liq_result.success, "addLiquidity failed");
    println!("  ✅ Liquidity Added: Reserve A = 100,000, Reserve B = 200,000 (Gas: {})", add_liq_result.gas_used);

    // 5. Test Call 2: swap(1_000, true) -> Swap 1,000 Token A for Token B (0.3% fee)
    let mut swap_calldata = function_selector("swap(uint256,bool)");
    swap_calldata.extend_from_slice(&encode_uint256(1_000));
    swap_calldata.extend_from_slice(&encode_bool(true));

    let swap_result = executor.execute_with_revm(
        deployer,
        Some(contract_address),
        0,
        swap_calldata,
        500_000,
        &ctx,
    ).await.expect("swap call execution failed");

    assert!(swap_result.success, "AMM Swap execution failed");
    println!("  ✅ Constant-Product AMM Swap Executed (1,000 Token A -> Token B) (Gas: {})", swap_result.gas_used);

    // 6. Test Call 3: stake(10,000)
    let mut stake_calldata = function_selector("stake(uint256)");
    stake_calldata.extend_from_slice(&encode_uint256(10_000));

    let stake_result = executor.execute_with_revm(
        deployer,
        Some(contract_address),
        0,
        stake_calldata,
        500_000,
        &ctx,
    ).await.expect("stake call execution failed");

    assert!(stake_result.success, "Staking 10,000 tokens failed");
    println!("  ✅ Staking 10,000 Tokens Successful (Gas: {})", stake_result.gas_used);

    // 7. Test Call 4: withdrawStaked(4,000)
    let mut withdraw_calldata = function_selector("withdrawStaked(uint256)");
    withdraw_calldata.extend_from_slice(&encode_uint256(4_000));

    let withdraw_result = executor.execute_with_revm(
        deployer,
        Some(contract_address),
        0,
        withdraw_calldata,
        500_000,
        &ctx,
    ).await.expect("withdrawStaked call execution failed");

    assert!(withdraw_result.success, "Withdrawing 4,000 staked tokens failed");
    println!("  ✅ Withdrew 4,000 Staked Tokens Successful (Gas: {})", withdraw_result.gas_used);

    println!("🎉 All DeFiVault Smart Contract EVM Integration Tests PASSED!");
}
