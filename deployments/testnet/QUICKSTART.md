# 🌐 Norn Enhanced Testnet - Quick Start Guide

## ⚡ 5-Minute Setup

### Step 1: Prerequisites (2 min)

```bash
# Verify Docker is installed
docker --version
docker-compose --version

# If not installed:
# Ubuntu/Debian:
sudo apt-get update && sudo apt-get install docker.io docker-compose

# Start Docker if not running
sudo systemctl start docker
```

### Step 2: Build and Start (2 min)

```bash
# Navigate to testnet directory
cd deployments/testnet

# Start the testnet
./start.sh
```

Expected output:
```
🌐 Starting Norn Enhanced Testnet...
🛑 Stopping existing containers...
🔨 Building Docker images...
🚀 Starting testnet...
✅ Testnet started!
```

### Step 3: Verify (1 min)

```bash
# Run verification tests
./test_enhanced_features.sh
```

## 🎯 What You Get

### 3 Node Network

| Node | Type | P2P | RPC |
|------|------|-----|-----|
| Validator 1 | Producer | 4001 | 50051 |
| Validator 2 | Producer | 4002 | 50052 |
| Observer | Fast Sync | 4003 | 50053 |

### Enhanced Features

- ✅ **Priority Transaction Pool**: Higher gas price = processed first
- ✅ **EIP-1559 Replacement**: Replace transactions with higher fees
- ✅ **Fast Sync**: Observer syncs quickly using snapshot
- ✅ **Monitoring**: Prometheus + Grafana dashboards

## 🧪 Quick Test

### Send a Transaction

```bash
# Using curl
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_getBlockByNumber",
    "params": ["latest", false],
    "id": 1
  }'
```

### Check Transaction Pool

```bash
# View pool stats
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "txpool_status",
    "params": [],
    "id": 1
  }'
```

### View Metrics

```bash
# Prometheus
open http://localhost:9090

# Grafana Dashboard
open http://localhost:3000
# Login: admin/admin
```

### Check Health

```bash
# Test health endpoints
./test_health_endpoints.sh

# Or check directly
curl http://localhost:8011/health | jq .
curl http://localhost:8011/ready | jq .
curl http://localhost:8011/live
```

## 📊 Monitoring Dashboard

### Grafana Dashboards

1. Open http://localhost:3000
2. Login with admin/admin
3. View "Norn Enhanced Testnet" dashboard

### Key Metrics to Watch

- **Transaction Pool Size**: Should grow as you submit txs
- **Block Height**: Should increase every ~5 seconds
- **TPS**: Transactions per second
- **Peer Count**: Should show 3 nodes connected

## 🔧 Common Commands

```bash
# View logs
docker-compose logs -f validator1

# Restart a node
docker-compose restart validator1

# Stop entire testnet
./stop.sh

# Stop and clean data
./stop.sh --clean

# Restart testnet
./start.sh
```

## 🎓 Next Steps

1. **Experiment with Enhanced Features**
   - Submit multiple transactions with different gas prices
   - Verify higher gas price txs are processed first
   - Test transaction replacement (EIP-1559)

2. **Performance Testing**
   - Run TPS tests: `./../../target/release/tps_test --rate 100`
   - Monitor Grafana for performance metrics
   - Adjust configuration for optimization

3. **Development**
   - Modify config files in `config/`
   - Restart nodes: `docker-compose restart`
   - Test new features

## 🐛 Quick Fixes

### Port Already in Use

```bash
# Check what's using the port
lsof -i :4001

# Stop the service or change port in config
```

### Nodes Not Connecting

```bash
# Check network
docker network ls
docker network inspect deployments_testnet_norn_testnet

# Verify bootstrap peers in config
cat config/validator2.toml | grep bootstrap
```

### Out of Sync

```bash
# Check sync status
curl -X POST http://localhost:50053 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_syncing","params":[],"id":1}'

# Restart observer
docker-compose restart observer
```

## 📈 Performance Tips

### Increase Throughput

Edit config files:
```toml
[txpool]
max_size = 40960  # Double the pool size

[core.consensus]
block_interval = 1  # Faster blocks (1 second instead of 5)
```

### Monitor Performance

Use Grafana to watch:
- Transaction pool fill rate
- Block production rate
- Network latency
- Memory usage

## 🎉 Success Indicators

You'll know everything is working when:

- ✅ All 3 nodes show as running: `docker-compose ps`
- ✅ Block height is increasing: `curl http://localhost:50051`
- ✅ Peer count is 3: Check Prometheus
- ✅ Metrics are visible: http://localhost:9090/metrics
- ✅ Grafana dashboard loads: http://localhost:3000

## 📚 Learn More

- Full documentation: [README.md](README.md)
- Implementation roadmap: [../../NEW_FEATURES_ROADMAP.md](../../NEW_FEATURES_ROADMAP.md)
- Project overview: [../../CLAUDE.md](../../CLAUDE.md)

---

**Ready to test? Run `./start.sh` now! 🚀**
