# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**rust-norn** is a Rust implementation of a blockchain node featuring PoVF (Proof of Verifiable Function) consensus with EVM compatibility. The codebase uses a modular workspace architecture with clear separation of concerns.

## Build and Development Commands

### Using Make (Recommended)
```bash
make build          # Build release binary
make test           # Run all tests
make check          # Cargo check
make fmt            # Format code
make clippy         # Run linter
make coverage       # Generate coverage report (requires cargo-llvm-cov)
make clean          # Clean build artifacts
make doc            # Generate and open documentation
make audit          # Security audit
```

### Using Cargo
```bash
cargo build --release              # Build workspace
cargo test --workspace              # Run all tests
cargo test -p norn-core             # Test specific crate
cargo fmt --all                     # Format code
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Running Single Tests
```bash
# Run a specific test module
cargo test -p norn-core state::tests::test_account_manager

# Run with output
cargo test -p norn-core -- --nocapture test_name

# Run integration tests
cargo test --test integration_test
```

### Docker Multi-Node Setup
```bash
docker-compose up -d       # Start 3-node network
./docker/logs.sh           # View logs
docker-compose down        # Stop nodes
```

## High-Level Architecture

### Workspace Layer Structure
```
Foundation:    norn-common (types) → norn-crypto (VRF/VDF)
Core:          norn-storage → norn-core (blockchain, consensus, evm, state)
Network:       norn-network (libp2p)
API:           norn-rpc (gRPC + Ethereum JSON-RPC)
Orchestration: norn-node → bin/norn (CLI)
Testing:       test_integration, db_test, tps_test, scalability_test
```

### Key Architectural Patterns

**Async/Sync Bridging**: The codebase extensively bridges async (Tokio) and sync contexts:
- `crates/core/src/state/cache.rs` provides `SyncStateManager` for EVM access
- `crates/core/src/evm/runtime.rs` uses `NornDatabaseAdapter` with sync wrappers
- EVM execution requires sync state access but operates in async blockchain context

**State Management** (`crates/core/src/state/`):
- `AccountStateManager` - Main async account/storage manager with dual locks (accounts + storage)
- `SyncStateManager` - Sync wrapper for EVM access
- `PersistentStateManager` - WAL-based persistence with versioning
- `StatePruningManager` - Storage optimization with snapshot management
- State uses Merkle Patricia Trie (MPT) in `merkle.rs` for state root calculation

**EVM Integration** (`crates/core/src/evm/`):
- Built on revm v14 with custom `NornDatabaseAdapter` bridging to `SyncStateManager`
- `EVMExecutor` executes transactions with gas tracking, event logs, receipts
- Precompiles, EIP-1559, access lists supported
- Contract code stored in `CodeStorage`, logs in `LogManager`, receipts in `ReceiptDB`

**PoVF Consensus** (`crates/core/src/consensus/`):
- Combines VRF (Verifiable Random Function) for leader selection + VDF (Verifiable Delay Function) for time delay
- `povf.rs`: PoVF engine with voting, rounds, validator management
- `producer.rs`: Block producer with VRF keypair, configurable intervals
- VDF calculator in `norn-crypto` uses sequential squaring modulo secp256k1 prime

**Block Flow**:
1. `BlockProducer` creates blocks using VRF election
2. Blocks go to `BlockBuffer` for validation/reorg handling
3. VDF verification happens in buffer before selection
4. Valid blocks pass to `Blockchain.add_block()` → `DataProcessor`
5. State updates applied through `AccountStateManager`

### Network Architecture (`crates/network/`)
- **libp2p stack**: TCP + Yamux muxing + Noise encryption
- **Discovery**: mDNS (local) + Kademlia DHT (remote)
- **Messaging**: Gossipsub for block/tx propagation
- **Compression**: Zstd/Snappy via `Compressor` with magic byte prefix (0xFF 0xCF)
- Messages defined in `messages/sync.rs` with encoder/validator

### Storage (`crates/storage/`)
- **SledDB** embedded database (not RocksDB anymore)
- Database files in configured `data_dir`
- Each node maintains isolated data directory
- WAL-based recovery in `crates/core/src/state/db.rs`

## Important Implementation Details

### Disabled Modules in execution/
The `crates/core/src/execution/mod.rs` has disabled modules (gas, executor, nonce) due to import issues:
```rust
// TODO: Fix imports in the following modules and re-enable them:
// pub mod gas;
// pub mod executor;
// pub mod nonce;
```
Use `crates/core/src/evm/` for EVM execution instead.

### Blockchain Methods
```rust
// Returns Option<Block>, NOT Result
blockchain.get_block_by_height(height: i64) -> Option<Block>
blockchain.get_block_by_hash(hash: &Hash) -> Option<Block>

// Returns () and always succeeds (uses buffer internally)
blockchain.add_block(block: Block)

// Access latest block:
let latest = blockchain.latest_block.read().await;
let height = latest.header.height;
```

### Transaction Types
- **Native**: Internal transactions (handled by native executor, not EVM router)
- **EVM**: Ethereum-compatible transactions (routed through `TransactionRouter`)
- Router in `crates/core/src/execution/router.rs` handles both types

### Configuration Files
- TOML-based with sections: `data_dir`, `rpc_address`, `[core.consensus]`, `[network]`
- Validator keys in `[core.consensus]` as public/private key pairs
- Must ensure unique ports and data_dir for multi-node local testing

### Error Handling Conventions
- **anyhow** for application error propagation
- **thiserror** for error type definitions
- `norn_common::error::NornError` enum with variants: Database, Network, Crypto, Validation, ConsensusError, Config, Io, Serialization, Internal

### Dependency Management
All dependencies centralized in root `Cargo.toml` `[workspace.dependencies]`. Add new deps by:
1. Adding to workspace dependencies with version
2. Referencing in crate-specific `Cargo.toml` as `crate-name = { workspace = true }`

## Testing Patterns

- **Unit tests**: Co-located in source files (`mod tests { ... }`)
- **Integration tests**: `test_integration/` crate
- **TPS testing**: `tps_test` with configurable rate/duration
- Run `make test` or `cargo test --workspace` for full test suite

### Test File Locations
- State tests: `crates/core/src/state/account.rs` (mod tests)
- EVM tests: `crates/core/src/evm/executor.rs` (mod tests)
- Network compression tests: `crates/network/src/compression.rs` (mod tests)

## Development Constraints

### Build Requirements
- **protoc** compiler required for gRPC compilation (tonic-build)
- LLVM/Clang required on some systems (set `LIBCLANG_PATH` on Windows)
- Rust Edition 2021 (stable)

### Configuration Validation
- Ports must be unique across nodes on same machine
- `data_dir` paths must be unique per node
- mDNS only works on local network (use `bootstrap_peers` for remote)
- RPC ports need to be different for multi-node local testing

### Key Management
- Keys auto-generated on first run if not present: `{data_dir}/node.key`
- Manual generation: `./target/release/norn generate-key --out node.key`
- Keys stored as hex-encoded keypairs
- Public keys used in consensus config for validator authorization

### Database Operations
- SledDB databases created automatically in `data_dir`
- Clean restart: delete `data_dir` and restart node
- WAL recovery: Implemented in `crates/core/src/state/db.rs`
- DB versioning: Check `check_db_version()` with `DB_VERSION` constant

## Node Orchestration

The `NornNode` service (`crates/node/src/service.rs`) orchestrates:
- Blockchain and transaction pool initialization
- PoVF consensus engine with VRF keypair
- Block producer with configurable interval
- Network service startup and event handling
- RPC servers (both gRPC and Ethereum JSON-RPC)
- Peer manager for connection tracking
- Block syncer for chain synchronization
- EVM executor for smart contract support

### Node Startup Flow
1. Initialize logging (configurable format/level)
2. Create metrics collector (if Prometheus enabled)
3. Start monitoring server (if health check enabled)
4. Initialize SledDB storage
5. Create blockchain with genesis
6. Setup account state manager
7. Initialize EVM executor
8. Create transaction pool (standard or enhanced)
9. Setup VRF keypair and VDF calculator
10. Initialize PoVF consensus engine
11. Create block producer
12. Start network service with libp2p
13. Start RPC servers
14. Spawn event loop for network messages

## Performance Testing

### TPS Testing
```bash
cargo build -p tps_test --release
./target/release/tps_test --rate 100 --duration 60
./target/release/tps_test --rate 500 --duration 120 --rpc-address 127.0.0.1:50051
./tps_test/max_tps_benchmark.sh  # Automated benchmark
```

The `tps_test` module provides real-time monitoring with configurable target TPS and duration.

## Module Cross-References

When working across modules:
- **State changes** go through `AccountStateManager` (async) or `SyncStateManager` (sync for EVM)
- **Block validation** uses `verify_block_vdf()` in `block_buffer.rs`
- **Transaction routing** via `TransactionRouter` in `execution/router.rs`
- **Network compression** via `Compressor` in `network/src/compression.rs`
- **Merkle proofs** via `verify_proof()` in `state/merkle.rs`

## Common Issues

### Async/Sync Interface Mismatch
EVM requires sync state access but blockchain is async. Use `SyncStateManager` wrapper for EVM operations.

### Type Alias Confusion
`Result<T>` in `norn_common::error` is `std::result::Result<T, NornError>`, not `anyhow::Result<T>`.

### Blockchain Method Signatures
`get_block_by_*` return `Option<Block>`, not `Result<Option<Block>>`. `add_block` returns `()`, not `Result`.

### VDF Verification
Called in `block_buffer.rs` during block processing. VDF output stored in block header `params` field as `GeneralParams`.
