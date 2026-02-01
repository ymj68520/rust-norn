# Norn 测试套件

统一的测试目录，包含所有测试模块。

---

## 📁 目录结构

```
tests/
├── Cargo.toml                    # 测试工作空间配置
├── README.md                     # 本文件
├── run_all_tests.sh              # 运行所有测试的脚本
│
├── integration/                  # 集成测试
│   ├── Cargo.toml
│   └── src/main.rs
│
├── unit/                         # 单元测试
│   ├── Cargo.toml
│   └── src/main.rs
│
├── performance/                  # 性能测试
│   ├── tps_test/                # TPS 测试
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   ├── run_tps_test.sh
│   │   └── max_tps_benchmark.sh
│   │
│   └── scalability_test/        # 可扩展性测试
│       ├── Cargo.toml
│       └── src/main.rs
│
├── tools/                        # 测试工具
│   ├── Cargo.toml
│   └── src/main.rs
│
└── e2e/                         # 端到端测试（独立脚本）
    ├── e2e_full_workflow_test.rs
    ├── integration_test.rs
    └── simple_verification.rs
```

---

## 🚀 快速开始

### 运行所有测试

```bash
# 从项目根目录运行
cd tests
./run_all_tests.sh

# 或手动运行各个测试
cargo test --workspace -p integration-test
cargo test --workspace -p unit-test
cargo test --workspace -p scalability-test
```

### 运行特定测试

#### 集成测试

```bash
cd tests
cargo run --bin integration-test
```

#### 单元测试

```bash
cd tests
cargo run --bin unit-test
```

#### TPS 性能测试

```bash
cd tests/performance/tps_test

# 默认测试（100 TPS, 60秒）
./run_tps_test.sh

# 自定义参数
./target/release/tps_test --rate 500 --duration 120

# 最大 TPS 基准测试
./max_tps_benchmark.sh
```

#### 可扩展性测试

```bash
cd tests
cargo run --bin scalability-test
```

#### 交易生成器

```bash
cd tests
cargo run --bin tx-generator
```

#### E2E 测试

```bash
cd tests/e2e

# 运行 E2E 测试
cargo test --test e2e_full_workflow_test
cargo test --test integration_test
```

---

## 📊 测试说明

### 1. 集成测试 (integration/)

测试跨模块的交互和集成：

- 区块链完整流程
- 跨 crate 功能验证
- API 集成测试

### 2. 单元测试 (unit/)

测试单个模块和功能：

- 数据库操作
- 核心数据结构
- 密码学功能

### 3. 性能测试 (performance/)

#### TPS 测试 (tps_test/)

测量系统的交易吞吐量：

- 目标 TPS: 1000+
- 持续时间: 可配置
- 实时监控和统计

**输出示例**:
```
Target TPS: 500
Duration: 60s
Total sent: 30000
Total confirmed: 26543
Actual TPS: 442.38
Success rate: 88.48%
```

#### 可扩展性测试 (scalability_test/)

测试系统在负载下的表现：

- 大规模区块处理
- 内存占用分析
- 性能瓶颈识别

### 4. 测试工具 (tools/)

用于测试的辅助工具：

- **tx-generator**: 生成测试交易
- 可用于压力测试
- 支持批量生成

### 5. E2E 测试 (e2e/)

端到端的完整流程测试：

- 完整的区块生成流程
- 交易从提交到确认
- 多节点交互

---

## 🛠️ 开发指南

### 添加新测试

#### 添加新的集成测试

1. 在 `integration/src/main.rs` 中添加测试函数
2. 使用 `#[tokio::test]` 标记异步测试

```rust
#[tokio::test]
async fn test_new_feature() {
    // 测试代码
    assert!(true);
}
```

#### 添加新的性能测试

1. 在 `performance/` 下创建新目录
2. 创建 `Cargo.toml` 和 `src/main.rs`
3. 在 `tests/Cargo.toml` 中添加新成员

```toml
[workspace]
members = [
    # ...
    "performance/your_test",
]
```

### 更新依赖

所有测试共享工作空间依赖。在 `tests/Cargo.toml` 中更新：

```toml
[workspace.dependencies]
# 添加或更新依赖
some-new-dep = "1.0"
```

---

## 📈 测试覆盖率

### 生成覆盖率报告

```bash
# 使用 tarpaulin
cargo tarpaulin --workspace --out Html --output-dir coverage/

# 查看报告
firefox coverage/index.html
```

### 覆盖率目标

| 组件 | 目标 | 当前 |
|------|------|------|
| 核心逻辑 | >90% | TBD |
| 密码学 | 100% | TBD |
| 状态管理 | >85% | TBD |
| 网络层 | >80% | TBD |

---

## 🔧 故障排查

### 测试失败

1. **检查环境**: 确保所有依赖已安装
   ```bash
   cargo build --workspace
   ```

2. **清理构建**:
   ```bash
   cargo clean
   ```

3. **单独运行失败的测试**:
   ```bash
   cd tests/<test_type>
   cargo test -- <test_name>
   ```

### 端口冲突

某些测试需要特定端口（如 50051）。确保端口可用：

```bash
# 检查端口占用
lsof -i :50051

# 终止占用进程
kill -9 <PID>
```

### 数据库锁定

测试使用临时目录。如果遇到锁定：

```bash
# 清理测试数据
rm -rf /tmp/norn_test_*
```

---

## 📝 相关文档

- [开发指南](../docs/开发指南.md)
- [测试文档](../docs/测试文档.md)
- [API 文档](../docs/API文档.md)

---

## 🤝 贡献

添加新测试时，请确保：

1. 测试可以独立运行
2. 清理临时资源
3. 提供清晰的测试名称
4. 包含必要的文档注释

---

**最后更新**: 2026-02-01
