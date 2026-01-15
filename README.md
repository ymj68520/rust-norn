# rust-norn - 高性能区块链节点

[![Rust](https://img.shields.io/badge/Rust-Edition%202021-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build](https://img.shields.io/badge/Build-Passing-green.svg)]()

> 一个用 Rust 实现的高性能区块链节点，采用创新的 PoVF (Proof of Verifiable Function) 共识机制。

---

## 📋 目录

- [项目简介](#项目简介)
- [核心特性](#核心特性)
- [快速开始](#快速开始)
- [项目架构](#项目架构)
- [技术栈](#技术栈)
- [核心组件](#核心组件)
- [配置与部署](#配置与部署)
- [二次开发](#二次开发)
- [常见问题](#常见问题)
- [性能指标](#性能指标)
- [贡献指南](#贡献指南)

---

## 🎯 项目简介

**rust-norn** 是一个用 Rust 实现的高性能区块链节点，复制了 go-norn 协议，采用了创新的 **PoVF (Proof of Verifiable Function，可验证函数证明)** 共识机制。该项目展示了 Rust 在系统级编程中的优势，包括类型安全、内存安全和高效的并发处理。

### 为什么选择 Rust？

| 特性 | Rust | Go |
|------|------|-----|
| **内存安全** | 编译时保证，无 GC | 运行时 GC |
| **性能** | 零成本抽象，接近 C | 良好，但低于 Rust |
| **并发** | 编译时数据竞争检查 | Goroutine + Channel |
| **类型系统** | 强类型，泛型强大 | 接口较简单 |
| **适用场景** | 系统级、高安全要求 | 快速开发、微服务 |

### 技术亮点

- ✅ **类型安全**: Rust 的类型系统在编译时捕获大量错误
- ✅ **内存安全**: 无需垃圾回收器，无数据竞争
- ✅ **高性能**: 零成本抽象，编译优化后性能接近 C/C++
- ✅ **并发性**: 基于 Tokio 的异步编程模型
- ✅ **可维护性**: 模块化设计，清晰的职责分离

---

## 🚀 核心特性

### 1. PoVF 共识机制

**PoVF (Proof of Verifiable Function)** 是一种创新的共识机制，结合了两种密码学原语：

- **VRF (Verifiable Random Function)**: 用于随机领导者选举
- **VDF (Verifiable Delay Function)**: 确保最小时间延迟

```rust
pub struct PoVFEngine {
    // VRF 密钥对（用于领导者选举）
    vrf_keypair: VRFKeyPair,

    // VDF 计算器（确保时间延迟）
    vdf_calculator: Arc<dyn VDFCalculator>,

    // 验证者权益
    validator_stakes: HashMap<PublicKey, u64>,
}
```

**优势**:
- 🎲 随机领导者选举，防止中心化
- ⏱️ 时间延迟保证，防止短程攻击
- 💡 低能耗，不需要大量计算
- ⚡ 快速确认，顺序计算加速最终性

### 2. 模块化架构

项目采用严格的分层架构，包含 8+ 个独立的 crate：

```
rust-norn/
├── bin/norn/          # CLI 可执行文件
├── crates/
│   ├── common/        # 公共基础库
│   ├── crypto/        # 密码学原语
│   ├── storage/       # 存储层
│   ├── core/          # 区块链核心
│   ├── network/       # P2P 网络
│   ├── rpc/           # gRPC API
│   └── node/          # 节点编排
└── tps_test/          # 性能测试工具
```

### 3. 完整的 P2P 网络

基于 **libp2p** 实现，支持：
- 🔍 **mDNS 发现**: 局域网自动发现
- 📢 **Gossipsub**: 消息传播协议
- 🗺️ **Kademlia DHT**: 分布式哈希表
- 🔐 **Noise 加密**: 加密通信
- 🔄 **Yamux 多路复用**: 流复用

### 4. 高性能存储

使用 **SledDB** 作为嵌入式数据库：
- ✅ 纯 Rust 实现，无 FFI 开销
- ✅ 嵌入式，单文件数据库
- ✅ 支持 ACID 事务
- ✅ 零配置，开箱即用

### 5. 完善的工具链

- 🧪 **TPS 测试工具**: 内置性能测试
- 🐳 **Docker 支持**: 开箱即用的多节点部署
- 📊 **监控指标**: Prometheus 集成
- 🔧 **gRPC API**: 完整的外部 API

---

## 🏁 快速开始

### 环境要求

| 组件 | 最低版本 | 推荐版本 |
|------|---------|---------|
| **Rust** | 1.70+ | 最新 Stable |
| **protoc** | 3.x | 最新版 |
| **操作系统** | Linux 5.4+ / macOS | Linux 6.x |

### 安装 Rust

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 配置环境
source $HOME/.cargo/env

# 安装开发组件
rustup component add rustfmt clippy rust-src

# 验证安装
rustc --version  # 应显示 1.70+
cargo --version
```

### 安装 protoc

```bash
# Linux (Debian/Ubuntu)
sudo apt update
sudo apt install -y protobuf-compiler

# macOS
brew install protobuf

# 验证
protoc --version
```

### 构建项目

```bash
# 克隆项目
git clone <repository-url>
cd rust-norn

# 获取依赖
cargo fetch

# 编译 Release 版本
cargo build --release

# 验证编译
./target/release/norn --help
```

### 运行单节点

```bash
# 创建配置文件
cat > config.toml << EOF
data_dir = "node_data"
rpc_address = "127.0.0.1:50051"

[core.consensus]
pub_key = "020000000000000000000000000000000000000000000000000000000000000001"
prv_key = "0000000000000000000000000000000000000000000000000000000000000001"

[network]
listen_address = "/ip4/0.0.0.0/tcp/4001"
bootstrap_peers = []
mdns = true
EOF

# 启动节点
./target/release/norn --config config.toml
```

### 运行多节点网络

#### 方法 1: 手动启动

```bash
# 终端 1
./target/release/norn --config node1_config.toml

# 终端 2
./target/release/norn --config node2_config.toml

# 终端 3
./target/release/norn --config node3_config.toml
```

#### 方法 2: Docker Compose

```bash
# 启动所有节点
docker-compose up -d

# 查看日志
docker-compose logs -f

# 停止所有节点
docker-compose down
```

### 运行 TPS 测试

```bash
# 构建 TPS 测试工具
cargo build -p tps_test --release

# 运行默认测试 (100 TPS, 60秒)
./target/release/tps_test

# 自定义测试
./target/release/tps_test --rate 500 --duration 120

# 最大 TPS 基准测试
./tps_test/max_tps_benchmark.sh
```

---

## 🏗️ 项目架构

### 系统分层架构

```
┌─────────────────────────────────────────────┐
│          bin/norn (CLI 入口)                 │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│       norn-node (节点编排层)                 │
│  - 服务协调                                   │
│  - 生命周期管理                               │
│  - 配置管理                                   │
└─────┬───────┬───────┬───────┬───────────────┘
      │       │       │       │
┌─────▼────┐ ┌▼──────┐ │ ┌────▼──────┐
│norn-rpc │ │norn-  │ │ │norn-     │
│(API层)  │ │network│ │ │core      │
└─────────┘ │(P2P)  │ │ │(区块链)  │
            └───────┘ │ └────┬─────┘
                      │       │
              ┌───────▼───────▼──────┐
              │   norn-common        │
              │   (共享类型和工具)    │
              └──────────────────────┘
                      │
      ┌───────────────┼───────────────┐
      │               │               │
┌─────▼────┐   ┌─────▼─────┐   ┌────▼────┐
│norn-crypto│  │norn-      │  │norn-    │
│(密码学)   │  │storage    │  │...      │
└──────────┘   └───────────┘   └─────────┘
```

### 数据流: 交易处理

```
客户端提交交易
    │
    ▼
[RPC Server] 接收交易
    │
    ▼
[TxPool] 验证交易
    │
    ├─→ 验证签名
    ├─→ 检查 nonce
    ├─→ 检查余额
    ├─→ 检查重复
    │
    ▼
[加入交易池] 等待打包
    │
    ▼
[BlockProducer] 选取交易
    │
    ▼
[Executor] 执行交易
    │
    ├─→ 扣除 gas
    ├─→ 执行转账
    ├─→ 更新状态
    │
    ▼
[生成区块] 计算梅克尔根和状态根
    │
    ▼
[广播到网络] Gossipsub 传播
```

### 数据流: 区块同步

```
新节点启动
    │
    ▼
[请求最新高度] → 对等节点
    │
    ▼
[比较本地高度]
    │
    ├─→ 本地 < 远程 → 需要同步
    │       │
    │       ▼
    │   [批量请求区块]
    │       │
    │       ▼
    │   [验证并执行]
    │       │
    │       ▼
    │   [更新本地链]
    │
    └─→ 本地 = 远程 → 已同步
```

### 设计模式

项目使用了多种设计模式：

1. **分层架构模式**: 清晰的职责分离
2. **依赖注入模式**: 易于测试和解耦
3. **服务定位器模式**: NodeService 作为中央协调器
4. **策略模式**: 共识引擎、存储后端可替换
5. **观察者模式**: 网络事件处理
6. **工厂模式**: 区块链初始化
7. **构建器模式**: 配置构建

---

## 🔧 技术栈

### 核心技术栈

| 类别 | 技术 | 版本 | 用途 |
|------|------|------|------|
| **语言** | Rust | Edition 2021 | 主要开发语言 |
| **构建工具** | Cargo | 内置 | 包管理和构建 |
| **异步运行时** | Tokio | 1.36 | 异步 I/O、任务调度 |
| **P2P 框架** | libp2p | 0.53 | P2P 网络栈 |
| **数据库** | SledDB | 0.34 | 持久化 KV 存储 |
| **缓存** | Moka | 0.12 | 内存缓存 |
| **gRPC 框架** | Tonic | 0.11 | RPC 服务端/客户端 |
| **Protobuf** | Prost | 0.12 | Protobuf 代码生成 |
| **日志框架** | Tracing | 0.1 | 结构化日志 |

### 密码学库

| 功能 | 库 | 原因 |
|------|-----|------|
| **ECDSA** | k256 | secp256k1，高效 |
| **VRF** | p256 + schnorrkel | NIST P-256 |
| **哈希** | sha2 | SHA-256 |
| **随机数** | rand | 安全随机 |

### 技术选型原则

1. **Rust 原生优先**: 减少 FFI 开销
2. **生态成熟度**: 选择广泛使用的库
3. **性能优先**: 选择零成本抽象
4. **类型安全**: 利用 Rust 类型系统
5. **可维护性**: 选择文档完善的库

---

## 📦 核心组件

### 1. norn-common - 公共基础库

**职责**: 提供项目中所有其他 crate 共享的数据结构、类型定义、trait 抽象和工具函数。

**核心类型**:

```rust
// 哈希 (256位)
pub struct Hash(pub [u8; 32]);

// 地址 (160位)
pub struct Address(pub [u8; 20]);

// 公钥 (33字节压缩公钥)
pub struct PublicKey(pub [u8; 33]);

// 交易
pub struct Transaction {
    pub hash: Hash,
    pub from: Address,
    pub to: Address,
    pub value: u64,
    pub nonce: u64,
    pub signature: Vec<u8>,
    pub timestamp: i64,
}

// 区块
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}
```

**Trait 抽象**:

```rust
#[async_trait]
pub trait DBInterface: Send + Sync {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    async fn put(&self, key: &[u8], value: Vec<u8>) -> Result<()>;
    async fn put_batch(&self, items: Vec<(Vec<u8>, Vec<u8>)>) -> Result<()>;
    async fn delete(&self, key: &[u8]) -> Result<()>;
}
```

### 2. norn-core - 区块链核心

**职责**: 实现区块链的核心业务逻辑。

**主要模块**:

- **blockchain.rs**: 区块链管理
- **txpool.rs**: 交易池
- **consensus/**: 共识机制 (PoVF)
- **execution/**: 交易执行
- **state/**: 状态管理
- **merkle.rs**: Merkle 树
- **fee.rs**: 费用计算
- **wallet.rs**: 钱包实现

**核心功能**:

```rust
pub struct Blockchain {
    db: Arc<dyn DBInterface>,
    block_cache: Cache<Hash, Block>,
    tx_cache: Cache<Hash, Transaction>,
    block_height_map: Cache<i64, Hash>,
    pub latest_block: Arc<RwLock<Block>>,
}

impl Blockchain {
    // 添加区块
    pub async fn add_block(&self, block: Block) -> Result<()>;

    // 查询区块
    pub async fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>>;

    // 查询高度
    pub async fn get_block_number(&self) -> Result<i64>;
}
```

**交易池**:

```rust
pub struct TxPool {
    transactions: Arc<RwLock<HashMap<Hash, Transaction>>>,
    by_sender: Arc<RwLock<HashMap<Address, Vec<Hash>>>>,
    nonces: Arc<RwLock<HashMap<Address, u64>>>,
    config: TxPoolConfig,
}

impl TxPool {
    // 添加交易
    pub async fn add_transaction(&self, tx: Transaction) -> Result<()>;

    // 获取待打包交易
    pub async fn get_transactions_for_block(&self) -> Result<Vec<Transaction>>;
}
```

### 3. norn-crypto - 密码学原语

**职责**: 实现密码学功能。

**主要功能**:

```rust
// VRF (可验证随机函数)
pub struct VRFKeyPair {
    public_key: p256::PublicKey,
    secret_key: p256::SecretKey,
}

impl VRFKeyPair {
    pub fn evaluate(&self, message: &[u8]) -> VRFOutput;
    pub fn verify(&self, message: &[u8], output: &VRFOutput) -> bool;
}

// VDF (可验证延迟函数)
pub trait VDFCalculator: Send + Sync {
    fn compute(&self, input: &[u8]) -> Vec<u8>;
    fn verify(&self, input: &[u8], output: &[u8]) -> bool;
}

// ECDSA 签名
pub fn sign_transaction(tx: &Transaction, key: &SigningKey) -> Signature;
pub fn verify_signature(tx: &Transaction, sig: &Signature, key: &VerifyingKey) -> bool;
```

### 4. norn-network - P2P 网络层

**职责**: 实现 P2P 网络通信。

**核心功能**:

```rust
pub struct NetworkService {
    swarm: Swarm<NetworkBehaviour>,
    event_rx: mpsc::Receiver<NetworkEvent>,
}

pub enum NetworkEvent {
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
    BlockReceived(Vec<u8>),
    TransactionReceived(Vec<u8>),
    ConsensusMessageReceived(Vec<u8>),
}

#[derive(NetworkBehaviour)]
pub struct NetworkBehaviour {
    gossipsub: Gossipsub,
    kademlia: Kademlia<MemoryStore>,
    mdns: TokioMdns,
    identify: Identify,
}
```

**特性**:
- ✅ mDNS 发现
- ✅ Gossipsub 消息传播
- ✅ Kademlia DHT
- ✅ Noise 加密
- ✅ Yamux 多路复用

### 5. norn-node - 节点编排层

**职责**: 协调所有服务，管理节点生命周期。

**核心结构**:

```rust
pub struct NornNode {
    blockchain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    network: Arc<NetworkService>,
    consensus: Arc<PoVFEngine>,
    block_producer: Arc<BlockProducer>,
    peer_manager: Arc<PeerManager>,
    syncer: Arc<BlockSyncer>,
    tx_handler: Arc<TxHandler>,
}
```

**启动流程**:

```rust
impl NornNode {
    pub async fn new(config: NodeConfig, keypair: Keypair) -> Result<Self> {
        // 1. 初始化数据库
        let db = Arc::new(SledDB::new(&config.data_dir)?);

        // 2. 初始化区块链
        let blockchain = Blockchain::new_with_fixed_genesis(db.clone()).await;

        // 3. 初始化交易池
        let tx_pool = Arc::new(TxPool::new());

        // 4. 初始化共识引擎
        let consensus = Arc::new(PoVFEngine::new(/* */));

        // 5. 启动网络服务
        let network = Arc::new(NetworkService::start(config.network, keypair).await?);

        // ...
    }

    pub async fn start(mut self) -> Result<()> {
        // 启动所有服务
        // 处理事件循环
    }
}
```

---

## ⚙️ 配置与部署

### 配置文件格式

```toml
# ============================================
# Norn 区块链节点配置
# ============================================

# 数据目录
data_dir = "/var/lib/norn"

# RPC 服务地址
rpc_address = "127.0.0.1:50051"

# ============================================
# 区块链核心配置
# ============================================
[core]
    # 共识机制配置
    [core.consensus]
    # 验证者公钥（十六进制格式）
    pub_key = "020000000000000000000000000000000000000000000000000000000000000001"

    # 验证者私钥（十六进制格式）
    # 警告：生产环境中应从安全存储加载
    prv_key = "0000000000000000000000000000000000000000000000000000000000000001"

# ============================================
# 网络配置
# ============================================
[network]
    # P2P 网络监听地址
    listen_address = "/ip4/0.0.0.0/tcp/4001"

    # 引导节点列表
    bootstrap_peers = [
        # "/ip4/192.168.1.100/tcp/4001/p2p/12D3KooW...",
    ]

    # 启用 mDNS 本地发现
    mdns = true
```

### 部署方案

#### 方案 1: 单机部署

**适用场景**: 开发测试

```bash
# 构建二进制文件
cargo build --release

# 启动节点
./target/release/norn --config config.toml
```

#### 方案 2: 分布式部署

**适用场景**: 生产环境

**网络拓扑**:

```
        ┌─────────────┐
        │   Node 1    │
        │  (Bootstrap) │
        │  192.168.1.10│
        └──────┬──────┘
               │
   ┌───────────┼───────────┐
   │           │           │
┌──▼──┐     ┌──▼──┐     ┌──▼──┐
│Node2│     │Node3│     │Node4│
│.11  │     │.12  │     │.13  │
└─────┘     └─────┘     └─────┘
```

**配置要点**:

**Node 1 (Bootstrap)**:
```toml
data_dir = "/var/lib/norn/node1"
rpc_address = "0.0.0.0:50051"
[network]
listen_address = "/ip4/0.0.0.0/tcp/4001"
bootstrap_peers = []
mdns = false
```

**Node 2, 3, 4**:
```toml
data_dir = "/var/lib/norn/node2"
rpc_address = "0.0.0.0:50052"
[network]
listen_address = "/ip4/0.0.0.0/tcp/4002"
bootstrap_peers = [
    "/ip4/192.168.1.10/tcp/4001/p2p/<NODE1_PEER_ID>",
]
mdns = false
```

#### 方案 3: Docker 部署

**Dockerfile**:

```dockerfile
FROM rust:1.70 as builder
WORKDIR /usr/src/norn
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates
RUN rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/src/norn/target/release/norn /app/
COPY --from=builder /usr/src/norn/config.toml /etc/norn/
EXPOSE 50051 4001
CMD ["./norn", "--config", "/etc/norn/config.toml"]
```

**docker-compose.yml**:

```yaml
version: '3.8'

services:
  norn-node1:
    build: .
    container_name: norn-node1
    ports:
      - "4001:4001"
      - "50051:50051"
    volumes:
      - ./configs/node1.toml:/etc/norn/config.toml:ro
      - node1_data:/data
    networks:
      - norn-network

  norn-node2:
    build: .
    container_name: norn-node2
    ports:
      - "4002:4002"
      - "50052:50051"
    volumes:
      - ./configs/node2.toml:/etc/norn/config.toml:ro
      - node2_data:/data
    networks:
      - norn-network
    depends_on:
      - norn-node1

volumes:
  node1_data:
  node2_data:

networks:
  norn-network:
    driver: bridge
```

**启动**:

```bash
docker-compose up -d
```

### 监控和日志

#### 日志配置

```bash
# 设置全局日志级别
export RUST_LOG=info

# 设置特定模块
export RUST_LOG=norn_core=debug,norn_network=info

# 运行
./target/release/norn --config config.toml
```

#### Prometheus 监控

```rust
use prometheus::{Counter, Histogram, IntGauge, Registry};

pub struct BlockchainMetrics {
    pub block_height: IntGauge,
    pub block_production_time: Histogram,
    pub transactions_total: Counter,
    pub transactions_failed: Counter,
}

impl BlockchainMetrics {
    pub fn new() -> Result<Self> {
        let block_height = register_int_gauge!(
            "norn_block_height",
            "Current blockchain height"
        )?;

        let transactions_total = register_counter!(
            "norn_transactions_total",
            "Total number of transactions processed"
        )?;

        // ...
    }
}
```

---

## 🛠️ 二次开发

### 修改策略

#### 推荐的修改位置

1. **配置扩展** (最安全)
   - 位置: `crates/common/src/types.rs` 或 `crates/node/src/config.rs`
   - 示例: 添加新的配置项

2. **插件开发**
   - 示例: 自定义共识机制
   - 使用 Trait 实现灵活扩展

3. **子类覆盖** (使用 Trait)
   - 示例: 自定义区块验证器

#### 应避免的修改

❌ **避免修改**:
- `norn-common/src/types.rs` 中的核心类型定义
- `norn-common/src/traits.rs` 中的 trait 签名
- `norn-common/src/genesis.rs` 中的创世区块

⚠️ **谨慎修改**:
- 数据库格式
- 网络协议
- RPC API

### 定制化示例

#### 示例 1: 添加新功能 - 智能合约支持

```rust
// crates/common/src/types.rs
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Transaction {
    Transfer(TransferTx),
    Contract(ContractTx),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractTx {
    pub from: Address,
    pub contract: Address,
    pub value: u64,
    pub data: Vec<u8>,
    pub nonce: u64,
    pub signature: Vec<u8>,
}
```

#### 示例 2: 自定义共识机制

```rust
#[async_trait]
pub trait ConsensusEngine: Send + Sync {
    async fn elect_leader(&self, round: u64) -> Result<PublicKey>;
    async fn verify_block(&self, block: &Block) -> Result<bool>;
}

pub struct MyCustomConsensus {
    validators: Vec<PublicKey>,
    current_index: Arc<AtomicUsize>,
}

#[async_trait]
impl ConsensusEngine for MyCustomConsensus {
    async fn elect_leader(&self, round: u64) -> Result<PublicKey> {
        let index = (round as usize) % self.validators.len();
        Ok(self.validators[index])
    }
}
```

#### 示例 3: 添加新的存储后端

```rust
use norn_common::traits::DBInterface;

pub struct RedisDB {
    client: redis::Client,
}

#[async_trait]
impl DBInterface for RedisDB {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut conn = self.client.get_async_connection().await?;
        let value: Option<Vec<u8>> = conn.get(key).await?;
        Ok(value)
    }

    async fn put(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
        let mut conn = self.client.get_async_connection().await?;
        conn.set(key, value).await?;
        Ok(())
    }
}
```

### 调试技巧

```bash
# 设置日志级别
export RUST_LOG=debug
./target/release/norn --config config.toml

# 只显示特定模块
export RUST_LOG=norn_core=debug,norn_network=info

# 性能分析
cargo install flamegraph
cargo flamegraph --bin norn -- --config config.toml

# 内存分析
valgrind --leak-check=full ./target/release/norn --config config.toml

# 网络抓包
tcpdump -i any -n 'tcp port 4001' -w norn.pcap
```

### 测试策略

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_blockchain_add_block() {
        let db = Arc::new(MockDB::new());
        let chain = Blockchain::new_with_fixed_genesis(db).await;

        let block = create_test_block(1);
        let result = chain.add_block(block).await;

        assert!(result.is_ok());
        assert_eq!(chain.latest_block.read().await.header.height, 1);
    }
}
```

---

## ❓ 常见问题

### Q1: 节点启动失败，提示"Database error"

**原因**:
- 数据目录权限不足
- 数据目录已被其他进程锁定
- 磁盘空间不足

**解决方案**:

```bash
# 检查目录权限
ls -la node_data/

# 修改权限
chmod 755 node_data/
chown $USER:$USER node_data/

# 检查磁盘空间
df -h

# 如果数据库损坏，重建
rm -rf node_data/
./target/release/norn --config config.toml
```

### Q2: 节点无法发现对等节点

**原因**:
- mDNS 在当前网络不可用
- bootstrap_peers 配置错误
- 防火墙阻止 P2P 端口

**解决方案**:

```bash
# 检查网络配置
ip addr show

# 检查防火墙
sudo iptables -L -n | grep 4001

# 如果使用 Docker，禁用 mDNS
mdns = false
bootstrap_peers = [
    "/ip4/192.168.1.100/tcp/4001/p2p/<PEER_ID>",
]
```

### Q3: 交易提交成功但未被打包

**原因**:
- 交易 nonce 不正确
- 交易池已满
- Gas 价格太低
- 节点不是验证者（不出块）

**解决方案**:

```bash
# 检查当前 nonce
# (需要 RPC 客户端调用 GetNonce)

# 检查 Gas 价格
# 确保交易的 gas_price 足够高

# 确认节点在出块
grep "Produced block" node_data/logs/norn.log
```

### Q4: 性能问题：TPS 低

**解决方案**:

**1. 调整出块间隔**
```toml
block_interval = 1  # 1 秒
```

**2. 优化交易执行**
```rust
// 批量执行交易
pub async fn execute_transactions(
    &self,
    transactions: Vec<Transaction>
) -> Result<Vec<Receipt>> {
    let results: Vec<_> = transactions.par_iter()
        .map(|tx| self.execute_transaction(tx))
        .collect();
    results
}
```

**3. 使用 SSD**
```bash
data_dir = "/ssd/norn_data"
```

### Q5: 内存占用过高

**解决方案**:

```bash
# 减小缓存
pub struct Blockchain {
    block_cache: Cache<Hash, Block>,      // 减小容量
    tx_cache: Cache<Hash, Transaction>,    // 减小容量
}

# 使用更紧凑的数据结构
use hashbrown::HashMap;

# 定期清理
tokio::spawn(async move {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        self.cleanup_old_data().await;
    }
});
```

---

## 📊 性能指标

### 关键指标

| 指标 | 值 |
|------|-----|
| **最大 TPS** | 1000+ |
| **出块间隔** | 1 秒（可配置） |
| **交易最终性** | ~2 个区块 |
| **内存占用** | ~200MB/节点 |
| **支持节点数** | 3+ (可扩展) |
| **网络协议** | libp2p (TCP + mDNS) |
| **共识机制** | PoVF (VRF + VDF) |

### TPS 测试结果

```
========================================
TPS Test Results
========================================
Duration: 60 seconds
Target TPS: 100
Submitted: 6000 transactions
Confirmed: 5987 transactions
Actual TPS: 99.78
Success Rate: 99.78%
========================================
Block Production Time:
  Min: 0.8s
  Max: 1.2s
  Avg: 1.0s
========================================
```

### 性能优化建议

1. **缓存优化**
   - 多级缓存
   - 智能预取

2. **批量处理**
   - 批量写入数据库
   - 批量验证交易

3. **并发优化**
   - 使用 Rayon 并行迭代
   - Tokio 并发任务

---

## 🤝 贡献指南

### 代码贡献流程

1. **Fork 并克隆**
   ```bash
   git clone https://github.com/YOUR_USERNAME/rust-norn.git
   cd rust-norn
   ```

2. **创建功能分支**
   ```bash
   git checkout -b feature/your-feature-name
   ```

3. **开发和测试**
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace
   ```

4. **提交更改**
   ```bash
   git add .
   git commit -m "feat: add your feature description"
   ```

5. **推送和创建 PR**
   ```bash
   git push origin feature/your-feature-name
   ```

### 提交消息格式

```
<type>(<scope>): <subject>

<body>

<footer>
```

**类型**:
- `feat`: 新功能
- `fix`: Bug 修复
- `docs`: 文档更新
- `style`: 代码格式
- `refactor`: 重构
- `test`: 添加测试
- `chore`: 构建/工具变更

### 最佳实践

1. **代码风格**
   - 使用有意义的变量名
   - 添加必要的注释
   - 遵循 Rust 命名规范

2. **错误处理**
   - 返回具体的错误
   - 避免使用 unwrap
   - 提供上下文信息

3. **文档注释**
   - 为公共 API 添加文档
   - 包含使用示例
   - 说明可能的错误

---

## 📚 资源链接

### 项目资源

- **项目地址**: `/home/ymj68520/projects/Rust/rust-norn`
- **文档目录**: `/home/ymj68520/projects/Rust/rust-norn/doc/`
- **源代码**: `/home/ymj68520/projects/Rust/rust-norn/crates/`
- **测试工具**: `/home/ymj68520/projects/Rust/rust-norn/tps_test/`

### 在线资源

- **Rust 官方文档**: https://doc.rust-lang.org/
- **Tokio 文档**: https://tokio.rs/
- **libp2p 文档**: https://docs.libp2p.io/
- **Cargo 书籍**: https://doc.rust-lang.org/cargo/

### 相关项目

- **Substrate** (Polkadot): https://github.com/paritytech/substrate
- **OpenEthereum**: https://github.com/openethereum/openethereum
- **Rust Ethereum**: https://github.com/rust-ethereum

### 学习资源

**书籍**:
- "The Rust Programming Language"
- "Programming Blockchain"
- "Mastering Blockchain"

**课程**:
- Coursera: "Blockchain Basics"
- Udemy: "Ethereum and Solidity"

**论文**:
- Bitcoin: https://bitcoin.org/bitcoin.pdf
- Ethereum: https://ethereum.github.io/yellowpaper/paper.pdf

---

## 📝 许可证

本项目采用 MIT 许可证。详见 [LICENSE](LICENSE) 文件。

---

## 📮 联系方式

- **Issues**: https://github.com/your-repo/rust-norn/issues
- **Discussions**: https://github.com/your-repo/rust-norn/discussions

---

## 🌟 致谢

感谢所有为本项目做出贡献的开发者！

特别感谢以下项目：
- Tokio 异步运行时
- libp2p 网络框架
- SledDB 嵌入式数据库
- Rust 社区

---

**Made with ❤️ using Rust**

---

*最后更新: 2025-01-14*
