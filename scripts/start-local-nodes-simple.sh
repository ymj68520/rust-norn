#!/bin/bash

# Simple local multi-node testing script (no tmux required)
# Uses background processes instead of tmux sessions

set -e

echo "==================================================="
echo "  Rust-Norn Local Multi-Node Test"
echo "==================================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
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
    pkill -f "norn --config" 2>/dev/null || true
    sleep 2
    print_info "Nodes stopped"
}

# Trap SIGINT and SIGTERM
trap cleanup SIGINT SIGTERM

# Stop any existing nodes
print_step "Stopping any existing nodes..."
pkill -f "norn --config" 2>/dev/null || true
sleep 2

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

# Start nodes in background with logging
print_step "Starting nodes..."

# Create logs directory
mkdir -p logs

# Node 1
nohup ./target/release/norn --config node1_config_local.toml > logs/node1.log 2>&1 &
NODE1_PID=$!
print_info "Node 1 started (PID: $NODE1_PID)"

# Wait for node 1 to initialize
sleep 3

# Node 2
nohup ./target/release/norn --config node2_config_local.toml > logs/node2.log 2>&1 &
NODE2_PID=$!
print_info "Node 2 started (PID: $NODE2_PID)"

sleep 2

# Node 3
nohup ./target/release/norn --config node3_config_local.toml > logs/node3.log 2>&1 &
NODE3_PID=$!
print_info "Node 3 started (PID: $NODE3_PID)"

echo ""
print_info "All nodes started!"
echo ""
echo "==================================================="
echo "  Node Information"
echo "==================================================="
echo ""
echo "Node 1 (Bootstrap):"
echo "  - PID: $NODE1_PID"
echo "  - P2P: 4001"
echo "  - gRPC: 50051"
echo "  - Ethereum RPC: http://localhost:50052"
echo "  - Log: logs/node1.log"
echo ""
echo "Node 2:"
echo "  - PID: $NODE2_PID"
echo "  - P2P: 4002"
echo "  - gRPC: 50052"
echo "  - Ethereum RPC: http://localhost:50053"
echo "  - Log: logs/node2.log"
echo ""
echo "Node 3:"
echo "  - PID: $NODE3_PID"
echo "  - P2P: 4003"
echo "  - gRPC: 50053"
echo "  - Ethereum RPC: http://localhost:50054"
echo "  - Log: logs/node3.log"
echo ""
echo "==================================================="
echo ""
echo "Commands:"
echo "  - View logs: tail -f logs/node1.log"
echo "  - Stop all: pkill -f 'norn --config'"
echo "  - Check status: ps aux | grep norn"
echo ""
echo "Waiting for nodes to initialize..."
sleep 5

# Save PIDs for cleanup
echo "$NODE1_PID $NODE2_PID $NODE3_PID" > /tmp/norn_nodes.pids

# Test connectivity
echo ""
print_step "Testing node status..."

if ps -p $NODE1_PID > /dev/null; then
    print_info "Node 1 is running"
else
    print_error "Node 1 failed to start"
    echo "Check logs: tail -f logs/node1.log"
fi

if ps -p $NODE2_PID > /dev/null; then
    print_info "Node 2 is running"
else
    print_error "Node 2 failed to start"
    echo "Check logs: tail -f logs/node2.log"
fi

if ps -p $NODE3_PID > /dev/null; then
    print_info "Node 3 is running"
else
    print_error "Node 3 failed to start"
    echo "Check logs: tail -f logs/node3.log"
fi

echo ""
print_step "Testing RPC connectivity (may take a few more seconds)..."

# Wait a bit more for RPC servers to start
sleep 3

# Test Node 1 RPC
if curl -s --max-time 3 http://localhost:50052 > /dev/null 2>&1; then
    print_info "Node 1 Ethereum RPC is responding on port 50052"
else
    print_info "Node 1 RPC starting... (check logs if this persists)"
fi

echo ""
echo "==================================================="
print_info "Nodes are starting up!"
echo "==================================================="
echo ""
echo "View real-time logs:"
echo "  tail -f logs/node1.log"
echo ""
echo "Press Ctrl+C to stop all nodes"
echo ""

# Keep script running
wait
