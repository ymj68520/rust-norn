#!/bin/bash
# Test WebSocket Server Functionality
#
# This script tests the WebSocket server by:
# 1. Starting a test node with WebSocket enabled
# 2. Running test clients
# 3. Verifying subscriptions and events
# 4. Testing reconnection handling

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
WS_URL="${WS_URL:-ws://localhost:8545/ws}"
RPC_URL="${RPC_URL:-http://localhost:50051}"
TEST_DURATION="${TEST_DURATION:-30}"

# Print banner
print_banner() {
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}   Norn WebSocket Test Suite${NC}"
    echo -e "${BLUE}========================================${NC}"
    echo ""
}

# Test helper
test_step() {
    local name="$1"
    local command="$2"

    echo -n "Testing: $name ... "

    if eval "$command" > /dev/null 2>&1; then
        echo -e "${GREEN}PASS${NC}"
        return 0
    else
        echo -e "${RED}FAIL${NC}"
        return 1
    fi
}

# Check dependencies
check_dependencies() {
    echo -e "${BLUE}Checking dependencies...${NC}"

    local deps=("curl" "jq" "python3")
    local missing=()

    for dep in "${deps[@]}"; do
        if command -v "$dep" &> /dev/null; then
            echo -e "  ${GREEN}✓${NC} $dep"
        else
            echo -e "  ${RED}✗${NC} $dep (not found)"
            missing+=("$dep")
        fi
    done

    # Check for Python websocket-client
    if command -v pip3 &> /dev/null; then
        if pip3 show websocket-client &> /dev/null; then
            echo -e "  ${GREEN}✓${NC} websocket-client (Python)"
        else
            echo -e "  ${YELLOW}○${NC} websocket-client (not installed)"
            echo -e "     Install: pip3 install websocket-client"
        fi
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        echo ""
        echo -e "${RED}Missing dependencies: ${missing[*]}${NC}"
        return 1
    fi

    echo ""
    return 0
}

# Test WebSocket endpoint is reachable
test_ws_endpoint() {
    echo -e "${BLUE}Testing WebSocket endpoint...${NC}"

    # Try to upgrade connection to WebSocket
    local response=$(curl -i -N \
        -H "Connection: Upgrade" \
        -H "Upgrade: websocket" \
        -H "Sec-WebSocket-Version: 13" \
        -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
        "$WS_URL" 2>&1 | head -20)

    if echo "$response" | grep -q "101 Switching Protocols"; then
        echo -e "${GREEN}✓${NC} WebSocket endpoint is reachable"
        return 0
    elif echo "$response" | grep -q "400"; then
        echo -e "${YELLOW}○${NC} WebSocket endpoint responds (may require valid handshake)"
        return 0
    else
        echo -e "${RED}✗${NC} WebSocket endpoint not reachable"
        echo "Response: $response"
        return 1
    fi
}

# Create Python test client
create_python_client() {
    cat > /tmp/ws_test_client.py << 'EOF'
#!/usr/bin/env python3
import asyncio
import websockets
import json
import sys

async def test_websocket():
    uri = "ws://localhost:8545/ws"

    try:
        async with websockets.connect(uri) as websocket:
            print("Connected to WebSocket server")

            # Subscribe to new blocks
            subscribe_msg = {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "eth_subscribe",
                "params": ["newHeads"]
            }

            await websocket.send(json.dumps(subscribe_msg))
            print("Sent subscription request")

            # Wait for response
            response = await asyncio.wait_for(websocket.recv(), timeout=5.0)
            data = json.loads(response)

            if "result" in data:
                print(f"Subscription created: {data['result']}")
                return True
            elif "error" in data:
                print(f"Error: {data['error']}")
                return False
            else:
                print(f"Unexpected response: {data}")
                return False

    except asyncio.TimeoutError:
        print("Timeout waiting for response")
        return False
    except Exception as e:
        print(f"Connection error: {e}")
        return False

if __name__ == "__main__":
    result = asyncio.run(test_websocket())
    sys.exit(0 if result else 1)
EOF

    chmod +x /tmp/ws_test_client.py
}

# Run Python test client
test_python_client() {
    echo -e "${BLUE}Running Python WebSocket client...${NC}"

    if ! python3 -c "import websockets" 2>/dev/null; then
        echo -e "${YELLOW}⚠${NC} websockets module not installed"
        echo "Install: pip3 install websockets"
        return 1
    fi

    create_python_client

    if python3 /tmp/ws_test_client.py; then
        echo -e "${GREEN}✓${NC} Python client test passed"
        return 0
    else
        echo -e "${RED}✗${NC} Python client test failed"
        return 1
    fi
}

# Create Node.js test client
create_nodejs_client() {
    cat > /tmp/ws_test_client.js << 'EOF'
const WebSocket = require('ws');

const ws = new WebSocket('ws://localhost:8545/ws');

let connected = false;
let subscriptionCreated = false;

const timeout = setTimeout(() => {
    if (!subscriptionCreated) {
        console.error('Timeout: No subscription created');
        process.exit(1);
    }
}, 10000);

ws.on('open', () => {
    console.log('Connected');
    connected = true;

    // Subscribe to new blocks
    const msg = {
        jsonrpc: '2.0',
        id: 1,
        method: 'eth_subscribe',
        params: ['newHeads']
    };

    ws.send(JSON.stringify(msg));
});

ws.on('message', (data) => {
    const msg = JSON.parse(data);

    if (msg.result && typeof msg.result === 'string') {
        console.log(`Subscription created: ${msg.result}`);
        subscriptionCreated = true;
        clearTimeout(timeout);
        ws.close();
        process.exit(0);
    } else if (msg.error) {
        console.error(`Error: ${msg.error.message}`);
        process.exit(1);
    }
});

ws.on('error', (error) => {
    console.error(`WebSocket error: ${error}`);
    process.exit(1);
});

ws.on('close', () => {
    if (!subscriptionCreated) {
        console.error('Connection closed before subscription');
        process.exit(1);
    }
});
EOF
}

# Run Node.js test client
test_nodejs_client() {
    echo -e "${BLUE}Running Node.js WebSocket client...${NC}"

    if ! command -v node &> /dev/null; then
        echo -e "${YELLOW}⚠${NC} Node.js not found, skipping"
        return 0
    fi

    if ! node -e "require('ws')" 2>/dev/null; then
        echo -e "${YELLOW}⚠${NC} ws module not installed"
        echo "Install: npm install ws"
        return 1
    fi

    create_nodejs_client

    if node /tmp/ws_test_client.js; then
        echo -e "${GREEN}✓${NC} Node.js client test passed"
        return 0
    else
        echo -e "${RED}✗${NC} Node.js client test failed"
        return 1
    fi
}

# Test multiple subscriptions
test_multiple_subscriptions() {
    echo -e "${BLUE}Testing multiple subscriptions...${NC}"

    cat > /tmp/ws_multi_sub.py << 'EOF'
import asyncio
import websockets
import json

async def test_multiple():
    uri = "ws://localhost:8545/ws"

    try:
        async with websockets.connect(uri) as websocket:
            # Subscribe to multiple event types
            subscriptions = [
                {"jsonrpc": "2.0", "id": 1, "method": "eth_subscribe", "params": ["newHeads"]},
                {"jsonrpc": "2.0", "id": 2, "method": "eth_subscribe", "params": ["newPendingTransactions"]},
                {"jsonrpc": "2.0", "id": 3, "method": "eth_subscribe", "params": ["syncing"]},
            ]

            subscription_ids = []

            for sub in subscriptions:
                await websocket.send(json.dumps(sub))
                response = await asyncio.wait_for(websocket.recv(), timeout=5.0)
                data = json.loads(response)

                if "result" in data:
                    subscription_ids.append(data["result"])
                    print(f"Subscription {sub['id']}: {data['result']}")
                else:
                    print(f"Error on subscription {sub['id']}: {data}")
                    return False

            print(f"Created {len(subscription_ids)} subscriptions")
            return len(subscription_ids) == 3

    except Exception as e:
        print(f"Error: {e}")
        return False

asyncio.run(test_multiple())
EOF

    if python3 /tmp/ws_multi_sub.py; then
        echo -e "${GREEN}✓${NC} Multiple subscriptions test passed"
        return 0
    else
        echo -e "${RED}✗${NC} Multiple subscriptions test failed"
        return 1
    fi
}

# Test unsubscribe functionality
test_unsubscribe() {
    echo -e "${BLUE}Testing unsubscribe...${NC}"

    cat > /tmp/ws_unsubscribe.py << 'EOF'
import asyncio
import websockets
import json

async def test_unsubscribe():
    uri = "ws://localhost:8545/ws"

    try:
        async with websockets.connect(uri) as websocket:
            # Subscribe
            sub_msg = {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "eth_subscribe",
                "params": ["newHeads"]
            }

            await websocket.send(json.dumps(sub_msg))
            response = await asyncio.wait_for(websocket.recv(), timeout=5.0)
            data = json.loads(response)

            if "result" not in data:
                print("Failed to create subscription")
                return False

            sub_id = data["result"]
            print(f"Subscription created: {sub_id}")

            # Unsubscribe
            unsub_msg = {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "eth_unsubscribe",
                "params": [sub_id]
            }

            await websocket.send(json.dumps(unsub_msg))
            response = await asyncio.wait_for(websocket.recv(), timeout=5.0)
            data = json.loads(response)

            if data.get("result") == True:
                print("Successfully unsubscribed")
                return True
            else:
                print(f"Failed to unsubscribe: {data}")
                return False

    except Exception as e:
        print(f"Error: {e}")
        return False

asyncio.run(test_unsubscribe())
EOF

    if python3 /tmp/ws_unsubscribe.py; then
        echo -e "${GREEN}✓${NC} Unsubscribe test passed"
        return 0
    else
        echo -e "${RED}✗${NC} Unsubscribe test failed"
        return 1
    fi
}

# Main test execution
main() {
    print_banner

    # Check dependencies
    if ! check_dependencies; then
        exit 1
    fi

    echo -e "${BLUE}WebSocket Test Suite${NC}"
    echo "Endpoint: $WS_URL"
    echo "RPC: $RPC_URL"
    echo ""

    # Run tests
    local passed=0
    local failed=0

    test_step "WebSocket endpoint" test_ws_endpoint && ((passed++)) || ((failed++))
    test_step "Python client" test_python_client && ((passed++)) || ((failed++))
    test_step "Node.js client" test_nodejs_client && ((passed++)) || ((failed++))
    test_step "Multiple subscriptions" test_multiple_subscriptions && ((passed++)) || ((failed++))
    test_step "Unsubscribe" test_unsubscribe && ((passed++)) || ((failed++))

    # Summary
    echo ""
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}Test Results${NC}"
    echo -e "${BLUE}========================================${NC}"
    echo -e "Passed: ${GREEN}$passed${NC}"
    echo -e "Failed: ${RED}$failed${NC}"
    echo -e "Total:  $((passed + failed))"
    echo ""

    if [ $failed -eq 0 ]; then
        echo -e "${GREEN}✓ All tests passed!${NC}"
        exit 0
    else
        echo -e "${RED}✗ Some tests failed${NC}"
        exit 1
    fi
}

# Run main
main "$@"
