# Norn Solidity JavaScript SDK

JavaScript SDK for deploying and interacting with Solidity smart contracts on the Norn blockchain.

## Setup

```bash
cd examples/javascript/solidity
npm install
```

## Configuration

Copy `.env.example` to `.env` and configure:

```env
NORN_ETH_RPC_URL=http://localhost:9545
PRIVATE_KEY=your_private_key_here
CHAIN_ID=31337
```

## Usage

### Deploy a Contract

```bash
# Set path to your compiled artifact
export CONTRACT_ARTIFACT=./artifacts/MyContract.json

# Deploy
node src/deploy.js
```

### Interact with a Contract

```bash
# Call a view function
node src/interact.js call 0x1234... getValue

# Send a transaction
node src/interact.js send 0x1234... setValue 42

# Get events
node src/interact.js events 0x1234... Transfer

# Check balance
node src/interact.js balance 0x1234...
```

### Programmatic Usage

```javascript
const { NornSoliditySDK } = require('./src/index');
const fs = require('fs');

// Initialize SDK
const sdk = new NornSoliditySDK('http://localhost:9545', process.env.PRIVATE_KEY);

// Load artifact
const artifact = sdk.loadArtifact('./artifacts/SimpleStorage.json');

// Deploy
const contract = await sdk.deploy(artifact);
const address = await contract.getAddress();

// Call function
const value = await sdk.call(address, 'get');
console.log('Value:', value.toString());

// Send transaction
await sdk.send(address, 'set', [42n]);

// Attach to existing contract
const existing = sdk.attach('0x1234...', artifact);
```

## Compile Solidity Contracts

```bash
# Using the Rust compiler (requires solc installed)
# The Rust backend handles compilation via the norn_solidity crate

# Or compile manually with solc:
solc --abi --bin --overwrite -o ./artifacts contracts/MyContract.sol
```

## Architecture

```
examples/javascript/solidity/
├── src/
│   ├── index.js       # NornSoliditySDK class
│   ├── deploy.js      # Contract deployment script
│   ├── interact.js    # Contract interaction CLI
│   ├── compile.js     # Solidity compilation
│   └── test.js        # End-to-end test
├── package.json
└── .env.example
```

## API Reference

### `NornSoliditySDK`

| Method | Description |
|--------|-------------|
| `constructor(rpcUrl, privateKey)` | Initialize SDK |
| `deploy(artifact, args, options)` | Deploy contract from artifact |
| `attach(address, artifact)` | Attach to existing contract |
| `call(address, fn, args)` | Call view/pure function |
| `send(address, fn, args, options)` | Send transaction |
| `getEvents(address, eventName)` | Get event logs |
| `encodeFunctionCall(abi, fn, args)` | Encode function data |
| `decodeFunctionResult(abi, fn, data)` | Decode return data |
