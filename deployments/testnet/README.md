# Norn Enhanced Testnet Deployment

This directory contains the complete setup for deploying a 3-node Norn testnet with enhanced features (enhanced transaction pool and fast sync).

## 🏗️ Architecture

### Nodes

- **Validator 1**: Block producer with enhanced txpool
- **Validator 2**: Block producer with enhanced txpool
- **Observer**: Non-validating node using fast sync

### Monitoring

- **Prometheus**: Metrics collection on port 9090
- **Grafana**: Visualization dashboard on port 3000
- **Health Checks**: HTTP endpoints on port 8000 for each node

### Network

All nodes are connected in a bridge network `norn_testnet` with fixed IP addresses.

## 🚀 Quick Start

### Prerequisites

```bash
# Install Docker
# Install docker-compose
# Or use docker compose (v2)
```

### 1. Start the Testnet

```bash
cd deployments/testnet
./start.sh
```

This will:
- Build Docker images with enhanced features
- Start 3 nodes (2 validators + 1 observer)
- Start Prometheus and Grafana
- Configure all networking

### 2. Verify Deployment

```bash
# Run automated tests
./test_enhanced_features.sh
```

### 3. Access Services

| Service | URL | Credentials |
|---------|-----|-------------|
| Validator 1 RPC | http://localhost:50051 | - |
| Validator 1 Health | http://localhost:8011/health | - |
| Validator 2 RPC | http://localhost:50052 | - |
| Validator 2 Health | http://localhost:8012/health | - |
| Observer RPC | http://localhost:50053 | - |
| Observer Health | http://localhost:8013/health | - |
| Prometheus | http://localhost:9090 | - |
| Grafana | http://localhost:3000 | admin/admin |

### 4. View Logs

```bash
# All nodes
docker-compose logs -f

# Specific node
docker-compose logs -f validator1
docker-compose logs -f validator2
docker-compose logs -f observer

# Prometheus
docker-compose logs -f prometheus
```

### 5. Stop the Testnet

```bash
# Stop containers (preserve data)
./stop.sh

# Stop and clean all data
./stop.sh --clean
```

## 🔧 Configuration

### Node Configs

Located in `config/` directory:

- `validator1.toml` - First validator (bootstrap peer)
- `validator2.toml` - Second validator
- `observer.toml` - Observer node (fast sync)

### Enhanced Features

All nodes have:
- ✅ Enhanced transaction pool (priority queue, EIP-1559)
- ✅ Fast sync mode
- ✅ Monitoring enabled
- ✅ Health check endpoints

## 📊 Monitoring

### Prometheus Metrics

Available at `http://localhost:9090`

Key metrics:
- `txpool_size` - Current transaction pool size
- `txpool_avg_gas_price` - Average gas price
- `block_height` - Current block number
- `txpackaged_total` - Total transactions packaged
- `peer_count` - Number of connected peers

### Grafana Dashboards

Access at `http://localhost:3000` (admin/admin)

Pre-configured dashboards:
- Norn Enhanced Testnet Overview
- Transaction Pool Statistics
- Block Production Rate
- Network Health

## 🧪 Testing Enhanced Features

### 1. Transaction Prioritization

```bash
# Submit transactions with different gas prices
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_sendRawTransaction",
    "params": ["0x..."],
    "id": 1
  }'
```

Higher gas price transactions should be processed first.

### 2. Transaction Replacement (EIP-1559)

Submit a new transaction with:
- Same sender address
- Same nonce
- 10% higher gas price

The old transaction should be replaced.

### 3. Fast Sync Verification

```bash
# Check observer sync status
curl -X POST http://localhost:50053 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_syncing","params":[],"id":1}'
```

The observer should sync using fast mode.

## 🐛 Troubleshooting

### Nodes not starting

```bash
# Check logs
docker-compose logs validator1

# Verify config
cat config/validator1.toml

# Rebuild
docker-compose build
docker-compose up -d
```

### Can't access RPC

```bash
# Check if node is running
docker-compose ps

# Check logs
docker-compose logs -f validator1

# Verify port mapping
docker-compose port validator1 50051
```

### Metrics not available

```bash
# Verify metrics endpoint
curl http://localhost:9090/metrics

# Check Prometheus targets
open http://localhost:9090/targets
```

## 🏥 Health Check Endpoints

Each node exposes comprehensive health check endpoints for monitoring and orchestration:

### Available Endpoints

- `GET /health` - Basic health status (200 OK / 503 Unavailable)
- `GET /health/detailed` - Component-level health details
- `GET /ready` - Readiness probe (Kubernetes-style)
- `GET /live` - Liveness probe (Kubernetes-style)
- `GET /metrics` - Prometheus metrics

### Testing Health

```bash
# Check validator 1 health
curl http://localhost:8011/health | jq .

# Check detailed health
curl http://localhost:8011/health/detailed | jq .

# Test readiness
curl http://localhost:8011/ready | jq .

# Test liveness
curl http://localhost:8011/live

# Get Prometheus metrics
curl http://localhost:8011/metrics | head -20
```

### Automated Health Testing

```bash
# Run health endpoint tests
./test_health_endpoints.sh
```

For detailed documentation, see [HEALTH_CHECK.md](HEALTH_CHECK.md).

## ⚡ Performance Testing

### Quick Performance Test

```bash
# Run performance benchmark
./benchmark-performance.sh
```

This tests:
- Enhanced transaction pool functionality
- Health endpoint response times
- Prometheus metrics availability
- Grafana dashboard status

### TPS Testing

Use the `tps_test` module:

```bash
# Build TPS test
cargo build -p tps_test --release

# Run against testnet
./target/release/tps_test --rate 100 --duration 60 --rpc-address 127.0.0.1:50051
```

### Monitor Results

Watch Grafana dashboard for real-time metrics:
- TPS (transactions per second)
- Block production rate
- Transaction pool size
- Average gas price

## 📈 Performance Testing

### Load Testing

Use the `tps_test` module:

```bash
# Build TPS test
cargo build -p tps_test --release

# Run against testnet
./target/release/tps_test --rate 100 --duration 60 --rpc-address 127.0.0.1:50051
```

### Monitor Results

Watch Grafana dashboard for real-time metrics:
- TPS (transactions per second)
- Block production rate
- Transaction pool size
- Average gas price

## 🔒 Security Considerations

This is a **testnet** deployment:

- ⚠️ Not suitable for production
- ⚠️ No secure key management
- ⚠️ Default credentials (Grafana: admin/admin)
- ⚠️ All nodes on same machine

For production:
- Use proper key management
- Distribute nodes across servers
- Enable TLS/SSL
- Use strong passwords
- Configure firewalls

## 📝 Customization

### Adding More Validators

1. Create new config file: `config/validatorN.toml`
2. Update `docker-compose.yml` with new service
3. Add to `genesis.json` validators array
4. Update network configuration

### Changing Consensus Parameters

Edit config files:
- `block_interval` - Block production time
- `stake_weights` - Validator stakes
- `max_txs_per_block` - Transaction limit

### Adjusting Pool Sizes

Edit config files:
```toml
[txpool]
max_size = 20480  # Increase pool size
```

## 📚 Related Documentation

- [NEW_FEATURES_ROADMAP.md](../../NEW_FEATURES_ROADMAP.md) - Implementation roadmap
- [CLAUDE.md](../../CLAUDE.md) - Project overview
- [../../README.md](../../README.md) - Main README

## 🆘 Support

For issues or questions:
- Check logs: `docker-compose logs -f`
- Review configuration
- Consult troubleshooting section above

---

**Version**: 1.0
**Last Updated**: 2026-01-30
**Status**: ✅ Ready for testing
