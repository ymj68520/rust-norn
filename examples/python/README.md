# Python Client Examples

This directory contains Python examples demonstrating how to interact with a Norn blockchain node.

## Prerequisites

- Python 3.8+
- pip (Python package manager)

## Setup

1. Install dependencies:
```bash
pip install -r requirements.txt
```

2. Create environment file:
```bash
cp .env.example .env
```

3. Configure `.env`:
```env
NORN_RPC_URL=http://127.0.0.1:50051
NORN_WS_URL=ws://127.0.0.1:50052
ACCOUNT_ADDRESS=0x...
RECIPIENT_ADDRESS=0x...
TRANSACTION_VALUE=1000000000000000000
GAS_PRICE=1000000000
GAS_LIMIT=21000
```

## Running Examples

### 1. Basic RPC Operations

Query blockchain information:

```bash
python basic_rpc.py
```

**Output:**
```
=== Norn RPC Client Example ===

1. Get Chain ID
   Chain ID: 0x1

2. Get Latest Block Number
   Block Number: 0x1a5

3. Get Block Information
   Block Hash: 0x...
   Miner: 0x...
   Timestamp: 0x...

4. Get Gas Price
   Gas Price: 1000000000 wei
```

### 2. Balance Checker

Check account balances and history:

```bash
python balance_checker.py
```

**Output:**
```
=== Balance Checker Example ===

Checking balance for: 0x0000...0000

1. Current Balance
   Balance (wei): 1,000,000,000,000,000,000
   Balance (ether): 1.000000000000000000

2. Balance History
   Block 0x0: 0.000000000000000000 ether
   Block 0x1: 1.000000000000000000 ether
   Block latest: 1.000000000000000000 ether

3. Multiple Accounts
   0x0000...0000: 1.000000 ether ✅
   0x1111...1111: 0.500000 ether ✅
```

### 3. Transaction Sender

Send transactions to the network:

```bash
python transaction_sender.py
```

**Output:**
```
=== Transaction Sender Example ===

1. Get Current Nonce
   Account: 0x0000...0000
   Nonce: 0

2. Get Gas Price
   Gas Price: 1000000000 wei

3. Transaction Information
   === Transaction Structure ===
   from: 0x0000...0000
   to: 0x1111...1111
   value: 1000000000000000000
   gasPrice: 1000000000
   gasLimit: 21000
   data: 0x
```

### 4. WebSocket Listener

Subscribe to blockchain events:

```bash
python websocket_listener.py
```

**Output:**
```
=== WebSocket Listener Example ===

✅ Connected to WebSocket: ws://127.0.0.1:50052

📡 Subscription request sent for newHeads
📡 Subscription request sent for newPendingTransactions

✅ Subscribed to newHeads with ID: 0x1
✅ Subscribed to newPendingTransactions with ID: 0x2

Listening for events (Ctrl+C to stop)...

🔗 [Block #1] New block received
   Height: 0x1a5
   Miner: 0x...
   Timestamp: 0x...

💰 [Tx #1] Pending transaction: 0x...
```

## API Methods Used

- `eth_chainId` - Get network chain ID
- `eth_blockNumber` - Get latest block number
- `eth_getBlockByNumber` - Get block details
- `eth_gasPrice` - Get current gas price
- `eth_getBalance` - Get account balance
- `eth_getTransactionCount` - Get account nonce
- `eth_sendRawTransaction` - Send transaction
- `eth_getTransactionReceipt` - Get transaction receipt
- WebSocket subscriptions: `newHeads`, `newPendingTransactions`

## Environment Variables

```env
# RPC endpoint (HTTP)
NORN_RPC_URL=http://127.0.0.1:50051

# WebSocket endpoint
NORN_WS_URL=ws://127.0.0.1:50052

# Account information
ACCOUNT_ADDRESS=0x0000000000000000000000000000000000000000
RECIPIENT_ADDRESS=0x1111111111111111111111111111111111111111

# Transaction parameters
TRANSACTION_VALUE=1000000000000000000
GAS_PRICE=1000000000
GAS_LIMIT=21000
```

## Utility Functions

### Convert wei to ether
```python
from balance_checker import wei_to_ether

balance_wei = 1000000000000000000
balance_ether = wei_to_ether(balance_wei)  # 1.0
```

### Convert ether to wei
```python
from balance_checker import ether_to_wei

balance_ether = 1.0
balance_wei = ether_to_wei(balance_ether)  # 1000000000000000000
```

## Error Handling

All examples include error handling:

```python
try:
    result = client.get_balance(account)
except Exception as e:
    print(f"Error: {e}")
```

## Advanced Usage

### Custom RPC Methods

Extend the client to support custom methods:

```python
class CustomClient(NornRPCClient):
    def custom_method(self, param):
        result = self._make_request("custom_method", [param])
        return result.get("result")
```

### Retry Logic

Examples include retry mechanisms for WebSocket connections:

```python
async def connect_with_retry(max_retries=3):
    for attempt in range(max_retries):
        try:
            listener = WebSocketListener()
            await listener.connect_and_listen()
            break
        except Exception as e:
            if attempt == max_retries - 1:
                raise
            await asyncio.sleep(2 ** attempt)
```

### Rate Limiting

For production use, implement rate limiting:

```python
import time
from functools import wraps

def rate_limit(calls_per_second):
    min_interval = 1.0 / calls_per_second
    last_called = [0.0]
    
    def decorator(func):
        def wrapper(*args, **kwargs):
            elapsed = time.time() - last_called[0]
            if elapsed < min_interval:
                time.sleep(min_interval - elapsed)
            result = func(*args, **kwargs)
            last_called[0] = time.time()
            return result
        return wrapper
    return decorator
```

## Testing

Run tests:

```bash
python -m pytest tests/
```

## Troubleshooting

### Connection refused
```python
# Check if the node is running
curl http://127.0.0.1:50051
```

### Invalid account format
```python
# Ensure account address is correct format
account = "0x" + "0" * 40  # Valid 160-bit address
```

### WebSocket connection issues
- Ensure WebSocket support is enabled on the node
- Check firewall rules
- Verify WS protocol is used (ws:// or wss://)

## Dependencies

- `requests` - HTTP client for RPC calls
- `websockets` - WebSocket client for subscriptions
- `python-dotenv` - Environment variable management

## Next Steps

1. Implement transaction signing with `eth_keys` or `web3.py`
2. Add contract interaction (ABI encoding/decoding)
3. Implement transaction batching
4. Create monitoring dashboards
5. Build automated trading/arbitrage bots

## Resources

- [Ethereum JSON-RPC API](https://ethereum.org/en/developers/docs/apis/json-rpc/)
- [Python Requests Documentation](https://docs.python-requests.org/)
- [Asyncio Documentation](https://docs.python.org/3/library/asyncio.html)
- [Web3.py Documentation](https://web3py.readthedocs.io/)

## Contributing

Improvements and additional examples are welcome!
