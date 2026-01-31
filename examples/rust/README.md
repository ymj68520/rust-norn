# Rust Client Examples

This directory contains Rust examples demonstrating how to interact with a Norn blockchain node using the JSON-RPC API.

## Prerequisites

- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- A running Norn node (local or remote)

## Setup

1. Copy the environment file and configure it:
```bash
cp .env.example .env
```

2. Edit `.env` with your configuration:
```bash
NORN_RPC_URL=http://127.0.0.1:50051          # RPC endpoint
NORN_WS_URL=ws://127.0.0.1:50052              # WebSocket endpoint (if supported)
ACCOUNT_ADDRESS=0x...                         # Your account address
PRIVATE_KEY=0x...                             # Your private key (for signing)
RECIPIENT_ADDRESS=0x...                       # Recipient for transactions
```

## Building

Build all examples:
```bash
cargo build --release
```

Build a specific example:
```bash
cargo build --release --bin basic_rpc
```

## Running Examples

### 1. Basic RPC Operations

Query block and chain information:

```bash
cargo run --release --bin basic_rpc
```

**What it does:**
- Gets chain ID
- Retrieves latest block number
- Fetches block details
- Gets current gas price

**Output:**
```
Chain ID: 0x1
Latest block number: 0x1a5
Block: { ... }
Current gas price: 1000000000 wei
```

### 2. Balance Checker

Check account balances:

```bash
cargo run --release --bin balance_checker
```

**What it does:**
- Queries account balance at current block
- Shows balance in wei and ether
- Queries historical balances

**Output:**
```
Account: 0x0000...0000
Balance (wei): 1000000000000000000
Balance (ether): 1.000000000000000000
```

### 3. Transaction Sender

Send transactions to the network:

```bash
cargo run --release --bin transaction_sender
```

**What it does:**
- Gets current account nonce
- Shows transaction structure
- Demonstrates signing process (educational)

**Note:** This is a demonstration of the transaction flow. For actual transaction sending, you need to:
1. Implement transaction signing with your private key
2. Encode the transaction as RLP
3. Send via `eth_sendRawTransaction`

### 4. WebSocket Listener

Subscribe to blockchain events:

```bash
cargo run --release --bin websocket_listener
```

**What it does:**
- Connects to WebSocket endpoint
- Subscribes to new block headers
- Subscribes to pending transactions
- Displays events as they occur

**Output:**
```
✅ Connected to WebSocket
✅ Subscribed to newHeads with ID: 0x1
✅ Subscribed to newPendingTransactions with ID: 0x2

🔗 [Block #1] New block received
   Height: 0x1a5
   Miner: 0x...
   Timestamp: 0x...
```

## Supported RPC Methods

The examples use the following Ethereum JSON-RPC methods:

### Account Information
- `eth_getBalance` - Get account balance
- `eth_getTransactionCount` - Get account nonce
- `eth_getCode` - Get contract code

### Chain Information
- `eth_chainId` - Get chain ID
- `eth_blockNumber` - Get latest block number
- `eth_gasPrice` - Get gas price

### Blocks
- `eth_getBlockByNumber` - Get block by number
- `eth_getBlockByHash` - Get block by hash

### Transactions
- `eth_sendRawTransaction` - Send signed transaction
- `eth_getTransaction` - Get transaction details
- `eth_getTransactionReceipt` - Get transaction receipt

### Subscriptions (WebSocket)
- `eth_subscribe` with `newHeads` - Subscribe to new blocks
- `eth_subscribe` with `newPendingTransactions` - Subscribe to pending transactions
- `eth_subscribe` with `logs` - Subscribe to contract events

## Security Considerations

⚠️ **Important:**

1. **Private Keys**: Never hardcode private keys in production code. Load them from:
   - Environment variables (for development only)
   - Hardware wallets
   - Key management systems
   - Secure vaults

2. **Endpoints**: Use HTTPS for remote RPC endpoints to prevent MITM attacks

3. **Transaction Signing**: Always verify transactions before signing

4. **Gas Limits**: Set appropriate gas limits to prevent failed transactions

## Environment Variables

```env
# RPC endpoint
NORN_RPC_URL=http://127.0.0.1:50051

# WebSocket endpoint (for subscriptions)
NORN_WS_URL=ws://127.0.0.1:50052

# Account management
ACCOUNT_ADDRESS=0x0000000000000000000000000000000000000000
PRIVATE_KEY=0x0000000000000000000000000000000000000000000000000000000000000001

# Transaction parameters
RECIPIENT_ADDRESS=0x1111111111111111111111111111111111111111
TRANSACTION_VALUE=1000000000000000000
GAS_PRICE=1000000000
GAS_LIMIT=21000
```

## Troubleshooting

### Connection refused
- Ensure the Norn node is running
- Check the RPC URL is correct
- Verify network connectivity

### Invalid account
- Ensure account address is properly formatted (0x + 40 hex characters)
- Check the account exists on the network

### Transaction failed
- Verify nonce is correct
- Check account has sufficient balance
- Ensure gas price and limit are adequate

## Advanced Usage

### Custom RPC Calls

To make custom RPC calls, modify the examples:

```rust
let response: String = client
    .request("custom_method", rpc_params![param1, param2])
    .await?;
```

### Error Handling

Examples use Rust's `Result` type with context:

```rust
let client = HttpClient::builder()
    .build(&rpc_url)
    .context("Failed to create HTTP client")?;
```

### Logging

Control log output with the `RUST_LOG` environment variable:

```bash
# Show all logs
RUST_LOG=debug cargo run --release --bin basic_rpc

# Show only specific module
RUST_LOG=norn=debug cargo run --release --bin basic_rpc

# Show warning and errors
RUST_LOG=warn cargo run --release --bin basic_rpc
```

## Resources

- [Ethereum JSON-RPC Specification](https://ethereum.org/en/developers/docs/apis/json-rpc/)
- [Rust Async Programming](https://tokio.rs/)
- [jsonrpsee Documentation](https://docs.rs/jsonrpsee/)

## Next Steps

1. Explore other blockchain methods
2. Implement contract interaction
3. Build transaction aggregation
4. Create monitoring services

## Contributing

Feel free to submit improvements and additional examples!
