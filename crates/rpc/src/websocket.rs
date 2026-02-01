//! WebSocket support for real-time event subscriptions
//!
//! This module provides WebSocket server functionality for real-time
//! blockchain event notifications, similar to Ethereum's eth_subscribe API.

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock, Mutex};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use norn_core::blockchain::Blockchain;
use norn_common::types::{Transaction, Block, Hash, Address};

/// Log filter for eth_subscribe logs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFilter {
    /// Filter by address (single address or array)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Vec<Vec<u8>>>,

    /// Filter by topics (array of topic hashes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<Vec<u8>>>,

    /// Filter by block range - from block (inclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_block: Option<String>,

    /// Filter by block range - to block (inclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_block: Option<String>,
}

impl LogFilter {
    /// Create a new log filter
    pub fn new() -> Self {
        Self {
            address: None,
            topics: None,
            from_block: None,
            to_block: None,
        }
    }

    /// Check if a log matches this filter
    pub fn matches(&self, log: &Log) -> bool {
        // Check address filter
        if let Some(ref addresses) = self.address {
            if !addresses.is_empty() {
                let log_addr = log.address.0.to_vec();
                if !addresses.iter().any(|a| a == &log_addr) {
                    return false;
                }
            }
        }

        // Check topics filter
        if let Some(ref filter_topics) = self.topics {
            if !filter_topics.is_empty() {
                // All filter topics must match (wildcard matching with empty bytes)
                if filter_topics.len() > log.topics.len() {
                    return false;
                }

                for (i, filter_topic) in filter_topics.iter().enumerate() {
                    if !filter_topic.is_empty() {
                        if i >= log.topics.len() || filter_topic != &log.topics[i] {
                            return false;
                        }
                    }
                }
            }
        }

        true
    }
}

impl Default for LogFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Log entry structure (emitted by smart contracts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    /// Account address that emitted the log
    pub address: Address,

    /// Array of topics (topic hashes from indexed parameters)
    pub topics: Vec<Vec<u8>>,

    /// Log data (non-indexed parameters)
    pub data: Vec<u8>,

    /// Block number where the log was emitted
    pub block_number: u64,

    /// Block hash
    pub block_hash: Hash,

    /// Transaction hash
    pub transaction_hash: Hash,

    /// Index of the log within the transaction
    pub log_index: u32,

    /// Index of the transaction in the block
    pub transaction_index: u32,
}

/// Log notification with filter context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogNotification {
    pub log: Log,
    pub timestamp: i64,
}

/// Subscription types supported by the WebSocket server
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionType {
    /// New block headers
    NewHeads,
    /// Pending transactions
    NewPendingTransactions,
    /// Transaction logs
    Logs,
    /// Sync status updates
    Syncing,
}

impl SubscriptionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SubscriptionType::NewHeads => "newHeads",
            SubscriptionType::NewPendingTransactions => "newPendingTransactions",
            SubscriptionType::Logs => "logs",
            SubscriptionType::Syncing => "syncing",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "newHeads" => Some(SubscriptionType::NewHeads),
            "newPendingTransactions" => Some(SubscriptionType::NewPendingTransactions),
            "logs" => Some(SubscriptionType::Logs),
            "syncing" => Some(SubscriptionType::Syncing),
            _ => None,
        }
    }
}

/// WebSocket message format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    /// Message type (subscription, notification, error)
    #[serde(rename = "type")]
    pub msg_type: String,

    /// Subscription ID (for subscription-related messages)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<String>,

    /// Event data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,

    /// Error message (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

impl WsMessage {
    /// Create a new subscription result message
    pub fn subscription(subscription_id: String) -> Self {
        Self {
            msg_type: "eth_subscription".to_string(),
            subscription: Some(subscription_id),
            result: None,
            error: None,
        }
    }

    /// Create a notification message
    pub fn notification(subscription_id: String, data: serde_json::Value) -> Self {
        Self {
            msg_type: "eth_subscription".to_string(),
            subscription: Some(subscription_id),
            result: Some(data),
            error: None,
        }
    }

    /// Create an error message
    pub fn error(code: i32, message: String) -> Self {
        Self {
            msg_type: "error".to_string(),
            subscription: None,
            result: None,
            error: Some(serde_json::json!({
                "code": code,
                "message": message
            })),
        }
    }
}

/// Event broadcaster for WebSocket subscriptions
#[derive(Clone)]
pub struct EventBroadcaster {
    /// Channel for new block events
    new_blocks: broadcast::Sender<BlockNotification>,

    /// Channel for pending transaction events
    pending_txs: broadcast::Sender<TransactionNotification>,

    /// Channel for sync status events
    sync_status: broadcast::Sender<SyncStatus>,

    /// Channel for log events
    logs: broadcast::Sender<LogNotification>,
}

/// Block notification with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockNotification {
    pub block: Block,
    pub timestamp: i64,
}

/// Transaction notification with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionNotification {
    pub transaction: Transaction,
    pub timestamp: i64,
}

/// Sync status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub syncing: bool,
    pub starting_block: u64,
    pub current_block: u64,
    pub highest_block: u64,
    pub timestamp: i64,
}

impl EventBroadcaster {
    /// Create a new event broadcaster
    pub fn new() -> Self {
        let (new_blocks, _) = broadcast::channel(1000);
        let (pending_txs, _) = broadcast::channel(5000);
        let (sync_status, _) = broadcast::channel(100);
        let (logs, _) = broadcast::channel(5000);

        Self {
            new_blocks,
            pending_txs,
            sync_status,
            logs,
        }
    }

    /// Publish a new block event
    pub fn publish_block(&self, block: Block) {
        let notification = BlockNotification {
            block,
            timestamp: chrono::Utc::now().timestamp(),
        };

        if let Err(e) = self.new_blocks.send(notification) {
            debug!("Failed to publish block event: {}", e);
        } else {
            debug!("Published new block event");
        }
    }

    /// Publish a pending transaction event
    pub fn publish_pending_tx(&self, tx: Transaction) {
        let notification = TransactionNotification {
            transaction: tx,
            timestamp: chrono::Utc::now().timestamp(),
        };

        if let Err(e) = self.pending_txs.send(notification) {
            debug!("Failed to publish pending tx event: {}", e);
        }
    }

    /// Publish sync status update
    pub fn publish_sync_status(&self, status: SyncStatus) {
        if let Err(e) = self.sync_status.send(status) {
            debug!("Failed to publish sync status: {}", e);
        }
    }

    /// Subscribe to new blocks
    pub fn subscribe_new_blocks(&self) -> broadcast::Receiver<BlockNotification> {
        self.new_blocks.subscribe()
    }

    /// Subscribe to pending transactions
    pub fn subscribe_pending_txs(&self) -> broadcast::Receiver<TransactionNotification> {
        self.pending_txs.subscribe()
    }

    /// Subscribe to sync status
    pub fn subscribe_sync_status(&self) -> broadcast::Receiver<SyncStatus> {
        self.sync_status.subscribe()
    }

    /// Publish a log event
    pub fn publish_log(&self, log: Log) {
        let notification = LogNotification {
            log,
            timestamp: chrono::Utc::now().timestamp(),
        };

        if let Err(e) = self.logs.send(notification) {
            debug!("Failed to publish log event: {}", e);
        } else {
            debug!("Published log event");
        }
    }

    /// Subscribe to log events
    pub fn subscribe_logs(&self) -> broadcast::Receiver<LogNotification> {
        self.logs.subscribe()
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

/// WebSocket server configuration
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// WebSocket server address
    pub address: String,

    /// Maximum number of concurrent connections
    pub max_connections: usize,

    /// Message size limit (in bytes)
    pub max_message_size: usize,

    /// Ping interval (in seconds)
    pub ping_interval: u64,

    /// Connection timeout (in seconds)
    pub connection_timeout: u64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            address: "0.0.0.0:8545".to_string(),
            max_connections: 1000,
            max_message_size: 10 * 1024 * 1024, // 10 MB
            ping_interval: 30,
            connection_timeout: 60,
        }
    }
}

/// WebSocket connection manager
pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, id: String, addr: String) {
        let info = ConnectionInfo {
            id: id.clone(),
            addr,
            connected_at: chrono::Utc::now().timestamp(),
            subscriptions: Vec::new(),
        };
        self.connections.write().await.insert(id.clone(), info);
        info!("Registered connection: {}", id);
    }

    pub async fn unregister(&self, id: &str) {
        self.connections.write().await.remove(id);
        info!("Unregistered connection: {}", id);
    }

    pub async fn add_subscription(&self, conn_id: &str, sub_type: SubscriptionType) {
        if let Some(conn) = self.connections.write().await.get_mut(conn_id) {
            conn.subscriptions.push(sub_type);
        }
    }

    pub async fn remove_subscription(&self, conn_id: &str, sub_id: &str) {
        if let Some(conn) = self.connections.write().await.get_mut(conn_id) {
            conn.subscriptions.retain(|s| s.as_str() != sub_id);
        }
    }

    pub async fn get_connection_count(&self) -> usize {
        self.connections.read().await.len()
    }
}

#[derive(Debug, Clone)]
struct ConnectionInfo {
    id: String,
    addr: String,
    connected_at: i64,
    subscriptions: Vec<SubscriptionType>,
}

/// WebSocket server
pub struct WebSocketServer {
    config: WebSocketConfig,
    broadcaster: EventBroadcaster,
    blockchain: Arc<Blockchain>,
    connection_manager: Arc<ConnectionManager>,
}

impl WebSocketServer {
    /// Create a new WebSocket server
    pub fn new(
        config: WebSocketConfig,
        broadcaster: EventBroadcaster,
        blockchain: Arc<Blockchain>,
    ) -> Self {
        Self {
            config,
            broadcaster,
            blockchain,
            connection_manager: Arc::new(ConnectionManager::new()),
        }
    }

    /// Build the router
    pub fn router(&self) -> Router {
        Router::new()
            .route("/ws", axum::routing::get(ws_handler))
            .route("/", axum::routing::get(ws_handler))  // Also serve on root
            .with_state((
                self.broadcaster.clone(),
                self.blockchain.clone(),
                self.connection_manager.clone(),
            ))
    }

    /// Start the WebSocket server
    pub async fn start(&self) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(&self.config.address).await?;
        info!("WebSocket server listening on {}", self.config.address);

        let app = self.router();

        axum::serve(listener, app).await?;
        Ok(())
    }
}

/// WebSocket handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State((broadcaster, blockchain, connection_manager)): State<(
        EventBroadcaster,
        Arc<Blockchain>,
        Arc<ConnectionManager>,
    )>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        handle_socket(socket, broadcaster, blockchain, connection_manager)
    })
}

/// Handle a WebSocket connection
async fn handle_socket(
    socket: WebSocket,
    broadcaster: EventBroadcaster,
    blockchain: Arc<Blockchain>,
    connection_manager: Arc<ConnectionManager>,
) {
    // Split the socket into sender and receiver
    let (mut sender, mut receiver) = socket.split();
    let conn_id = Uuid::new_v4().to_string();

    // Get peer address if available
    let peer_addr = "unknown".to_string();  // Axum doesn't expose peer addr easily

    // Register connection
    connection_manager.register(conn_id.clone(), peer_addr).await;

    // Send welcome message
    let welcome = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": serde_json::json!({
            "message": "Connected to Norn WebSocket API",
            "available_subscriptions": ["newHeads", "newPendingTransactions", "logs", "syncing"]
        })
    });

    if let Ok(text) = serde_json::to_string(&welcome) {
        let _ = sender.send(Message::Text(text)).await;
    }

    // Create channels for event forwarding
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut subscriptions: HashMap<String, SubscriptionType> = HashMap::new();
    let mut subscription_counter = 0u32;

    // Clone for the event forwarding task
    let sender_clone = Arc::new(Mutex::new(sender));
    let sender_for_main_loop = sender_clone.clone();
    let conn_id_clone = conn_id.clone();

    // Spawn event forwarding task
    let event_task = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let sender = sender_clone.clone();
            tokio::spawn(async move {
                let mut sender = sender.lock().await;
                if let Ok(text) = serde_json::to_string(&event) {
                    let _ = sender.send(Message::Text(text)).await;
                }
            });
        }
    });

    // Handle incoming messages
    while let Some(result) = receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                if let Ok(req) = serde_json::from_str::<serde_json::Value>(&text) {
                    handle_client_message(
                        &req,
                        &broadcaster,
                        &event_tx,
                        &mut subscriptions,
                        &mut subscription_counter,
                        &conn_id,
                        &connection_manager,
                    ).await;
                } else {
                    let error = WsMessage::error(-32700, "Parse error".to_string());
                    send_json(&sender_for_main_loop, error).await;
                }
            }
            Ok(Message::Ping(data)) => {
                // Respond with pong
                let _ = sender_for_main_loop.lock().await.send(Message::Pong(data)).await;
            }
            Ok(Message::Pong(_)) => {
                // Ignore pong responses
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket connection {} closed by client", conn_id);
                break;
            }
            Err(e) => {
                error!("WebSocket error on {}: {}", conn_id, e);
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    event_task.abort();
    connection_manager.unregister(&conn_id).await;
    info!("WebSocket connection {} finalized", conn_id);
}

/// Handle client messages
async fn handle_client_message(
    req: &serde_json::Value,
    broadcaster: &EventBroadcaster,
    event_tx: &mpsc::UnboundedSender<WsMessage>,
    subscriptions: &mut HashMap<String, SubscriptionType>,
    subscription_counter: &mut u32,
    conn_id: &str,
    connection_manager: &Arc<ConnectionManager>,
) {
    let method = req.get("method").and_then(|m| m.as_str());
    let id = req.get("id");

    match method {
        Some("eth_subscribe") => {
            if let Some(params) = req.get("params").and_then(|p| p.as_array()) {
                if let Some(subscription_type) = params.first().and_then(|t| t.as_str()) {
                    if let Some(sub_type) = SubscriptionType::from_str(subscription_type) {
                        *subscription_counter += 1;
                        let subscription_id = format!("0x{:x}", subscription_counter);

                        subscriptions.insert(subscription_id.clone(), sub_type.clone());
                        connection_manager.add_subscription(conn_id, sub_type.clone()).await;

                        // Send subscription confirmation
                        let _response = WsMessage::subscription(subscription_id.clone());

                        if let Some(req_id) = id {
                            let _full_response = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": req_id,
                                "result": subscription_id
                            });
                            let _ = event_tx.send(WsMessage {
                                msg_type: "response".to_string(),
                                subscription: None,
                                result: Some(serde_json::Value::String(subscription_id.clone())),
                                error: None,
                            });
                        }

                        // Start forwarding events for this subscription
                        let filter = if sub_type == SubscriptionType::Logs {
                            params.get(1).and_then(|f| serde_json::from_value::<LogFilter>(f.clone()).ok())
                        } else {
                            None
                        };

                        start_event_forwarding(
                            broadcaster,
                            event_tx,
                            subscription_id.clone(),
                            sub_type.clone(),
                            filter,
                        );

                        info!("Connection {} subscribed to {} as {}", conn_id, sub_type.as_str(), subscription_id);
                    } else {
                        let error = WsMessage::error(-32602, format!("Unknown subscription type: {}", subscription_type));
                        let _ = event_tx.send(error);
                    }
                }
            } else {
                let error = WsMessage::error(-32602, "Invalid params".to_string());
                let _ = event_tx.send(error);
            }
        }
        Some("eth_unsubscribe") => {
            if let Some(params) = req.get("params").and_then(|p| p.as_array()) {
                if let Some(sub_id) = params.first().and_then(|s| s.as_str()) {
                    if subscriptions.remove(sub_id).is_some() {
                        connection_manager.remove_subscription(conn_id, sub_id).await;

                        let response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": true
                        });
                        info!("Connection {} unsubscribed from {}", conn_id, sub_id);
                    } else {
                        let error = WsMessage::error(-32000, format!("Subscription not found: {}", sub_id));
                        let _ = event_tx.send(error);
                    }
                }
            }
        }
        _ => {
            let error = WsMessage::error(-32601, format!("Method not found: {:?}", method));
            let _ = event_tx.send(error);
        }
    }
}

/// Start forwarding events for a subscription
fn start_event_forwarding(
    broadcaster: &EventBroadcaster,
    event_tx: &mpsc::UnboundedSender<WsMessage>,
    subscription_id: String,
    sub_type: SubscriptionType,
    filter: Option<LogFilter>,
) {
    let event_tx = event_tx.clone();
    let sub_id = subscription_id.clone();

    match sub_type {
        SubscriptionType::NewHeads => {
            let mut rx = broadcaster.subscribe_new_blocks();
            tokio::spawn(async move {
                while let Ok(notification) = rx.recv().await {
                    let data = serde_json::json!({
                        "subscription": sub_id,
                        "result": {
                            "hash": format!("0x{}", hex::encode(&notification.block.header.block_hash.0)),
                            "parentHash": format!("0x{}", hex::encode(&notification.block.header.prev_block_hash.0)),
                            "number": notification.block.header.height,
                            "timestamp": notification.block.header.timestamp,
                            "transactions": notification.block.transactions.len(),
                        }
                    });

                    let msg = WsMessage::notification(sub_id.clone(), data);
                    let _ = event_tx.send(msg);
                }
            });
        }
        SubscriptionType::NewPendingTransactions => {
            let mut rx = broadcaster.subscribe_pending_txs();
            tokio::spawn(async move {
                while let Ok(notification) = rx.recv().await {
                    let data = serde_json::json!({
                        "subscription": sub_id,
                        "result": format!("0x{}", hex::encode(&notification.transaction.body.hash.0))
                    });

                    let msg = WsMessage::notification(sub_id.clone(), data);
                    let _ = event_tx.send(msg);
                }
            });
        }
        SubscriptionType::Syncing => {
            let mut rx = broadcaster.subscribe_sync_status();
            tokio::spawn(async move {
                while let Ok(status) = rx.recv().await {
                    let data = serde_json::json!({
                        "subscription": sub_id,
                        "result": {
                            "syncing": status.syncing,
                            "startingBlock": status.starting_block,
                            "currentBlock": status.current_block,
                            "highestBlock": status.highest_block,
                        }
                    });

                    let msg = WsMessage::notification(sub_id.clone(), data);
                    let _ = event_tx.send(msg);
                }
            });
        }
        SubscriptionType::Logs => {
            let log_filter = filter.unwrap_or_default();
            let mut rx = broadcaster.subscribe_logs();
            tokio::spawn(async move {
                while let Ok(notification) = rx.recv().await {
                    if log_filter.matches(&notification.log) {
                        let data = serde_json::json!({
                            "subscription": sub_id,
                            "result": {
                                "address": format!("0x{}", hex::encode(&notification.log.address.0)),
                                "topics": notification.log.topics.iter()
                                    .map(|t| format!("0x{}", hex::encode(t)))
                                    .collect::<Vec<_>>(),
                                "data": format!("0x{}", hex::encode(&notification.log.data)),
                                "blockNumber": notification.log.block_number,
                                "blockHash": format!("0x{}", hex::encode(&notification.log.block_hash.0)),
                                "transactionHash": format!("0x{}", hex::encode(&notification.log.transaction_hash.0)),
                                "logIndex": format!("0x{:x}", notification.log.log_index),
                                "transactionIndex": format!("0x{:x}", notification.log.transaction_index),
                            }
                        });

                        let msg = WsMessage::notification(sub_id.clone(), data);
                        let _ = event_tx.send(msg);
                    }
                }
            });
        }
    }
}

/// Send JSON message through the sender
async fn send_json(sender: &Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>, msg: WsMessage) {
    let mut s = sender.lock().await;
    if let Ok(text) = serde_json::to_string(&msg) {
        let _ = s.send(Message::Text(text)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_message_creation() {
        let msg = WsMessage::subscription("0x1a".to_string());
        assert_eq!(msg.msg_type, "eth_subscription");
        assert_eq!(msg.subscription, Some("0x1a".to_string()));
    }

    #[test]
    fn test_broadcaster_creation() {
        let broadcaster = EventBroadcaster::new();
        let _rx = broadcaster.subscribe_new_blocks();
        let _rx = broadcaster.subscribe_pending_txs();
        let _rx = broadcaster.subscribe_sync_status();
        let _rx = broadcaster.subscribe_logs();
    }

    #[test]
    fn test_log_filter_creation() {
        let filter = LogFilter::new();
        assert!(filter.address.is_none());
        assert!(filter.topics.is_none());
        assert!(filter.from_block.is_none());
        assert!(filter.to_block.is_none());
    }

    #[test]
    fn test_log_filter_address_matching() {
        let mut filter = LogFilter::new();
        let addr = vec![vec![1u8; 20]];
        filter.address = Some(addr.clone());

        let log = Log {
            address: Address([1u8; 20]),
            topics: vec![],
            data: vec![],
            block_number: 1,
            block_hash: Hash([0u8; 32]),
            transaction_hash: Hash([0u8; 32]),
            log_index: 0,
            transaction_index: 0,
        };

        assert!(filter.matches(&log));

        let log_mismatch = Log {
            address: Address([2u8; 20]),
            topics: vec![],
            data: vec![],
            block_number: 1,
            block_hash: Hash([0u8; 32]),
            transaction_hash: Hash([0u8; 32]),
            log_index: 0,
            transaction_index: 0,
        };

        assert!(!filter.matches(&log_mismatch));
    }

    #[test]
    fn test_log_filter_topic_matching() {
        let mut filter = LogFilter::new();
        let topic = vec![vec![1u8; 32]];
        filter.topics = Some(topic.clone());

        let log = Log {
            address: Address([0u8; 20]),
            topics: vec![vec![1u8; 32]],
            data: vec![],
            block_number: 1,
            block_hash: Hash([0u8; 32]),
            transaction_hash: Hash([0u8; 32]),
            log_index: 0,
            transaction_index: 0,
        };

        assert!(filter.matches(&log));

        let log_mismatch = Log {
            address: Address([0u8; 20]),
            topics: vec![vec![2u8; 32]],
            data: vec![],
            block_number: 1,
            block_hash: Hash([0u8; 32]),
            transaction_hash: Hash([0u8; 32]),
            log_index: 0,
            transaction_index: 0,
        };

        assert!(!filter.matches(&log_mismatch));
    }

    #[tokio::test]
    async fn test_log_publication_and_subscription() {
        let broadcaster = EventBroadcaster::new();
        let mut rx = broadcaster.subscribe_logs();

        let log = Log {
            address: Address([0u8; 20]),
            topics: vec![],
            data: vec![],
            block_number: 1,
            block_hash: Hash([0u8; 32]),
            transaction_hash: Hash([0u8; 32]),
            log_index: 0,
            transaction_index: 0,
        };

        broadcaster.publish_log(log.clone());

        let notification = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            rx.recv()
        ).await;

        assert!(notification.is_ok());
        assert!(notification.unwrap().is_ok());
    }

    #[test]
    fn test_subscription_type_serialize() {
        let sub = SubscriptionType::NewHeads;
        let json = serde_json::to_string(&sub).unwrap();
        assert_eq!(json, "\"newHeads\"");
    }

    #[test]
    fn test_subscription_type_from_str() {
        assert_eq!(SubscriptionType::from_str("newHeads"), Some(SubscriptionType::NewHeads));
        assert_eq!(SubscriptionType::from_str("newPendingTransactions"), Some(SubscriptionType::NewPendingTransactions));
        assert_eq!(SubscriptionType::from_str("invalid"), None);
    }

    #[test]
    fn test_websocket_config_default() {
        let config = WebSocketConfig::default();
        assert_eq!(config.address, "0.0.0.0:8545");
        assert_eq!(config.max_connections, 1000);
        assert_eq!(config.ping_interval, 30);
    }

    #[tokio::test]
    async fn test_connection_manager() {
        let manager = ConnectionManager::new();
        manager.register("conn1".to_string(), "127.0.0.1:8080".to_string()).await;

        assert_eq!(manager.get_connection_count().await, 1);

        manager.add_subscription("conn1", SubscriptionType::NewHeads).await;
        manager.unregister("conn1").await;

        assert_eq!(manager.get_connection_count().await, 0);
    }
}
