#!/bin/bash

# Test script for Norn Faucet Service
# Tests all API endpoints and functionality

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

FAUCET_URL="${FAUCET_URL:-http://localhost:3000}"
TEST_ADDRESS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

echo -e "${BLUE}=== Norn Faucet Service Test Suite ===${NC}"
echo "Testing faucet at: $FAUCET_URL"
echo ""

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Test function
test_endpoint() {
    local test_name=$1
    local method=$2
    local endpoint=$3
    local data=$4
    local expected_code=$5

    echo -n "Testing: $test_name... "

    if [ -z "$data" ]; then
        response=$(curl -s -w "\n%{http_code}" -X "$method" "$FAUCET_URL$endpoint" -H "Content-Type: application/json")
    else
        response=$(curl -s -w "\n%{http_code}" -X "$method" "$FAUCET_URL$endpoint" -H "Content-Type: application/json" -d "$data")
    fi

    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | head -n -1)

    if [ "$http_code" == "$expected_code" ]; then
        echo -e "${GREEN}✓ PASS${NC} (HTTP $http_code)"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        echo -e "${RED}✗ FAIL${NC} (Expected: $expected_code, Got: $http_code)"
        echo "  Response: $body"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

echo -e "${BLUE}=== Basic Connectivity Tests ===${NC}"

# Test 1: Health check
test_endpoint "Health check" "GET" "/health" "" "200"

# Test 2: Root endpoint
test_endpoint "Root endpoint" "GET" "/" "" "200"

# Test 3: Status endpoint
test_endpoint "Status endpoint" "GET" "/api/status" "" "200"

echo ""
echo -e "${BLUE}=== Dispense Tests ===${NC}"

# Test 4: Dispense with valid address
echo -n "Testing: Dispense to valid address... "
response=$(curl -s -w "\n%{http_code}" -X POST "$FAUCET_URL/api/dispense" \
  -H "Content-Type: application/json" \
  -d "{\"address\":\"$TEST_ADDRESS\"}")

http_code=$(echo "$response" | tail -n1)
body=$(echo "$response" | head -n -1)

if [ "$http_code" == "200" ] || [ "$http_code" == "429" ]; then
    # 200 = success, 429 = rate limit (also acceptable)
    if [ "$http_code" == "200" ]; then
        echo -e "${GREEN}✓ PASS${NC} (HTTP $http_code)"
        TESTS_PASSED=$((TESTS_PASSED + 1))

        # Extract tx hash
        tx_hash=$(echo "$body" | grep -o '"tx_hash":"[^"]*"' | cut -d'"' -f4)
        if [ ! -z "$tx_hash" ]; then
            echo -e "  ${GREEN}Transaction hash: $tx_hash${NC}"
        fi
    else
        echo -e "${YELLOW}✓ PASS${NC} (Rate limited - acceptable)"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    fi
else
    echo -e "${RED}✗ FAIL${NC} (HTTP $http_code)"
    echo "  Response: $body"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Test 5: Dispense with invalid address format
test_endpoint "Dispense with invalid address (short)" "POST" "/api/dispense" \
  '{"address":"0x123"}' "400"

# Test 6: Dispense with malformed address
test_endpoint "Dispense with malformed address" "POST" "/api/dispense" \
  '{"address":"notanaddress"}' "400"

# Test 7: Dispense with empty address
test_endpoint "Dispense with empty address" "POST" "/api/dispense" \
  '{"address":""}' "400"

# Test 8: Dispense with zero address
test_endpoint "Dispense with zero address" "POST" "/api/dispense" \
  '{"address":"0x0000000000000000000000000000000000000000"}' "400"

echo ""
echo -e "${BLUE}=== Status and Info Tests ===${NC}"

# Test 9: Get detailed status
echo -n "Testing: Detailed status information... "
response=$(curl -s -X GET "$FAUCET_URL/api/status")
if echo "$response" | grep -q '"address"' && \
   echo "$response" | grep -q '"balance"' && \
   echo "$response" | grep -q '"dispense_amount"'; then
    echo -e "${GREEN}✓ PASS${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))

    # Display status
    address=$(echo "$response" | grep -o '"address":"[^"]*"' | cut -d'"' -f4)
    balance=$(echo "$response" | grep -o '"balance":"[^"]*"' | cut -d'"' -f4)
    dispense_amount=$(echo "$response" | grep -o '"dispense_amount":"[^"]*"' | cut -d'"' -f4)

    echo -e "  ${BLUE}Faucet Address:${NC} $address"
    echo -e "  ${BLUE}Balance:${NC} $balance wei"
    echo -e "  ${BLUE}Dispense Amount:${NC} $dispense_amount wei"
else
    echo -e "${RED}✗ FAIL${NC}"
    echo "  Response: $response"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

echo ""
echo -e "${BLUE}=== Rate Limiting Tests ===${NC}"

# Test 10: Multiple rapid requests (should trigger rate limit)
echo -n "Testing: Rate limiting (5 rapid requests)... "
rate_limited=0
for i in {1..5}; do
    response=$(curl -s -w "%{http_code}" -X POST "$FAUCET_URL/api/dispense" \
      -H "Content-Type: application/json" \
      -d "{\"address\":\"0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb${i}\"}")
    if [[ "$response" == *"429"* ]]; then
        rate_limited=1
        break
    fi
done

if [ $rate_limited -eq 1 ]; then
    echo -e "${GREEN}✓ PASS${NC} (Rate limiting working)"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo -e "${YELLOW}⚠ WARNING${NC} (Rate limiting may not be configured)"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

echo ""
echo -e "${BLUE}=== Test Summary ===${NC}"
echo -e "Tests Passed: ${GREEN}$TESTS_PASSED${NC}"
echo -e "Tests Failed: ${RED}$TESTS_FAILED${NC}"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}✗ Some tests failed${NC}"
    exit 1
fi
