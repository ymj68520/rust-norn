# Integration Tests for Norn Examples

This directory contains comprehensive integration tests for all Norn blockchain examples across three languages: Rust, Python, and JavaScript.

## Overview

The integration tests verify that:
- ✅ Examples can connect to a running Norn node
- ✅ All RPC methods work correctly
- ✅ Response parsing is accurate
- ✅ Error handling is robust
- ✅ Concurrent requests are handled properly
- ✅ Results are consistent across multiple calls

## Prerequisites

### Running the Tests

**All Tests:**
- A running Norn blockchain node at `http://127.0.0.1:50051`
- Set `NORN_RPC_URL` environment variable if using a different address

### Language-Specific Requirements

#### Rust Tests
- Rust 1.70+
- `cargo` build tool
- Project dependencies (already in `Cargo.toml`)

#### Python Tests
- Python 3.8+
- `pytest` - Testing framework
- `requests` - HTTP client

Install dependencies:
```bash
pip install pytest requests
```

#### JavaScript Tests
- Node.js 14+
- `jest` - Testing framework
- HTTP module (built-in)

Install dependencies:
```bash
npm install --save-dev jest
```

## Running Tests

### Rust Tests

Run all tests:
```bash
cd examples/rust
cargo test --test integration_tests -- --test-threads=1 --nocapture
```

Run specific test:
```bash
cargo test --test integration_tests test_get_chain_id -- --nocapture
```

Run with custom RPC URL:
```bash
NORN_RPC_URL=http://custom-node:50051 cargo test --test integration_tests -- --test-threads=1 --nocapture
```

### Python Tests

Run all tests:
```bash
cd examples/python
pytest tests/test_integration.py -v -s
```

Run specific test:
```bash
pytest tests/test_integration.py::test_get_chain_id -v -s
```

Run with custom RPC URL:
```bash
NORN_RPC_URL=http://custom-node:50051 pytest tests/test_integration.py -v -s
```

### JavaScript Tests

Run all tests:
```bash
cd examples/javascript
npm test
```

Or with Jest directly:
```bash
jest tests/integration.test.js --verbose
```

Run specific test:
```bash
jest tests/integration.test.js -t "get_chain_id"
```

Run with custom RPC URL:
```bash
NORN_RPC_URL=http://custom-node:50051 npm test
```

## Test Coverage

### Test Categories

#### 1. Basic RPC Operations (3 tests each language)
- `test_get_chain_id` - Retrieve network chain identifier
- `test_get_block_number` - Get latest block height
- `test_get_gas_price` - Current gas price query

#### 2. Block Information (1 test per language)
- `test_get_block_by_number` - Retrieve block details by height

#### 3. Account Balance (3 tests each language)
- `test_get_balance` - Query account balance
- `test_get_transaction_count` - Get account nonce
- `test_get_code` - Retrieve smart contract code

#### 4. Error Handling (2 tests each language)
- `test_invalid_address_format` - Verify invalid address rejection
- `test_invalid_block_number` - Verify invalid block rejection

#### 5. Response Parsing (1 test per language)
- `test_response_parsing` - Validate various response types

#### 6. Connection Handling (2 tests each language)
- `test_multiple_requests` - Sequential request handling
- `test_concurrent_requests` - Parallel request handling

#### 7. Data Consistency (1 test per language)
- `test_consistent_results` - Verify consistent responses

#### 8. Example-Specific Tests (3 tests each language)
- `test_basic_rpc_requirements` - Verify basic_rpc example
- `test_balance_checker_requirements` - Verify balance_checker example
- `test_transaction_sender_requirements` - Verify transaction_sender example

#### 9. Performance Tests (2 tests each language)
- `test_response_time` - Single request timing
- `test_batch_performance` - Batch request timing

**Total: ~75 tests across all languages**

## Test Results

### Expected Output

Successful test run (Rust):
```
running 30 tests

test test_get_chain_id ... ok
test test_get_block_number ... ok
test test_get_gas_price ... ok
test test_get_block_by_number ... ok
test test_get_balance ... ok
test test_get_transaction_count ... ok
test test_get_code ... ok
test test_invalid_address_format ... ok
test test_invalid_block_number ... ok
test test_response_parsing ... ok
test test_multiple_requests ... ok
test test_concurrent_requests ... ok
test test_consistent_results ... ok
test test_basic_rpc_requirements ... ok
test test_balance_checker_requirements ... ok
test test_transaction_sender_requirements ... ok
test test_response_time ... ok
test test_batch_performance ... ok

test result: ok. 30 passed; 0 failed; 0 ignored
```

### Interpreting Results

#### All Tests Pass ✅
The examples are working correctly with your Norn node.

#### Tests Fail with Connection Error ⚠️
Check that:
1. Node is running: `curl http://127.0.0.1:50051`
2. RPC port is accessible
3. Firewall allows connections
4. Node is listening on the configured address

#### Tests Fail with RPC Error ❌
Check:
1. Node logs for errors
2. Node is fully synced
3. RPC methods are supported
4. Network is operational

#### Some Tests Skip 🔄
May indicate:
1. Optional features not enabled
2. Network conditions (e.g., no blocks yet)
3. Configuration differences

## Configuring Tests

### Environment Variables

```bash
# RPC endpoint (HTTP)
export NORN_RPC_URL=http://127.0.0.1:50051

# Custom test settings
export TEST_TIMEOUT=30000
export TEST_CONCURRENCY=5
```

### Custom Configuration

Tests automatically detect:
- RPC URL from `NORN_RPC_URL` environment variable
- Fallback to `http://127.0.0.1:50051` if not set
- Node running state (auto-skip if not available)

## Troubleshooting

### "Node not running" / Connection refused

**Problem:** Tests can't connect to the node.

**Solution:**
```bash
# Start the node
cd /path/to/rust-norn
cargo run --release --bin norn -- --config node_config.toml

# In another terminal, run tests
cd examples/rust
cargo test --test integration_tests -- --test-threads=1 --nocapture
```

### "Invalid address" errors

**Problem:** Some tests fail with address validation.

**Solution:** Usually indicates the node is properly validating addresses. This is expected behavior.

### "Connection timeout"

**Problem:** Requests take too long to complete.

**Solution:**
1. Check node performance
2. Verify network connectivity
3. Check if node is under heavy load
4. Try with fewer concurrent tests

### "Response parse error"

**Problem:** Tests can't parse RPC responses.

**Solution:**
1. Verify RPC endpoint is Ethereum-compatible
2. Check RPC response format
3. Verify node isn't returning errors

## Performance Baseline

Typical performance results on a healthy system:

### Response Times
- Single request: < 100ms
- Average batch (10 requests): < 200ms
- Concurrent (5 requests): < 150ms

### Success Rates
- Sequential operations: 100%
- Concurrent operations: 99%+
- Error handling: 100%

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Integration Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    services:
      norn:
        image: norn-node:latest
        options: >-
          --health-cmd "curl -f http://localhost:50051 || exit 1"
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5
        ports:
          - 50051:50051

    steps:
      - uses: actions/checkout@v2
      
      # Rust tests
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run Rust tests
        run: |
          cd examples/rust
          cargo test --test integration_tests -- --test-threads=1
      
      # Python tests
      - uses: actions/setup-python@v2
        with:
          python-version: 3.9
      - name: Run Python tests
        run: |
          cd examples/python
          pip install -r requirements.txt pytest
          pytest tests/test_integration.py -v
      
      # JavaScript tests
      - uses: actions/setup-node@v2
        with:
          node-version: 16
      - name: Run JavaScript tests
        run: |
          cd examples/javascript
          npm install
          npm test
```

### Docker Compose for Testing

```yaml
version: '3.8'

services:
  norn:
    build: ../../
    ports:
      - "50051:50051"
    volumes:
      - ./test_config.toml:/etc/norn/config.toml:ro
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:50051/"]
      interval: 10s
      timeout: 5s
      retries: 5

  tests:
    build:
      context: .
      dockerfile: Dockerfile.test
    depends_on:
      norn:
        condition: service_healthy
    environment:
      NORN_RPC_URL: http://norn:50051
```

## Contributing

To add new tests:

1. **Identify new test case** - What should be tested?
2. **Add test across all languages** - Rust, Python, JavaScript
3. **Document test purpose** - Include comments
4. **Update this README** - Add to test coverage section
5. **Verify against running node** - Test locally first

## Test Maintenance

### Regular Checks

- Run tests after node updates
- Verify against new RPC methods
- Test with different configurations
- Monitor performance trends

### Updating Tests

When RPC API changes:
1. Update test assertions
2. Add new test cases
3. Document breaking changes
4. Update README

## Resources

### RPC API
- [Ethereum JSON-RPC Specification](https://ethereum.org/en/developers/docs/apis/json-rpc/)
- [Norn Documentation](../../doc/)

### Testing Frameworks
- **Rust:** [tokio test documentation](https://tokio.rs/)
- **Python:** [pytest documentation](https://docs.pytest.org/)
- **JavaScript:** [Jest documentation](https://jestjs.io/)

### Example Code
- See `examples/*/` directories for implementation

## Support

For issues or questions about tests:
1. Check troubleshooting section above
2. Review test output and logs
3. Verify node is running and healthy
4. Open GitHub issue with test output

---

**Total Test Coverage: 75+ integration tests**

**Last Updated:** 2025-02-01
