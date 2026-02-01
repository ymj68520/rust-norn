#!/bin/bash

# Test script to verify norn blockchain is ready for Remix
# This script checks if all required RPC methods are working

set -e

RPC_URL="${RPC_URL:-http://localhost:51051}"

echo "Testing Norn Blockchain for Remix Compatibility"
echo "=============================================="
echo "RPC URL: $RPC_URL"
echo ""

# Color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Test function
test_rpc() {
    local method=$1
    local params=$2
    local description=$3

    echo -n "Testing $description... "

    response=$(curl -s -X POST "$RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}")

    if echo "$response" | grep -q '"result"'; then
        echo -e "${GREEN}✓ PASS${NC}"
        if [ "$VERBOSE" = "true" ]; then
            echo "  Response: $response" | head -c 200
            echo ""
        fi
        return 0
    else
        echo -e "${RED}✗ FAIL${NC}"
        echo "  Error: $response"
        return 1
    fi
}

# Check if node is running
echo "1. Checking if node is running..."
if ! curl -s "$RPC_URL" > /dev/null 2>&1; then
    echo -e "${RED}Cannot connect to $RPC_URL${NC}"
    echo "Please start the norn node first:"
    echo "  ./target/release/norn --config node1_config.toml"
    exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""

# Test required methods
echo "2. Testing Remix-required methods:"

test_rpc "web3_clientVersion" "[]" "web3_clientVersion"
test_rpc "eth_chainId" "[]" "eth_chainId"
test_rpc "eth_blockNumber" "[]" "eth_blockNumber"
test_rpc "net_version" "[]" "net_version"

echo ""
echo "3. Testing account methods:"

# Get a test address (use zero address for testing)
ZERO_ADDRESS="0x0000000000000000000000000000000000000000"

test_rpc "eth_getBalance" "[\"$ZERO_ADDRESS\", \"latest\"]" "eth_getBalance"
test_rpc "eth_getTransactionCount" "[\"$ZERO_ADDRESS\", \"latest\"]" "eth_getTransactionCount"
test_rpc "eth_getCode" "[\"$ZERO_ADDRESS\", \"latest\"]" "eth_getCode"

echo ""
echo "4. Testing block methods:"

test_rpc "eth_getBlockByNumber" "[\"latest\", false]" "eth_getBlockByNumber"

echo ""
echo "5. Testing gas methods:"

test_rpc "eth_gasPrice" "[]" "eth_gasPrice"

echo ""
echo "6. Testing contract interaction (eth_call):"

# Simple eth_call to get balance
test_rpc "eth_call" "[{\"to\":\"$ZERO_ADDRESS\",\"data\":\"0x\"},\"latest\"]" "eth_call"

echo ""
echo "7. Testing development methods:"

# Test faucet (will give ETH to zero address)
test_rpc "dev_faucet" "[\"$ZERO_ADDRESS\", \"1000000000000000000\"]" "dev_faucet"

echo ""
echo "=============================================="
echo -e "${GREEN}All tests completed!${NC}"
echo ""
echo "Your norn node is ready for Remix deployment!"
echo ""
echo "Next steps:"
echo "1. Open https://remix.ethereum.org/"
echo "2. In 'Deploy & Run Transactions' > 'Environment', select 'Custom Provider'"
echo "3. Enter: $RPC_URL"
echo "4. Click 'OK' to connect"
echo ""
echo "For more information, see REMIX_GUIDE.md"
