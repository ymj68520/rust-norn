#!/bin/bash
# Test Norn Blockchain Network Connectivity
# This script verifies that all nodes are running and accessible

set -e

echo "=========================================="
echo "  Norn Blockchain - Network Test"
echo "=========================================="
echo ""

# Define nodes
declare -A NODES
NODES["Node 1 (Bootstrap)"]="localhost:50051"
NODES["Node 2"]="localhost:50052"
NODES["Node 3"]="localhost:50053"
NODES["Node 4"]="localhost:50054"

# Check if nc (netcat) is available
if ! command -v nc &> /dev/null; then
    echo "Warning: netcat not found, using /dev/tcp fallback"
    USE_DEVTCP=true
else
    USE_DEVTCP=false
fi

# Function to check port
check_port() {
    local host=$(echo $1 | cut -d: -f1)
    local port=$(echo $1 | cut -d: -f2)
    
    if [ "$USE_DEVTCP" = true ]; then
        (echo > /dev/tcp/$host/$port) 2>/dev/null
    else
        nc -z -w2 $host $port 2>/dev/null
    fi
}

echo "Checking RPC endpoints..."
echo ""

ALL_OK=true
for name in "${!NODES[@]}"; do
    addr="${NODES[$name]}"
    if check_port "$addr"; then
        echo "  ✓ $name ($addr) - ONLINE"
    else
        echo "  ✗ $name ($addr) - OFFLINE"
        ALL_OK=false
    fi
done

echo ""
echo "=========================================="

if [ "$ALL_OK" = true ]; then
    echo "  All nodes are online!"
    echo "=========================================="
    echo ""
    echo "You can now interact with the nodes via gRPC:"
    echo ""
    echo "  Node 1: grpcurl -plaintext localhost:50051 list"
    echo "  Node 2: grpcurl -plaintext localhost:50052 list"
    echo "  Node 3: grpcurl -plaintext localhost:50053 list"
    echo "  Node 4: grpcurl -plaintext localhost:50054 list"
    echo ""
    exit 0
else
    echo "  Some nodes are offline!"
    echo "=========================================="
    echo ""
    echo "Check logs for details:"
    echo "  ./docker/logs.sh"
    echo ""
    exit 1
fi
