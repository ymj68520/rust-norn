#!/bin/bash
set -e

CONFIG_FILE=${1:-"config/enhanced.toml"}

# Check if config exists
if [ ! -f "$CONFIG_FILE" ]; then
    echo "❌ Configuration file not found: $CONFIG_FILE"
    echo "Run ./scripts/setup_enhanced.sh first to create configuration"
    exit 1
fi

echo "🚀 Starting Norn node with enhanced features..."
echo "📝 Using config: $CONFIG_FILE"
echo ""

# Check if binary exists
if [ ! -f "./target/release/norn" ]; then
    echo "❌ Binary not found. Run: cargo build --release --features production"
    exit 1
fi

# Set environment variables
export RUST_LOG=${RUST_LOG:-info,norn=debug}
export TXPOOL_ENHANCED=${TXPOOL_ENHANCED:-true}
export SYNC_MODE=${SYNC_MODE:-fast}

echo "Environment:"
echo "  RUST_LOG=$RUST_LOG"
echo "  TXPOOL_ENHANCED=$TXPOOL_ENHANCED"
echo "  SYNC_MODE=$SYNC_MODE"
echo ""

# Start the node
echo "Starting node..."
./target/release/norn --config "$CONFIG_FILE"
