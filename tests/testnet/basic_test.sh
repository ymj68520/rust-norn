#!/bin/bash
set -e

echo "🧪 Week 3 Testnet Basic Tests"
echo "==============================="
echo ""

# Color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Function to print test result
print_result() {
    if [ $1 -eq 0 ]; then
        echo -e "${GREEN}✅ PASS${NC}: $2"
        ((TESTS_PASSED++))
    else
        echo -e "${RED}❌ FAIL${NC}: $2"
        ((TESTS_FAILED++))
    fi
}

# Test 1: Check if containers are running
echo "Test 1: Container Status"
echo "------------------------"
CONTAINER_COUNT=$(docker-compose -f docker-compose-testnet.yml ps -q | wc -l)
if [ $CONTAINER_COUNT -ge 3 ]; then
    print_result 0 "Containers are running (found $CONTAINER_COUNT containers)"
else
    print_result 1 "Expected at least 3 containers, found $CONTAINER_COUNT"
fi
echo ""

# Test 2: Health Check - Validator 1
echo "Test 2: Validator 1 Health Check"
echo "----------------------------------"
if curl -s http://localhost:8080/health > /dev/null 2>&1; then
    HEALTH=$(curl -s http://localhost:8080/health)
    echo "$HEALTH" | jq . 2>/dev/null || echo "$HEALTH"
    STATUS=$(echo "$HEALTH" | jq -r '.status' 2>/dev/null || echo "unknown")
    if [ "$STATUS" = "healthy" ]; then
        print_result 0 "Validator 1 is healthy"
    else
        print_result 1 "Validator 1 status: $STATUS"
    fi
else
    print_result 1 "Cannot connect to Validator 1 health endpoint"
fi
echo ""

# Test 3: Health Check - Validator 2
echo "Test 3: Validator 2 Health Check"
echo "----------------------------------"
if curl -s http://localhost:8081/health > /dev/null 2>&1; then
    HEALTH=$(curl -s http://localhost:8081/health)
    STATUS=$(echo "$HEALTH" | jq -r '.status' 2>/dev/null || echo "unknown")
    if [ "$STATUS" = "healthy" ]; then
        print_result 0 "Validator 2 is healthy"
    else
        print_result 1 "Validator 2 status: $STATUS"
    fi
else
    print_result 1 "Cannot connect to Validator 2 health endpoint"
fi
echo ""

# Test 4: Health Check - Validator 3
echo "Test 4: Validator 3 Health Check"
echo "----------------------------------"
if curl -s http://localhost:8082/health > /dev/null 2>&1; then
    HEALTH=$(curl -s http://localhost:8082/health)
    STATUS=$(echo "$HEALTH" | jq -r '.status' 2>/dev/null || echo "unknown")
    if [ "$STATUS" = "healthy" ]; then
        print_result 0 "Validator 3 is healthy"
    else
        print_result 1 "Validator 3 status: $STATUS"
    fi
else
    print_result 1 "Cannot connect to Validator 3 health endpoint"
fi
echo ""

# Test 5: Prometheus Metrics - Validator 1
echo "Test 5: Validator 1 Prometheus Metrics"
echo "---------------------------------------"
if curl -s http://localhost:9090/metrics > /dev/null 2>&1; then
    METRICS=$(curl -s http://localhost:9090/metrics)
    if echo "$METRICS" | grep -q "norn_"; then
        METRIC_COUNT=$(echo "$METRICS" | grep "^norn_" | wc -l)
        print_result 0 "Found $METRIC_COUNT Norn metrics"
        echo "Sample metrics:"
        echo "$METRICS" | grep "^norn_" | head -3
    else
        print_result 1 "No Norn metrics found"
    fi
else
    print_result 1 "Cannot connect to Validator 1 metrics endpoint"
fi
echo ""

# Test 6: Prometheus Server
echo "Test 6: Prometheus Server"
echo "--------------------------"
if curl -s http://localhost:9091/-/healthy > /dev/null 2>&1; then
    print_result 0 "Prometheus server is accessible"
    PROMETHEUS_TARGETS=$(curl -s http://localhost:9091/api/v1/targets | jq -r '.data.activeTargets[] | select(.labels.job | contains("norn")) | .labels.job' 2>/dev/null | wc -l)
    echo "Found $PROMETHEUS_TARGETS Norn targets in Prometheus"
else
    print_result 1 "Cannot connect to Prometheus server"
fi
echo ""

# Test 7: Grafana Server
echo "Test 7: Grafana Server"
echo "-----------------------"
if curl -s http://localhost:3000/api/health > /dev/null 2>&1; then
    print_result 0 "Grafana server is accessible"
else
    print_result 1 "Cannot connect to Grafana server"
fi
echo ""

# Test 8: Docker Logs
echo "Test 8: Check Node Logs"
echo "-----------------------"
if docker-compose -f docker-compose-testnet.yml logs validator1 2>&1 | grep -q "Logging initialized"; then
    print_result 0 "Logging initialization found in logs"
else
    print_result 1 "Logging initialization not found in logs"
fi

if docker-compose -f docker-compose-testnet.yml logs validator1 2>&1 | grep -q "Monitoring server started"; then
    print_result 0 "Monitoring server startup found in logs"
else
    print_result 1 "Monitoring server startup not found in logs"
fi
echo ""

# Summary
echo "======================"
echo "Test Summary"
echo "======================"
echo -e "${GREEN}Passed: $TESTS_PASSED${NC}"
echo -e "${RED}Failed: $TESTS_FAILED${NC}"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}🎉 All tests passed!${NC}"
    exit 0
else
    echo -e "${YELLOW}⚠️  Some tests failed${NC}"
    exit 1
fi
