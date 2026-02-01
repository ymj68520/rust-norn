# Health Check Endpoints Documentation

## Overview

The Norn node provides comprehensive health check endpoints for monitoring, orchestration, and load balancing purposes. These endpoints follow standard Kubernetes patterns for liveness and readiness probes.

## Endpoints

### 1. Liveness Probe `/live`

**Purpose**: Determine if the node is alive and responsive.

**Method**: `GET`

**Response**:
```json
{
  "status": "alive",
  "uptime_seconds": 3600
}
```

**Status Codes**:
- `200 OK` - Node is alive
- `503 Service Unavailable` - Node is not responding (should never happen if response is received)

**Usage**:
```bash
curl http://localhost:8000/live
```

**Kubernetes Example**:
```yaml
livenessProbe:
  httpGet:
    path: /live
    port: 8000
  initialDelaySeconds: 30
  periodSeconds: 10
```

### 2. Readiness Probe `/ready`

**Purpose**: Determine if the node is ready to accept transactions.

**Method**: `GET`

**Response**:
```json
{
  "ready": true,
  "message": "Node is ready to accept transactions",
  "checks": [
    {
      "name": "peers_connected",
      "status": "pass",
      "message": "Connected to 3 peers",
      "last_check": "2026-01-30T12:00:00Z"
    },
    {
      "name": "blockchain_initialized",
      "status": "pass",
      "message": "Block height: 12345",
      "last_check": "2026-01-30T12:00:00Z"
    }
  ]
}
```

**Status Codes**:
- `200 OK` - Node is ready
- `503 Service Unavailable` - Node is not ready (initializing, syncing, or degraded)

**Usage**:
```bash
curl http://localhost:8000/ready
```

**Kubernetes Example**:
```yaml
readinessProbe:
  httpGet:
    path: /ready
    port: 8000
  initialDelaySeconds: 10
  periodSeconds: 5
```

### 3. Basic Health Check `/health`

**Purpose**: Simple health status for quick monitoring.

**Method**: `GET`

**Response**:
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_seconds": 3600,
  "block_height": 12345,
  "peer_count": 3,
  "txpool_size": 100,
  "is_healthy": true
}
```

**Status Codes**:
- `200 OK` - Node is healthy
- `503 Service Unavailable` - Node is unhealthy

**Usage**:
```bash
curl http://localhost:8000/health
```

### 4. Detailed Health Check `/health/detailed`

**Purpose**: Comprehensive health status with component-level details.

**Method**: `GET`

**Response**:
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_seconds": 3600,
  "components": [
    {
      "name": "blockchain",
      "status": "healthy",
      "message": "Block height: 12345",
      "last_check": "2026-01-30T12:00:00Z"
    },
    {
      "name": "network",
      "status": "healthy",
      "message": "Connected to 3 peers",
      "last_check": "2026-01-30T12:00:00Z"
    },
    {
      "name": "txpool",
      "status": "healthy",
      "message": "Pool size: 100",
      "last_check": "2026-01-30T12:00:00Z"
    },
    {
      "name": "consensus",
      "status": "healthy",
      "message": "PoVF consensus running",
      "last_check": "2026-01-30T12:00:00Z"
    },
    {
      "name": "storage",
      "status": "healthy",
      "message": "SledDB storage operational",
      "last_check": "2026-01-30T12:00:00Z"
    }
  ],
  "metrics": {
    "block_height": 12345,
    "peer_count": 3,
    "txpool_size": 100,
    "sync_status": "synced"
  },
  "is_healthy": true
}
```

**Status Codes**:
- `200 OK` - All components healthy
- `503 Service Unavailable` - One or more components unhealthy

**Usage**:
```bash
curl http://localhost:8000/health/detailed
```

### 5. Metrics Endpoint `/metrics`

**Purpose**: Prometheus metrics exposition.

**Method**: `GET`

**Response**: Plain text Prometheus format

**Usage**:
```bash
curl http://localhost:8000/metrics
```

## Configuration

Health check endpoints are configured in `node_config.toml`:

```toml
[monitoring]
health_check_enabled = true
health_check_address = "0.0.0.0:8000"
prometheus_enabled = true
prometheus_address = "0.0.0.0:9090"
```

## Component Status Values

Each component can have the following status values:

- **healthy**: Component is functioning normally
- **degraded**: Component is operational but below optimal performance
- **unhealthy**: Component is not functioning properly
- **unknown**: Component status cannot be determined

## Health Determination

### Basic Health (`/health`)
Node is considered healthy if:
- `peer_count > 0` (at least one peer connected)
- `block_height >= 0` (blockchain initialized)
- `txpool_size >= 0` (transaction pool initialized)

### Readiness (`/ready`)
Node is considered ready if:
- `peer_count > 0` (connected to network)
- `block_height >= 0` (blockchain synced)

### Detailed Health (`/health/detailed`)
Node is considered healthy if ALL components report status as "healthy".

## Monitoring Integration

### Prometheus

Prometheus can scrape the `/metrics` endpoint:

```yaml
scrape_configs:
  - job_name: 'norn-node'
    static_configs:
      - targets: ['localhost:8000']
    metrics_path: '/metrics'
```

### Grafana

Use the pre-configured dashboards:
- Import `deployments/testnet/grafana/dashboards/norn-full-dashboard.json`
- Import `deployments/testnet/grafana/dashboards/norn-performance.json`

### Alertmanager

Example alert rules:

```yaml
groups:
  - name: norn_node
    rules:
      - alert: NodeUnhealthy
        expr: up{job="norn-node"} == 0
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "Node {{ $labels.instance }} is down"

      - alert: NoPeersConnected
        expr: norn_peer_connections == 0
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Node {{ $labels.instance }} has no peer connections"
```

## Testing Health Endpoints

### Quick Test Script

```bash
#!/bin/bash
# test_health_endpoints.sh

BASE_URL="http://localhost:8000"

echo "Testing liveness..."
curl -s ${BASE_URL}/live | jq .

echo "Testing readiness..."
curl -s ${BASE_URL}/ready | jq .

echo "Testing basic health..."
curl -s ${BASE_URL}/health | jq .

echo "Testing detailed health..."
curl -s ${BASE_URL}/health/detailed | jq .

echo "Testing metrics..."
curl -s ${BASE_URL}/metrics | head -20
```

### Load Test

```bash
# Test endpoint response time
for i in {1..100}; do
  curl -w "@curl-format.txt" -o /dev/null -s http://localhost:8000/health
done
```

## Troubleshooting

### Health Check Returns 503

1. Check if node is starting up: `docker-compose logs -f validator1`
2. Verify peer connections: Check network configuration
3. Check blockchain sync: `curl http://localhost:8000/health/detailed`

### Readiness Check Fails

1. Verify bootstrap peers are reachable
2. Check if other nodes are running
3. Review logs for connection errors

### Metrics Not Available

1. Verify Prometheus is enabled in config
2. Check if metrics endpoint is accessible
3. Review Prometheus scrape configuration

## Best Practices

1. **Liveness**: Use for detecting deadlocks and hangs
2. **Readiness**: Use for traffic routing decisions
3. **Health**: Use for basic monitoring and alerting
4. **Detailed Health**: Use for deep debugging and analysis

**Important**: Liveness should rarely fail. If it does, the node should be restarted. Readiness can fail temporarily during startup or sync.

---

**Version**: 1.0
**Last Updated**: 2026-01-30
**Status**: ✅ Implemented
