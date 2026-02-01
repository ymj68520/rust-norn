# Norn 文档中心

欢迎来到 Norn 区块链项目文档中心。这里包含了完整的项目文档，帮助您快速了解和使用 Norn。

---

## 📚 文档导航

### 核心文档

| 文档 | 描述 | 适合人群 |
|------|------|---------|
| [架构文档](./架构文档.md) | 系统架构、核心组件、技术栈 | 架构师、高级开发者 |
| [开发指南](./开发指南.md) | 环境搭建、开发流程、代码规范 | 开发者 |
| [API 文档](./API文档.md) | gRPC 和 JSON-RPC API 接口 | DApp 开发者、集成商 |
| [性能优化](./性能优化.md) | 性能调优、监控、最佳实践 | 运维人员、性能工程师 |
| [状态剪裁](./状态剪裁.md) | 数据剪裁策略、快照管理 | 运维人员、节点运营者 |
| [测试文档](./测试文档.md) | 单元测试、集成测试、TPS 测试 | 测试工程师、开发者 |

### 英文文档

| 文档 | 描述 |
|------|------|
| [ALERTING.md](./ALERTING.md) | 告警配置和监控系统 |
| [WEBSOCKET.md](./WEBSOCKET.md) | WebSocket 接口文档 |
| [SECURITY_AUDIT_REPORT.md](./SECURITY_AUDIT_REPORT.md) | 安全审计报告 |
| [PERFORMANCE.md](./PERFORMANCE.md) | 性能测试报告 |

---

## 🚀 快速开始

### 1. 首次使用？

建议按以下顺序阅读：

1. **了解架构** → [架构文档](./架构文档.md)
2. **搭建环境** → [开发指南 - 环境准备](./开发指南.md#环境准备)
3. **构建运行** → [开发指南 - 构建项目](./开发指南.md#构建项目)
4. **调用 API** → [API 文档](./API文档.md)

### 2. 开发者？

请重点关注：

- [开发指南](./开发指南.md) - 完整的开发工作流
- [测试文档](./测试文档.md) - 如何编写和运行测试
- [代码规范](./开发指南.md#代码规范) - Rust 编码规范

### 3. 运维人员？

请重点关注：

- [性能优化](./性能优化.md) - 性能调优指南
- [状态剪裁](./状态剪裁.md) - 存储管理策略
- [ALERTING.md](./ALERTING.md) - 监控告警配置

### 4. DApp 开发者？

请重点关注：

- [API 文档](./API文档.md) - 完整的 API 参考
- [WEBSOCKET.md](./WEBSOCKET.md) - 实时订阅接口

---

## 📖 文档主题索引

### 共识机制

- **PoVF 共识**: [架构文档 - 共识机制](./架构文档.md#共识机制)
- VRF 随机选举: [架构文档 - VRF](./架构文档.md#vrf-可验证随机函数)
- VDF 时延保证: [架构文档 - VDF](./架构文档.md#vdf-可验证延迟函数)

### 状态管理

- **Merkle Patricia Tree**: [架构文档 - 状态管理](./架构文档.md#状态管理)
- 状态缓存: [性能优化 - 缓存优化](./性能优化.md#缓存优化)
- 状态剪裁: [状态剪裁文档](./状态剪裁.md)

### 网络层

- **P2P 协议**: [架构文档 - 网络协议](./架构文档.md#网络协议)
- **libp2p**: [架构文档 - 网络层](./架构文档.md#2-网络层-norn-network)
- **消息压缩**: [性能优化 - 网络优化](./性能优化.md#网络优化)

### 性能优化

- **缓存策略**: [性能优化 - 缓存优化](./性能优化.md#缓存优化)
- **并发优化**: [性能优化 - 并发优化](./性能优化.md#并发优化)
- **EVM 优化**: [性能优化 - EVM 执行优化](./性能优化.md#evm-执行优化)
- **TPS 测试**: [测试文档 - TPS 测试](./测试文档.md#tps-测试)

### 安全

- **安全审计**: [SECURITY_AUDIT_REPORT.md](./SECURITY_AUDIT_REPORT.md)
- **加密算法**: [架构文档 - 安全考虑](./架构文档.md#安全考虑)

---

## 🔍 按场景查找

### 场景：部署测试节点

```bash
# 1. 克隆代码
git clone https://github.com/your-org/rust-norn.git
cd rust-norn

# 2. 构建项目
cargo build --release

# 3. 生成密钥
./target/release/norn generate-key --out node.key

# 4. 配置节点（参考 README.md）
cp production_config.toml config.toml
vim config.toml

# 5. 启动节点
./target/release/norn --config config.toml
```

**相关文档**: [开发指南 - 构建项目](./开发指南.md#构建项目)

### 场景：连接节点并查询

```bash
# 使用 curl 查询最新区块号
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# 查询账户余额
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x...","latest"],"id":1}'
```

**相关文档**: [API 文档 - 使用示例](./API文档.md#使用示例)

### 场景：发送交易

```javascript
const { ethers } = require('ethers');

const provider = new ethers.JsonRpcProvider('http://localhost:50051');
const wallet = new ethers.Wallet(PRIVATE_KEY, provider);

// 发送 ETH
const tx = await wallet.sendTransaction({
  to: "0x742d35Cc6634C0532925a3b844Bc4c1f8e07C94",
  value: ethers.parseEther("1.0")
});

await tx.wait();
console.log("Transaction confirmed:", tx.hash);
```

**相关文档**: [API 文档 - 发送交易](./API文档.md#2-发送交易)

### 场景：性能调优

1. **启用缓存**: [性能优化 - 缓存优化](./性能优化.md#缓存优化)
2. **配置线程池**: [性能优化 - 并发优化](./性能优化.md#并发优化)
3. **监控指标**: [性能优化 - 监控与分析](./性能优化.md#监控与分析)
4. **运行 TPS 测试**: [测试文档 - TPS 测试](./测试文档.md#tps-测试)

### 场景：运行多节点网络

```bash
# 使用 Docker Compose 启动 3 节点网络
docker-compose up -d

# 查看日志
./docker/logs.sh

# 停止网络
docker-compose down
```

**相关文档**: [README.md - Docker 多节点设置](../README.md#docker-多节点设置)

---

## 🛠️ 常用命令参考

### 构建和测试

```bash
# 完整开发流程
make dev              # 格式化 + 检查 + 测试
make build            # 构建发布版本
make test             # 运行所有测试
make ci               # CI 流程（格式 + Clippy + 测试）

# TPS 性能测试
./target/release/tps_test --rate 500 --duration 60

# 生成文档
make doc              # 生成并打开文档
```

### 节点操作

```bash
# 生成密钥
./target/release/norn generate-key --out node.key

# 启动节点
./target/release/norn --config config.toml

# 检查节点状态
curl http://localhost:50051/health

# 查看连接的对等节点
curl http://localhost:50051/peers
```

### 数据库操作

```bash
# 查看数据库大小
du -sh node_data/sled/

# 清理数据（删除并重新同步）
rm -rf node_data/
./target/release/norn --config config.toml
```

---

## 📊 性能指标速查

| 指标 | 数值 | 说明 |
|------|------|------|
| **最大 TPS** | 1000+ | 优化后的峰值吞吐量 |
| **出块间隔** | 1 秒 | 默认配置，可调整 |
| **交易确认** | ~2 秒 | 约 2 个区块 |
| **内存占用** | ~200MB | 单节点运行时 |
| **同步速度** | ~500 blk/s | Fast Sync 模式 |

**更多性能数据**: [性能优化文档](./性能优化.md#性能指标)

---

## 🔗 外部资源

### 官方资源

- **GitHub**: https://github.com/your-org/rust-norn
- **Issues**: https://github.com/your-org/rust-norn/issues
- **Discussions**: https://github.com/your-org/rust-norn/discussions

### 技术栈文档

- **Rust**: https://www.rust-lang.org/learn
- **Tokio**: https://tokio.rs/
- **libp2p**: https://docs.libp2p.io/
- **revm (EVM)**: https://github.com/bluealloy/revm
- **SledDB**: https://sled.rs/

### 以太坊资源

- **Ethereum.org**: https://ethereum.org/developers/
- **Web3.js**: https://web3js.readthedocs.io/
- **Ethers.js**: https://docs.ethers.org/

---

## ❓ 获取帮助

### 报告问题

如果您发现 bug 或有功能建议：

1. 搜索 [已有 Issues](https://github.com/your-org/rust-norn/issues)
2. 创建新 Issue，使用模板：
   - Bug 报告：提供复现步骤、日志、环境信息
   - 功能请求：描述使用场景、期望行为

### 贡献代码

欢迎贡献！请阅读：

- [开发指南 - 贡献流程](./开发指南.md#贡献流程)
- [开发指南 - 代码规范](./开发指南.md#代码规范)

### 社区交流

- **Discord**: [加入我们的 Discord](https://discord.gg/rust-norn)
- **GitHub Discussions**: [参与讨论](https://github.com/your-org/rust-norn/discussions)

---

## 📝 文档维护

### 文档更新

文档随项目持续更新，每次发布新版本时同步更新。

### 贡献文档

发现文档错误或想要改进？

```bash
# 1. Fork 项目
# 2. 编辑文档
vim docs/xxx.md

# 3. 预览 Markdown（可选）
# 使用 VSCode 预览插件或 Markdown 查看器

# 4. 提交 PR
git add docs/xxx.md
git commit -m "docs: update xxx documentation"
git push origin feature/docs-update
```

### 文档规范

- 使用 Markdown 格式
- 中文文档使用简体中文
- 代码示例可运行
- 包含必要注释
- 更新日期和版本号

---

## 🗂️ 文档目录结构

```
docs/
├── README.md                  # 文档中心（本文件）
├── 架构文档.md                # 系统架构和核心组件
├── 开发指南.md                # 开发环境和流程
├── API文档.md                 # API 接口文档
├── 性能优化.md                # 性能调优指南
├── 状态剪裁.md                # 数据剪裁和快照
├── 测试文档.md                # 测试策略和工具
├── ALERTING.md                # 监控告警（英文）
├── WEBSOCKET.md               # WebSocket 接口（英文）
├── SECURITY_AUDIT_REPORT.md   # 安全审计（英文）
├── PERFORMANCE.md             # 性能报告（英文）
├── api/                       # API 子目录（已废弃）
├── architecture/              # 架构子目录（空）
├── guides/                    # 指南子目录（空）
└── testing/                   # 测试子目录（空）
```

---

## 📜 许可证

本文档采用 [MIT License](../LICENSE)。

---

**最后更新**: 2026-02-01
**文档版本**: 1.0.0
**维护者**: Norn 开发团队
