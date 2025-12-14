.PHONY: help build test clean check fmt clippy coverage docker-build docker-run

# Default target
help:
	@echo "Available commands:"
	@echo "  build        - Build the project in release mode"
	@echo "  test         - Run all tests"
	@echo "  check        - Run cargo check"
	@echo "  fmt          - Format code"
	@echo "  clippy       - Run clippy lints"
	@echo "  coverage     - Generate test coverage report"
	@echo "  clean        - Clean build artifacts"
	@echo "  docker-build - Build Docker image"
	@echo "  docker-run   - Run Docker containers"
	@echo "  doc          - Generate documentation"

# Build the project
build:
	cargo build --release --workspace

# Run tests
test:
	cargo test --workspace

# Run tests with coverage
test-coverage:
	cargo llvm-cov --workspace --lcov --output-path lcov.info

# Check code without building
check:
	cargo check --workspace

# Format code
fmt:
	cargo fmt --all

# Run clippy lints
clippy:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

# Generate coverage report
coverage:
	cargo llvm-cov --workspace --html --output-dir target/coverage

# Clean build artifacts
clean:
	cargo clean

# Build Docker image
docker-build:
	docker build -t rust-norn:latest .

# Run with docker-compose
docker-run:
	docker-compose up -d

# Stop docker-compose
docker-stop:
	docker-compose down

# Generate documentation
doc:
	cargo doc --workspace --no-deps --open

# Install development dependencies
setup:
	rustup component add rustfmt clippy
	cargo install cargo-llvm-cov
	cargo install cargo-audit

# Run security audit
audit:
	cargo audit

# Run benchmarks
bench:
	cargo bench --workspace

# Check for outdated dependencies
outdated:
	cargo install cargo-outdated
	cargo outdated

# Update dependencies
update:
	cargo update

# Create release
release: check test
	@echo "Creating release..."
	cargo build --release --workspace
	@echo "Release built successfully!"

# Development workflow
dev: fmt check test
	@echo "Development workflow completed!"

# Full CI workflow
ci: fmt clippy test
	@echo "CI workflow completed!"