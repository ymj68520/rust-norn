#!/bin/bash
# View logs for Norn Blockchain nodes
# Usage: ./logs.sh [node-name] [-f]
# Example: ./logs.sh norn-node1 -f

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

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

if [ $# -eq 0 ]; then
    # Show logs for all nodes, follow mode
    echo "Showing logs for all nodes (Ctrl+C to exit)..."
    $COMPOSE_CMD -f docker/docker-compose.yml logs -f
else
    # Pass all arguments to docker-compose logs
    $COMPOSE_CMD -f docker/docker-compose.yml logs "$@"
fi
