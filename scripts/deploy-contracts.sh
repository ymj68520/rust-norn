#!/bin/bash

# Smart contract deployment and testing script
# This script deploys contracts to the running norn network

set -e

echo "==================================================="
echo "  Deploy and Test Smart Contracts"
echo "==================================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Check if Node.js dependencies are installed
if [ ! -d "node_modules" ]; then
    echo -e "${YELLOW}Installing Node.js dependencies...${NC}"
    npm install
fi

# Check if contracts are compiled
if [ ! -d "artifacts" ]; then
    echo -e "${YELLOW}Compiling smart contracts...${NC}"
    npx hardhat compile
fi

# Check if node is running
echo -e "${GREEN}Checking if node is running...${NC}"
if ! curl -s http://localhost:8545 > /dev/null 2>&1; then
    echo -e "${RED}Error: Node is not running on http://localhost:8545${NC}"
    echo "Please start the nodes first using: ./start-nodes.sh"
    exit 1
fi

echo -e "${GREEN}Node is running!${NC}"
echo ""

# Test basic connectivity
echo -e "${GREEN}Testing RPC connectivity...${NC}"
CHAIN_ID=$(curl -s -X POST http://localhost:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
    | grep -o '"result":"[^"]*"' | cut -d'"' -f4)

if [ -n "$CHAIN_ID" ]; then
    echo -e "${GREEN}Connected! Chain ID: $CHAIN_ID${NC}"
else
    echo -e "${RED}Failed to connect to RPC${NC}"
    exit 1
fi

# Get block number
BLOCK_NUMBER=$(curl -s -X POST http://localhost:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
    | grep -o '"result":"[^"]*"' | cut -d'"' -f4)

echo -e "${GREEN}Current block number: $BLOCK_NUMBER${NC}"
echo ""

# Option to deploy contracts
echo "Would you like to deploy smart contracts? (y/n)"
read -r response

if [[ "$response" =~ ^([yY][eE][sS]|[yY])$ ]]; then
    echo ""
    echo -e "${GREEN}Deploying contracts...${NC}"
    echo -e "${YELLOW}Note: You'll need to configure a private key in .env file${NC}"
    echo ""

    # Check if .env file exists
    if [ ! -f ".env" ]; then
        echo -e "${YELLOW}Creating .env file...${NC}"
        cat > .env << 'EOF'
# Norn RPC URL
NORN_RPC_URL=http://localhost:8545

# Private key for deployment (replace with your private key)
# Generate a key using: npx hardhat node --keyfile
PRIVATE_KEY=

# Chain ID
CHAIN_ID=31337
EOF
        echo -e "${RED}Please edit .env file and add a private key${NC}"
        echo "You can generate a test key using: npx hardhat node"
        exit 1
    fi

    # Deploy using Hardhat
    npx hardhat run contracts/scripts/deploy.js --network norn
fi

echo ""
echo "==================================================="
echo -e "${GREEN}Setup Complete!${NC}"
echo "==================================================="
echo ""
echo "Next steps:"
echo "  1. Edit .env file and add your private key"
echo "  2. Deploy contracts: npm run deploy"
echo "  3. Run tests: npm test"
echo "  4. View logs: docker compose logs -f"
echo ""
echo "RPC Endpoints:"
echo "  - Node 1: http://localhost:8545"
echo "  - Node 2: http://localhost:8546"
echo "  - Node 3: http://localhost:8547"
echo ""
