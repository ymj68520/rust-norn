# Norn 测试快速参考

## 🚀 快速开始

### 从项目根目录运行所有测试

```bash
./run_tests.sh
```

### 进入测试目录

```bash
cd tests
```

---

## 📋 测试命令速查

### 运行所有测试
```bash
./run_all_tests.sh
```

### 运行特定测试

#### 单元测试
```bash
cargo run --bin unit-test
```

#### 集成测试
```bash
cargo run --bin integration-test
```

#### 可扩展性测试
```bash
cargo run --bin scalability-test
```

#### TPS 性能测试
```bash
cd performance/tps_test
./run_tps_test.sh              # 默认测试
./max_tps_benchmark.sh         # 最大 TPS 基准测试
```

#### E2E 测试
```bash
cd e2e
cargo test --test e2e_full_workflow_test
cargo test --test integration_test
```

---

## 📁 目录结构

```
tests/
├── integration/          → 集成测试
├── unit/                → 单元测试
├── performance/         → 性能测试
│   ├── tps_test/       → TPS 测试
│   └── scalability_test/ → 可扩展性测试
├── tools/               → 测试工具
└── e2e/                → E2E 测试
```

---

## 🔧 开发命令

### 构建所有测试
```bash
cargo build --workspace
```

### 清理构建
```bash
cargo clean
```

### 检查编译
```bash
cargo check --workspace
```

---

## 📊 测试说明

| 测试类型 | 说明 | 运行时间 |
|---------|------|---------|
| unit-test | 数据库操作、基础功能 | ~5s |
| integration-test | 跨模块集成 | ~10s |
| scalability-test | 大规模数据测试 | ~30s |
| tps_test | 性能压测 | 可配置（默认 60s） |

---

## 📖 详细文档

- `README.md` - 完整测试文档
- `MIGRATION.md` - 从旧结构迁移指南
- `../docs/测试文档.md` - 项目测试指南

---

## ❓ 获取帮助

```bash
# 查看测试帮助
cd tests
cargo run --bin <test_name> -- --help
```

---

**更新日期**: 2026-02-01
