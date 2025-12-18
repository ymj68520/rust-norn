#!/bin/bash
# Start Norn Blockchain Multi-Node Network
# This script builds and starts all nodes

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "=========================================="
echo "  Norn Blockchain - Multi-Node Setup"
echo "=========================================="
echo ""

# Check if docker-compose is available
if command -v docker-compose &> /dev/null; then
    COMPOSE_CMD="docker-compose"
elif docker compose version &> /dev/null; then
    COMPOSE_CMD="docker compose"
else
    echo "Error: docker-compose is not installed"
    exit 1
fi

cd "$PROJECT_DIR"

echo "Building Docker images..."
$COMPOSE_CMD -f docker/docker-compose.yml build

echo ""
echo "Starting nodes..."
$COMPOSE_CMD -f docker/docker-compose.yml up -d

echo ""
echo "Waiting for nodes to start..."
sleep 5

echo ""
echo "Node status:"
$COMPOSE_CMD -f docker/docker-compose.yml ps

echo ""
echo "=========================================="
echo "  Network is up and running!"
echo "=========================================="
echo ""
echo "  Node 1: http://localhost:50051 (Bootstrap)"
echo "  Node 2: http://localhost:50052"
echo "  Node 3: http://localhost:50053"
echo "  Node 4: http://localhost:50054"
echo ""
echo "  View logs:  ./docker/logs.sh"
echo "  Stop nodes: ./docker/stop-nodes.sh"
echo "  Test network: ./docker/test-network.sh"
echo ""
