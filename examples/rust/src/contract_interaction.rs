/// Smart Contract Interaction Example
/// 
/// This example demonstrates how to interact with smart contracts on the Norn blockchain:
/// - Encoding contract function calls using ABI
/// - Reading from contracts (eth_call)
/// - Calling contract functions (eth_sendRawTransaction)
/// - Decoding return values
/// - Handling ERC-20 tokens as a real-world example
/// 
/// Note: In production, use a library like ethers-rs or web3.rs for ABI encoding.
/// This example shows the concepts manually for educational purposes.

use reqwest::Client;
use serde_json::{json, Value};
use std::env;
use std::error::Error;

const ERC20_TRANSFER_SELECTOR: &str = "a9059cbb"; // keccak256("transfer(address,uint256)")[:4]
const ERC20_BALANCE_OF_SELECTOR: &str = "70a08231"; // keccak256("balanceOf(address)")[:4]

/// Represents an RPC client for contract interactions
struct ContractClient {
    rpc_url: String,
    client: Client,
}

impl ContractClient {
    /// Create a new contract client
    fn new(rpc_url: String) -> Self {
        ContractClient {
            rpc_url,
            client: Client::new(),
        }
    }

    /// Call a contract function (read-only, no gas cost)
    /// This uses eth_call for view/pure functions
    async fn call_function(
        &self,
        from: &str,
        contract_address: &str,
        data: &str,
    ) -> Result<String, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_call",
                "params": [
                    {
                        "from": from,
                        "to": contract_address,
                        "data": data
                    },
                    "latest"
                ],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            Ok(result.as_str().unwrap_or("").to_string())
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Get the balance of an ERC-20 token for an address
    async fn get_erc20_balance(
        &self,
        token_address: &str,
        account_address: &str,
    ) -> Result<String, Box<dyn Error>> {
        // Encode: balanceOf(address)
        // Selector: 0x70a08231
        // Pad address to 32 bytes (remove 0x and pad with zeros)
        let padded_address = format!("{:0>64}", account_address.trim_start_matches("0x"));
        let data = format!("0x70a08231{}", padded_address);

        let result = self.call_function("0x0000000000000000000000000000000000000000", token_address, &data).await?;

        // Result is returned as hex-encoded 256-bit integer
        Ok(result)
    }

    /// Encode a transfer call for an ERC-20 token
    /// Returns the encoded data to be used in a transaction
    fn encode_erc20_transfer(
        &self,
        recipient: &str,
        amount_wei: &str,
    ) -> Result<String, Box<dyn Error>> {
        // Selector for transfer(address,uint256)
        let mut data = String::from("0xa9059cbb");

        // Encode recipient address (pad to 32 bytes)
        let padded_recipient = format!("{:0>64}", recipient.trim_start_matches("0x"));
        data.push_str(&padded_recipient);

        // Encode amount (pad to 32 bytes)
        // Convert amount to hex and pad
        let amount_int = u128::from_str_radix(amount_wei, 10)
            .unwrap_or(0);
        let amount_hex = format!("{:0>64x}", amount_int);
        data.push_str(&amount_hex);

        Ok(data)
    }

    /// Simulate reading contract storage (eth_getStorageAt)
    async fn get_storage_at(
        &self,
        contract_address: &str,
        position: &str,
    ) -> Result<String, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_getStorageAt",
                "params": [contract_address, position, "latest"],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            Ok(result.as_str().unwrap_or("").to_string())
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Get the code of a contract (verify if address is a contract)
    async fn get_code(
        &self,
        contract_address: &str,
    ) -> Result<String, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_getCode",
                "params": [contract_address, "latest"],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            Ok(result.as_str().unwrap_or("").to_string())
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Decode uint256 from hex string (big-endian)
    fn decode_uint256(&self, hex_str: &str) -> u128 {
        let cleaned = hex_str.trim_start_matches("0x");
        u128::from_str_radix(cleaned, 16).unwrap_or(0)
    }

    /// Format wei to a readable token amount (assuming 18 decimals like ETH/most ERC-20s)
    fn format_token_amount(&self, wei: u128, decimals: u32) -> f64 {
        let divisor = 10_f64.powi(decimals as i32);
        wei as f64 / divisor
    }
}

/// Main example demonstrating contract interactions
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    let rpc_url = env::var("RPC_URL")
        .unwrap_or_else(|_| "http://localhost:8545".to_string());

    let client = ContractClient::new(rpc_url);

    println!("=== Smart Contract Interaction Examples ===\n");

    // Example 1: Verify if an address is a contract
    let example_contract = "0x0000000000000000000000000000000000000001"; // System contract
    println!("1. Checking if address is a contract:");
    println!("   Address: {}", example_contract);
    match client.get_code(example_contract).await {
        Ok(code) => {
            if code == "0x" {
                println!("   Result: Not a contract (EOA or empty)");
            } else {
                println!("   Result: Contract found (code length: {} bytes)", (code.len() - 2) / 2);
            }
        }
        Err(e) => println!("   Error: {}", e),
    }

    // Example 2: Encode ERC-20 transfer call
    println!("\n2. Encoding ERC-20 transfer call:");
    let recipient = "0x742d35Cc6634C0532925a3b844Bc9e7595f32D23";
    let amount_wei = "1000000000000000000"; // 1 token (assuming 18 decimals)
    
    match client.encode_erc20_transfer(recipient, amount_wei) {
        Ok(encoded_data) => {
            println!("   Recipient: {}", recipient);
            println!("   Amount: {} wei", amount_wei);
            println!("   Encoded data: {}", encoded_data);
            println!("   (This data would be used in eth_sendRawTransaction)");
        }
        Err(e) => println!("   Error: {}", e),
    }

    // Example 3: Demonstrate storage access pattern
    println!("\n3. Contract Storage Access Pattern:");
    println!("   To read contract state, use eth_getStorageAt");
    println!("   - Position 0: Often total supply for ERC-20");
    println!("   - Position 1: Often owner address");
    println!("   - Position 2+: Depends on contract design");
    println!("   Storage slots are 32 bytes (256 bits)");

    // Example 4: Educational explanation of ABI encoding
    println!("\n4. Understanding ABI Encoding:");
    println!("   For function: transfer(address to, uint256 amount)");
    println!("   - Selector (first 4 bytes): keccak256('transfer(address,uint256)')[0:4]");
    println!("   - Parameter 1 (address): Padded to 32 bytes");
    println!("   - Parameter 2 (uint256): Padded to 32 bytes");
    println!("   - Total: 4 + 32 + 32 = 68 bytes (136 hex chars)");

    // Example 5: Common contract addresses format
    println!("\n5. Working with Contract Addresses:");
    let test_addresses = vec![
        ("EOA Example", "0x742d35Cc6634C0532925a3b844Bc9e7595f32D23"),
        ("Contract Example", "0x0000000000000000000000000000000000000001"),
        ("Zero Address", "0x0000000000000000000000000000000000000000"),
    ];
    
    for (name, addr) in test_addresses {
        println!("   {}: {}", name, addr);
    }

    println!("\n=== Key Points ===");
    println!("✓ Use eth_call for read-only contract calls (no gas, no state changes)");
    println!("✓ Use eth_sendRawTransaction for state-changing calls (costs gas)");
    println!("✓ ABI encoding is deterministic - same function call always produces same data");
    println!("✓ Always verify you're calling the correct contract address");
    println!("✓ In production, use libraries (ethers-rs, web3.rs) for complex ABI encoding");

    Ok(())
}
