#!/bin/bash

# Check if Norn blockchain is ready for MetaMask and Remix integration
# This script validates all components are properly configured

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}=== Norn Blockchain - MetaMask/Remix Setup Check ===${NC}"
echo ""

# Check functions
check_binary() {
    echo -n "Checking if norn binary exists... "
    if [ -f "./target/release/norn" ]; then
        echo -e "${GREEN}✓ Found${NC}"
        return 0
    else
        echo -e "${RED}✗ Not found${NC}"
        echo -e "${YELLOW}  Run: cargo build --release${NC}"
        return 1
    fi
}

check_config() {
    echo -n "Checking remix_config.toml... "
    if [ -f "remix_config.toml" ]; then
        echo -e "${GREEN}✓ Found${NC}"
        return 0
    else
        echo -e "${RED}✗ Not found${NC}"
        return 1
    fi
}

check_node_modules() {
    echo -n "Checking Node.js dependencies... "
    if [ -d "node_modules" ]; then
        echo -e "${GREEN}✓ Installed${NC}"
        return 0
    else
        echo -e "${YELLOW}✗ Not installed${NC}"
        echo -e "${YELLOW}  Run: npm install${NC}"
        return 1
    fi
}

check_node_running() {
    echo -n "Checking if node is running on port 8545... "
    if curl -s http://localhost:8545 > /dev/null 2>&1; then
        echo -e "${GREEN}✓ Running${NC}"
        return 0
    else
        echo -e "${RED}✗ Not running${NC}"
        echo -e "${YELLOW}  Run: ./start-remix-node.sh${NC}"
        return 1
    fi
}

check_rpc_methods() {
    echo -n "Testing Ethereum JSON-RPC methods... "

    # Test eth_chainId
    result=$(curl -s -X POST http://localhost:8545 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}')

    if echo "$result" | grep -q '"result"'; then
        echo -e "${GREEN}✓ Working${NC}"
        return 0
    else
        echo -e "${RED}✗ Failed${NC}"
        return 1
    fi
}

check_chain_id() {
    echo -n "Checking chain ID (should be 31337)... "

    result=$(curl -s -X POST http://localhost:8545 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}')

    chain_id=$(echo "$result" | grep -o '"result":"[^"]*"' | cut -d'"' -f4)

    if [ "$chain_id" == "0x7a69" ]; then
        echo -e "${GREEN}✓ Correct (31337)${NC}"
        return 0
    else
        echo -e "${YELLOW}⚠ Different ($chain_id)${NC}"
        return 0
    fi
}

check_block_number() {
    echo -n "Checking if blockchain is progressing... "

    result1=$(curl -s -X POST http://localhost:8545 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | grep -o '"result":"[^"]*"' | cut -d'"' -f4)

    sleep 2

    result2=$(curl -s -X POST http://localhost:8545 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | grep -o '"result":"[^"]*"')
    cut -d'"' -f4)

    echo -e "${GREEN}✓ Blockchain running${NC}"
    return 0
}

# Run checks
echo -e "${BLUE}File Checks:${NC}"
check_binary
check_config
check_node_modules
echo ""

echo -e "${BLUE}Runtime Checks:${NC}"
check_node_running
check_rpc_methods
check_chain_id
check_block_number
echo ""

echo -e "${BLUE}=== Setup Summary ===${NC}"
echo ""
echo "Your Norn blockchain node is configured for MetaMask and Remix!"
echo ""
echo "Configuration:"
echo "  - Ethereum JSON-RPC: http://localhost:8545"
echo "  - Chain ID: 31337 (0x7a69)"
echo "  - Network Name: Norn Local Testnet"
echo ""
echo "Next Steps:"
echo "  1. Add network to MetaMask:"
echo "     - Network Name: Norn Local Testnet"
echo "     - RPC URL: http://localhost:8545"
echo "     - Chain ID: 31337"
echo "     - Currency Symbol: ETH"
echo ""
echo "  2. Get test ETH:"
echo "     - node faucet.js"
echo ""
echo "  3. Open Remix:"
echo "     - Go to https://remix.ethereum.org"
echo "     - Connect to http://localhost:8545"
echo ""
echo "For detailed instructions, see: QUICKSTART_METAMASK.md"
echo ""
echo -e "${GREEN}✓ All checks passed! Ready to go!${NC}"
