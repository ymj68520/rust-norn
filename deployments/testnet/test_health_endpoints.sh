#!/bin/bash
# Test Health Check Endpoints
#
# This script tests all health check endpoints to ensure they're working correctly.

set -e

BASE_URL="${HEALTH_CHECK_URL:-http://localhost:8000}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "==================================="
echo "Health Check Endpoint Test Suite"
echo "==================================="
echo "Base URL: $BASE_URL"
echo ""

# Test helper function
test_endpoint() {
    local endpoint=$1
    local description=$2

    echo -n "Testing $description... "

    if response=$(curl -s -w "\n%{http_code}" "$BASE_URL$endpoint" 2>/dev/null); then
        http_code=$(echo "$response" | tail -n1)
        body=$(echo "$response" | sed '$d')

        if [ "$http_code" = "200" ] || [ "$http_code" = "503" ]; then
            echo -e "${GREEN}✓ PASS${NC} (HTTP $http_code)"
            echo "$body" | jq . 2>/dev/null || echo "$body"
        else
            echo -e "${RED}✗ FAIL${NC} (HTTP $http_code)"
            return 1
        fi
    else
        echo -e "${RED}✗ FAIL${NC} (Connection failed)"
        return 1
    fi
    echo ""
}

echo "1. Liveness Probe"
echo "-------------------"
test_endpoint "/live" "Liveness endpoint"

echo "2. Readiness Probe"
echo "-------------------"
test_endpoint "/ready" "Readiness endpoint"

echo "3. Basic Health Check"
echo "-------------------"
test_endpoint "/health" "Basic health endpoint"

echo "4. Detailed Health Check"
echo "-------------------"
test_endpoint "/health/detailed" "Detailed health endpoint"

echo "5. Metrics Endpoint"
echo "-------------------"
echo -n "Testing Prometheus metrics... "

if response=$(curl -s -w "\n%{http_code}" "$BASE_URL/metrics" 2>/dev/null); then
    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    if [ "$http_code" = "200" ]; then
        echo -e "${GREEN}✓ PASS${NC} (HTTP $http_code)"
        echo "Sample metrics:"
        echo "$body" | head -20
    else
        echo -e "${RED}✗ FAIL${NC} (HTTP $http_code)"
    fi
else
    echo -e "${RED}✗ FAIL${NC} (Connection failed)"
fi
echo ""

echo "==================================="
echo "Test Suite Complete"
echo "==================================="

echo ""
echo "Quick Health Summary:"
echo "-------------------"
curl -s "$BASE_URL/health" | jq '{
    status: .status,
    uptime: (.uptime_seconds | tostring + "s"),
    block_height: .block_height,
    peer_count: .peer_count,
    txpool_size: .txpool_size
}' 2>/dev/null || echo "Failed to parse health status"

echo ""
echo "For detailed health: curl $BASE_URL/health/detailed"
echo "For Prometheus metrics: curl $BASE_URL/metrics"
