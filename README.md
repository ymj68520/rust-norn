# rust-norn: A Norn Blockchain Node in Rust

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/your-repo/rust-norn)
[![Rust Version](https://img.shields.io/badge/rust-2021%20stable-orange)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**rust-norn** is a high-performance blockchain node implementation in Rust, replicating the go-norn protocol with a **Proof of Verifiable Function (PoVF)** consensus mechanism. It features a modular architecture, leveraging Rust's type safety, memory safety, and concurrency primitives for secure and efficient blockchain operations.

## 🌟 Key Features

- **PoVF Consensus**: Novel Proof of Verifiable Function combining VRF for leader election and VDF for sequential computation
- **Modular Architecture**: Clean separation across 8+ crates with well-defined responsibilities
- **High Performance**: Built on Tokio async runtime with SledDB for efficient storage
- **P2P Networking**: libp2p stack with mDNS discovery, Gossipsub, and Kademlia DHT
- **gRPC API**: Complete external API for blockchain interactions
- **Multi-Node Support**: Easy deployment of multi-node networks with Docker Compose
- **Performance Testing**: Comprehensive TPS testing tools with real-time monitoring
- **Developer Friendly**: Extensive documentation, examples, and testing utilities

## 📋 Table of Contents

- [Quick Start](#quick-start)
- [Project Architecture](#project-architecture)
- [Development Guide](#development-guide)
- [Testing](#testing)
- [Deployment](#deployment)
- [Documentation](#documentation)
- [Performance](#performance)
- [Contributing](#contributing)
- [License](#license)

## 🚀 Quick Start

### Prerequisites

- **Rust** Edition 2021 (stable)
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Protocol Buffers Compiler** (protoc)
  - Linux: `sudo apt install protobuf-compiler`
  - macOS: `brew install protobuf`
  - Windows: Download from [protobuf releases](https://github.com/protocolbuffers/protobuf/releases)

### Build

```bash
# Clone the repository
git clone https://github.com/your-repo/rust-norn.git
cd rust-norn

# Build in release mode
cargo build --release

# Or use Make
make build
```

### Run a Single Node

```bash
# Create a configuration file (config.toml)
cat > config.toml << EOF
data_dir = "data"
rpc_address = "127.0.0.1:50051"

[core]
    [core.consensus]
    pub_key = "020000000000000000000000000000000000000000000000000000000000000001"
    prv_key = "0000000000000000000000000000000000000000000000000000000000000001"

[network]
listen_address = "/ip4/0.0.0.0/tcp/4001"
bootstrap_peers = []
mdns = true
EOF

# Start the node
./target/release/norn --config config.toml
```

The node will:
- Start listening for P2P connections on port 4001
- Expose gRPC API on port 50051
- Generate keys automatically if not present
- Begin block production with default configuration

### Run Multi-Node Network

```bash
# Terminal 1
./target/release/norn --config node1_config.toml

# Terminal 2
./target/release/norn --config node2_config.toml

# Terminal 3
./target/release/norn --config node3_config.toml
```

Or use Docker Compose:
```bash
docker-compose up -d
```

## 🏗️ Project Architecture

### Workspace Structure

```
rust-norn/
├── bin/norn/              # CLI executable
├── crates/
│   ├── common/            # Shared types and utilities
│   ├── crypto/            # Cryptographic primitives (VRF, ECDSA, VDF)
│   ├── storage/           # SledDB integration
│   ├── core/              # Blockchain engine and consensus
│   ├── network/           # libp2p P2P networking
│   ├── rpc/               # gRPC server
│   └── node/              # Service orchestration
├── test_integration/      # Integration tests
├── tps_test/             # TPS performance testing
└── scalability_test/     # Performance benchmarks
```

### Core Components

| Component | Description | Key Technologies |
|-----------|-------------|------------------|
| **norn-common** | Shared data structures, types, error handling | serde, chrono |
| **norn-crypto** | VRF (P-256), ECDSA (secp256k1), VDF calculator | p256, k256, rand |
| **norn-storage** | Persistent key-value storage | SledDB |
| **norn-core** | Blockchain, consensus, tx pool, validation | tokio, async-trait |
| **norn-network** | P2P networking, peer discovery, gossip | libp2p |
| **norn-rpc** | gRPC API for external clients | tonic, prost |
| **norn-node** | Service coordination and lifecycle | tokio |

### Consensus Mechanism

**Proof of Verifiable Function (PoVF)** combines:

1. **VRF (Verifiable Random Function)**: Randomly selects block proposers based on stake weights
2. **VDF (Verifiable Delay Function)**: Ensures sequential time delay between consensus rounds
3. **Verification**: All nodes verify VRF proofs and VDF outputs

## 💻 Development Guide

### Setup Development Environment

```bash
# Install development dependencies
make setup

# Run development workflow (format + check + test)
make dev

# Run CI workflow (format + clippy + test)
make ci
```

### Common Development Tasks

#### Add New Dependency

```bash
# Add to workspace Cargo.toml
# [workspace.dependencies]
# your-crate = "1.0"

# Use in crate Cargo.toml
# [dependencies]
# your-crate = { workspace = true }
```

#### Run Specific Tests

```bash
# Test specific crate
cargo test -p norn-core

# Test specific module
cargo test test_block_validation

# Run with output
cargo test -- --nocapture

# Run tests in release mode
cargo test --release
```

#### Code Quality

```bash
# Format code
cargo fmt --all

# Run linter
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Generate documentation
cargo doc --workspace --no-deps --open
```

### Project Guidelines

- **Code Organization**: Keep crate responsibilities clear and focused
- **Testing**: Write unit tests alongside code, integration tests in `test_integration/`
- **Error Handling**: Use `anyhow` for application errors, `thiserror` for error types
- **Async**: Use `tokio` for all async operations
- **Logging**: Use `tracing` for structured logging

See [DEVELOPER_GUIDE.md](docs/guides/DEVELOPER_GUIDE.md) for detailed development documentation.

## 🧪 Testing

### Test Structure

```
test_integration/     # Cross-component integration tests
db_test/             # Database validation tests
scalability_test/    # Performance benchmarks
tps_test/            # TPS load testing
```

### Running Tests

```bash
# All tests
cargo test --workspace

# Integration tests
cargo test --test integration_test

# Database tests
cargo test -p db_test

# Performance benchmarks
cargo bench --workspace
```

### TPS Performance Testing

```bash
# Build TPS test tool
cargo build -p tps_test --release

# Run default test (100 TPS, 60 seconds)
./target/release/tps_test

# Custom test
./target/release/tps_test --rate 500 --duration 120

# Maximum TPS benchmark
./tps_test/max_tps_benchmark.sh
```

See [TESTING.md](docs/testing/TESTING.md) for comprehensive testing documentation.

## 🚢 Deployment

### Local Deployment

```bash
# Single node
./target/release/norn --config production_config.toml

# Multi-node with mDNS discovery
# Each node needs unique ports and data_dir
./target/release/norn --config node1_config.toml &
./target/release/norn --config node2_config.toml &
./target/release/norn --config node3_config.toml &
```

### Docker Deployment

```bash
# Build Docker image
docker build -t rust-norn:latest .

# Start 3-node network
docker-compose up -d

# View logs
./docker/logs.sh

# Test connectivity
./docker/test-network.sh

# Stop network
docker-compose down
```

### Production Considerations

- **Configuration**: Use proper production config with secure keys
- **Security**: Enable firewall, use TLS for RPC, secure key management
- **Monitoring**: Set up metrics collection and alerting
- **Backup**: Regular backups of `data_dir`
- **Network**: Use bootstrap_peers instead of mDNS for remote nodes

See [DEPLOYMENT.md](docs/guides/DEPLOYMENT.md) for detailed deployment guides.

## 📚 Documentation

### Core Documentation

- **[CLAUDE.md](CLAUDE.md)** - AI/Developer assistance guide
- **[Architecture](docs/architecture/ARCHITECTURE.md)** - System architecture and design
- **[API Reference](docs/api/API.md)** - gRPC API documentation
- **[Developer Guide](docs/guides/DEVELOPER_GUIDE.md)** - Development workflows
- **[Testing Guide](docs/testing/TESTING.md)** - Testing strategies and tools
- **[Deployment Guide](docs/guides/DEPLOYMENT.md)** - Production deployment

### Component Documentation

- **[PoVF Consensus](docs/architecture/CONSENSUS.md)** - Consensus mechanism details
- **[Cryptography](docs/architecture/CRYPTO.md)** - Cryptographic primitives
- **[Networking](docs/architecture/NETWORKING.md)** - P2P networking architecture
- **[Storage](docs/architecture/STORAGE.md)** - Database and persistence

### Example Code

```rust
// Connect to blockchain via gRPC
use norn_rpc::blockchain_client::BlockchainClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = BlockchainClient::connect("http://127.0.0.1:50051").await?;

    // Get current block number
    let response = client.get_block_number(()).await?;
    println!("Current block: {}", response.into_inner().number);

    Ok(())
}
```

See the `rpc/examples/` directory for more examples.

## ⚡ Performance

### Benchmarks

Latest test results (from `PERFORMANCE_TEST_REPORT.md`):

| Metric | Value |
|--------|-------|
| Max TPS Achieved | 1000+ |
| Block Interval | 1 second |
| Transaction Finality | ~2 blocks |
| Memory Usage | ~200MB per node |
| Disk I/O | Optimized with SledDB |

### Optimization Tips

1. **Release Builds**: Always use `--release` for production
2. **Database**: SledDB automatically caches hot data
3. **Network**: Tune libp2p parameters for your network size
4. **Consensus**: Adjust `block_interval` based on requirements

Run your own benchmarks:
```bash
./tps_test/max_tps_benchmark.sh
```

## 🤝 Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Workflow

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes
4. Run tests: `make dev`
5. Commit: `git commit -m 'Add amazing feature'`
6. Push: `git push origin feature/amazing-feature`
7. Open a Pull Request

### Code Review Standards

- All code must pass `make ci`
- New features need tests
- Documentation updated for public APIs
- Follow Rust style guidelines (`cargo fmt`)

## 📊 Project Status

### ✅ Implemented

- Complete modular workspace architecture
- Cryptographic primitives (VRF, ECDSA, VDF)
- SledDB persistent storage
- libp2p P2P networking with discovery
- gRPC API with protobuf definitions
- Transaction pool and validation
- Basic PoVF consensus engine
- Block producer with VRF-based leader election
- Fee and reward distribution system
- Wallet implementation
- Multi-node Docker deployment
- TPS performance testing tools

### 🚧 In Progress

- Complete PoVF consensus integration
- Full blockchain state management
- Comprehensive block validation
- Network synchronization protocols
- Production-ready monitoring and metrics

### 🔜 Planned

- Smart contract support
- Light client mode
- State pruning
- Snapshot synchronization
- Advanced cryptographic features
- Mobile wallet integration

## ❓ FAQ

<details>
<summary><b>How do I reset the blockchain?</b></summary>

Delete the data directory and restart:
```bash
rm -rf node1_data
./target/release/norn --config node1_config.toml
```
</details>

<details>
<summary><b>Why aren't my nodes discovering each other?</b></summary>

Check:
- mDNS is enabled in config
- Nodes are on the same network
- Firewall allows traffic
- For Docker: use `bootstrap_peers` instead of mDNS
</details>

<details>
<summary><b>How do I increase TPS?</b></summary>

Adjust block producer config:
```toml
block_interval = 1  # Lower = faster blocks
```
And ensure sufficient CPU/resources.
</details>

## 📜 License

This project is licensed under the MIT License - see [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- **go-norn**: Original Go implementation that inspired this project
- **libp2p**: Excellent P2P networking library
- **Tokio**: Amazing async runtime
- **SledDB**: Modern embedded database

## 📞 Support

- **Issues**: [GitHub Issues](https://github.com/your-repo/rust-norn/issues)
- **Discussions**: [GitHub Discussions](https://github.com/your-repo/rust-norn/discussions)
- **Documentation**: [docs/](docs/)

---

**Built with ❤️ in Rust**
