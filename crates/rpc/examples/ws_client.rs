//! WebSocket Client Example for Norn Blockchain
//!
//! This example demonstrates how to connect to the Norn WebSocket API
//! and subscribe to real-time blockchain events.
//!
//! Run with: cargo run --example ws_client

use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::connect_async;
use tracing::{info, warn, error};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Connect to WebSocket server
    let url = "ws://localhost:8545/ws";
    info!("Connecting to {}", url);

    let (ws_stream, _) = connect_async(url).await?;
    info!("Connected!");

    let (mut write, mut read) = ws_stream.split();

    // Subscribe to new blocks
    let subscribe_msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_subscribe",
        "params": ["newHeads"]
    });

    info!("Sending subscription request for newHeads");
    write.send(subscribe_msg.to_string().into()).await?;

    // Subscribe to pending transactions
    let subscribe_tx = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "eth_subscribe",
        "params": ["newPendingTransactions"]
    });

    info!("Sending subscription request for newPendingTransactions");
    write.send(subscribe_tx.to_string().into()).await?;

    // Handle incoming messages
    while let Some(message) = read.next().await {
        match message {
            Ok(msg) => {
                if msg.is_text() {
                    if let Ok(text) = msg.into_text() {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                            handle_message(json);
                        }
                    }
                }
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

fn handle_message(msg: serde_json::Value) {
    // Check if it's a subscription notification
    if let Some(_subscription) = msg.get("subscription") {
        if let Some(result) = msg.get("result") {
            info!("📨 Notification received: {}", serde_json::to_string_pretty(result).unwrap_or_default());
        }
    }
    // Check if it's a response to our subscription request
    else if let Some(result) = msg.get("result") {
        if result.is_string() {
            info!("✅ Subscribed successfully! Subscription ID: {}", result);
        } else {
            info!("Response: {}", serde_json::to_string_pretty(&msg).unwrap_or_default());
        }
    }
    // Check if it's an error
    else if let Some(_error) = msg.get("error") {
        warn!("❌ Error received: {}", serde_json::to_string_pretty(&msg).unwrap_or_default());
    }
}
