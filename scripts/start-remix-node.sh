#!/bin/bash

# Quick start script for norn node with Remix support
# This script builds (if needed) and starts a node configured for Remix development

set -e

echo "Starting Norn Node for Remix Development"
echo "=========================================="
echo ""

# Build if needed
if [ ! -f "./target/release/norn" ]; then
    echo "Building norn..."
    cargo build --release
    echo ""
fi

# Clean up old data if exists
if [ -d "./remix_node_data" ]; then
    echo "Removing old node data..."
    rm -rf ./remix_node_data
    echo ""
fi

# Start node
echo "Starting node with remix_config.toml..."
echo ""
echo "Services will be available at:"
echo "  - gRPC:      0.0.0.0:7545"
echo "  - Ethereum JSON-RPC: http://localhost:8545  (for Remix)"
echo "  - P2P:       /ip4/0.0.0.0/tcp/40101"
echo ""
echo "Press Ctrl+C to stop the node"
echo ""

./target/release/norn --config remix_config.toml
