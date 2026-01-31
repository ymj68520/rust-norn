use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use anyhow::{Result, Context};
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let rpc_url = std::env::var("NORN_RPC_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
    
    info!("Connecting to Norn node at: {}", rpc_url);

    let client = HttpClientBuilder::default()
        .build(&rpc_url)
        .context("Failed to create HTTP client")?;

    info!("\n=== Example 1: Get Chain ID ===");
    match get_chain_id(&client).await {
        Ok(chain_id) => info!("Chain ID: {}", chain_id),
        Err(e) => error!("Failed to get chain ID: {}", e),
    }

    info!("\n=== Example 2: Get Latest Block Number ===");
    match get_block_number(&client).await {
        Ok(block_num) => info!("Latest block number: {}", block_num),
        Err(e) => error!("Failed to get block number: {}", e),
    }

    info!("\n=== Example 3: Get Block Information ===");
    match get_block_by_number(&client, "0x1").await {
        Ok(block) => {
            info!("Block information retrieved");
            info!("Block: {:?}", block);
        }
        Err(e) => error!("Failed to get block (might not exist yet): {}", e),
    }

    info!("\n=== Example 4: Get Gas Price ===");
    match get_gas_price(&client).await {
        Ok(price) => info!("Current gas price: {} wei", price),
        Err(e) => error!("Failed to get gas price: {}", e),
    }

    info!("\n✅ Basic RPC examples completed!");

    Ok(())
}

async fn get_chain_id(client: &jsonrpsee::http_client::HttpClient) -> Result<String> {
    let response: String = client
        .request("eth_chainId", jsonrpsee::rpc_params![])
        .await
        .context("RPC request failed")?;
    
    Ok(response)
}

async fn get_block_number(client: &jsonrpsee::http_client::HttpClient) -> Result<String> {
    let response: String = client
        .request("eth_blockNumber", jsonrpsee::rpc_params![])
        .await
        .context("RPC request failed")?;
    
    Ok(response)
}

async fn get_block_by_number(client: &jsonrpsee::http_client::HttpClient, block_number: &str) -> Result<serde_json::Value> {
    let response: serde_json::Value = client
        .request("eth_getBlockByNumber", jsonrpsee::rpc_params![block_number, false])
        .await
        .context("RPC request failed")?;
    
    Ok(response)
}

async fn get_gas_price(client: &jsonrpsee::http_client::HttpClient) -> Result<String> {
    let response: String = client
        .request("eth_gasPrice", jsonrpsee::rpc_params![])
        .await
        .context("RPC request failed")?;
    
    Ok(response)
}
