# Norn Blockchain - Smart Contracts

This directory contains smart contracts and development tools for the norn blockchain.

## 🚀 Quick Start

### Prerequisites

- Node.js >= 18.x
- npm or yarn

### Installation

```bash
npm install
```

## 📜 Contracts

### NornToken (ERC20)
A standard ERC20 token with minting and burning capabilities.

**Features:**
- Initial supply: 10,000,000 NORN
- Total supply cap: 1,000,000,000 NORN
- Owner can mint and burn tokens
- OpenZeppelin ERC20 implementation

**Usage:**
```solidity
NornToken token = new NornToken();
token.mint(address, amount);
token.burn(address, amount);
```

### SimpleStorage
A simple storage contract demonstrating basic EVM functionality.

**Features:**
- Store/retrieve uint256 values
- Store/retrieve string messages
- Add and retrieve items with metadata
- Event emissions for all state changes

**Usage:**
```solidity
SimpleStorage storage = new SimpleStorage();
storage.setValue(42);
storage.setMessage("Hello, Norn!");
storage.addItem("Some data");
```

### Ballot
A voting contract demonstrating complex state management.

**Features:**
- Chairperson-controlled voting rights
- Vote delegation
- Multiple proposals
- Winner calculation

**Usage:**
```solidity
string[] proposals = ["Proposal 1", "Proposal 2"];
Ballot ballot = new Ballot(proposals);
ballot.giveRightToVote(voter);
ballot.vote(proposalId);
```

## 🧪 Testing

### Run all tests
```bash
npm test
```

### Run specific test file
```bash
npx hardhat test contracts/test/NornToken.test.js
```

### Run tests with gas reporting
```bash
npx hardhat test --reporter gas
```

### Test coverage
```bash
npx hardhat coverage
```

## 🚢 Deployment

### Deploy to local network
```bash
# Start local Hardhat node (in separate terminal)
npm run node

# Deploy contracts
npm run deploy:local
```

### Deploy to remote network
```bash
# Set up environment variables
export NORN_RPC_URL="http://your-node:8545"
export PRIVATE_KEY="your-private-key"

# Deploy
npm run deploy
```

## 📝 Scripts

### Compile contracts
```bash
npm run compile
```

### Clean build artifacts
```bash
npm run clean
```

### Start Hardhat node
```bash
npm run node
```

## 🔧 Configuration

### Hardhat Config
Configuration is in `hardhat.config.js`:

```javascript
networks: {
  norn: {
    url: "http://localhost:8545",
    chainId: 31337,
    accounts: [...]
  }
}
```

### Compiler Settings
- Solidity version: 0.8.20
- Optimizer enabled: 200 runs

## 📊 Gas Optimization

All contracts are optimized for gas efficiency:

| Contract | Deployment Gas | Average Tx Gas |
|----------|---------------|----------------|
| NornToken | ~1,200,000 | ~50,000 |
| SimpleStorage | ~800,000 | ~30,000 |
| Ballot | ~1,500,000 | ~40,000 |

## 🔍 Integration with Norn

These contracts are designed to work seamlessly with the norn blockchain's EVM implementation:

- ✅ EIP-1559 fee market support
- ✅ EIP-2930 access lists
- ✅ EIP-170 contract size limits (24KB)
- ✅ Full ERC20 standard compliance
- ✅ Event logging with bloom filters
- ✅ Transaction receipts

## 📖 Examples

### Mint tokens
```javascript
const token = await ethers.getContractAt("NornToken", tokenAddress);
await token.mint(recipient, ethers.parseEther("1000"));
```

### Store value
```javascript
const storage = await ethers.getContractAt("SimpleStorage", storageAddress);
await storage.setValue(42);
const value = await storage.getValue();
```

### Vote in ballot
```javascript
const ballot = await ethers.getContractAt("Ballot", ballotAddress);
await ballot.giveRightToVote(voterAddress);
await ballot.vote(0); // Vote for first proposal
```

## 🐛 Debugging

### View transaction details
```bash
npx hardhat run scripts/debug.js --network norn
```

### Console logging
```bash
npx hardhat console --network norn
```

## 📚 Resources

- [Hardhat Documentation](https://hardhat.org/docs)
- [OpenZeppelin Contracts](https://docs.openzeppelin.com/contracts)
- [Solidity Documentation](https://docs.soliditylang.org)
- [Ethereum EIPs](https://eips.ethereum.org/)

## 🤝 Contributing

When adding new contracts:

1. Follow Solidity style guide
2. Add comprehensive NatSpec comments
3. Include test coverage for all functions
4. Optimize for gas efficiency
5. Document gas usage in README

## ⚠️ Security

- Never commit private keys
- Use `.env` for sensitive data
- Audit contracts before mainnet deployment
- Follow security best practices

## 📄 License

MIT License - See LICENSE file for details
