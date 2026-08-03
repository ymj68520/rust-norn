# rust-norn

<div align="center">

[![Rust](https://img.shields.io/badge/Rust-Edition%202021-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build](https://img.shields.io/badge/Build-Passing-green.svg)]()

**高性能 Rust 区块链节点实现 | PoVF 共识 | EVM 兼容**

</div>

---

## 📖 简介

**rust-norn** 是一个用 Rust 实现的高性能区块链节点，采用创新的 **PoVF (Proof of Verifiable Function，可验证函数证明)** 共识机制，同时兼容以太坊 EVM。

### 核心特性

- 🎲 **PoVF 共识** - 结合 VRF 随机选举 + VDF 时间延迟
- ⚡ **高性能** - 零成本抽象，接近 C/C++ 的性能
- 🔒 **内存安全** - 编译时保证，无需 GC，无数据竞争
- 🔄 **EVM 兼容** - 支持以太坊智能合约
- 🌐 **P2P 网络** - 基于 libp2p 的去中心化通信
- 📦 **模块化设计** - 清晰的分层架构，易于扩展

---

## 🚀 快速开始

### 环境要求

| 组件 | 版本要求 |
|------|---------|
| **Rust** | 1.70+ (Edition 2021) |
| **protoc** | 3.x+ |
| **操作系统** | Linux 5.4+ / macOS |

### 安装

```bash
# 克隆项目
git clone https://github.com/your-org/rust-norn.git
cd rust-norn

# 安装 Rust（如果尚未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 安装 protoc（Ubuntu/Debian）
sudo apt update && sudo apt install -y protobuf-compiler

# macOS
brew install protobuf
```

### 构建

```bash
# 使用 Make（推荐）
make build

# 或使用 Cargo
cargo build --release

# 运行测试
make test
```

### 运行节点

```bash
# 生成节点密钥
./target/release/norn generate-key --out node.key

# 启动单节点
./target/release/norn --config config.toml

# 或使用 Docker Compose 启动多节点网络
docker-compose up -d
```

---

## 🏗️ 项目架构

```
┌─────────────────────────────────────────┐
│            bin/norn (CLI)                 │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│         norn-node (编排层)                │
├────────┬────────┬────────┬────────┬──────┤
│ RPC   │Network │ Core   │Crypto  │...   │
└────────┴────────┴────────┴────────┴──────┘
         │
┌────────▼─────────┐
│   norn-common     │
│   (共享类型)      │
└──────────────────┘
```

### 核心模块

| 模块 | 说明 |
|------|------|
| **norn-common** | 共享类型、trait、工具函数 |
| **norn-crypto** | 密码学原语（VRF、VDF、ECDSA） |
| **norn-storage** | SledDB 持久化存储 |
| **norn-core** | 区块链核心（共识、状态、EVM） |
| **norn-network** | libp2p P2P 网络层 |
| **norn-rpc** | gRPC + Ethereum JSON-RPC API |
| **norn-node** | 节点服务编排 |

---

## ⚙️ 配置示例

```toml
# 数据目录
data_dir = "node_data"

# RPC 服务
rpc_address = "127.0.0.1:50051"

# 共识配置
[core.consensus]
pub_key = "020000000000000000000000000000000000000000000000000000000000000001"
prv_key = "0000000000000000000000000000000000000000000000000000000000000001"

# 网络配置
[network]
listen_address = "/ip4/0.0.0.0/tcp/4001"
bootstrap_peers = []
mdns = false # V2 requires explicit bootstrap peers
```

---

## 🧪 测试

```bash
# 运行所有测试
make test

# 运行特定 crate 测试
cargo test -p norn-core

# TPS 性能测试
cargo build -p tps_test --release
./target/release/tps_test --rate 100 --duration 60
```

---

## 📚 文档

- **开发指南**: [CLAUDE.md](./CLAUDE.md) - 面向开发者的架构说明
- **技术文档**: [doc/](./doc/) - 中文技术文档
- **API 文档**: [docs/](./docs/) - API 参考

---

## 🤝 贡献

我们欢迎各种形式的贡献！

### 贡献流程

1. Fork 项目
2. 创建功能分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'feat: add AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 开启 Pull Request

### 代码规范

```bash
# 格式化代码
make fmt

# 运行 linter
make clippy

# 运行测试
make test
```

---

## 📊 性能指标

| 指标 | 值 |
|------|-----|
| **最大 TPS** | 1000+ |
| **出块间隔** | 1 秒（可配置） |
| **交易最终性** | ~2 个区块 |
| **内存占用** | ~200MB/节点 |

---

## 🔧 技术栈

- **语言**: Rust (Edition 2021)
- **异步运行时**: Tokio 1.36
- **P2P 框架**: libp2p 0.53
- **数据库**: SledDB 0.34
- **EVM**: revm v14
- **RPC**: Tonic 0.11 + jsonrpsee 0.20

---

## 📝 许可证

本项目采用 [MIT](LICENSE) 许可证。

---

## 📮 联系方式

- **Issues**: [提交问题](https://github.com/your-org/rust-norn/issues)
- **Discussions**: [参与讨论](https://github.com/your-org/rust-norn/discussions)

---

<div align="center">

**Made with ❤️ using Rust**

</div>
