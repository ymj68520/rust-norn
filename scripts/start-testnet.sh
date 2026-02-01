#!/bin/bash
set -e

echo "🌐 Starting Norn Testnet"
echo "========================"
echo ""

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "❌ Docker is not running. Please start Docker first."
    exit 1
fi

echo "✅ Docker is running"

# Function to prompt for cleanup
prompt_cleanup() {
    read -p "🗑️  Clean previous data? This will delete all blockchain data. (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo "Cleaning up..."
        docker-compose -f docker-compose-testnet.yml down -v 2>/dev/null || true
        docker volume rm $(docker volume ls -q | grep -E "(validator|prometheus|grafana)" || true) 2>/dev/null || true
        echo "✅ Cleanup complete"
    else
        echo "Keeping existing data"
    fi
}

# Parse command line arguments
CLEAN=false
BUILD_ONLY=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --clean)
            CLEAN=true
            shift
            ;;
        --build-only)
            BUILD_ONLY=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--clean] [--build-only]"
            exit 1
            ;;
    esac
done

# Prompt for cleanup if not specified
if [ "$CLEAN" = false ]; then
    prompt_cleanup
fi

# Build Docker images
echo ""
echo "📦 Building Docker images..."
if docker-compose -f docker-compose-testnet.yml build; then
    echo "✅ Docker images built successfully"
else
    echo "❌ Docker build failed"
    exit 1
fi

# If build-only, exit here
if [ "$BUILD_ONLY" = true ]; then
    echo "✅ Build complete. Exiting (--build-only specified)"
    exit 0
fi

# Stop any existing containers
echo ""
echo "🛑 Stopping existing containers..."
docker-compose -f docker-compose-testnet.yml down 2>/dev/null || true

# Start testnet
echo ""
echo "🚀 Starting testnet..."
if docker-compose -f docker-compose-testnet.yml up -d; then
    echo "✅ Testnet started successfully"
else
    echo "❌ Failed to start testnet"
    exit 1
fi

# Wait for nodes to initialize
echo ""
echo "⏳ Waiting for nodes to initialize (15 seconds)..."
sleep 15

# Check container status
echo ""
echo "📊 Container Status:"
echo "===================="
docker-compose -f docker-compose-testnet.yml ps

# Display endpoints
echo ""
echo "✅ Testnet is running!"
echo ""
echo "🌐 Endpoints:"
echo "============="
echo ""
echo "Validator Nodes:"
echo "  Validator 1 P2P:        http://localhost:4001"
echo "  Validator 2 P2P:        http://localhost:4002"
echo "  Validator 3 P2P:        http://localhost:4003"
echo ""
echo "RPC Endpoints:"
echo "  Validator 1 gRPC:       http://localhost:50051"
echo "  Validator 2 gRPC:       http://localhost:50052"
echo "  Validator 3 gRPC:       http://localhost:50053"
echo ""
echo "  Validator 1 Ethereum:   http://localhost:51337"
echo "  Validator 2 Ethereum:   http://localhost:51338"
echo "  Validator 3 Ethereum:   http://localhost:51339"
echo ""
echo "Monitoring:"
echo "  Prometheus:             http://localhost:9091"
echo "  Grafana:                http://localhost:3000 (admin/admin)"
echo ""
echo "Health Checks:"
echo "  Validator 1:            http://localhost:8080/health"
echo "  Validator 2:            http://localhost:8081/health"
echo "  Validator 3:            http://localhost:8082/health"
echo ""
echo "Metrics:"
echo "  Validator 1:            http://localhost:9090/metrics"
echo "  Validator 2:            http://localhost:9091/metrics"
echo "  Validator 3:            http://localhost:9092/metrics"
echo ""
echo "📝 Logs:"
echo "  docker-compose -f docker-compose-testnet.yml logs -f"
echo ""
echo "🛑 Stop testnet:"
echo "  docker-compose -f docker-compose-testnet.yml down"
echo ""
echo "🧪 Run tests:"
echo "  ./tests/testnet/basic_test.sh"
echo ""
