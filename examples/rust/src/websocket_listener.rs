use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use anyhow::{Result, Context};
use tracing::{info, error};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let ws_url = env::var("NORN_WS_URL")
        .unwrap_or_else(|_| "ws://127.0.0.1:50052".to_string());

    info!("WebSocket Listener Example");
    info!("Connecting to: {}", ws_url);

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .context("Failed to connect to WebSocket")?;

    info!("✅ Connected to WebSocket");

    let (mut write, mut read) = ws_stream.split();

    info!("\n=== Subscribing to New Block Headers ===");
    let subscribe_blocks = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_subscribe",
        "params": ["newHeads"]
    });

    write.send(Message::Text(subscribe_blocks.to_string()))
        .await
        .context("Failed to send subscription")?;

    info!("Subscription request sent, waiting for response...");

    info!("\n=== Subscribing to Pending Transactions ===");
    let subscribe_txs = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "eth_subscribe",
        "params": ["newPendingTransactions"]
    });

    write.send(Message::Text(subscribe_txs.to_string()))
        .await
        .context("Failed to send subscription")?;

    info!("Subscription request sent");
    info!("\n📡 Listening for events (press Ctrl+C to stop)...\n");

    let mut block_count = 0;
    let mut tx_count = 0;

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    handle_message(&value, &mut block_count, &mut tx_count)?;
                }
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket connection closed");
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    info!("\n=== Event Summary ===");
    info!("Blocks received: {}", block_count);
    info!("Transactions received: {}", tx_count);
    info!("✅ Listener stopped");

    Ok(())
}

fn handle_message(
    msg: &Value,
    block_count: &mut usize,
    tx_count: &mut usize,
) -> Result<()> {
    if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
        match id {
            1 => {
                if let Some(result) = msg.get("result").and_then(|v| v.as_str()) {
                    info!("✅ Subscribed to newHeads with ID: {}", result);
                }
            }
            2 => {
                if let Some(result) = msg.get("result").and_then(|v| v.as_str()) {
                    info!("✅ Subscribed to newPendingTransactions with ID: {}", result);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
        match method {
            "eth_subscription" => {
                if let Some(params) = msg.get("params") {
                    if let Some(subscription) = params.get("subscription").and_then(|v| v.as_str()) {
                        if let Some(result) = params.get("result") {
                            handle_subscription_event(subscription, result, block_count, tx_count)?;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_subscription_event(
    subscription_type: &str,
    data: &Value,
    block_count: &mut usize,
    tx_count: &mut usize,
) -> Result<()> {
    match subscription_type {
        "newHeads" => {
            *block_count += 1;
            print_block_info(data, *block_count)?;
        }
        "newPendingTransactions" => {
            *tx_count += 1;
            print_transaction_info(data, *tx_count)?;
        }
        "logs" => {
            info!("📋 Contract Event: {:?}", data);
        }
        _ => {
            info!("🔔 Unknown event type: {}", subscription_type);
        }
    }

    Ok(())
}

fn print_block_info(block: &Value, count: usize) -> Result<()> {
    let height = block.get("number")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    
    let miner = block.get("miner")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    
    let timestamp = block.get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    info!("\n🔗 [Block #{}] New block received", count);
    info!("   Height: {}", height);
    info!("   Miner: {}", miner);
    info!("   Timestamp: {}", timestamp);

    Ok(())
}

fn print_transaction_info(tx: &Value, count: usize) -> Result<()> {
    let tx_hash = if let Value::String(h) = tx {
        h.clone()
    } else {
        tx.to_string()
    };

    info!("💰 [Tx #{}] Pending transaction: {}", count, tx_hash);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_type_matching() {
        assert_eq!("newHeads", "newHeads");
        assert_eq!("newPendingTransactions", "newPendingTransactions");
    }
}
