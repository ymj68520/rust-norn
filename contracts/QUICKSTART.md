# Quick Start Guide - Norn Smart Contracts

This guide will help you quickly deploy and test smart contracts on the norn blockchain.

## Prerequisites

1. **Install Node.js dependencies:**
```bash
npm install
```

2. **Start the norn blockchain:**
```bash
# In a separate terminal
./target/release/norn --config config.toml
```

## Step 1: Compile Contracts

```bash
npm run compile
```

Expected output:
```
Compiled 3 Solidity files successfully
```

## Step 2: Run Tests

### Run all tests:
```bash
npm test
```

### Run specific test suites:
```bash
# Unit tests only
npm run test:unit

# End-to-end tests
npm run test:e2e
```

Expected results:
- ✅ NornToken: 9 passing
- ✅ SimpleStorage: 8 passing
- ✅ E2E: 10 scenarios

## Step 3: Deploy Contracts

### Option A: Deploy to local Hardhat network

1. **Start Hardhat node:**
```bash
npm run node
```

2. **Deploy contracts (in new terminal):**
```bash
npm run deploy:local
```

### Option B: Deploy to running norn node

1. **Configure RPC endpoint:**
```bash
export NORN_RPC_URL="http://localhost:50051"
```

2. **Deploy:**
```bash
npm run deploy
```

Expected deployment output:
```
Deploying contracts to network: norn_local

Deploying NornToken...
NornToken deployed to: 0x...

Deploying SimpleStorage...
SimpleStorage deployed to: 0x...

Deploying Ballot...
Ballot deployed to: 0x...

=== Deployment Summary ===
Network: norn_local
Deployer: 0x...
```

## Step 4: Interact with Deployed Contracts

### Using Hardhat Console

```bash
npx hardhat console --network norn_local
```

**Example interactions:**

```javascript
// Get deployed contract addresses
const deployment = require("./contracts/deploy/deployment.json")
const { NornToken, SimpleStorage, Ballot } = deployment.contracts

// Connect to NornToken
const token = await ethers.getContractAt("NornToken", NornToken)

// Check balance
const balance = await token.balanceOf(deployment.deployer)
console.log("Balance:", ethers.formatEther(balance), "NORN")

// Mint tokens
await token.mint("0x...", ethers.parseEther("1000"))

// Connect to SimpleStorage
const storage = await ethers.getContractAt("SimpleStorage", SimpleStorage)

// Store value
await storage.setValue(42)
const value = await storage.getValue()
console.log("Stored value:", value.toString())

// Add item
await storage.addItem("My first item")
const itemCount = await storage.itemCount()
console.log("Total items:", itemCount.toString())
```

## Step 5: Run Performance Benchmarks

```bash
npm run benchmark
```

This will test:
- ERC20 operations (transfer, mint, approve)
- Storage operations (read/write)
- Batch operations
- Stress tests

## Common Tasks

### Mint Tokens

```javascript
const token = await ethers.getContractAt("NornToken", TOKEN_ADDRESS)
await token.mint(RECIPIENT_ADDRESS, ethers.parseEther("1000"))
```

### Vote in Ballot

```javascript
const ballot = await ethers.getContractAt("Ballot", BALLOT_ADDRESS)
await ballot.giveRightToVote(VOTER_ADDRESS)
await ballot.connect(signer).vote(PROPOSAL_ID)
```

### Store Data

```javascript
const storage = await ethers.getContractAt("SimpleStorage", STORAGE_ADDRESS)
await storage.setValue(123)
await storage.setMessage("Hello, Norn!")
```

## Troubleshooting

### "Network not found" error
Ensure your norn node is running and RPC URL is correct in `.env`.

### "Insufficient funds" error
Make sure the deployer account has ETH for gas. Check with:
```javascript
const balance = await ethers.provider.getBalance(address)
console.log(ethers.formatEther(balance), "ETH")
```

### "Contract too large" error
Your contract exceeds the 24KB EIP-170 limit. Try:
- Removing unused code
- Enabling optimizer in `hardhat.config.js`
- Splitting into multiple contracts

### Tests failing
```bash
# Clean and recompile
npm run clean
npm run compile
npm test
```

## Next Steps

1. **Create your own contract:**
```bash
# Create new file
touch contracts/MyContract.sol

# Write your contract
# Add tests in contracts/test/MyContract.test.js

# Run tests
npm test
```

2. **Deploy to testnet:**
```bash
# Update .env with testnet RPC
export NORN_RPC_URL="https://testnet.norn.io"
npm run deploy
```

3. **Verify contracts:**
```bash
npx hardhat verify --network testnet CONTRACT_ADDRESS CONSTRUCTOR_ARGS
```

## Resources

- **Full Documentation:** [contracts/README.md](contracts/README.md)
- **Hardhat Docs:** https://hardhat.org/docs
- **OpenZeppelin:** https://docs.openzeppelin.com/contracts
- **Solidity Docs:** https://docs.soliditylang.org

## Support

- **Issues:** https://github.com/ymj68520/rust-norn/issues
- **Discussions:** https://github.com/ymj68520/rust-norn/discussions
- **Docs:** https://norn.io/docs
