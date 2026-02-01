#!/bin/bash

# Start Norn Faucet Service
# Production-grade faucet for token distribution

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Norn Faucet Service ===${NC}"
echo ""

# Check if binary exists
if [ ! -f "./target/release/faucet" ]; then
    echo -e "${YELLOW}Faucet binary not found. Building...${NC}"
    cargo build --release -p norn-faucet
fi

# Check .env file
if [ ! -f ".env" ]; then
    echo -e "${RED}Error: .env file not found${NC}"
    echo ""
    echo "Please create a .env file with the following variables:"
    echo "  FAUCET_PRIVATE_KEY=0x..."
    echo "  FAUCET_RPC_URL=http://localhost:8545"
    echo ""
    echo "Generate a private key:"
    echo "  openssl rand -hex 32"
    echo ""
    exit 1
fi

# Load environment variables
set -a
source .env
set +a

# Check required variables
if [ -z "$FAUCET_PRIVATE_KEY" ]; then
    echo -e "${RED}Error: FAUCET_PRIVATE_KEY not set in .env${NC}"
    exit 1
fi

echo -e "${GREEN}Configuration:${NC}"
echo "  Server: http://localhost:3000"
echo "  RPC: ${FAUCET_RPC_URL:-http://localhost:8545}"
echo "  Database: ./faucet_data"
echo ""

# Create data directory
mkdir -p faucet_data

# Start faucet
echo -e "${GREEN}Starting faucet service...${NC}"
echo ""

./target/release/faucet \
  --server-addr "0.0.0.0:3000" \
  --rpc-url "${FAUCET_RPC_URL:-http://localhost:8545}" \
  --private-key "$FAUCET_PRIVATE_KEY" \
  --dispense-amount "${FAUCET_DISPENSE_AMOUNT:-1000000000000000000000}" \
  --debug
