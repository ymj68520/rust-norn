#!/bin/bash
set -e

echo "🛑 Stopping Norn Enhanced Testnet..."
echo ""

cd "$(dirname "$0")"

# Stop all containers
echo "Stopping containers..."
docker-compose down

# Optional: Clean up data volumes
if [ "$1" == "--clean" ]; then
    echo ""
    echo "🧹 Cleaning up data volumes..."
    docker-compose down -v
    echo "⚠️  All blockchain data has been deleted"
fi

echo ""
echo "✅ Testnet stopped!"
echo ""
echo "To restart:"
echo "  ./start.sh"
echo ""
