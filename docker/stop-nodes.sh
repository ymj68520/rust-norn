#!/bin/bash
# Stop Norn Blockchain Multi-Node Network
# This script stops all nodes and optionally removes volumes

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "=========================================="
echo "  Norn Blockchain - Stop Network"
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

# Check for --clean flag
if [ "$1" == "--clean" ] || [ "$1" == "-c" ]; then
    echo "Stopping nodes and removing volumes..."
    $COMPOSE_CMD -f docker/docker-compose.yml down -v
    echo ""
    echo "All nodes stopped and data volumes removed."
else
    echo "Stopping nodes (keeping data volumes)..."
    $COMPOSE_CMD -f docker/docker-compose.yml down
    echo ""
    echo "All nodes stopped. Data volumes are preserved."
    echo ""
    echo "To also remove data volumes, run:"
    echo "  ./docker/stop-nodes.sh --clean"
fi

echo ""
echo "Done!"
