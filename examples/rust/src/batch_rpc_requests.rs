/// Batch RPC Requests Example
/// 
/// This example demonstrates how to efficiently batch multiple RPC calls
/// into a single request, which is much faster than making separate requests.
/// 
/// Benefits of batching:
/// - Single HTTP round trip instead of multiple
/// - Better performance for sequential operations
/// - Atomicity for reading state at the same block height
/// - Reduced latency in network calls
///
/// Use cases:
/// - Getting balances for multiple addresses
/// - Fetching multiple blocks' data
/// - Reading multiple contract states
/// - Pre-flight checks before transaction submission

use reqwest::Client;
use serde_json::{json, Value};
use std::env;
use std::error::Error;

/// Represents an RPC client capable of batch requests
struct BatchRpcClient {
    rpc_url: String,
    client: Client,
}

impl BatchRpcClient {
    /// Create a new batch RPC client
    fn new(rpc_url: String) -> Self {
        BatchRpcClient {
            rpc_url,
            client: Client::new(),
        }
    }

    /// Execute a batch of RPC requests
    /// Returns results in the same order as the requests
    async fn batch_request(&self, requests: Vec<Value>) -> Result<Vec<Value>, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&requests)
            .send()
            .await?;

        let results: Vec<Value> = response.json().await?;
        Ok(results)
    }

    /// Batch request to get balances for multiple addresses
    async fn get_balances_batch(
        &self,
        addresses: Vec<&str>,
    ) -> Result<Vec<(String, String)>, Box<dyn Error>> {
        let mut requests = Vec::new();

        for (index, address) in addresses.iter().enumerate() {
            requests.push(json!({
                "jsonrpc": "2.0",
                "method": "eth_getBalance",
                "params": [address, "latest"],
                "id": index + 1
            }));
        }

        let responses = self.batch_request(requests).await?;

        let mut balances = Vec::new();
        for (index, address) in addresses.iter().enumerate() {
            let response = &responses[index];
            if let Some(result) = response.get("result") {
                let balance = result.as_str().unwrap_or("0x0").to_string();
                balances.push((address.to_string(), balance));
            }
        }

        Ok(balances)
    }

    /// Batch request to get block details for multiple block numbers
    async fn get_blocks_batch(
        &self,
        block_numbers: Vec<&str>,
    ) -> Result<Vec<Value>, Box<dyn Error>> {
        let mut requests = Vec::new();

        for (index, block_num) in block_numbers.iter().enumerate() {
            requests.push(json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [block_num, false],
                "id": index + 1
            }));
        }

        let responses = self.batch_request(requests).await?;

        let mut blocks = Vec::new();
        for response in responses {
            if let Some(result) = response.get("result") {
                blocks.push(result.clone());
            }
        }

        Ok(blocks)
    }

    /// Batch request to get transaction details for multiple hashes
    async fn get_transactions_batch(
        &self,
        tx_hashes: Vec<&str>,
    ) -> Result<Vec<Value>, Box<dyn Error>> {
        let mut requests = Vec::new();

        for (index, tx_hash) in tx_hashes.iter().enumerate() {
            requests.push(json!({
                "jsonrpc": "2.0",
                "method": "eth_getTransactionByHash",
                "params": [tx_hash],
                "id": index + 1
            }));
        }

        let responses = self.batch_request(requests).await?;

        let mut transactions = Vec::new();
        for response in responses {
            if let Some(result) = response.get("result") {
                transactions.push(result.clone());
            }
        }

        Ok(transactions)
    }

    /// Batch request to check multiple storage slots
    async fn get_storage_batch(
        &self,
        contract_address: &str,
        positions: Vec<&str>,
    ) -> Result<Vec<String>, Box<dyn Error>> {
        let mut requests = Vec::new();

        for (index, position) in positions.iter().enumerate() {
            requests.push(json!({
                "jsonrpc": "2.0",
                "method": "eth_getStorageAt",
                "params": [contract_address, position, "latest"],
                "id": index + 1
            }));
        }

        let responses = self.batch_request(requests).await?;

        let mut storage_values = Vec::new();
        for response in responses {
            if let Some(result) = response.get("result") {
                let value = result.as_str().unwrap_or("0x0").to_string();
                storage_values.push(value);
            }
        }

        Ok(storage_values)
    }

    /// Mixed batch request - combines different RPC methods
    async fn mixed_batch_request(
        &self,
        chain_id_only: bool,
        get_gas_price: bool,
        block_number_only: bool,
    ) -> Result<Value, Box<dyn Error>> {
        let mut requests = Vec::new();
        let mut request_id = 1;

        if chain_id_only {
            requests.push(json!({
                "jsonrpc": "2.0",
                "method": "eth_chainId",
                "params": [],
                "id": request_id
            }));
            request_id += 1;
        }

        if get_gas_price {
            requests.push(json!({
                "jsonrpc": "2.0",
                "method": "eth_gasPrice",
                "params": [],
                "id": request_id
            }));
            request_id += 1;
        }

        if block_number_only {
            requests.push(json!({
                "jsonrpc": "2.0",
                "method": "eth_blockNumber",
                "params": [],
                "id": request_id
            }));
            request_id += 1;
        }

        let responses = self.batch_request(requests).await?;

        Ok(json!({
            "responses": responses
        }))
    }

    /// Convert hex string to decimal
    fn hex_to_decimal(&self, hex_str: &str) -> u64 {
        u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).unwrap_or(0)
    }

    /// Format wei to ether
    fn wei_to_ether(&self, wei_hex: &str) -> f64 {
        let wei = u128::from_str_radix(wei_hex.trim_start_matches("0x"), 16).unwrap_or(0);
        wei as f64 / 1e18
    }
}

/// Main example demonstrating batch RPC requests
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    let rpc_url = env::var("RPC_URL")
        .unwrap_or_else(|_| "http://localhost:8545".to_string());

    let client = BatchRpcClient::new(rpc_url);

    println!("=== Batch RPC Requests Examples ===\n");

    // Example 1: Batch balance queries
    println!("1. Batch Balance Queries:");
    println!("   Querying balances for multiple addresses in one request...");
    let addresses = vec![
        "0x742d35Cc6634C0532925a3b844Bc9e7595f32D23",
        "0x0000000000000000000000000000000000000000",
        "0x1111111111111111111111111111111111111111",
    ];

    match client.get_balances_batch(addresses.clone()).await {
        Ok(balances) => {
            println!("   Results:");
            for (address, balance) in balances {
                let balance_decimal = client.hex_to_decimal(&balance);
                let balance_ether = client.wei_to_ether(&balance);
                println!(
                    "   {} -> {} Wei ({} ETH)",
                    address, balance_decimal, balance_ether
                );
            }
        }
        Err(e) => println!("   Error: {}", e),
    }

    // Example 2: Batch block queries
    println!("\n2. Batch Block Queries:");
    println!("   Fetching multiple blocks in one request...");
    let block_numbers = vec!["0x1", "0x2", "0x3"];

    match client.get_blocks_batch(block_numbers).await {
        Ok(blocks) => {
            println!("   Fetched {} blocks successfully", blocks.len());
            for block in blocks {
                if let Some(number) = block.get("number") {
                    if let Some(miner) = block.get("miner") {
                        println!("   Block {}: miner {}", number, miner);
                    }
                }
            }
        }
        Err(e) => println!("   Error: {}", e),
    }

    // Example 3: Batch storage queries
    println!("\n3. Batch Storage Queries:");
    println!("   Reading multiple storage slots from a contract...");
    let contract = "0x0000000000000000000000000000000000000001";
    let positions = vec!["0x0", "0x1", "0x2"];

    match client.get_storage_batch(contract, positions).await {
        Ok(values) => {
            println!("   Results from contract storage:");
            for (idx, value) in values.iter().enumerate() {
                println!("   Position {}: {}", idx, value);
            }
        }
        Err(e) => println!("   Error: {}", e),
    }

    // Example 4: Mixed batch request
    println!("\n4. Mixed Batch Request:");
    println!("   Combining different RPC methods in one batch...");

    match client.mixed_batch_request(true, true, true).await {
        Ok(result) => {
            println!("   Results:");
            if let Some(responses) = result.get("responses").and_then(|r| r.as_array()) {
                for response in responses {
                    if let Some(result_val) = response.get("result") {
                        println!("   {}", result_val);
                    }
                }
            }
        }
        Err(e) => println!("   Error: {}", e),
    }

    // Example 5: Educational information
    println!("\n5. Batch Request Performance Benefits:");
    println!("   ✓ Single HTTP connection for multiple calls");
    println!("   ✓ Results atomic at the same block height");
    println!("   ✓ Reduced round-trip latency");
    println!("   ✓ Better for reading multiple state snapshots");
    println!("   ✓ Can batch up to 100+ requests (depends on node)");

    println!("\n6. Batch Request Patterns:");
    println!("   Pattern 1: Get state before transaction");
    println!("     - Query nonce, gas price, balances all at once");
    println!("   Pattern 2: Multi-address monitoring");
    println!("     - Check balances/nonces for multiple accounts");
    println!("   Pattern 3: Contract audit");
    println!("     - Read multiple storage slots at once");
    println!("   Pattern 4: Historical data fetching");
    println!("     - Get multiple blocks' data in parallel");

    println!("\n=== Key Points ===");
    println!("✓ Batch requests must be sent as JSON array, not individual objects");
    println!("✓ Results are returned in the same order as requests");
    println!("✓ Each request in the batch must have unique 'id'");
    println!("✓ All requests in a batch are executed at the same block height");
    println!("✓ Error in one request doesn't affect others in the batch");

    Ok(())
}
