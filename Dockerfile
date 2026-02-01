# Multi-stage build for rust-norn
FROM rust:1.75 AS builder

# Install protobuf compiler
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy Cargo files
COPY Cargo.toml Cargo.lock ./
COPY bin/norn/Cargo.toml ./bin/norn/
COPY crates/ ./crates/

# Build dependencies first to cache them
RUN cargo build --release --bins

# Copy source code
COPY bin/norn/src ./bin/norn/src/
COPY crates/common/src ./crates/common/src/
COPY crates/crypto/src ./crates/crypto/src/
COPY crates/storage/src ./crates/storage/src/
COPY crates/core/src ./crates/core/src/
COPY crates/network/src ./crates/network/src/
COPY crates/rpc/src ./crates/rpc/src/
COPY crates/node/src ./crates/node/src/

# Build the application
RUN cargo build --release --bins

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -r -s /bin/false norn

# Copy the binary from builder stage
COPY --from=builder /app/target/release/norn /usr/local/bin/norn

# Create data directory
RUN mkdir -p /data && chown norn:norn /data

# Switch to non-root user
USER norn

# Expose ports
EXPOSE 4001 50051 8545

# Set working directory
WORKDIR /data

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD norn --version || exit 1

# Run the application
CMD ["norn", "--config", "/etc/norn/config.toml"]