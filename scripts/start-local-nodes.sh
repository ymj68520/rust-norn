#!/bin/bash

# Local multi-node testing script (no Docker required)
# This script starts 3 norn nodes locally for testing

set -e

echo "==================================================="
echo "  Rust-Norn Local Multi-Node Test (No Docker)"
echo "==================================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

# Check if binary exists
if [ ! -f "./target/release/norn" ]; then
    print_error "Binary not found. Building..."
    cargo build --release
fi

# Clean up function
cleanup() {
    print_info "Stopping all nodes..."
    for pid in "${pids[@]}"; do
        kill "$pid" 2>/dev/null || true
    done

    # Clean up tmux sessions
    tmux kill-session -t norn-node1 2>/dev/null || true
    tmux kill-session -t norn-node2 2>/dev/null || true
    tmux kill-session -t norn-node3 2>/dev/null || true

    print_info "Cleanup complete"
    exit 0
}

# Trap SIGINT and SIGTERM
trap cleanup SIGINT SIGTERM

# Check if tmux is installed
if ! command -v tmux &> /dev/null; then
    print_error "tmux is not installed. Please install tmux:"
    echo "  Ubuntu/Debian: sudo apt-get install tmux"
    echo "  CentOS/RHEL: sudo yum install tmux"
    echo "  macOS: brew install tmux"
    exit 1
fi

# Clean up old data
print_step "Cleaning up old data directories..."
rm -rf node1_data node2_data node3_data
mkdir -p node1_data node2_data node3_data

# Generate keys for each node
print_step "Generating node keys..."
./target/release/norn generate-key --out node1_data/node.key
./target/release/norn generate-key --out node2_data/node.key
./target/release/norn generate-key --out node3_data/node.key

# Create configuration files
print_step "Creating configuration files..."

# Node 1 configuration (bootstrap node)
cat > node1_config_local.toml << 'EOF'
data_dir = "node1_data"
rpc_address = "127.0.0.1:50051"

[core.consensus]
# Load from file
pub_key = "loaded_from_file"
prv_key = "loaded_from_file"

[network]
listen_address = "/ip4/0.0.0.0/tcp/4001"
bootstrap_peers = []
mdns = true
EOF

# Node 2 configuration
cat > node2_config_local.toml << 'EOF'
data_dir = "node2_data"
rpc_address = "127.0.0.1:50052"

[core.consensus]
pub_key = "loaded_from_file"
prv_key = "loaded_from_file"

[network]
listen_address = "/ip4/0.0.0.0/tcp/4002"
bootstrap_peers = ["/ip4/127.0.0.1/tcp/4001"]
mdns = true
EOF

# Node 3 configuration
cat > node3_config_local.toml << 'EOF'
data_dir = "node3_data"
rpc_address = "127.0.0.1:50053"

[core.consensus]
pub_key = "loaded_from_file"
prv_key = "loaded_from_file"

[network]
listen_address = "/ip4/0.0.0.0/tcp/4003"
bootstrap_peers = ["/ip4/127.0.0.1/tcp/4001"]
mdns = true
EOF

# Start nodes in tmux sessions
print_step "Starting nodes in tmux sessions..."

# Node 1
tmux new-session -d -s norn-node1 "./target/release/norn --config node1_config_local.toml"
print_info "Node 1 started in tmux session: norn-node1"

# Wait a bit for node 1 to start
sleep 2

# Node 2
tmux new-session -d -s norn-node2 "./target/release/norn --config node2_config_local.toml"
print_info "Node 2 started in tmux session: norn-node2"

# Wait a bit
sleep 2

# Node 3
tmux new-session -d -s norn-node3 "./target/release/norn --config node3_config_local.toml"
print_info "Node 3 started in tmux session: norn-node3"

echo ""
print_info "All nodes started!"
echo ""
echo "==================================================="
echo "  Node Information"
echo "==================================================="
echo ""
echo "Node 1 (Bootstrap):"
echo "  - P2P: 4001"
echo "  - gRPC: 50051"
echo "  - Ethereum RPC: http://localhost:50052"
echo "  - Logs: tmux attach-session -t norn-node1"
echo ""
echo "Node 2:"
echo "  - P2P: 4002"
echo "  - gRPC: 50052"
echo "  - Ethereum RPC: http://localhost:50053"
echo "  - Logs: tmux attach-session -t norn-node2"
echo ""
echo "Node 3:"
echo "  - P2P: 4003"
echo "  - gRPC: 50053"
echo "  - Ethereum RPC: http://localhost:50054"
echo "  - Logs: tmux attach-session -t norn-node3"
echo ""
echo "==================================================="
echo ""
echo "Commands:"
echo "  - View all logs: tmux list-sessions"
echo "  - Attach to node: tmux attach-session -t norn-node1"
echo "  - Detach: Ctrl+B, then D"
echo "  - Stop all: Press Ctrl+C or run: pkill -f 'norn --config'"
echo ""
echo "Waiting for nodes to start..."
sleep 5

# Test connectivity
echo ""
print_step "Testing RPC connectivity..."

if curl -s http://localhost:50052 > /dev/null 2>&1; then
    CHAIN_ID=$(curl -s -X POST http://localhost:50052 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
        | grep -o '"result":"[^"]*"' | cut -d'"' -f4)

    if [ -n "$CHAIN_ID" ]; then
        print_info "Node 1 RPC is responding! Chain ID: $CHAIN_ID"
    fi
else
    print_warn "Node 1 RPC not yet responding (may take a few more seconds)"
fi

echo ""
echo "==================================================="
print_info "Ready for smart contract testing!"
echo "==================================================="
echo ""
echo "To deploy smart contracts, edit .env and run:"
echo "  NORN_RPC_URL=http://localhost:50052 npm run deploy"
echo ""
echo "Press Ctrl+C to stop all nodes"
echo ""

# Keep script running
wait
