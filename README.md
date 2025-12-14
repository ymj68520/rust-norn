# rust-norn: A Norn Blockchain Node in Rust

This repository contains a Rust port of the `go-norn` blockchain node, aiming to replicate its Proof of Verifiable Function (PoVF) consensus mechanism and core functionalities in a robust, performant, and idiomatic Rust environment.

## Project Overview

`rust-norn` is a blockchain node re-implemented in Rust. It features a modular architecture, leveraging Rust's type safety and concurrency primitives. Key components include:

*   **`norn-common`**: Shared data structures, types, and utility functions.
*   **`norn-crypto`**: Cryptographic primitives, including P-256 VRF, ECDSA, and VDF.
*   **`norn-storage`**: Persistent key-value store using SledDB (migrated from RocksDB).
*   **`norn-core`**: The heart of the blockchain, managing the ledger, transaction pool, and block buffer.
*   **`norn-network`**: P2P communication layer built on `rust-libp2p` for peer discovery and message propagation.
*   **`norn-rpc`**: gRPC server for external API interactions.
*   **`norn-node`**: Orchestrates all services, bringing up the full node.
*   **`bin/norn`**: The main executable for running the node.

## Prerequisites

*   **Rust Toolchain**: Rust Edition 2021 (stable recommended). Install via `rustup`: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
*   **Protocol Buffers**: Required for gRPC compilation:
    *   **Windows**: Download from [protobuf releases](https://github.com/protocolbuffers/protobuf/releases) and extract `protoc.exe` to a directory in your PATH.
    *   **Linux/macOS**: Install via package manager (e.g., `sudo apt install protobuf-compiler` or `brew install protobuf`).

## Build Instructions

1.  **Clone the repository** (if you haven't already):
    ```bash
    git clone <your-repo-url>
    cd rust-norn
    ```

2.  **Build the project**:
    ```bash
    cargo build --release
    ```
    This will compile all crates in the workspace and produce the `norn` executable in `target/release/`.

## Running the Node

### Single Node Setup

1.  **Create a configuration file** (`config.toml`):
    ```toml
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
    ```

2.  **Start the node**:
    ```bash
    ./target/release/norn --config config.toml
    ```
    The node will start listening for P2P connections on port 4001 and expose its gRPC API on port 50051.

### Multi-Node Setup (Example: 2 Nodes)

1.  **Create configuration files for each node**:

    **`node1_config.toml`**:
    ```toml
    data_dir = "node1_data"
    rpc_address = "127.0.0.1:50051"

    [core]
        [core.consensus]
        pub_key = "020000000000000000000000000000000000000000000000000000000000000001"
        prv_key = "0000000000000000000000000000000000000000000000000000000000000001"

    [network]
    listen_address = "/ip4/0.0.0.0/tcp/4001"
    bootstrap_peers = []
    mdns = true
    ```

    **`node2_config.toml`**:
    ```toml
    data_dir = "node2_data"
    rpc_address = "127.0.0.1:50052"

    [core]
        [core.consensus]
        pub_key = "020000000000000000000000000000000000000000000000000000000000000001"
        prv_key = "0000000000000000000000000000000000000000000000000000000000000001"

    [network]
    listen_address = "/ip4/0.0.0.0/tcp/4002"
    bootstrap_peers = []
    mdns = true
    ```

2.  **Start both nodes in separate terminals**:
    ```bash
    # Terminal 1
    ./target/release/norn --config node1_config.toml

    # Terminal 2
    ./target/release/norn --config node2_config.toml
    ```

    With mDNS enabled, the nodes should automatically discover each other.

## gRPC API Interaction

Once the nodes are running, you can interact with their gRPC API. The `BlockchainService` provides the following methods:

### Available Methods

*   `GetBlockByHash`: Retrieve a block by its hash
*   `GetBlockNumber`: Get the current block height
*   `GetTransactionByBlockHashAndIndex`: Get a transaction from a block
*   `GetTransactionByBlockNumberAndIndex`: Get a transaction from a block by height
*   `SendTransaction`: Submit a new transaction
*   `SendTransactionWithData`: Submit a transaction with data

### Testing with Python

Here's a simple Python script to test the gRPC connection:

```python
import socket

def test_rpc_connection(host, port, node_name):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.connect((host, port))
        print(f"{node_name} RPC server is reachable on {host}:{port}")
    except:
        print(f"{node_name} RPC server is NOT reachable on {host}:{port}")
    finally:
        s.close()

# Test connections to both nodes
test_rpc_connection("127.0.0.1", 50051, "Node1")
test_rpc_connection("127.0.0.1", 50052, "Node2")
```

## Configuration Details

### Network Configuration

*   **`listen_address`**: P2P network listening address in libp2p format
*   **`bootstrap_peers`**: List of peer addresses to connect to on startup
*   **`mdns`**: Enable/disable local peer discovery via mDNS

### RPC Configuration

*   **`rpc_address`**: gRPC server listening address (e.g., "127.0.0.1:50051")

### Storage Configuration

*   **`data_dir`**: Directory for storing blockchain data and node keys

## Development Status

### ✅ Implemented Features

*   Basic project structure and build system
*   Cryptographic primitives (ECDSA, VRF, VDF calculator)
*   Database layer using SledDB
*   P2P networking with libp2p (TCP transport, mDNS discovery)
*   gRPC API with full protobuf definitions
*   Transaction pool and basic validation
*   Node orchestration and startup
*   Multi-node deployment capability

### ⚠️ Partially Implemented

*   PoVF consensus mechanism (VDF calculator exists but needs completion)
*   Blockchain state transitions
*   Full block validation logic
*   Network synchronization

### ❌ Not Yet Implemented

*   Complete consensus algorithm implementation
*   Smart contract support
*   Advanced network security features
*   Production-ready monitoring and metrics

## Testing

The project includes several test modules:

*   **Unit tests**: In each crate
*   **Database tests**: `db_test` module
*   **Integration tests**: `test_integration` module
*   **Scalability tests**: `scalability_test` module

Run all tests with:
```bash
cargo test --workspace
```

## Troubleshooting

### Build Issues

*   **protoc not found**: Ensure Protocol Buffers compiler is installed and in your PATH
*   **LLVM/Clang errors**: Install LLVM and set `LIBCLANG_PATH` environment variable on Windows

### Runtime Issues

*   **Port conflicts**: Ensure configured ports are not in use
*   **Permission denied**: Check file permissions for the data directory
*   **Peer discovery failure**: Verify mDNS is enabled and firewall settings allow local network traffic

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the LICENSE file for details.