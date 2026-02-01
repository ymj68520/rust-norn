# TPS 测试快速指南

## 快速开始

### 1. 编译测试工具

```bash
# 方式 1: 使用 Makefile
make build

# 方式 2: 直接使用 Cargo
cargo build -p tps_test --release
```

### 2. 启动测试节点

```bash
# 方式 1: 使用 Docker（推荐）
cd docker
./start-nodes.sh

# 方式 2: 手动启动
./target/release/norn --config config.toml
```

### 3. 运行 TPS 测试

```bash
# 使用自动化脚本（推荐）
cd tps_test
./run_tps_test.sh

# 或直接运行
./target/release/tps_test
```

## 测试场景

### 场景 1: 基准测试（100 TPS, 60秒）

```bash
./target/release/tps_test --rate 100 --duration 60
```

**预期结果**: 系统应该能够达到 90% 以上的达成率

### 场景 2: 中等负载（500 TPS, 120秒）

```bash
./target/release/tps_test --rate 500 --duration 120
```

**预期结果**: 测试系统在中等负载下的表现

### 场景 3: 高负载压力测试（1000 TPS, 300秒）

```bash
./target/release/tps_test --rate 1000 --duration 300 --batch-size 50
```

**预期结果**: 找出系统性能瓶颈

### 场景 4: 使用自动化脚本

```bash
# 默认配置（100 TPS, 60秒）
./run_tps_test.sh

# 自定义配置
RATE=500 DURATION=120 ./run_tps_test.sh

# 压力测试
RATE=1000 DURATION=300 BATCH_SIZE=50 ./run_tps_test.sh
```

## 环境变量配置

使用自动化脚本时，可以通过环境变量配置：

```bash
# 指定 RPC 地址
RPC_ADDRESS=127.0.0.1:50052 ./run_tps_test.sh

# 完整配置
RPC_ADDRESS=127.0.0.1:50051 \
RATE=500 \
DURATION=120 \
BATCH_SIZE=20 \
./run_tps_test.sh
```

## 多节点测试

如果使用 Docker Compose 启动了多个节点，可以分别测试：

```bash
# 测试节点 1
./target/release/tps_test --rpc-address 127.0.0.1:50051 --rate 200

# 测试节点 2
./target/release/tps_test --rpc-address 127.0.0.1:50052 --rate 200

# 测试节点 3
./target/release/tps_test --rpc-address 127.0.0.1:50053 --rate 200
```

## 结果解读

### 成功的测试

```
✅ 优秀: TPS 达成率 95.00% >= 90%
✅ 优秀: 交易成功率 100.00% >= 99%
📊 交易打包率: 95.00%
```

### 需要优化的测试

```
❌ 需要优化: TPS 达成率 45.00% < 50%
❌ 需要优化: 交易成功率 92.00% < 95%
📊 交易打包率: 80.00%
```

可能的原因：
- 节点处理能力不足
- 共识机制限制
- 网络带宽瓶颈
- 数据库性能问题

## 故障排查

### 问题 1: 无法连接到 RPC

```bash
# 检查节点是否运行
nc -zv 127.0.0.1 50051

# 查看节点日志
docker logs norn-node1
```

### 问题 2: TPS 很低

- 检查节点 CPU 和内存使用情况
- 查看节点日志是否有错误
- 增加批次大小（`--batch-size`）
- 减少目标 TPS

### 问题 3: 交易打包率低

- 增加等待时间（修改代码中的 30 秒）
- 检查共识机制是否正常工作
- 查看交易池状态

## 性能优化建议

1. **调整批次大小**: 增加 `--batch-size` 可以提高吞吐量
2. **优化数据库**: 使用更快的存储（SSD）
3. **增加节点资源**: 分配更多 CPU 和内存
4. **网络优化**: 减少网络延迟
5. **共识优化**: 优化区块生成和验证逻辑

## 下一步

- 查看 `README.md` 了解详细信息
- 阅读源码了解实现细节
- 根据测试结果优化系统性能
- 尝试不同的测试场景和参数
