#!/bin/bash

# Simple RPC connectivity test script

echo "Testing Norn Node RPC Connectivity"
echo "=================================="
echo ""

# Test endpoints
ENDPOINTS=(
    "http://localhost:50052"
    "http://localhost:50053"
    "http://localhost:50054"
)

for endpoint in "${ENDPOINTS[@]}"; do
    echo "Testing $endpoint..."

    # Test with timeout
    if curl -s --max-time 3 -X POST "$endpoint" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
        > /dev/null 2>&1; then

        # Get chain ID
        CHAIN_ID=$(curl -s -X POST "$endpoint" \
            -H "Content-Type: application/json" \
            -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
            | grep -o '"result":"[^"]*"' | cut -d'"' -f4)

        # Get block number
        BLOCK_NUM=$(curl -s -X POST "$endpoint" \
            -H "Content-Type: application/json" \
            -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
            | grep -o '"result":"[^"]*"' | cut -d'"' -f4)

        echo "  ✓ Connected!"
        echo "  Chain ID: $CHAIN_ID"
        echo "  Block: $BLOCK_NUM"
    else
        echo "  ✗ Failed to connect"
    fi
    echo ""
done

echo "=================================="
echo "Test complete!"
