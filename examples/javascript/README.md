# JavaScript Client Examples

This directory contains JavaScript examples demonstrating how to interact with a Norn blockchain node.

## Prerequisites

- Node.js 14+ ([Download Node.js](https://nodejs.org/))
- npm (included with Node.js)

## Setup

1. Install dependencies:
```bash
npm install
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
PRIVATE_KEY=0x...
```

## Running Examples

### 1. Basic RPC Operations

Query blockchain information:

```bash
npm run basic-rpc
```

Or:

```bash
node basic_rpc.js
```

**Output:**
```
=== Basic RPC Example ===

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

✅ Basic RPC example completed!
```

### 2. Balance Checker

Check account balances:

```bash
npm run balance-checker
```

Or:

```bash
node balance_checker.js
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
npm run transaction-sender
```

Or:

```bash
node transaction_sender.js
```

**Output:**
```
=== Transaction Sender Example ===

From: 0x0000...0000
To: 0x1111...1111

1. Get Current Nonce
   Nonce: 0

2. Get Gas Price
   Gas Price: 1000000000 wei

3. Transaction Information
   === Example Transaction Structure ===
   From: 0x...
   To: 0x...
   Nonce: 0
   Value: 1000000000000000000 wei (1 ether)
   Gas Price: 1000000000 wei
   Gas Limit: 21000
   Data: 0x (empty for value transfer)
```

### 4. WebSocket Listener

Subscribe to blockchain events:

```bash
npm run websocket-listener
```

Or:

```bash
node websocket_listener.js
```

**Output:**
```
=== WebSocket Listener Example ===

✅ Connected to WebSocket: ws://127.0.0.1:50052

📡 Subscription request sent for newHeads
📡 Subscription request sent for newPendingTransactions

✅ Subscribed to newHeads with ID: 0x1
✅ Subscribed to newPendingTransactions with ID: 0x2

Listening for events (press Ctrl+C to stop)...

🔗 [Block] New block received
   Height: 0x1a5
   Miner: 0x...
   Timestamp: 0x...

💰 [Tx] Pending transaction: 0x...
```

## Dependencies

- **axios** - HTTP client for RPC calls
- **ws** - WebSocket client for subscriptions
- **dotenv** - Environment variable management

Install with:
```bash
npm install
```

## File Structure

```
examples/javascript/
├── package.json              # npm configuration
├── .env.example              # Environment variables template
├── .gitignore                # Git ignore file
├── basic_rpc.js              # Basic RPC operations
├── balance_checker.js        # Balance checking
├── transaction_sender.js     # Transaction sending
├── websocket_listener.js     # WebSocket subscriptions
└── README.md                 # This file
```

## API Methods

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
PRIVATE_KEY=0x0000000000000000000000000000000000000000000000000000000000000001
TRANSACTION_VALUE=1000000000000000000
GAS_PRICE=1000000000
GAS_LIMIT=21000
```

## Common Tasks

### Check Account Balance

```javascript
const axios = require('axios');

async function getBalance(address) {
  const response = await axios.post('http://127.0.0.1:50051', {
    jsonrpc: '2.0',
    id: 1,
    method: 'eth_getBalance',
    params: [address, 'latest'],
  });
  
  const wei = parseInt(response.data.result, 16);
  const ether = wei / 1e18;
  return ether;
}
```

### Send a Transaction

```javascript
const ethers = require('ethers');

async function sendTransaction() {
  const provider = new ethers.providers.JsonRpcProvider(RPC_URL);
  const wallet = new ethers.Wallet(PRIVATE_KEY, provider);
  
  const tx = {
    to: RECIPIENT,
    value: ethers.utils.parseEther('1.0'),
  };
  
  const txResponse = await wallet.sendTransaction(tx);
  const receipt = await txResponse.wait();
  return receipt;
}
```

### Listen to Events

```javascript
const WebSocket = require('ws');

function subscribeToBlocks(wsUrl) {
  const ws = new WebSocket(wsUrl);
  
  ws.on('open', () => {
    ws.send(JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method: 'eth_subscribe',
      params: ['newHeads'],
    }));
  });
  
  ws.on('message', (data) => {
    console.log('New block:', JSON.parse(data));
  });
}
```

## Error Handling

All examples include error handling:

```javascript
try {
  const balance = await getBalance(account);
  console.log(balance);
} catch (error) {
  console.error('Error:', error.message);
}
```

## Advanced Usage

### Using Web3.js

```bash
npm install web3
```

```javascript
const Web3 = require('web3');
const web3 = new Web3('http://127.0.0.1:50051');

const balance = await web3.eth.getBalance(address);
```

### Using ethers.js

```bash
npm install ethers
```

```javascript
const ethers = require('ethers');
const provider = new ethers.providers.JsonRpcProvider(RPC_URL);
const balance = await provider.getBalance(address);
```

### Rate Limiting

```javascript
async function rateLimit(fn, callsPerSecond) {
  const minInterval = 1000 / callsPerSecond;
  let lastCalled = Date.now();
  
  return async function(...args) {
    const elapsed = Date.now() - lastCalled;
    if (elapsed < minInterval) {
      await new Promise(resolve => 
        setTimeout(resolve, minInterval - elapsed)
      );
    }
    lastCalled = Date.now();
    return fn(...args);
  };
}
```

## Troubleshooting

### Connection refused
- Ensure the Norn node is running
- Check the RPC URL is correct
- Verify network connectivity

```bash
curl http://127.0.0.1:50051
```

### Invalid account format
```javascript
// Ensure account is correct format
const account = '0x' + '0'.repeat(40);  // Valid
```

### WebSocket connection issues
- Check if node supports WebSocket
- Verify WS protocol (ws:// or wss://)
- Check firewall rules

## Testing

Run tests:
```bash
npm test
```

## Next Steps

1. Explore contract interaction
2. Implement transaction batching
3. Build monitoring dashboards
4. Create automated strategies

## Resources

- [Ethereum JSON-RPC API](https://ethereum.org/en/developers/docs/apis/json-rpc/)
- [ethers.js Documentation](https://docs.ethers.org/)
- [Web3.js Documentation](https://web3js.readthedocs.io/)
- [Axios Documentation](https://axios-http.com/)

## Contributing

Improvements and additional examples are welcome!
