# go-norn-rs Context for Gemini

This `GEMINI.md` file provides context about the `go-norn-rs` project to assist in future interactions.

## Project Overview

`go-norn-rs` is a Rust implementation of the `go-norn` blockchain node. It is designed to be a robust, performant, and type-safe blockchain node featuring a Proof of Verifiable Function (PoVF) consensus mechanism. The project is organized as a Cargo workspace containing multiple crates.

### Core Architecture

The project follows a modular architecture where functionality is split across several crates in the `crates/` directory:

*   **`bin/norn`**: The entry point application (CLI). Orchestrates initialization and startup.
*   **`norn-core`**: The central logic. Contains the blockchain ledger, transaction pool (`txpool`), block buffer, and consensus logic.
*   **`norn-network`**: Handles P2P networking using `libp2p`. Manages peer discovery (`mdns`, `gossipsub`) and transport.
*   **`norn-storage`**: Persistence layer using `rocksdb`.
*   **`norn-rpc`**: gRPC interface using `tonic` for external interaction (e.g., submitting transactions).
*   **`norn-crypto`**: Cryptographic primitives including ECDSA (P-256) and VRF implementations.
*   **`norn-common`**: Shared types, traits, and utilities used across the workspace.
*   **`norn-node`**: High-level service that ties all components (network, core, rpc) together.

## Key Technologies & Dependencies

*   **Async Runtime**: `tokio`
*   **Networking**: `libp2p` (Gossipsub, Kademlia, Noise, Yamux)
*   **RPC**: `tonic` (gRPC implementation via `prost`)
*   **Storage**: `rocksdb`
*   **Cryptography**: `k256`, `p256`, `schnorrkel` (VRF)
*   **Logging**: `tracing`

## Building and Running

### Prerequisites

*   **Rust Toolchain**: Stable (2021 edition).
*   **LLVM/Clang**: Required for building `rocksdb`.
*   **Protoc**: Protocol buffers compiler (required for `tonic`).

### Build Commands

```bash
# Build the release binary
cargo build --release

# Run tests
cargo test --workspace
```

### Running the Node

The binary is located at `target/release/norn`.

1.  **Generate a Keypair**:
    ```bash
    ./target/release/norn generate-key --out data/node.key
    ```

2.  **Configuration**:
    Ensure a `config.toml` exists (refer to `README.md` for the template).

3.  **Start**:
    ```bash
    ./target/release/norn --config config.toml
    ```

## Development Conventions

*   **Workspace**: All crates are part of a single workspace defined in the root `Cargo.toml`.
*   **Error Handling**: Uses `anyhow` for applications and `thiserror` for library crates.
*   **Async/Await**: Heavy usage of `async` with `tokio`.
*   **Tracing**: Use `tracing::info!`, `warn!`, `error!` etc. for logging.
