# API 文档

**版本**: 1.0.0
**最后更新**: 2026-02-01

---

## 目录

- [概述](#概述)
- [RPC 接口](#rpc-接口)
- [数据类型](#数据类型)
- [使用示例](#使用示例)

---

## 概述

Norn 区块链提供两种 API 接口：

1. **gRPC API** - 高性能的底层 RPC 接口
2. **Ethereum JSON-RPC API** - 以太坊兼容的 JSON-RPC 接口

两种接口都支持完整的区块链操作，包括查询区块、发送交易、获取状态等。

---

## RPC 接口

### 1. gRPC API

gRPC API 基于 Protocol Buffers 定义，提供类型安全的高性能接口。

#### 服务端点

| 方法 | 描述 | 请求类型 | 响应类型 |
|------|------|----------|----------|
| `GetBlockByHash` | 根据区块哈希获取区块 | `BlockHashRequest` | `BlockResponse` |
| | GetBlockByNumber | 根据区块高度获取区块 | `BlockNumberRequest` | `BlockResponse` |
| | GetBlockNumber | 获取最新区块高度 | `Empty` | `BlockNumberResponse` |
| | SendTransaction | 发送交易到交易池 | `TransactionRequest` | `SendTransactionResponse` |
| | GetBalance | 查询账户余额 | `BalanceRequest` | `BalanceResponse` |
| | GetTransactionCount | 获取账户 nonce | `NonceRequest` | `NonceResponse` |
| | Call | 智能合约调用（只读） | `CallRequest` | `CallResponse` |
| | GetChainID | 获取链 ID | `Empty` | `ChainIdResponse` |

#### 示例

**获取区块信息**

```bash
# 使用 grpcurl
grpcurl -plaintext localhost:50051 norn.blockchain.Block/GetBlockByNumber
```

```python
import grpc
import norn_pb2

# 连接服务端
channel = grpc.insecure_channel('localhost:50051')
stub = norn_pb2.BlockchainStub(channel)

# 获取最新高度
request = norn_pb2.BlockNumberRequest()
response = stub.GetBlockNumber(request)
print(f"Latest block: {response.block_number}")
```

### 2. Ethereum JSON-RPC API

兼容以太坊的 JSON-RPC 接口，支持 MetaMask、Remix 等工具。

#### 支持的方法

| 方法 | 描述 |
|------|------|
| `eth_getBlockByNumber` | 根据高度获取区块 |
| `eth_getBlockByHash` | 根据哈希获取区块 |
| `eth_getBlockTransactionCountByHash` | 获取区块交易数量 |
| `eth_getBlockByHash` | 获取区块信息 |
| `eth_getTransactionByHash` | 根据哈希获取交易 |
| `eth_getTransactionReceipt` | 获取交易收据 |
| `eth_getBalance` | 查询余额 |
| `eth_getTransactionCount` | 获取 nonce |
| `eth_call` | 智能合约调用（只读） |
| `eth_estimateGas` | 估算 gas 消耗 |
| `eth_chainId` | 获取链 ID |
| `eth_blockNumber` | 获取最新区块号 |
| `eth_sendRawTransaction` | 发送原始交易 |
| `eth_getCode` | 获取合约代码 |
| `web3_clientVersion` | 获取客户端版本 |

#### 示例

**使用 curl**

```bash
# 获取最新区块号
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# 获取账户余额
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x742d35Cc6634C0532925a3b844Bc4c1f8e07C94", "latest"],"id":1}'

# 发送交易
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_sendRawTransaction",
    "params":["0x..."],
    "id":1
  }'
```

**使用 Web3.js**

```javascript
const Web3 = require('web3');
const web3 = new Web3('http://localhost:50051');

// 获取区块号
web3.eth.getBlockNumber().then(console.log);

// 获取余额
web3.eth.getBalance("0x742d35Cc6634C0532925a3b844Bc4c1f8e07C94", "latest")
  .then(balance => console.log(web3.utils.fromWei(balance, "ether")));
```

**使用 Ethers.js**

```javascript
const { ethers } = require('ethers');

const provider = new ethers.JsonRpcProvider('http://localhost:50051');
const signer = new ethers.Wallet.createRandom();

// 发送交易
const tx = await signer.sendTransaction({
  to: "0x742d35Cc6634C0532925a3b844Bc4c1f8e07C94",
  value: ethers.parseEther("1.0")
});
await tx.wait();
```

---

## 数据类型

### 核心类型

#### Block（区块）
```json
{
  "header": {
    "timestamp": 1234567890,
    "prev_block_hash": "0x...",
    "block_hash": "0x...",
    "merkle_root": "0x...",
    "state_root": "0x...",
    "height": 1000,
    "public_key": "0x...",
    "params": "0x...",
    "gas_limit": 30000000,
    "base_fee": 1000000000
  },
  "transactions": [...]
}
```

#### Transaction（交易）
```json
{
  "hash": "0x...",
  "address": "0x...",
  "receiver": "0x...",
  "gas": 21000,
  "nonce": 0,
  "data": "0x...",
  "value": "1000000000000000000",
  "timestamp": 1234567890,
  "signature": "0x...",
  "tx_type": 0,
  "chain_id": 31337,
  "max_fee_per_gas": 5000000000,
  "max_priority_fee_per_gas": 2000000000,
  "access_list": null
}
```

#### Receipt（交易收据）
```json
{
  "transaction_hash": "0x...",
  "transaction_index": 0,
  "block_hash": "0x...",
  "block_number": 1000,
  "from": "0x...",
  "to": "0x...",
  "cumulative_gas_used": 21000,
  "logs": [...],
  "logs_bloom": "0x...",
  "status": "0x1",
  "contract_address": "0x..."
}
```

---

## 使用示例

### 1. 查询区块

#### gRPC

```python
import grpc
import norn_pb2

channel = grpc.insecure_channel('localhost:50051')
stub = norn_pb2.BlockchainStub(channel)

# 根据高度获取区块
request = norn_pb2.BlockNumberRequest()
request.height = 1000
response = stub.GetBlockByNumber(request)

print(f"区块高度: {response.block.header.height}")
print(f"交易数量: {len(response.block.transactions)}")
```

#### JSON-RPC

```bash
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":[0x3E8, false],"id":1}'
```

### 2. 发送交易

#### 构建交易

```javascript
const { ethers } = require('ethers');

const provider = new ethers.JsonRpcProvider('http://localhost:50051');
const wallet = new ethers.Wallet(PRIVATE_KEY, provider);

// 发送 ETH 转账
const tx = await wallet.sendTransaction({
  to: "0x742d35Cc6634C0532925a3b844Bc4c1f8e07C94",
  value: ethers.parseEther("1.0"),
  gasLimit: 21000
});

console.log("交易哈希:", tx.hash);
await tx.wait();
console.log("确认区块:", tx.blockNumber);
```

#### 发送原始交易

```bash
# 1. 构建交易
# 2. 签名交易
# 3. 通过 RPC 发送

curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_sendRawTransaction",
    "params":["0x<已签名的交易>"],
    "id":1
  }'
```

### 3. 智能合约交互

#### 部署合约

```javascript
const { ethers } = require('ethers');
const fs = require('fs');

const provider = new ethers.JsonRpcProvider('http://localhost:50051');
const wallet = new ethers.Wallet(PRIVATE_KEY, provider);

// 读取合约 ABI 和 bytecode
const abi = JSON.parse(fs.readFileSync('./MyContract.abi', 'utf8'));
const bytecode = fs.readFileSync('./MyContract.bin', 'utf8');

// 部署合约
const factory = new ethers.ContractFactory(abi, wallet);
const contract = await factory.deploy(bytecode);
await contract.deployed();
console.log("合约地址:", contract.address);
```

#### 调用合约

```javascript
const contract = new ethers.Contract(
  "0xContractAddress...",
  abi,
  wallet
);

// 调用只读方法
const value = await contract.myMethod();
console.log("方法返回值:", value);

// 发送交易调用方法
const tx = await contract.myMethod(param1, param2);
await tx.wait();
console.log("交易收据:", txreceipt);
```

#### 监听事件

```javascript
// 监听 Transfer 事件
contract.on("Transfer", (from, to, value, event) => {
  console.log(`从 ${from} 转账 ${value} 到 ${to}`);
});
```

### 4. 查询状态

#### 获取余额

```bash
# ETH 余额
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x742d35Cc6634C0532925a3b844Bc4c1f8e07C94", "latest"],"id":1}'
```

#### 获取 Nonce

```bash
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getTransactionCount","params":["0x742d35Cc6634C05325a3b844Bc4c1f8e07C94", "latest"],"id":1}'
```

#### 获取合约代码

```bash
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getCode","params":["0xContractAddress...", "latest"],"id":1}'
```

---

## 错误代码

| 错误代码 | 描述 |
|---------|------|
| `-32700` | 交易 nonce 太低 |
| `-32602` | 交易 nonce 太高 |
| `-32000` | 交易 gas 不足 |
| `-32603` | 合约执行失败 |

---

## 高级功能

### 批量请求

可以通过 JSON-RPC 批量请求接口来提升性能：

```bash
curl -X POST http://localhost:50051 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getBlockByNumber",
    "params":[[0x3E8, false], [0x3E9, false], [0x3EA, false]],
    "id":1
  }'
```

### WebSocket 订阅

```javascript
const Web3 = require('web3');
const web3 = new Web3('ws://localhost:50051');

// 订阅新区块
web3.eth.subscribe('newBlockHeaders', (error, result) => {
  if (!error) {
    console.log("新区块:", result.number);
  }
});

// 订阅待处理交易
web3.eth.subscribe('pendingTransactions', (error, result) => {
  if (!error) {
    console.log("待处理交易:", result);
  }
});
```

---

## 相关文档

- [架构文档](./架构文档.md)
- [开发指南](./开发指南.md)
- [性能优化](./性能优化.md)
- [状态剪裁](./状态剪裁.md)
