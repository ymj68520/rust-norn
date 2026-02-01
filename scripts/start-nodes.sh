#!/bin/bash

# Multi-node smart contract testing script for rust-norn
# This script starts 3 nodes and deploys smart contracts for testing

set -e

echo "==================================================="
echo "  Rust-Norn Multi-Node Smart Contract Test"
echo "==================================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Check if Docker is available
if ! command -v docker &> /dev/null; then
    echo -e "${RED}Error: Docker is not installed${NC}"
    echo "Please install Docker to run multi-node tests"
    exit 1
fi

# Function to print colored output
print_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Clean up any existing containers and volumes
print_info "Cleaning up existing containers and volumes..."
docker compose down -v 2>/dev/null || true

# Build Docker images
print_info "Building Docker images..."
if ! docker compose build; then
    echo -e "${RED}Error: Failed to build Docker images${NC}"
    exit 1
fi

# Start the nodes
print_info "Starting 3-node network..."
if ! docker compose up -d; then
    echo -e "${RED}Error: Failed to start nodes${NC}"
    exit 1
fi

echo ""
print_info "Waiting for nodes to start..."
sleep 5

# Check if nodes are running
print_info "Checking node status..."
docker compose ps

echo ""
print_info "Node endpoints:"
echo "  Node 1 (Bootstrap):"
echo "    - P2P: 4001"
echo "    - gRPC: 50051"
echo "    - Ethereum RPC: http://localhost:8545"
echo ""
echo "  Node 2:"
echo "    - P2P: 4002"
echo "    - gRPC: 50052"
echo "    - Ethereum RPC: http://localhost:8546"
echo ""
echo "  Node 3:"
echo "    - P2P: 4003"
echo "    - gRPC: 50053"
echo "    - Ethereum RPC: http://localhost:8547"
echo ""

# Wait for user input
echo "Press Ctrl+C to stop the nodes..."
echo ""

# Follow logs
docker compose logs -f
