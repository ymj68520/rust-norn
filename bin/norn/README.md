# norn - 命令行工具 (CLI)

## 1. 模块概述 (Overview)

**norn** 是 rust-norn 区块链系统的"命令行控制台"和"启动器"，相当于银行的"网点总控操作台"。如果把整个区块链系统比作一家银行，norn CLI 就是银行柜员的"操作终端"，提供启动节点、生成密钥、管理配置等核心操作。

这个模块是整个区块链系统的**入口程序**，虽然代码量不大，但职责关键：负责解析用户命令、加载配置文件、初始化所有服务、启动节点运行。

**核心业务价值**：
- 🎯 **一键启动**：单条命令启动完整的区块链节点
- 🔑 **密钥管理**：自动生成和安全管理节点身份密钥
- ⚙️ **配置灵活**：支持通过配置文件灵活配置节点参数
- 📊 **日志完善**：统一的日志管理，便于问题排查和监控

---

## 2. 核心功能列表 (Key Features)

- **节点启动**
  - 一条命令启动完整的区块链节点
  - 自动加载配置文件和密钥
  - 初始化所有核心服务（区块链、网络、共识、RPC）
  - 就像"一键开启银行网点"，所有系统自动上线

- **密钥生成与管理**
  - 支持手动生成节点身份密钥（Ed25519）
  - 自动保存密钥到文件系统
  - 如果密钥不存在，启动时自动生成
  - 就像"生成员工工牌和签名"，确保身份唯一

- **配置文件加载**
  - 支持 TOML 格式的配置文件
  - 支持命令行覆盖配置项
  - 灵活的数据目录配置
  - 就像"读取营业手册"，按规则运行

- **日志系统初始化**
  - 统一的日志格式和级别管理
  - 支持环境变量控制日志详细程度
  - 便于开发调试和生产运维
  - 就像"开启监控摄像头"，记录所有操作

- **优雅关闭处理**
  - 捕获系统信号（Ctrl+C、SIGTERM）
  - 自动保存节点状态
  - 安全关闭所有服务
  - 就像"网点关门"，确保数据安全

---

## 3. 业务流程/使用场景 (Use Cases)

### 场景一：首次启动节点

**场景描述**：新员工（用户）需要首次开启一个银行网点（区块链节点）。

**业务流程**：
1. **准备阶段**
   - 确保已安装 norn 程序
   - 准备好配置文件 config.toml

2. **生成密钥**（可选）
   ```bash
   ./norn generate-key --out node.key
   ```
   - 生成 Ed25519 密钥对
   - 保存到指定文件
   - 如果不执行此步，启动时会自动生成

3. **启动节点**
   ```bash
   ./norn --config config.toml
   ```
   - 读取配置文件
   - 加载或生成密钥
   - 初始化所有服务
   - 节点开始运行

**业务价值**：
- 一键启动，操作简单
- 密钥自动管理，降低出错风险
- 新人友好，快速上手

### 场景二：多节点网络部署

**场景描述**：需要在同一台机器上运行多个节点，组成测试网络。

**业务流程**：
1. **准备多个配置文件**
   - node1_config.toml（端口 4001、50051）
   - node2_config.toml（端口 4002、50052）
   - node3_config.toml（端口 4003、50053）

2. **为每个节点生成独立密钥**
   ```bash
   ./norn generate-key --out node1.key
   ./norn generate-key --out node2.key
   ./norn generate-key --out node3.key
   ```

3. **启动节点**（在不同终端）
   ```bash
   # 终端 1
   ./norn --config node1_config.toml

   # 终端 2
   ./norn --config node2_config.toml

   # 终端 3
   ./norn --config node3_config.toml
   ```

**业务价值**：
- 支持本地多节点测试
- 灵活的端口和目录配置
- 便于开发调试和功能验证

### 场景三：生产环境部署

**场景描述**：在生产服务器上部署区块链节点。

**业务流程**：
1. **编译发布版本**
   ```bash
   cargo build --release
   ```

2. **准备生产配置**
   - 配置生产级数据目录（如 `/var/lib/norn`）
   - 配置正确的 RPC 绑定地址
   - 配置引导节点地址
   - 配置验证者密钥

3. **生成并备份密钥**
   ```bash
   ./target/release/norn generate-key --out /secure/location/node.key
   ```

4. **以后台模式启动**
   ```bash
   nohup ./target/release/norn --config production.toml > norn.log 2>&1 &
   ```

5. **监控节点运行**
   ```bash
   # 查看日志
   tail -f norn.log

   # 检查进程
   ps aux | grep norn

   # 检查 RPC 服务
   curl http://localhost:50051/grpc.health
   ```

**业务价值**：
- 支持生产级部署
- 完善的日志和监控
- 稳定可靠的运行

---

## 4. 部署与配置要求 (Deployment & Configuration)

### 环境要求

- **操作系统**：Linux、macOS、Windows
- **硬件要求**：
  - CPU：4 核心及以上
  - 内存：8GB 及以上
  - 磁盘：100GB 及以上（SSD 推荐）

- **软件依赖**：
  - 无需额外依赖，所有功能内置在二进制文件中

### 关键配置项

**命令行参数**：
```bash
norn [OPTIONS]

Options:
  -c, --config <FILE>     配置文件路径 [默认: config.toml]
  -d, --data-dir <DIR>    数据目录路径（覆盖配置文件）
  -h, --help              显示帮助信息
  -V, --version           显示版本信息
```

**子命令**：
```bash
norn generate-key [OPTIONS]

Options:
  -o, --out <FILE>    密钥输出文件路径 [默认: node.key]
```

### 配置文件示例

完整的配置文件示例（`config.toml`）：

```toml
# 数据目录
data_dir = "/var/lib/norn"

# RPC 服务地址
rpc_address = "127.0.0.1:50051"

# 日志配置
[logging]
    level = "info"

# 网络配置
[network]
    # P2P 监听地址
    listen_address = "/ip4/0.0.0.0/tcp/4001"

    # 引导节点
    bootstrap_peers = []

    # mDNS 本地发现
    mdns = true

# 核心配置
[core]
    # 共识配置
    [core.consensus]
        # 验证者公钥
        pub_key = "020000000000000000000000000000000000000000000000000000000000000001"

        # 验证者私钥（生产环境应使用环境变量或密钥管理服务）
        prv_key = "0000000000000000000000000000000000000000000000000000000000000001"

    # 区块生产者配置
    [core.producer]
        # 是否启用区块生产
        is_validator = true

        # 出块间隔（秒）
        block_interval = 1
```

### 编译与安装

```bash
# 克隆仓库
git clone https://github.com/your-org/rust-norn.git
cd rust-norn

# 编译发布版本
cargo build --release

# 二进制文件位置
./target/release/norn

# 可选：安装到系统路径
sudo cp ./target/release/norn /usr/local/bin/
```

---

## 5. 接口与集成说明 (API & Integration)

norn CLI 是一个独立的可执行程序，不提供编程接口，而是通过命令行参数与用户交互。

### 主要命令

#### 1. 启动节点
```bash
# 使用默认配置
./norn

# 指定配置文件
./norn --config /path/to/config.toml

# 覆盖数据目录
./norn --config config.toml --data-dir /custom/data/dir
```

#### 2. 生成密钥
```bash
# 生成密钥到默认位置（node.key）
./norn generate-key

# 生成密钥到指定位置
./norn generate-key --out /secure/path/node.key

# 生成密钥并在配置文件中使用
./norn generate-key --out /var/lib/norn/node.key
```

#### 3. 查看帮助
```bash
# 查看主命令帮助
./norn --help

# 查看子命令帮助
./norn generate-key --help
```

### 环境变量

```bash
# 设置日志级别
RUST_LOG=debug ./norn --config config.toml

# 设置为 trace 级别（最详细）
RUST_LOG=trace ./norn --config config.toml

# 设置为警告级别（仅警告和错误）
RUST_LOG=warn ./norn --config config.toml

# 只显示特定模块的日志
RUST_LOG=norn_node=debug,norn_network=info ./norn --config config.toml
```

### 系统服务集成（Linux systemd）

创建 `/etc/systemd/system/norn.service`：

```ini
[Unit]
Description=Norn Blockchain Node
After=network.target

[Service]
Type=simple
User=norn
WorkingDirectory=/var/lib/norn
ExecStart=/usr/local/bin/norn --config /etc/norn/config.toml
Restart=on-failure
RestartSec=5s

# 日志
StandardOutput=journal
StandardError=journal

# 安全
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

使用 systemd 管理：
```bash
# 重载配置
sudo systemctl daemon-reload

# 启动服务
sudo systemctl start norn

# 查看状态
sudo systemctl status norn

# 查看日志
sudo journalctl -u norn -f

# 开机自启
sudo systemctl enable norn
```

---

## 6. 常见问题 (FAQ)

### Q1：如何查看节点是否正常启动？

**A**：
```bash
# 1. 检查进程
ps aux | grep norn

# 2. 查看日志（如果前台运行）
tail -f norn.log

# 3. 检查 RPC 端口
netstat -tuln | grep 50051

# 4. 测试 RPC 服务
curl http://localhost:50051/grpc.health

# 5. 查看节点日志中的 "Node started" 消息
```

### Q2：密钥文件丢失了怎么办？

**A**：
- **密钥丢失的后果**：节点身份改变，需要重新配置到网络
- **恢复方法**：
  1. 生成新的密钥：`./norn generate-key --out node.key`
  2. 更新配置文件中的公钥
  3. 如果是验证者节点，需要更新网络的验证者集合
- **预防措施**：
  - 定期备份密钥文件到安全位置
  - 生产环境使用密钥管理服务（如 AWS KMS）

### Q3：如何同时运行多个节点？

**A**：
1. 为每个节点创建独立的配置文件
2. 确保每个节点使用不同的端口：
   - P2P 端口（network.listen_address）
   - RPC 端口（rpc_address）
3. 每个节点使用不同的数据目录
4. 在不同终端启动，或使用后台模式

**示例**：
```bash
# 节点 1
./norn --config node1.toml &
echo $! > node1.pid

# 节点 2
./norn --config node2.toml &
echo $! > node2.pid

# 停止所有节点
kill $(cat node1.pid) $(cat node2.pid)
```

### Q4：如何查看节点日志？

**A**：
```bash
# 方式一：前台运行，直接看输出
./norn --config config.toml

# 方式二：后台运行，查看日志文件
nohup ./norn --config config.toml > norn.log 2>&1 &
tail -f norn.log

# 方式三：使用 systemd
sudo journalctl -u norn -f

# 方式四：过滤特定模块的日志
RUST_LOG=norn_network=debug ./norn --config config.toml 2>&1 | grep network
```

### Q5：如何优雅地停止节点？

**A**：
```bash
# 方式一：前台运行，按 Ctrl+C

# 方式二：发送 SIGTERM 信号
kill -SIGTERM <pid>

# 方式三：使用 systemd
sudo systemctl stop norn

# 方式四：查找并杀死进程
pkill -TERM norn

# 注意：不要使用 SIGKILL（kill -9），会导致数据未保存
```

### Q6：节点启动失败怎么办？

**A**：常见排查步骤：

1. **检查配置文件**
   ```bash
   # 验证 TOML 语法
   cat config.toml
   ```

2. **检查端口占用**
   ```bash
   # 检查 P2P 端口
   netstat -tuln | grep 4001

   # 检查 RPC 端口
   netstat -tuln | grep 50051
   ```

3. **检查数据目录权限**
   ```bash
   ls -ld /var/lib/norn
   ```

4. **查看详细日志**
   ```bash
   RUST_LOG=debug ./norn --config config.toml
   ```

5. **尝试清理数据目录重新同步**
   ```bash
   # 备份数据
   mv /var/lib/norn /var/lib/norn.bak

   # 重新启动（会从头同步）
   ./norn --config config.toml
   ```

### Q7：如何升级节点版本？

**A**：
1. **停止运行的节点**
   ```bash
   kill -SIGTERM <pid>
   ```

2. **备份数据目录**
   ```bash
   cp -r /var/lib/norn /var/lib/norn.backup
   ```

3. **编译新版本**
   ```bash
   git pull
   cargo build --release
   ```

4. **替换二进制文件**
   ```bash
   cp ./target/release/norn /usr/local/bin/norn
   ```

5. **启动新版本**
   ```bash
   ./norn --config config.toml
   ```

6. **观察日志确认升级成功**
   ```bash
   tail -f norn.log
   ```

---

## 技术支持

如有疑问或需要技术支持，请参考项目主文档或联系技术支持团队。

**使用提示**：
1. 生产环境务必定期备份数据目录和密钥文件
2. 建议使用 systemd 或 supervisor 管理节点进程
3. 配置日志轮转，避免日志文件过大
4. 监控磁盘使用情况，预留足够空间
