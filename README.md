# go-norn-rs: A Norn Blockchain Node in Rust

This repository contains a Rust port of the `go-norn` blockchain node, aiming to replicate its Proof of Verifiable Function (PoVF) consensus mechanism and core functionalities in a robust, performant, and idiomatic Rust environment.

## Project Overview

`go-norn-rs` is a blockchain node re-implemented in Rust. It features a modular architecture, leveraging Rust's type safety and concurrency primitives. Key components include:

*   **`norn-common`**: Shared data structures, types, and utility functions.
*   **`norn-crypto`**: Cryptographic primitives, including custom P-256 VRF and ECDSA.
*   **`norn-storage`**: Persistent key-value store using RocksDB.
*   **`norn-core`**: The heart of the blockchain, managing the ledger, transaction pool, and block buffer.
*   **`norn-network`**: P2P communication layer built on `rust-libp2p` for peer discovery and message propagation.
*   **`norn-rpc`**: gRPC server for external API interactions.
*   **`norn-node`**: Orchestrates all services, bringing up the full node.
*   **`bin/norn`**: The main executable for running the node.

## Prerequisites

*   **Rust Toolchain**: Rust Edition 2021 (stable recommended). Install via `rustup`: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
*   **LLVM/Clang (for Windows/macOS/Linux users)**: Some dependencies (like `rocksdb` through `zstd-sys`) require `libclang` for their build scripts.
    *   **Windows**: Install LLVM from [llvm.org](https://llvm.org/builds/). Ensure `PATH` is set correctly, and you might need to set `LIBCLANG_PATH` environment variable to the `bin` directory of your LLVM installation (e.g., `C:\ Program Files\LLVM\bin`).
    *   **Linux/macOS**: Usually available via package manager (e.g., `sudo apt install libclang-dev` on Debian/Ubuntu, `brew install llvm` on macOS).
*   **`grpcurl` (Optional, for sending transactions)**: A command-line tool for interacting with gRPC servers. Install from [github.com/fullstorydev/grpcurl](https://github.com/fullstorydev/grpcurl).

## Build Instructions

1.  **Clone the repository** (if you haven't already):
    ```bash
    git clone <your-repo-url>
    cd go-norn-rs
    ```
2.  **Build the project**:
    ```bash
    cargo build --release
    ```
    This will compile all crates in the workspace and produce the `norn` executable in `target/release/`.

## Running the Node

You can start the node using the generated executable. A `config.toml` is expected in the current working directory or specified via `--config`.

1.  **Generate a default `config.toml` (example):**
    You'll need a `config.toml`. An example might look like this (create this file in the `go-norn-rs` root):
    ```toml
    [core]
    # Core related configurations
    
    [network]
    listen_address = "/ip4/0.0.0.0/tcp/0"
    bootstrap_peers = []
    mdns = true
    
    [rpc_address]
    ip = "127.0.0.1"
    port = 50051
    
    [data_dir]
    path = "data" # Where node data (DB, keypair) will be stored
    ```
    _Note_: The `CoreConfig` structure will also require details for `consensus.pub_key` and `consensus.prv_key` in a real scenario. For initial testing, you might need to fill these or generate a dummy.

2.  **Generate a keypair (optional, if you don't have one):**
    ```bash
    ./target/release/norn generate-key --out data/node.key
    ```
    This will create `data/node.key` (or your specified path). The `norn` executable will automatically load or generate a keypair in your `data_dir` if it doesn't exist.

3.  **Start the Norn Node**:
    ```bash
    ./target/release/norn --config config.toml
    ```
    The node will start listening for P2P connections and expose its gRPC API.

## Sending a Transaction (via gRPC)

Once the node is running, you can interact with its gRPC API to send transactions. We'll use `grpcurl` for this example.

The `norn-rpc` crate defines a `BlockchainService` with methods like `SendTransaction`.

### Example: Sending a "Set" Data Transaction

A data transaction typically has a `type` (e.g., "set", "append"), a `receiver` address, a `key`, and a `value`.

1.  **Ensure Node is Running**: Start your Norn node as described above.

2.  **Send Transaction using `grpcurl`**:

    ```bash
    grpcurl -plaintext -d '{ "type": "set", "receiver": "0x123...", "key": "my_data_key", "value": "{\"test\": \"value\"}" }' \
      127.0.0.1:50051 blockchain.BlockchainService/SendTransaction
    ```
    *   `-plaintext`: Connects without TLS.
    *   `-d '{...}'`: Provides the JSON payload for the `SendTransactionReq` message.
        *   `type`: "set" or "append"
        *   `receiver`: The target address (hex string).
        *   `key`: The key for the data.
        *   `value`: The data itself (as a JSON string).
    *   `127.00.1:50051`: The gRPC server address (as defined in your `config.toml`).
    *   `blockchain.BlockchainService/SendTransaction`: The gRPC service and method to call.

    **Example Response**:
    ```json
    {
      "txHash": "a1b2c3d4e5f6..."
    }
    ```
    (The `txHash` will be a placeholder until signing logic is fully implemented and returned).

This documentation provides a basic guide to get started with your Rust-ported `go-norn` node.
