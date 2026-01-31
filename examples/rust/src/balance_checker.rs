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
    
    let account = std::env::var("ACCOUNT_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string());

    info!("Connecting to Norn node at: {}", rpc_url);
    info!("Checking balance for account: {}", account);

    let client = HttpClientBuilder::default()
        .build(&rpc_url)
        .context("Failed to create HTTP client")?;

    match check_balance(&client, &account, "latest").await {
        Ok(balance) => {
            let balance_wei: u128 = u128::from_str_radix(
                balance.trim_start_matches("0x"),
                16
            ).context("Failed to parse balance")?;
            
            let balance_ether = balance_wei as f64 / 1e18;
            
            info!("\n=== Balance Information ===");
            info!("Account: {}", account);
            info!("Balance (wei): {}", balance_wei);
            info!("Balance (ether): {:.18}", balance_ether);
        }
        Err(e) => error!("Failed to check balance: {}", e),
    }

    info!("\n=== Historical Balance Check ===");
    for block in &["0x0", "0x1", "latest"] {
        match check_balance(&client, &account, block).await {
            Ok(balance) => {
                let balance_wei = u128::from_str_radix(
                    balance.trim_start_matches("0x"),
                    16
                ).unwrap_or(0);
                info!("Block {}: {} wei", block, balance_wei);
            }
            Err(e) => info!("Block {}: Not available - {}", block, e),
        }
    }

    info!("\n✅ Balance check completed!");
    Ok(())
}

async fn check_balance(
    client: &jsonrpsee::http_client::HttpClient,
    address: &str,
    block: &str,
) -> Result<String> {
    let response: String = client
        .request("eth_getBalance", jsonrpsee::rpc_params![address, block])
        .await
        .context("RPC request failed")?;
    
    Ok(response)
}

fn wei_to_ether(wei: u128) -> f64 {
    wei as f64 / 1e18
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wei_to_ether_conversion() {
        assert_eq!(wei_to_ether(0), 0.0);
        assert_eq!(wei_to_ether(1_000_000_000_000_000_000), 1.0);
        assert_eq!(wei_to_ether(500_000_000_000_000_000), 0.5);
    }
}
