#!/bin/bash
# Test Alerting and Monitoring System
#
# This script tests the alerting system by:
# 1. Verifying Prometheus is scraping metrics
# 2. Checking if alert rules are loaded
# 3. Triggering test alerts
# 4. Verifying Alertmanager receives alerts
# 5. Testing notification delivery

set -e

PROMETHEUS_URL="${PROMETHEUS_URL:-http://localhost:9090}"
ALERTMANAGER_URL="${ALERTMANAGER_URL:-http://localhost:9093}"

echo "=========================================="
echo "Norn Alerting System Test"
echo "=========================================="
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test function
test_step() {
    local name="$1"
    local command="$2"
    local expected="$3"

    echo -n "Testing: $name ... "

    if eval "$command" > /dev/null 2>&1; then
        if [ -n "$expected" ]; then
            result=$(eval "$command")
            if echo "$result" | grep -q "$expected"; then
                echo -e "${GREEN}PASS${NC}"
                return 0
            else
                echo -e "${RED}FAIL${NC} (Expected: $expected, Got: $result)"
                return 1
            fi
        else
            echo -e "${GREEN}PASS${NC}"
            return 0
        fi
    else
        echo -e "${RED}FAIL${NC}"
        return 1
    fi
}

# Test 1: Prometheus is accessible
test_step "Prometheus is accessible" \
    "curl -sf $PROMETHEUS_URL/-/healthy" \
    "Prometheus is Healthy"

# Test 2: Alertmanager is accessible
test_step "Alertmanager is accessible" \
    "curl -sf $ALERTMANAGER_URL/-/healthy" \
    "OK"

# Test 3: Metrics are being scraped
echo ""
echo "Checking metrics endpoints..."
METRICS_COUNT=$(curl -s "$PROMETHEUS_URL/api/v1/query?query=up" | jq '.data.result | length')
if [ "$METRICS_COUNT" -gt 0 ]; then
    echo -e "${GREEN}PASS${NC}: Found $METRICS_COUNT targets being scraped"
else
    echo -e "${RED}FAIL${NC}: No targets found"
fi

# Test 4: Alert rules are loaded
echo ""
echo "Checking alert rules..."
RULES_LOADED=$(curl -s "$PROMETHEUS_URL/api/v1/rules" | jq '[.data.groups[].rules[] | select(.type=="alerting")] | length')
if [ "$RULES_LOADED" -gt 0 ]; then
    echo -e "${GREEN}PASS${NC}: $RULES_LOADED alert rules loaded"

    # Show first few rules
    echo ""
    echo "Sample alert rules:"
    curl -s "$PROMETHEUS_URL/api/v1/rules" | \
        jq -r '.data.groups[].rules[] | select(.type=="alerting") | "  - \(.name) [\(.state)]"' | \
        head -10
else
    echo -e "${RED}FAIL${NC}: No alert rules loaded"
fi

# Test 5: Send test alert
echo ""
echo "Sending test alert to Alertmanager..."
TEST_ALERT=$(cat <<EOF
[
  {
    "labels": {
      "alertname": "TestAlert",
      "severity": "info",
      "instance": "test-node",
      "job": "norn-test"
    },
    "annotations": {
      "summary": "Test alert from monitoring system",
      "description": "This is a test alert to verify notification delivery"
    }
  }
]
EOF
)

if curl -sf -X POST "$ALERTMANAGER_URL/api/v1/alerts" \
    -H "Content-Type: application/json" \
    -d "$TEST_ALERT" > /dev/null; then
    echo -e "${GREEN}PASS${NC}: Test alert sent successfully"

    # Wait for alert to be processed
    sleep 2

    # Check if alert was received
    ALERT_RECEIVED=$(curl -s "$ALERTMANAGER_URL/api/v1/alerts" | \
        jq '[.[] | select(.labels.alertname=="TestAlert")] | length')

    if [ "$ALERT_RECEIVED" -gt 0 ]; then
        echo -e "${GREEN}PASS${NC}: Test alert received by Alertmanager"
    else
        echo -e "${YELLOW}WARN${NC}: Test alert not found in Alertmanager (may need more time)"
    fi
else
    echo -e "${RED}FAIL${NC}: Failed to send test alert"
fi

# Test 6: Check critical alerts (should be firing or resolved)
echo ""
echo "Checking critical alerts state..."
CRITICAL_ALERTS=$(curl -s "$PROMETHEUS_URL/api/v1/alerts" | \
    jq '[.data.alerts[] | select(.labels.severity=="critical")] | length')

echo "Critical alerts: $CRITICAL_ALERTS"

# Show firing alerts
echo ""
echo "Currently firing alerts:"
curl -s "$PROMETHEUS_URL/api/v1/alerts" | \
    jq -r '.data.alerts[] | select(.state=="firing") | "\(.labels.alertname) - \(.labels.severity) - \(.state)"' | \
    while read -r line; do
        if [ -n "$line" ]; then
            echo "  $line"
        fi
    done || echo "  No firing alerts"

# Test 7: Verify notification configuration
echo ""
echo "Checking notification receivers..."
RECEIVERS=$(curl -s "$ALERTMANAGER_URL/api/v1/status" | \
    jq '.data.config.receiverNames | length')

echo "Configured receivers: $RECEIVERS"
curl -s "$ALERTMANAGER_URL/api/v1/status" | \
    jq -r '.data.config.receiverNames[]' | \
    while read -r receiver; do
        echo "  - $receiver"
    done

# Test 8: Test specific metrics
echo ""
echo "Checking key metrics availability..."
METRICS_TO_CHECK=(
    "norn_block_height"
    "norn_peer_connections"
    "norn_txpool_size"
    "norn_tps"
)

for metric in "${METRICS_TO_CHECK[@]}"; do
    VALUE=$(curl -s "$PROMETHEUS_URL/api/v1/query?query=$metric" | \
        jq -r '.data.result[0].value[1] // "null"')

    if [ "$VALUE" != "null" ] && [ -n "$VALUE" ]; then
        echo -e "  ${GREEN}✓${NC} $metric = $VALUE"
    else
        echo -e "  ${YELLOW}○${NC} $metric = no data"
    fi
done

# Summary
echo ""
echo "=========================================="
echo "Test Summary"
echo "=========================================="
echo "Prometheus URL: $PROMETHEUS_URL"
echo "Alertmanager URL: $ALERTMANAGER_URL"
echo ""
echo "Next steps:"
echo "1. View alerts: $ALERTMANAGER_URL/#/alerts"
echo "2. View metrics: $PROMETHEUS_URL/graph"
echo "3. View status: $ALERTMANAGER_URL/#/status"
echo "4. Check Grafana dashboards: http://localhost:3000"
echo ""
echo "For full documentation, see: docs/ALERTING.md"
echo "=========================================="
