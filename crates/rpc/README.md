# norn-rpc - 远程过程调用服务模块

## 1. 模块概述 (Overview)

**norn-rpc** 是 rust-norn 区块链系统的"网上银行门户"和"API 网关"，为外部应用程序提供访问区块链服务的标准接口。如果把整个区块链系统比作一家银行，norn-rpc 就是银行的"网上银行系统"和"API 接口平台"，让第三方应用（钱包、浏览器、交易系统）能够与区块链进行交互。

这个模块解决了**系统开放性**问题：即使区块链内部实现再强大，如果没有标准的外部接口，也无法被用户和开发者使用。norn-rpc 提供了基于 gRPC 的高性能、类型安全的 API 接口。

**核心业务价值**：
- 🌐 **开放接入能力**：为外部应用提供标准化的访问接口
- 🔒 **安全通信**：支持加密通信，保障数据传输安全
- ⚡ **高性能处理**：基于 gRPC 的高性能 RPC 框架
- 📝 **类型安全**：使用 Protocol Buffers 定义接口，确保类型安全

---

## 2. 核心功能列表 (Key Features)

- **gRPC 服务接口**
  - 基于 gRPC 框架的远程调用接口
  - 使用 Protocol Buffers 进行高效的数据序列化
  - 支持多种编程语言的客户端生成
  - 就像"标准化 API 文档"，让开发者轻松对接

- **查询类接口**
  - **查询区块**：根据区块哈希或高度查询区块信息
  - **查询交易**：根据交易哈希查询交易详情
  - **查询余额**：查询账户地址的余额和 nonce
  - **查询链高度**：获取当前区块链的最新高度
  - 就像"账户查询系统"，提供全面的信息查询

- **写入类接口**
  - **发送交易**：提交新的交易到区块链网络
  - 就像"转账汇款"，用户发起资金转移

- **流式订阅接口**
  - 订阅新区块事件，实时获取区块生成通知
  - 订阅新交易事件，实时监控交易状态
  - 就像"实时消息推送"，第一时间获取更新

- **鉴权与访问控制**
  - 支持基本的 API 访问控制
  - 可配置的速率限制，防止滥用
  - 就像"门禁系统"，控制访问权限

---

## 3. 业务流程/使用场景 (Use Cases)

### 场景一：用户查询账户余额

**场景描述**：用户通过钱包应用查询自己的账户余额。

**业务流程**：
1. **客户端发起请求**
   - 用户在钱包应用中输入账户地址
   - 钱包应用通过 gRPC 客户端连接到 norn-rpc 服务

2. **服务端处理**
   - norn-rpc 接收查询请求
   - 调用 norn-core 的接口查询账户状态
   - 获取账户余额、nonce 等信息

3. **返回结果**
   - norn-rpc 将查询结果序列化为 Protobuf 格式
   - 通过 gRPC 返回给客户端
   - 钱包应用解析结果并展示给用户

**业务价值**：
- 提供标准化的查询接口，方便钱包应用对接
- 支持多种编程语言的客户端（Go、Python、Java、JavaScript 等）
- 为用户提供实时、准确的账户信息

### 场景二：用户发起转账交易

**场景描述**：用户通过钱包应用向另一个账户转账。

**业务流程**：
1. **构建交易**
   - 钱包应用构建交易数据（发送方、接收方、金额、Gas 价格等）
   - 使用用户私钥对交易进行签名

2. **提交交易**
   - 钱包应用通过 `SendTransaction` RPC 接口提交交易
   - norn-rpc 接收交易数据，转发给 norn-core

3. **交易验证**
   - norn-core 的交易池验证交易合法性
   - 验证通过后加入内存池，等待打包

4. **返回交易哈希**
   - norn-rpc 将交易哈希返回给客户端
   - 客户端可以根据交易哈希追踪交易状态

5. **交易确认**
   - 交易被打包进区块后，客户端可以查询交易状态
   - 当区块获得足够确认数后，交易被标记为"已确认"

**业务价值**：
- 提供标准化的交易提交接口
- 支持多种钱包应用无缝对接
- 为用户提供清晰的交易反馈

### 场景三：监控新区块生成

**场景描述**：区块链浏览器需要实时监控新区块的生成。

**业务流程**：
1. **建立订阅**
   - 区块浏览器通过 gRPC 流式订阅连接到 norn-rpc
   - 订阅"新区块事件"流

2. **实时推送**
   - 当共识引擎生成新块后，norn-rpc 接收到事件通知
   - 通过流式连接实时推送区块信息给客户端
   - 区块浏览器实时更新展示

3. **数据展示**
   - 区块浏览器解析区块数据
   - 展示区块高度、交易列表、时间戳等信息

**业务透明价值**：
- 提供实时数据监控能力
- 支持区块链浏览器的数据展示
- 为开发者和用户提供透明的网络状态

---

## 4. 部署与配置要求 (Deployment & Configuration)

### 环境要求

- **网络端口**：需要开放 RPC 服务端口
  - 默认端口：50051
  - 支持配置多个监听地址

- **并发连接**：根据业务需求调整
  - 默认支持数百个并发连接
  - 可通过配置文件调整连接池大小

### 关键配置项

```toml
# RPC 服务地址
rpc_address = "127.0.0.1:50051"

# 服务配置
[rpc]
    # 最大并发连接数
    max_connections = 100

    # 请求超时时间（秒）
    timeout = 30

    # 是否启用流式订阅
    enable_subscription = true
```

**配置说明**：
- `rpc_address`：RPC 服务监听地址
  - `0.0.0.0` 表示监听所有网卡（仅限内网环境）
  - 公网部署建议绑定具体 IP 或使用反向代理

- `max_connections`：最大并发连接数
  - 根据服务器性能调整
  - 过大会占用过多资源，过小会限制并发

- `timeout`：请求超时时间
  - 查询类接口建议设置较短超时（5-10 秒）
  - 提交交易接口建议设置较长超时（30-60 秒）

### 部署架构建议

**单机部署**：
```
外部应用 → norn-rpc → norn-core → norn-storage
```

**生产环境部署**（推荐）：
```
外部应用
    ↓
Nginx 反向代理（负载均衡、HTTPS）
    ↓
norn-rpc (多实例)
    ↓
norn-core (多节点)
```

---

## 5. 接口与集成说明 (API & Integration)

### 主要 RPC 接口

#### 1. 查询类接口

**GetBlockByHash** - 根据哈希查询区块
```protobuf
message GetBlockByHashRequest {
    bytes hash = 1;
}

message GetBlockByHashResponse {
    Block block = 1;
}
```

**GetBlockNumber** - 查询当前链高度
```protobuf
message GetBlockNumberRequest {}

message GetBlockNumberResponse {
    int64 height = 1;
}
```

**GetBalance** - 查询账户余额
```protobuf
message GetBalanceRequest {
    bytes address = 1;
}

message GetBalanceResponse {
    uint64 balance = 1;
    uint64 nonce = 2;
}
```

#### 2. 写入类接口

**SendTransaction** - 发送交易
```protobuf
message SendTransactionRequest {
    bytes transaction = 1;
}

message SendTransactionResponse {
    bytes hash = 1;  // 交易哈希
}
```

#### 3. 订阅类接口（流式）

**SubscribeBlocks** - 订阅新区块
```protobuf
message SubscribeBlocksRequest {
    // 可选过滤条件
}

message BlockNotification {
    Block block = 1;
}
```

### 客户端集成示例

#### Rust 客户端
```rust
use norn_rpc::norn_client::NornClient;

async fn query_balance(client: &mut NornClient, address: &[u8]) -> Result<u64> {
    let req = GetBalanceRequest {
        address: address.to_vec(),
    };

    let resp = client.get_balance(req).await?;
    Ok(resp.balance)
}
```

#### Python 客户端（需要生成代码）
```python
import grpc
import norn_pb2  # 编译后的 Protobuf 模块

def get_balance(stub, address):
    req = norn_pb2.GetBalanceRequest(address=address)
    resp = stub.GetBalance(req)
    return resp.balance
```

### 接口安全说明

- **访问控制**：生产环境建议部署在反向代理后面，配置身份认证
- **速率限制**：建议配置请求速率限制，防止滥用
- **HTTPS 加密**：生产环境必须使用 HTTPS，保障通信安全

---

## 6. 常见问题 (FAQ)

### Q1：如何获取 Protobuf 定义文件？

**A**：
- Protobuf 定义文件位于 `norn-rpc/src/protos/` 目录
- 可以使用 `protoc` 编译器生成不同语言的客户端代码
- 参考各语言的 gRPC 教程进行集成

### Q2：RPC 服务支持 WebSocket 吗？

**A**：当前版本基于 gRPC 框架，使用 HTTP/2 通信。如果需要 WebSocket 支持：
- 可以在 norn-rpc 基础上封装 WebSocket 层
- 或使用其他 WebSocket 框架（如 `tokio-tungstenite`）

### Q3：如何提高 RPC 服务的性能？

**A**：
- **增加服务器资源**：提高 CPU、内存配置
- **调整连接池大小**：适当增加最大连接数
- **使用连接复用**：客户端使用长连接，避免频繁建连
- **启用压缩**：启用 gRPC 的消息压缩功能

### Q4：可以同时运行多个 RPC 服务实例吗？

**A**：可以，但需要注意：
- 不同实例需要监听不同端口
- 前端需要配置负载均衡器
- 确保多个实例连接到相同的 norn-core 节点（或节点集群）

### Q5：如何处理 RPC 服务的错误？

**A**：
- **客户端重试**：实现指数退避的重试机制
- **超时处理**：合理设置请求超时时间
- **错误日志**：记录错误日志，便于排查问题
- **监控告警**：配置错误率监控，及时发现问题

---

## 技术支持

如有疑问或需要技术支持，请参考项目主文档或联系技术支持团队。

**API 文档提示**：完整的 API 接口定义位于 `norn-rpc/src/protos/` 目录，可以使用 `protoc` 工具生成客户端代码。建议开发者熟悉 gRPC 和 Protocol Buffers 的使用。
