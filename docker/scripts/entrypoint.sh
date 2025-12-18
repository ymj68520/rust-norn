#!/bin/bash
set -e

echo "=========================================="
echo "  Norn Blockchain Node - ${NODE_NAME}"
echo "=========================================="

# Wait for bootstrap node if specified
if [ -n "$WAIT_FOR_NODE" ]; then
    echo "Waiting for bootstrap node: $WAIT_FOR_NODE..."
    
    # Extract host and port from WAIT_FOR_NODE (format: host:port)
    WAIT_HOST=$(echo $WAIT_FOR_NODE | cut -d: -f1)
    WAIT_PORT=$(echo $WAIT_FOR_NODE | cut -d: -f2)
    
    # Wait up to 60 seconds for the bootstrap node
    for i in $(seq 1 60); do
        if nc -z "$WAIT_HOST" "$WAIT_PORT" 2>/dev/null; then
            echo "Bootstrap node is ready!"
            break
        fi
        echo "Attempt $i/60: Waiting for $WAIT_HOST:$WAIT_PORT..."
        sleep 1
    done
    
    # Additional delay to ensure node is fully started
    sleep 3
fi

# Generate keypair if not exists
if [ ! -f /data/node.key ]; then
    echo "Generating new node keypair..."
    norn generate-key --out /data/node.key
    echo "Keypair generated successfully"
else
    echo "Using existing keypair from /data/node.key"
fi

echo ""
echo "Node Configuration:"
echo "  - Config file: /etc/norn/config.toml"
echo "  - Data directory: /data"
echo "  - RPC Address: ${RPC_ADDRESS}"
echo "  - P2P Port: ${P2P_PORT}"
echo ""

# Start the norn node
echo "Starting Norn node..."
exec norn --config /etc/norn/config.toml --data-dir /data
