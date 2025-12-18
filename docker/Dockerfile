# Multi-stage build for rust-norn multi-node deployment
FROM rust:latest AS builder

# Install protobuf compiler
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy all project files needed for build
COPY Cargo.toml Cargo.lock ./
COPY bin/ ./bin/
COPY crates/ ./crates/

# Copy test directories that are workspace members (even if empty stubs)
# to satisfy workspace requirements
COPY test_integration/ ./test_integration/
COPY db_test/ ./db_test/
COPY scalability_test/ ./scalability_test/

# Build the application (release mode)
RUN cargo build --release --bin norn

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -r -s /bin/false norn

# Copy the binary from builder stage
COPY --from=builder /app/target/release/norn /usr/local/bin/norn

# Copy entrypoint script
COPY docker/scripts/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Create data and config directories
RUN mkdir -p /data /etc/norn && chown -R norn:norn /data /etc/norn

# Switch to non-root user
USER norn

# Set environment variables
ENV RUST_LOG=info
ENV NODE_NAME=norn-node
ENV RPC_ADDRESS=0.0.0.0:50051
ENV P2P_PORT=4001
ENV BOOTSTRAP_PEERS=""
ENV WAIT_FOR_NODE=""

# Expose ports (P2P and RPC)
EXPOSE 4001 50051

# Set working directory
WORKDIR /data

# Health check - check if RPC port is listening
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD nc -z localhost ${RPC_PORT:-50051} || exit 1

# Run the application via entrypoint
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
