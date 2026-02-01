#!/bin/bash
set -e

echo "🌐 Starting Norn Enhanced Testnet..."
echo ""

# Check if Docker is running
if ! docker info &> /dev/null; then
    echo "❌ Docker is not running. Please start Docker first."
    exit 1
fi

# Check if docker-compose is available
if ! command -v docker-compose &> /dev/null; then
    echo "❌ docker-compose is not installed."
    exit 1
fi

# Stop any existing containers
echo "🛑 Stopping existing containers..."
docker-compose down 2>/dev/null || true

# Build images
echo "🔨 Building Docker images..."
docker-compose build

# Start network
echo "🚀 Starting testnet..."
docker-compose up -d

# Wait for nodes to be ready
echo ""
echo "⏳ Waiting for nodes to initialize..."
sleep 15

# Check status
echo ""
echo "✅ Testnet started!"
echo ""
docker-compose ps
echo ""
echo "📍 Endpoints:"
echo "  Validator 1 P2P:   http://localhost:4001"
echo "  Validator 1 RPC:   http://localhost:50051"
echo "  Validator 1 Metrics: http://localhost:9090"
echo ""
echo "  Validator 2 P2P:   http://localhost:4002"
echo "  Validator 2 RPC:   http://localhost:50052"
echo "  Validator 2 Metrics: http://localhost:9091"
echo ""
echo "  Observer P2P:      http://localhost:4003"
echo "  Observer RPC:      http://localhost:50053"
echo "  Observer Metrics:  http://localhost:9092"
echo ""
echo "  Prometheus:        http://localhost:9090"
echo "  Grafana:           http://localhost:3000 (admin/admin)"
echo ""
echo "📊 To view logs:"
echo "  docker-compose logs -f validator1"
echo "  docker-compose logs -f validator2"
echo "  docker-compose logs -f observer"
echo ""
echo "🛑 To stop testnet:"
echo "  docker-compose down"
echo ""
echo "✨ Enhanced features enabled:"
echo "  ✓ Enhanced transaction pool (priority queue, EIP-1559)"
echo "  ✓ Fast sync mechanism"
echo "  ✓ Monitoring (Prometheus + Grafana)"
