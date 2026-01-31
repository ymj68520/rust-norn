use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use anyhow::Result;
use anyhow::Context;
use tracing::{info, error};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let rpc_url = env::var("NORN_RPC_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
    
    let account = env::var("ACCOUNT_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string());
    
    let recipient = env::var("RECIPIENT_ADDRESS")
        .unwrap_or_else(|_| "0x1111111111111111111111111111111111111111".to_string());

    info!("Transaction Sender Example");
    info!("RPC URL: {}", rpc_url);
    info!("From: {}", account);
    info!("To: {}", recipient);

    let client = HttpClientBuilder::default()
        .build(&rpc_url)
        .context("Failed to create HTTP client")?;

    info!("\n=== Getting Account Nonce ===");
    match get_transaction_count(&client, &account, "latest").await {
        Ok(nonce_hex) => {
            let nonce = u64::from_str_radix(
                nonce_hex.trim_start_matches("0x"),
                16
            ).context("Failed to parse nonce")?;
            
            info!("Current nonce: {}", nonce);

            info!("\n=== Sending Transaction ===");
            print_transaction_example(&account, &recipient, nonce);
        }
        Err(e) => error!("Failed to get transaction count: {}", e),
    }

    info!("\n✅ Transaction sender example completed!");
    info!("NOTE: To actually send transactions, implement transaction signing with eth_sendRawTransaction");

    Ok(())
}

async fn get_transaction_count(
    client: &jsonrpsee::http_client::HttpClient,
    address: &str,
    block: &str,
) -> Result<String> {
    let response: String = client
        .request("eth_getTransactionCount", jsonrpsee::rpc_params![address, block])
        .await
        .context("RPC request failed")?;
    
    Ok(response)
}

#[allow(dead_code)]
async fn get_transaction_receipt(
    client: &jsonrpsee::http_client::HttpClient,
    tx_hash: &str,
) -> Result<serde_json::Value> {
    let response: serde_json::Value = client
        .request("eth_getTransactionReceipt", jsonrpsee::rpc_params![tx_hash])
        .await
        .context("RPC request failed")?;
    
    Ok(response)
}

#[allow(dead_code)]
async fn send_raw_transaction(
    client: &jsonrpsee::http_client::HttpClient,
    signed_tx: &str,
) -> Result<String> {
    let response: String = client
        .request("eth_sendRawTransaction", jsonrpsee::rpc_params![signed_tx])
        .await
        .context("RPC request failed")?;
    
    Ok(response)
}

fn print_transaction_example(from: &str, to: &str, nonce: u64) {
    info!("\n=== Example Transaction Structure ===");
    info!("From: {}", from);
    info!("To: {}", to);
    info!("Nonce: {}", nonce);
    info!("Value: 1000000000000000000 wei (1 ether)");
    info!("Gas Price: 1000000000 wei");
    info!("Gas Limit: 21000");
    info!("Data: 0x (empty for value transfer)");
    info!("\nTo send this transaction:");
    info!("1. Create the transaction structure");
    info!("2. Sign it with your private key using ECDSA");
    info!("3. Encode it as RLP");
    info!("4. Send via eth_sendRawTransaction with 0x prefix");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_example() {
        print_transaction_example(
            "0x0000000000000000000000000000000000000000",
            "0x1111111111111111111111111111111111111111",
            0,
        );
    }
}
