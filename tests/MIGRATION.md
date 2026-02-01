# 测试目录迁移指南

## 📁 新的测试结构

所有测试已从项目根目录整合到统一的 `tests/` 目录下。

### 目录映射

| 旧位置 | 新位置 | 说明 |
|--------|--------|------|
| `test_integration/` | `tests/integration/` | 集成测试 |
| `db_test/` | `tests/unit/` | 单元测试（数据库） |
| `scalability_test/` | `tests/performance/scalability_test/` | 可扩展性测试 |
| `tps_test/` | `tests/performance/tps_test/` | TPS 性能测试 |
| `test_tx_gen/` | `tests/tools/` | 测试工具（交易生成器） |

### 新结构

```
tests/
├── Cargo.toml                    # 测试工作空间配置
├── README.md                     # 测试文档
├── run_all_tests.sh              # 统一测试运行脚本
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
│   │   └── run_tps_test.sh
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
    └── integration_test.rs
```

---

## 🚀 如何运行测试

### 方法 1: 从项目根目录运行（推荐）

```bash
# 运行所有测试
./run_tests.sh

# 或进入 tests 目录
cd tests
./run_all_tests.sh
```

### 方法 2: 运行特定测试

```bash
# 进入 tests 目录
cd tests

# 单元测试
cargo run --bin unit-test

# 集成测试
cargo run --bin integration-test

# 可扩展性测试
cargo run --bin scalability-test

# TPS 测试
cd performance/tps_test
./run_tps_test.sh
```

### 方法 3: 使用 Cargo

```bash
# 在 tests 目录下
cd tests

# 构建所有测试
cargo build --workspace

# 运行特定测试
cargo run --bin <test_name>
```

---

## 🔄 从旧结构迁移

### 更新 CI/CD 脚本

**旧命令**:
```bash
cargo test -p test_integration
cargo test -p db_test
cargo test -p scalability_test
./tps_test/run_tps_test.sh
```

**新命令**:
```bash
cd tests
cargo run --bin integration-test
cargo run --bin unit-test
cargo run --bin scalability-test
cd performance/tps_test && ./run_tps_test.sh
```

或使用统一脚本:
```bash
cd tests && ./run_all_tests.sh
```

### 更新文档

更新项目 README 和开发文档，指向新的测试目录：

```markdown
## 运行测试

```bash
# 运行所有测试
./run_tests.sh

# 或进入测试目录
cd tests
./run_all_tests.sh
```

详细文档请参阅 [tests/README.md](tests/README.md)
```

---

## ⚠️ 注意事项

### 1. 旧目录暂时保留

旧的测试目录（`test_integration/`, `db_test/` 等）暂时保留在项目根目录，以确保向后兼容。这些目录将在未来的版本中移除。

### 2. 路径变更

如果你在脚本中硬编码了测试路径，需要更新：

- `test_integration/` → `tests/integration/`
- `db_test/` → `tests/unit/`
- `tps_test/` → `tests/performance/tps_test/`

### 3. Cargo.toml 更新

主 `Cargo.toml` 已更新，测试模块不再作为工作空间成员。测试现在由独立的 `tests/Cargo.toml` 管理。

---

## 📖 参考文档

- [tests/README.md](./README.md) - 完整测试文档
- [docs/测试文档.md](../docs/测试文档.md) - 测试指南

---

## 🤝 贡献

添加新测试时，请遵循新的目录结构：

1. 在 `tests/` 下创建相应的子目录
2. 更新 `tests/Cargo.toml` 添加新成员
3. 在 `tests/README.md` 中添加文档

---

**迁移日期**: 2026-02-01
**版本**: 1.0.0
