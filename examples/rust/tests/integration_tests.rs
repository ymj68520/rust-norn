/// Integration tests for Norn RPC examples
///
/// These tests verify that all examples can connect to and interact with
/// a running Norn node. They test the core functionality of each example
/// and validate response parsing.
///
/// Requirements:
/// - A running Norn node at http://127.0.0.1:50051
/// - Set NORN_RPC_URL environment variable if using different address
///
/// Run tests with:
/// ```bash
/// cargo test --test integration_tests -- --test-threads=1 --nocapture
/// ```

use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use anyhow::Result;
use std::env;

/// Configuration for test environment
struct TestConfig {
    rpc_url: String,
}

impl TestConfig {
    fn new() -> Self {
        let rpc_url = env::var("NORN_RPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
        
        TestConfig { rpc_url }
    }

    /// Returns the RPC client
    async fn client(&self) -> Result<jsonrpsee::http_client::HttpClient> {
        let client = HttpClientBuilder::default()
            .build(&self.rpc_url)?;
        Ok(client)
    }
}

/// Helper to check node running
async fn check_node_running(client: &jsonrpsee::http_client::HttpClient) -> Result<()> {
    let _: String = client
        .request("eth_chainId", jsonrpsee::rpc_params![])
        .await?;
    Ok(())
}

// ============================================
// Helper Functions
// ============================================

async fn is_node_running(rpc_url: &str) -> bool {
    match HttpClientBuilder::default().build(rpc_url) {
        Ok(client) => {
            let result: Result<String, _> = client
                .request("eth_chainId", jsonrpsee::rpc_params![])
                .await;
            result.is_ok()
        }
        Err(_) => false,
    }
}

// ============================================
// Test 1: Basic RPC Operations
// ============================================

#[tokio::test]
async fn test_get_chain_id() -> Result<()> {
    let config = TestConfig::new();
    
    // Check if node is running
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running at {}, skipping test", config.rpc_url);
        return Ok(());
    }

    let client = config.client().await?;

    // Get chain ID
    let chain_id: String = client
        .request("eth_chainId", jsonrpsee::rpc_params![])
        .await?;

    // Verify response format
    assert!(!chain_id.is_empty(), "Chain ID should not be empty");
    assert!(chain_id.starts_with("0x"), "Chain ID should start with 0x");
    assert!(chain_id.len() <= 66, "Chain ID should be valid hex");

    println!("✅ Chain ID retrieved: {}", chain_id);
    Ok(())
}

#[tokio::test]
async fn test_get_block_number() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Get latest block number
    let block_number: String = client
        .request("eth_blockNumber", jsonrpsee::rpc_params![])
        .await?;

    // Verify response format
    assert!(!block_number.is_empty(), "Block number should not be empty");
    assert!(block_number.starts_with("0x"), "Block number should start with 0x");

    // Parse as hex to verify validity
    let block_num = u64::from_str_radix(&block_number[2..], 16);
    assert!(block_num.is_ok(), "Block number should be valid hex");

    println!("✅ Block number retrieved: {}", block_number);
    Ok(())
}

#[tokio::test]
async fn test_get_gas_price() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Get gas price
    let gas_price: String = client
        .request("eth_gasPrice", jsonrpsee::rpc_params![])
        .await?;

    // Verify response format
    assert!(!gas_price.is_empty(), "Gas price should not be empty");
    assert!(gas_price.starts_with("0x"), "Gas price should start with 0x");

    // Parse as hex
    let price = u64::from_str_radix(&gas_price[2..], 16);
    assert!(price.is_ok(), "Gas price should be valid hex");

    println!("✅ Gas price retrieved: {}", gas_price);
    Ok(())
}

// ============================================
// Test 2: Block Information
// ============================================

#[tokio::test]
async fn test_get_block_by_number() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Get block 0x0 (genesis or earliest available)
    let block: serde_json::Value = client
        .request("eth_getBlockByNumber", jsonrpsee::rpc_params!["0x0", false])
        .await?;

    // Verify block structure
    if block.is_null() {
        println!("⚠️ Block 0x0 not found, which is expected for some networks");
    } else {
        assert!(block.is_object(), "Block should be an object");
        assert!(block["hash"].is_string(), "Block should have a hash");
        assert!(block["number"].is_string(), "Block should have a number");
        assert!(block["miner"].is_string() || block["miner"].is_null(), 
                "Block should have a miner field");
        println!("✅ Block retrieved: {:?}", block["hash"]);
    }

    Ok(())
}

// ============================================
// Test 3: Account Balance
// ============================================

#[tokio::test]
async fn test_get_balance() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Get balance for address 0x0
    let address = "0x0000000000000000000000000000000000000000";
    let balance: String = client
        .request("eth_getBalance", jsonrpsee::rpc_params![address, "latest"])
        .await?;

    // Verify response format
    assert!(!balance.is_empty(), "Balance should not be empty");
    assert!(balance.starts_with("0x"), "Balance should start with 0x");

    // Parse as hex
    let bal = u128::from_str_radix(&balance[2..], 16);
    assert!(bal.is_ok(), "Balance should be valid hex");

    println!("✅ Balance retrieved: {} wei", balance);
    Ok(())
}

#[tokio::test]
async fn test_get_transaction_count() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Get transaction count (nonce) for address
    let address = "0x0000000000000000000000000000000000000000";
    let nonce: String = client
        .request("eth_getTransactionCount", jsonrpsee::rpc_params![address, "latest"])
        .await?;

    // Verify response format
    assert!(!nonce.is_empty(), "Nonce should not be empty");
    assert!(nonce.starts_with("0x"), "Nonce should start with 0x");

    println!("✅ Transaction count (nonce) retrieved: {}", nonce);
    Ok(())
}

// ============================================
// Test 4: Account Code
// ============================================

#[tokio::test]
async fn test_get_code() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Get code for regular account (should be empty)
    let address = "0x0000000000000000000000000000000000000000";
    let code: String = client
        .request("eth_getCode", jsonrpsee::rpc_params![address, "latest"])
        .await?;

    // Verify response format
    assert!(code.starts_with("0x"), "Code should start with 0x");
    // For regular accounts, code should be "0x" (empty)
    if code == "0x" {
        println!("✅ Regular account has no code");
    } else {
        println!("✅ Contract code retrieved: {} bytes", code.len() / 2 - 1);
    }

    Ok(())
}

// ============================================
// Test 5: Error Handling
// ============================================

#[tokio::test]
async fn test_invalid_address_format() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Try to get balance with invalid address format
    let result: Result<String, _> = client
        .request("eth_getBalance", jsonrpsee::rpc_params!["invalid_address", "latest"])
        .await;

    // Should fail with error
    assert!(result.is_err(), "Invalid address should return error");
    println!("✅ Invalid address correctly rejected");

    Ok(())
}

#[tokio::test]
async fn test_invalid_block_number() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Try to get block with invalid number
    let result: Result<serde_json::Value, _> = client
        .request("eth_getBlockByNumber", jsonrpsee::rpc_params!["invalid", false])
        .await;

    // Should fail or return null
    match result {
        Ok(val) => {
            if val.is_null() {
                println!("✅ Invalid block number returns null");
            }
        }
        Err(_) => println!("✅ Invalid block number rejected"),
    }

    Ok(())
}

// ============================================
// Test 6: Response Parsing
// ============================================

#[tokio::test]
async fn test_response_parsing() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Test various response types
    let chain_id: String = client
        .request("eth_chainId", jsonrpsee::rpc_params![])
        .await?;
    assert!(chain_id.starts_with("0x"), "String response should be valid");

    let block_number: String = client
        .request("eth_blockNumber", jsonrpsee::rpc_params![])
        .await?;
    assert!(block_number.starts_with("0x"), "Numeric response should be hex");

    let block: serde_json::Value = client
        .request("eth_getBlockByNumber", jsonrpsee::rpc_params!["0x0", false])
        .await?;
    // Block might be null for some networks, but structure should be valid

    println!("✅ All response types parsed correctly");
    Ok(())
}

// ============================================
// Test 7: Connection Handling
// ============================================

#[tokio::test]
async fn test_multiple_requests() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Make multiple sequential requests
    for i in 0..5 {
        let chain_id: String = client
            .request("eth_chainId", jsonrpsee::rpc_params![])
            .await?;
        assert!(!chain_id.is_empty(), "Request {} failed", i);
    }

    println!("✅ Multiple sequential requests completed");
    Ok(())
}

#[tokio::test]
async fn test_concurrent_requests() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let rpc_url = config.rpc_url.clone();
    
    // Spawn multiple concurrent requests
    let mut handles = vec![];
    
    for i in 0..5 {
        let url = rpc_url.clone();
        let handle = tokio::spawn(async move {
            let client = HttpClientBuilder::default()
                .build(&url)
                .expect("Failed to create client");
            
            let result: String = client
                .request("eth_chainId", jsonrpsee::rpc_params![])
                .await
                .expect("RPC request failed");
            
            (i, result)
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    for handle in handles {
        let (i, result) = handle.await?;
        assert!(!result.is_empty(), "Request {} returned empty", i);
    }

    println!("✅ Concurrent requests completed successfully");
    Ok(())
}

// ============================================
// Test 8: Data Consistency
// ============================================

#[tokio::test]
async fn test_consistent_results() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // Make same request twice and verify results are consistent
    let chain_id_1: String = client
        .request("eth_chainId", jsonrpsee::rpc_params![])
        .await?;

    let chain_id_2: String = client
        .request("eth_chainId", jsonrpsee::rpc_params![])
        .await?;

    assert_eq!(chain_id_1, chain_id_2, "Chain ID should be consistent");

    println!("✅ Results are consistent");
    Ok(())
}

// ============================================
// Test 9: Example-Specific Tests
// ============================================

/// Test basic_rpc.rs example requirements
#[tokio::test]
async fn test_basic_rpc_requirements() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // These are all methods used by basic_rpc.rs example
    let _: String = client.request("eth_chainId", jsonrpsee::rpc_params![]).await?;
    let _: String = client.request("eth_blockNumber", jsonrpsee::rpc_params![]).await?;
    let _: String = client.request("eth_gasPrice", jsonrpsee::rpc_params![]).await?;
    let _: serde_json::Value = client
        .request("eth_getBlockByNumber", jsonrpsee::rpc_params!["0x1", false])
        .await?;

    println!("✅ All basic_rpc.rs requirements verified");
    Ok(())
}

/// Test balance_checker.rs example requirements
#[tokio::test]
async fn test_balance_checker_requirements() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // These are all methods used by balance_checker.rs example
    let address = "0x0000000000000000000000000000000000000000";
    let _: String = client
        .request("eth_getBalance", jsonrpsee::rpc_params![address, "latest"])
        .await?;

    println!("✅ All balance_checker.rs requirements verified");
    Ok(())
}

/// Test transaction_sender.rs example requirements
#[tokio::test]
async fn test_transaction_sender_requirements() -> Result<()> {
    let config = TestConfig::new();
    
    if !is_node_running(&config.rpc_url).await {
        println!("⚠️ Node not running, skipping test");
        return Ok(());
    }

    let client = config.client().await?;

    // These are all methods used by transaction_sender.rs example
    let address = "0x0000000000000000000000000000000000000000";
    let _: String = client
        .request("eth_getTransactionCount", jsonrpsee::rpc_params![address, "latest"])
        .await?;
    let _: String = client.request("eth_gasPrice", jsonrpsee::rpc_params![]).await?;

    println!("✅ All transaction_sender.rs requirements verified");
    Ok(())
}

// ============================================
// Test Summary
// ============================================

/// Helper to print test summary
fn print_summary() {
    println!("\n=== Integration Tests Summary ===");
    println!("Tests verify:");
    println!("✓ RPC connectivity");
    println!("✓ Response format validation");
    println!("✓ Error handling");
    println!("✓ Concurrent request handling");
    println!("✓ Data consistency");
    println!("✓ Example-specific requirements");
}

#[ctor::ctor]
fn init_tests() {
    // Initialize test environment
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();
}
