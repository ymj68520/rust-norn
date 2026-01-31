# Norn Blockchain Node Examples

This directory contains practical examples demonstrating how to interact with a Norn blockchain node using various programming languages and tools.

## Overview

The examples show how to:
- Connect to the RPC endpoint
- Query blockchain state (balances, blocks, transactions)
- Send transactions
- Subscribe to real-time events via WebSocket
- Implement common blockchain operations

## Directory Structure

```
examples/
├── README.md                       # This file
├── rust/                           # Rust examples (9 files)
│   ├── Cargo.toml
│   ├── .env.example
│   ├── src/
│   │   ├── basic_rpc.rs                    # Basic RPC queries
│   │   ├── balance_checker.rs              # Balance queries
│   │   ├── transaction_sender.rs           # Transaction creation
│   │   ├── websocket_listener.rs           # WebSocket subscriptions
│   │   ├── contract_interaction.rs         # Smart contract interaction
│   │   ├── batch_rpc_requests.rs           # Batch RPC calls
│   │   ├── health_check_monitoring.rs      # Node health monitoring
│   │   └── rate_limiting_utilities.rs      # Rate limiting patterns
│   └── README.md
├── python/                         # Python examples (9 files)
│   ├── requirements.txt
│   ├── .env.example
│   ├── basic_rpc.py
│   ├── balance_checker.py
│   ├── transaction_sender.py
│   ├── websocket_listener.py
│   ├── contract_interaction.py
│   ├── batch_rpc_requests.py
│   ├── health_check_monitoring.py
│   ├── rate_limiting_utilities.py
│   └── README.md
└── javascript/                     # JavaScript/Node.js examples (9 files)
    ├── package.json
    ├── .env.example
    ├── basic_rpc.js
    ├── balance_checker.js
    ├── transaction_sender.js
    ├── websocket_listener.js
    ├── contract_interaction.js
    ├── batch_rpc_requests.js
    ├── health_check_monitoring.js
    ├── rate_limiting_utilities.js
    └── README.md
```

## Quick Start

### 1. Start a Norn Node

First, ensure you have a running Norn node:

```bash
# From the project root
cargo run --release --bin norn -- --config node1_config.toml
```

The node will start listening on:
- **RPC**: http://127.0.0.1:50051
- **WebSocket**: ws://127.0.0.1:50052 (if enabled)

### 2. Choose Your Language

Select the language you're comfortable with and follow the README in that directory.

## Examples Comparison

| Feature | Rust | Python | JavaScript |
|---------|------|--------|------------|
| **Setup Time** | Medium | Fast | Fast |
| **Performance** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| **Type Safety** | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐ |
| **Ease of Use** | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| **Best For** | Production | Learning | Web Integration |

## Example Overview

### Basic RPC Operations

Query basic blockchain information without authentication.

**Demonstrates:**
- Chain ID retrieval
- Latest block number
- Block details
- Gas price queries

**Files:**
- Rust: `rust/src/basic_rpc.rs`
- Python: `python/basic_rpc.py`
- JavaScript: `javascript/basic_rpc.js`

**Try it:**

```bash
# Rust
cd examples/rust && cargo run --release --bin basic_rpc

# Python
cd examples/python && python basic_rpc.py

# JavaScript
cd examples/javascript && node basic_rpc.js
```

### Balance Checker

Query account balances at different blocks.

**Demonstrates:**
- Balance queries
- Wei to ether conversion
- Historical balance lookups
- Multiple account queries

**Files:**
- Rust: `rust/src/balance_checker.rs`
- Python: `python/balance_checker.py`
- JavaScript: `javascript/balance_checker.js`

**Try it:**

```bash
# Rust
cd examples/rust && cargo run --release --bin balance_checker

# Python
cd examples/python && python balance_checker.py

# JavaScript
cd examples/javascript && node balance_checker.js
```

### Transaction Sender

Prepare and send transactions to the network.

**Demonstrates:**
- Transaction creation
- Nonce retrieval
- Gas price queries
- Transaction signing (demonstrated)
- RLP encoding (explained)

**Files:**
- Rust: `rust/src/transaction_sender.rs`
- Python: `python/transaction_sender.py`
- JavaScript: `javascript/transaction_sender.js`

**Try it:**

```bash
# Rust
cd examples/rust && cargo run --release --bin transaction_sender

# Python
cd examples/python && python transaction_sender.py

# JavaScript
cd examples/javascript && node transaction_sender.js
```

### WebSocket Listener

Subscribe to real-time blockchain events.

**Demonstrates:**
- WebSocket connections
- Event subscriptions
- Real-time block notifications
- Pending transaction tracking
- Event filtering

**Files:**
- Rust: `rust/src/websocket_listener.rs`
- Python: `python/websocket_listener.py`
- JavaScript: `javascript/websocket_listener.js`

**Try it:**

```bash
# Rust
cd examples/rust && cargo run --release --bin websocket_listener

# Python
cd examples/python && python websocket_listener.py

# JavaScript
cd examples/javascript && node websocket_listener.js
```

### Smart Contract Interaction

Interact with smart contracts using ABI encoding/decoding.

**Demonstrates:**
- Reading contract state (eth_call)
- Encoding contract function calls
- ERC-20 token interactions
- Batch contract queries
- Contract code verification

**Files:**
- Rust: `rust/src/contract_interaction.rs`
- Python: `python/contract_interaction.py`
- JavaScript: `javascript/contract_interaction.js`

**Try it:**

```bash
# Rust
cd examples/rust && cargo run --release --bin contract_interaction

# Python
cd examples/python && python contract_interaction.py

# JavaScript
cd examples/javascript && node contract_interaction.js
```

### Batch RPC Requests

Efficiently combine multiple RPC calls into a single request.

**Demonstrates:**
- Batch request formatting
- Multiple balance queries
- Block data fetching
- Transaction lookups
- Storage slot reads

**Files:**
- Rust: `rust/src/batch_rpc_requests.rs`
- Python: `python/batch_rpc_requests.py`
- JavaScript: `javascript/batch_rpc_requests.js`

**Try it:**

```bash
# Rust
cd examples/rust && cargo run --release --bin batch_rpc_requests

# Python
cd examples/python && python batch_rpc_requests.py

# JavaScript
cd examples/javascript && node batch_rpc_requests.js
```

### Health Check and Monitoring

Monitor blockchain node health and performance metrics.

**Demonstrates:**
- Node connectivity checks
- Sync status verification
- Peer connection monitoring
- Gas price tracking
- Performance metrics collection
- Alert threshold patterns

**Files:**
- Rust: `rust/src/health_check_monitoring.rs`
- Python: `python/health_check_monitoring.py`
- JavaScript: `javascript/health_check_monitoring.js`

**Try it:**

```bash
# Rust
cd examples/rust && cargo run --release --bin health_check_monitoring

# Python
cd examples/python && python health_check_monitoring.py

# JavaScript
cd examples/javascript && node health_check_monitoring.js
```

### Rate-Limiting Utilities

Implement various rate-limiting strategies for RPC calls.

**Demonstrates:**
- Token bucket algorithm
- Sliding window rate limiting
- Per-method limits
- Exponential backoff
- Adaptive rate limiting

**Files:**
- Rust: `rust/src/rate_limiting_utilities.rs`
- Python: `python/rate_limiting_utilities.py`
- JavaScript: `javascript/rate_limiting_utilities.js`

**Try it:**

```bash
# Rust
cd examples/rust && cargo run --bin rate_limiting_utilities

# Python
cd examples/python && python rate_limiting_utilities.py

# JavaScript
cd examples/javascript && node rate_limiting_utilities.js
```

## RPC Methods Covered

### Information Queries
- `eth_chainId` - Network chain identifier
- `eth_blockNumber` - Latest block height
- `eth_gasPrice` - Current gas price

### Account State
- `eth_getBalance` - Account balance at block
- `eth_getTransactionCount` - Transaction count (nonce)
- `eth_getCode` - Contract code

### Block Data
- `eth_getBlockByNumber` - Get block by height
- `eth_getBlockByHash` - Get block by hash

### Transactions
- `eth_sendRawTransaction` - Submit signed transaction
- `eth_getTransaction` - Transaction details
- `eth_getTransactionReceipt` - Transaction receipt

### WebSocket Subscriptions
- `eth_subscribe newHeads` - New block headers
- `eth_subscribe newPendingTransactions` - Pending transactions
- `eth_subscribe logs` - Contract events

## Configuration

All examples use environment variables for configuration.

### Environment Variables

```env
# RPC Endpoint (HTTP)
NORN_RPC_URL=http://127.0.0.1:50051

# WebSocket Endpoint
NORN_WS_URL=ws://127.0.0.1:50052

# Account Addresses
ACCOUNT_ADDRESS=0x0000000000000000000000000000000000000000
RECIPIENT_ADDRESS=0x1111111111111111111111111111111111111111

# Transaction Parameters
PRIVATE_KEY=0x0000000000000000000000000000000000000000000000000000000000000001
TRANSACTION_VALUE=1000000000000000000
GAS_PRICE=1000000000
GAS_LIMIT=21000
```

### Setup

Each language directory has an `.env.example` file:

```bash
# Copy and configure
cp .env.example .env
```

Then edit `.env` with your values:
```env
NORN_RPC_URL=http://your-node:50051
ACCOUNT_ADDRESS=0xyour_account_address
# ... other variables
```

## Common Patterns

### Error Handling

**Rust:**
```rust
match client.request("eth_blockNumber", rpc_params![]).await {
    Ok(result) => println!("Block: {}", result),
    Err(e) => eprintln!("Error: {}", e),
}
```

**Python:**
```python
try:
    result = client.get_balance(address)
except Exception as e:
    print(f"Error: {e}")
```

**JavaScript:**
```javascript
try {
  const balance = await getBalance(address);
  console.log(balance);
} catch (error) {
  console.error('Error:', error.message);
}
```

### Retry Logic

All examples can be extended with retry logic for production use:

**Rust:**
```rust
for attempt in 0..3 {
    match result {
        Ok(val) => return Ok(val),
        Err(e) if attempt < 2 => {
            tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
            continue;
        }
        Err(e) => return Err(e),
    }
}
```

### Rate Limiting

For production workloads, implement rate limiting:

**Python:**
```python
from time import time, sleep
from functools import wraps

def rate_limit(calls_per_second):
    min_interval = 1.0 / calls_per_second
    last = [0.0]
    
    def decorator(f):
        def wrapper(*args, **kwargs):
            elapsed = time() - last[0]
            if elapsed < min_interval:
                sleep(min_interval - elapsed)
            result = f(*args, **kwargs)
            last[0] = time()
            return result
        return wrapper
    return decorator
```

## Security Considerations

⚠️ **Important Security Notes:**

1. **Private Keys**: Never hardcode private keys. Use:
   - Environment variables (development only)
   - Hardware wallets
   - Key management systems
   - Secure vaults

2. **RPC Endpoints**: Use HTTPS/WSS for remote endpoints

3. **Transaction Verification**: Always verify transactions before signing

4. **Gas Limits**: Set appropriate limits to prevent fund loss

5. **Testing**: Test extensively on testnet before production

## Performance Tips

1. **Batch Requests**: Send multiple queries in one request
2. **Connection Pooling**: Reuse HTTP connections
3. **WebSocket**: Use subscriptions instead of polling
4. **Caching**: Cache frequently accessed data
5. **Async Operations**: Use async I/O for non-blocking calls

## Troubleshooting

### Node Not Running
```bash
# Check if node is responding
curl http://127.0.0.1:50051
```

### Connection Refused
- Ensure node is running
- Check RPC port is 50051
- Verify firewall settings

### Invalid Account Format
- Ensure 42-character hex address (0x + 40 hex chars)
- Example: `0x` + 40 hex digits

### Transaction Failures
- Verify account nonce
- Check account has sufficient balance
- Ensure gas price and limit are adequate

### WebSocket Issues
- Confirm WebSocket support is enabled
- Verify WS port is 50052
- Check firewall allows WebSocket

## Next Steps

1. **Learn More**: Read individual README files for each language
2. **Extend**: Add contract interaction examples
3. **Monitor**: Build monitoring dashboards
4. **Automate**: Create automated trading/arbitrage bots
5. **Scale**: Deploy examples across multiple nodes

## Advanced Topics

### Contract Interaction

Once comfortable with basic examples, explore:
- ABI encoding/decoding
- Contract deployment
- Event filtering
- State queries

### Development Workflows

- Local testing with private testnet
- Integration testing with CI/CD
- Performance benchmarking
- Security auditing

### Production Deployment

- Connection pooling
- Error recovery
- Rate limiting
- Health checks
- Monitoring and alerting

## Resources

### Official Documentation
- [Ethereum JSON-RPC](https://ethereum.org/en/developers/docs/apis/json-rpc/)
- [Norn Documentation](../doc/)

### Language-Specific
- **Rust**: [Tokio Guide](https://tokio.rs/) | [jsonrpsee Docs](https://docs.rs/jsonrpsee/)
- **Python**: [Requests Docs](https://docs.python-requests.org/) | [Asyncio Docs](https://docs.python.org/3/library/asyncio.html)
- **JavaScript**: [ethers.js](https://docs.ethers.org/) | [Web3.js](https://web3js.readthedocs.io/)

### Tools
- [MetaMask](https://metamask.io/) - Browser wallet
- [Remix IDE](https://remix.ethereum.org/) - Smart contract development
- [Hardhat](https://hardhat.org/) - Ethereum development environment
- [Foundry](https://book.getfoundry.sh/) - Rust EVM toolkit

## Contributing

Contributions are welcome! Ideas for new examples:
- Contract interaction
- Multi-call batching
- Real-time monitoring
- Arbitrage bots
- Wallet management

## Support

For issues or questions:
1. Check the individual language READMEs
2. Review troubleshooting sections
3. Check Norn documentation
4. Open a GitHub issue

## License

These examples are provided as educational material. See LICENSE for details.

---

**Happy coding! 🚀**

Start with the language you're most comfortable with and explore the examples step by step.
