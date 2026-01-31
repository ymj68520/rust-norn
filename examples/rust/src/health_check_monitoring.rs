/// Health Check and Monitoring Example
/// 
/// This example demonstrates how to monitor the health of a blockchain node
/// and implement various health checks:
/// 
/// Health Checks:
/// - Node connectivity (can we reach the RPC endpoint?)
/// - Chain synchronization (is the node syncing?)
/// - Latest block info (what's the current height?)
/// - Gas price trends (how does gas vary?)
/// - Peer count (how many peers are connected?)
/// - Transaction pool size (how many pending transactions?)
///
/// Monitoring patterns:
/// - Periodic health checks
/// - Alert triggers for anomalies
/// - Performance metrics collection
/// - Network condition tracking

use reqwest::Client;
use serde_json::{json, Value};
use std::env;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

/// Health status of the node
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub is_connected: bool,
    pub is_syncing: bool,
    pub latest_block: u64,
    pub latest_block_time: u64,
    pub gas_price: String,
    pub peer_count: usize,
    pub pending_transactions: usize,
    pub network_id: String,
    pub client_version: String,
    pub timestamp: u64,
}

/// RPC client with health check capabilities
struct MonitoringClient {
    rpc_url: String,
    client: Client,
}

impl MonitoringClient {
    /// Create a new monitoring client
    fn new(rpc_url: String) -> Self {
        MonitoringClient {
            rpc_url,
            client: Client::new(),
        }
    }

    /// Basic connectivity check
    async fn check_connectivity(&self) -> bool {
        match self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "web3_clientVersion",
                "params": [],
                "id": 1
            }))
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    /// Check if node is syncing
    async fn check_sync_status(&self) -> Result<Value, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_syncing",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            Ok(result.clone())
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Get latest block number
    async fn get_latest_block(&self) -> Result<u64, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_blockNumber",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            let hex_str = result.as_str().unwrap_or("0x0");
            let block_num = u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)?;
            Ok(block_num)
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Get block timestamp
    async fn get_block_timestamp(&self, block_num: u64) -> Result<u64, Box<dyn Error>> {
        let block_hex = format!("0x{:x}", block_num);
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [&block_hex, false],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            if let Some(timestamp) = result.get("timestamp") {
                let hex_str = timestamp.as_str().unwrap_or("0x0");
                let ts = u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)?;
                return Ok(ts);
            }
        }

        Ok(0)
    }

    /// Get current gas price
    async fn get_gas_price(&self) -> Result<String, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_gasPrice",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            Ok(result.as_str().unwrap_or("0x0").to_string())
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Get number of peers connected
    async fn get_peer_count(&self) -> Result<usize, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "net_peerCount",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            let hex_str = result.as_str().unwrap_or("0x0");
            let count = usize::from_str_radix(hex_str.trim_start_matches("0x"), 16)?;
            Ok(count)
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Get pending transactions count
    async fn get_pending_tx_count(&self) -> Result<usize, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": ["pending", false],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            if let Some(txs) = result.get("transactions") {
                if let Some(tx_array) = txs.as_array() {
                    return Ok(tx_array.len());
                }
            }
        }

        Ok(0)
    }

    /// Get network ID
    async fn get_network_id(&self) -> Result<String, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "net_version",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            Ok(result.as_str().unwrap_or("unknown").to_string())
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Get client version
    async fn get_client_version(&self) -> Result<String, Box<dyn Error>> {
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "web3_clientVersion",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        let body: Value = response.json().await?;

        if let Some(result) = body.get("result") {
            Ok(result.as_str().unwrap_or("unknown").to_string())
        } else if let Some(error) = body.get("error") {
            Err(format!("RPC Error: {}", error).into())
        } else {
            Err("Unexpected response format".into())
        }
    }

    /// Perform comprehensive health check
    async fn perform_health_check(&self) -> Result<HealthStatus, Box<dyn Error>> {
        let is_connected = self.check_connectivity().await;

        let is_syncing = match self.check_sync_status().await {
            Ok(Value::Bool(false)) => false,
            _ => true,
        };

        let latest_block = self.get_latest_block().await.unwrap_or(0);
        let latest_block_time = self.get_block_timestamp(latest_block).await.unwrap_or(0);
        let gas_price = self.get_gas_price().await.unwrap_or_else(|_| "0x0".to_string());
        let peer_count = self.get_peer_count().await.unwrap_or(0);
        let pending_transactions = self.get_pending_tx_count().await.unwrap_or(0);
        let network_id = self.get_network_id().await.unwrap_or_else(|_| "unknown".to_string());
        let client_version = self.get_client_version().await.unwrap_or_else(|_| "unknown".to_string());

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        Ok(HealthStatus {
            is_connected,
            is_syncing,
            latest_block,
            latest_block_time,
            gas_price,
            peer_count,
            pending_transactions,
            network_id,
            client_version,
            timestamp: now,
        })
    }

    /// Convert hex gas price to gwei
    fn gas_price_to_gwei(&self, gas_price_hex: &str) -> f64 {
        let wei = u128::from_str_radix(gas_price_hex.trim_start_matches("0x"), 16).unwrap_or(0);
        wei as f64 / 1e9
    }
}

/// Main example demonstrating health checks and monitoring
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    let rpc_url = env::var("RPC_URL")
        .unwrap_or_else(|_| "http://localhost:8545".to_string());

    let client = MonitoringClient::new(rpc_url);

    println!("=== Health Check and Monitoring Examples ===\n");

    // Perform health check
    println!("Performing health check...\n");

    match client.perform_health_check().await {
        Ok(health) => {
            println!("=== Health Status ===");
            println!("Connected: {}", if health.is_connected { "✓ YES" } else { "✗ NO" });
            println!("Syncing: {}", if health.is_syncing { "⚠ YES" } else { "✓ NO" });
            println!("Latest Block: {}", health.latest_block);
            println!("Block Time: {} ({})", health.latest_block_time, health.timestamp);
            println!("Gas Price: {} ({:.2} Gwei)", health.gas_price, 
                     client.gas_price_to_gwei(&health.gas_price));
            println!("Peers Connected: {}", health.peer_count);
            println!("Pending Transactions: {}", health.pending_transactions);
            println!("Network ID: {}", health.network_id);
            println!("Client Version: {}", health.client_version);
            println!("Timestamp: {}", health.timestamp);

            // Health indicators
            println!("\n=== Health Indicators ===");
            if health.is_connected {
                println!("✓ Node is reachable");
            } else {
                println!("✗ Node is unreachable - cannot connect to RPC endpoint");
            }

            if !health.is_syncing && health.is_connected {
                println!("✓ Node is fully synced");
            } else if health.is_syncing {
                println!("⚠ Node is syncing - may have delayed data");
            }

            if health.peer_count > 0 {
                println!("✓ Node has {} peer(s) connected", health.peer_count);
            } else {
                println!("✗ No peers connected - node may be isolated");
            }

            if health.pending_transactions > 0 {
                println!("ℹ {} transactions in mempool", health.pending_transactions);
            }
        }
        Err(e) => println!("Health check failed: {}", e),
    }

    // Monitoring examples
    println!("\n=== Monitoring Patterns ===");

    println!("\n1. Periodic Health Checks:");
    println!("   - Check connectivity every 30 seconds");
    println!("   - Alert if node becomes unreachable");
    println!("   - Track syncing status changes");

    println!("\n2. Performance Metrics:");
    println!("   - Track block time trends");
    println!("   - Monitor gas price variations");
    println!("   - Count peer connections over time");

    println!("\n3. Anomaly Detection:");
    println!("   - Alert if block time > 30 seconds");
    println!("   - Alert if gas price spikes > 2x baseline");
    println!("   - Alert if peer count drops to 0");

    println!("\n4. Threshold-based Alerts:");
    println!("   - Warning: peer_count < 3");
    println!("   - Critical: peer_count == 0");
    println!("   - Warning: pending_transactions > 1000");

    println!("\n=== Recommended Health Check Intervals ===");
    println!("   - Basic connectivity: Every 10-30 seconds");
    println!("   - Full health check: Every 1-5 minutes");
    println!("   - Historical metrics: Every 1 hour (collect aggregates)");

    Ok(())
}
