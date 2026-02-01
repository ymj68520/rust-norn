# Norn WebSocket API Documentation

**Version**: 1.0
**Last Updated**: 2026-01-31
**Status**: Production Ready

---

## Table of Contents

1. [Overview](#overview)
2. [Getting Started](#getting-started)
3. [API Reference](#api-reference)
4. [Events](#events)
5. [Examples](#examples)
6. [Best Practices](#best-practices)
7. [Troubleshooting](#troubleshooting)

---

## Overview

The Norn WebSocket API provides real-time notifications for blockchain events, similar to Ethereum's `eth_subscribe` API. It enables clients to receive push notifications when:

- New blocks are mined
- Pending transactions are received
- Blockchain sync status changes
- Transaction logs are available

### Features

- **Real-time Notifications**: Instant push notifications for blockchain events
- **Multiple Subscriptions**: Subscribe to multiple event types in a single connection
- **Efficient**: Binary WebSocket protocol with low overhead
- **Standard JSON-RPC 2.0**: Compatible with standard Ethereum tools
- **Connection Management**: Automatic reconnection and subscription tracking

### WebSocket Endpoint

```
ws://localhost:8545/ws
```

For secure connections (when configured):

```
wss://localhost:8545/ws
```

---

## Getting Started

### Connection Example

#### JavaScript/Node.js

```javascript
const WebSocket = require('ws');

const ws = new WebSocket('ws://localhost:8545/ws');

ws.on('open', () => {
    // Connected!
    console.log('Connected to Norn WebSocket API');
});

ws.on('message', (data) => {
    const message = JSON.parse(data);
    console.log('Received:', message);
});
```

#### Python

```python
import asyncio
import websockets
import json

async def main():
    uri = "ws://localhost:8545/ws"
    async with websockets.connect(uri) as websocket:
        # Connected!
        print("Connected to Norn WebSocket API")

        # Subscribe to events
        subscribe_msg = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_subscribe",
            "params": ["newHeads"]
        }
        await websocket.send(json.dumps(subscribe_msg))

        # Receive messages
        while True:
            message = await websocket.recv()
            data = json.loads(message)
            print("Received:", data)

asyncio.run(main())
```

#### Rust

```rust
use tokio_tungstenite::connect_async;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = "ws://localhost:8545/ws";
    let (ws_stream, _) = connect_async(url).await?;

    // Subscribe to events
    let subscribe_msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_subscribe",
        "params": ["newHeads"]
    });

    // ... handle messages

    Ok(())
}
```

---

## API Reference

### eth_subscribe

Subscribe to blockchain events.

**Parameters**:
1. `subscription_type` (string): Type of subscription to create

**Returns**:
- `subscription_id` (hex string): Unique identifier for this subscription

**Subscription Types**:

| Type | Description |
|------|-------------|
| `newHeads` | New block headers |
| `newPendingTransactions` | Pending transactions in mempool |
| `logs` | Transaction logs (filtering supported) |
| `syncing` | Sync status updates |

**Example Request**:

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "eth_subscribe",
    "params": ["newHeads"]
}
```

**Example Response**:

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": "0x1"
}
```

---

### eth_unsubscribe

Unsubscribe from blockchain events.

**Parameters**:
1. `subscription_id` (hex string): Subscription identifier to cancel

**Returns**:
- `true` (boolean): If subscription was successfully removed

**Example Request**:

```json
{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "eth_unsubscribe",
    "params": ["0x1"]
}
```

**Example Response**:

```json
{
    "jsonrpc": "2.0",
    "id": 2,
    "result": true
}
```

---

## Events

### New Block Headers (newHeads)

Notification when a new block is mined.

**Event Format**:

```json
{
    "subscription": "0x1",
    "result": {
        "hash": "0x1234...",
        "parentHash": "0xabcd...",
        "number": 12345,
        "timestamp": 1738272000,
        "transactions": 150
    }
}
```

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| hash | string | Block hash (hex) |
| parentHash | string | Parent block hash (hex) |
| number | uint64 | Block number |
| timestamp | uint64 | Block timestamp |
| transactions | uint64 | Number of transactions |

---

### Pending Transactions (newPendingTransactions)

Notification when a new transaction enters the mempool.

**Event Format**:

```json
{
    "subscription": "0x2",
    "result": "0xabcd..."
}
```

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| result | string | Transaction hash (hex) |

---

### Sync Status (syncing)

Notification when sync status changes.

**Event Format**:

```json
{
    "subscription": "0x3",
    "result": {
        "syncing": true,
        "startingBlock": 10000,
        "currentBlock": 12345,
        "highestBlock": 13000
    }
}
```

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| syncing | boolean | Whether node is currently syncing |
| startingBlock | uint64 | Sync starting block number |
| currentBlock | uint64 | Current sync block number |
| highestBlock | uint64 | Highest known block number |

---

### Transaction Logs (logs)

Notification when a matching transaction log is emitted.

**Event Format**:

```json
{
    "subscription": "0x4",
    "result": {
        "address": "0x1234...",
        "topics": ["0xabcd..."],
        "data": "0x...",
        "blockNumber": 12345,
        "transactionHash": "0xabcd...",
        "logIndex": 0
    }
}
```

**Note**: Logs subscription with filtering is planned for future release.

---

## Examples

### Complete Example: Monitor New Blocks

```javascript
const WebSocket = require('ws');

const ws = new WebSocket('ws://localhost:8545/ws');

ws.on('open', () => {
    // Subscribe to new blocks
    ws.send(JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'eth_subscribe',
        params: ['newHeads']
    }));
});

ws.on('message', (data) => {
    const msg = JSON.parse(data);

    if (msg.subscription && msg.result) {
        console.log(`🧱 New Block #${msg.result.number}`);
        console.log(`   Hash: ${msg.result.hash}`);
        console.log(`   Transactions: ${msg.result.transactions}`);
    }
});
```

### Complete Example: Track Pending Transactions

```javascript
const WebSocket = require('ws');

const ws = new WebSocket('ws://localhost:8545/ws');

ws.on('open', () => {
    // Subscribe to pending transactions
    ws.send(JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'eth_subscribe',
        params: ['newPendingTransactions']
    }));
});

ws.on('message', (data) => {
    const msg = JSON.parse(data);

    if (msg.subscription && msg.result) {
        const txHash = msg.result;
        console.log(`📝 Pending Transaction: ${txHash}`);
    }
});
```

### Complete Example: Monitor Sync Progress

```javascript
const WebSocket = require('ws');

const ws = new WebSocket('ws://localhost:8545/ws');

let wasSyncing = false;

ws.on('open', () => {
    ws.send(JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'eth_subscribe',
        params: ['syncing']
    }));
});

ws.on('message', (data) => {
    const msg = JSON.parse(data);

    if (msg.subscription && msg.result) {
        const status = msg.result;

        if (status.syncing) {
            const progress = ((status.currentBlock - status.startingBlock) /
                             (status.highestBlock - status.startingBlock)) * 100;
            console.log(`🔄 Syncing... ${progress.toFixed(2)}%`);
            console.log(`   Block ${status.currentBlock} / ${status.highestBlock}`);
        } else if (wasSyncing) {
            console.log('✅ Sync complete!');
        }

        wasSyncing = status.syncing;
    }
});
```

### Complete Example: Multiple Subscriptions

```javascript
const WebSocket = require('ws');

const ws = new WebSocket('ws://localhost:8545/ws');
const subscriptions = {};

ws.on('open', () => {
    // Subscribe to multiple event types
    const subs = [
        { id: 1, type: 'newHeads', name: 'Blocks' },
        { id: 2, type: 'newPendingTransactions', name: 'Pending Txs' },
        { id: 3, type: 'syncing', name: 'Sync' }
    ];

    subs.forEach(sub => {
        ws.send(JSON.stringify({
            jsonrpc: '2.0',
            id: sub.id,
            method: 'eth_subscribe',
            params: [sub.type]
        }));
    });
});

ws.on('message', (data) => {
    const msg = JSON.parse(data);

    // Track subscription IDs
    if (msg.result && typeof msg.result === 'string') {
        const requestId = msg.id;
        const subId = msg.result;
        subscriptions[requestId] = subId;
        console.log(`✅ Subscribed: Request #${requestId} -> ${subId}`);
        return;
    }

    // Handle notifications
    if (msg.subscription && msg.result) {
        const subId = msg.subscription;
        const result = msg.result;

        // Determine event type based on content
        if (result.number !== undefined) {
            console.log(`🧱 Block #${result.number}: ${result.hash}`);
        } else if (result.startsWith && result.startsWith('0x')) {
            console.log(`📝 Pending TX: ${result}`);
        } else if (result.syncing !== undefined) {
            console.log(`🔄 Sync: ${result.syncing ? 'Active' : 'Idle'}`);
        }
    }
});
```

---

## Best Practices

### 1. Connection Management

**Always handle reconnection**:

```javascript
let ws;
let subscriptions = [];

function connect() {
    ws = new WebSocket('ws://localhost:8545/ws');

    ws.on('open', () => {
        console.log('Connected');

        // Resubscribe to previous subscriptions
        subscriptions.forEach(sub => {
            ws.send(JSON.stringify(sub));
        });
    });

    ws.on('close', () => {
        console.log('Disconnected, reconnecting in 3s...');
        setTimeout(connect, 3000);
    });
}

connect();
```

### 2. Error Handling

**Handle parse errors and protocol errors**:

```javascript
ws.on('message', (data) => {
    try {
        const msg = JSON.parse(data);

        if (msg.error) {
            console.error('Server error:', msg.error);
            return;
        }

        // Process message
        handleMessage(msg);
    } catch (e) {
        console.error('Parse error:', e);
    }
});
```

### 3. Heartbeat/Ping

**Monitor connection health**:

```javascript
const HEARTBEAT_INTERVAL = 30000; // 30 seconds

setInterval(() => {
    if (ws.readyState === WebSocket.OPEN) {
        ws.ping();
    }
}, HEARTBEAT_INTERVAL);

ws.on('pong', () => {
    console.log('Connection healthy');
});
```

### 4. Subscription Cleanup

**Unsubscribe when done**:

```javascript
function cleanup() {
    // Send unsubscribe for all active subscriptions
    Object.values(subscriptions).forEach(subId => {
        ws.send(JSON.stringify({
            jsonrpc: '2.0',
            id: Date.now(),
            method: 'eth_unsubscribe',
            params: [subId]
        }));
    });

    ws.close();
}

process.on('SIGINT', cleanup);
```

### 5. Rate Limiting

**Don't overwhelm the server**:

```javascript
// Queue subscription requests
const subscriptionQueue = [];
const RATE_LIMIT_DELAY = 100; // ms between requests

function queueSubscription(sub) {
    subscriptionQueue.push(sub);
    processQueue();
}

function processQueue() {
    if (subscriptionQueue.length > 0 && ws.readyState === WebSocket.OPEN) {
        const sub = subscriptionQueue.shift();
        ws.send(JSON.stringify(sub));
        setTimeout(processQueue, RATE_LIMIT_DELAY);
    }
}
```

---

## Troubleshooting

### Connection Issues

**Problem**: Cannot connect to WebSocket server

**Solutions**:
1. Verify WebSocket endpoint is correct: `ws://localhost:8545/ws`
2. Check if node is running: `curl http://localhost:8545/health`
3. Verify firewall allows connections on port 8545
4. Check node logs for errors

### No Events Received

**Problem**: Successfully subscribed but no notifications

**Solutions**:
1. Verify subscription was confirmed (check response message)
2. Check blockchain is producing blocks (use RPC to get latest block)
3. Ensure you're listening for the right event format
4. Check for error messages in the WebSocket stream

### Connection Drops

**Problem**: Connection closes unexpectedly

**Solutions**:
1. Implement automatic reconnection (see Best Practices)
2. Add heartbeat/ping monitoring
3. Check server logs for errors
4. Verify network stability

### Parse Errors

**Problem**: JSON parse errors on incoming messages

**Solutions**:
1. Ensure you're handling both text and binary messages
2. Add try-catch around JSON parsing
3. Log raw messages for debugging
4. Verify message format matches specification

---

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| -32700 | Parse error | Invalid JSON |
| -32600 | Invalid request | JSON-RPC request is invalid |
| -32601 | Method not found | Method doesn't exist or is not available |
| -32602 | Invalid params | Invalid method parameters |
| -32000 | Server error | Internal server error |

**Example Error Response**:

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "error": {
        "code": -32601,
        "message": "Method not found"
    }
}
```

---

## Additional Resources

- [Ethereum WebSocket API](https://docs.ethereum.org/api/websocket)
- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification)
- [WebSocket Protocol RFC](https://datatracker.ietf.org/doc/html/rfc6455)
- [JavaScript WebSocket MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket)

---

**Support**: For issues or questions, please open a GitHub issue or contact the Norn team.
