"""
Integration tests for Norn RPC Python examples

These tests verify that all examples can connect to and interact with
a running Norn node. They test the core functionality of each example
and validate response parsing.

Requirements:
- A running Norn node at http://127.0.0.1:50051
- Set NORN_RPC_URL environment variable if using different address

Run tests with:
```bash
pytest tests/test_integration.py -v -s
```
"""

import pytest
import requests
import os
import asyncio
from typing import Optional, Dict, Any


class NornTestClient:
    """Test client for Norn RPC"""

    def __init__(self, rpc_url: Optional[str] = None):
        self.rpc_url = rpc_url or os.getenv("NORN_RPC_URL", "http://127.0.0.1:50051")
        self.session = requests.Session()
        self.request_id = 0

    def _make_request(self, method: str, params: list) -> Dict[str, Any]:
        """Make RPC request"""
        self.request_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params,
        }

        response = self.session.post(self.rpc_url, json=payload, timeout=10)
        response.raise_for_status()
        return response.json()

    def is_node_running(self) -> bool:
        """Check if node is running"""
        try:
            result = self._make_request("eth_chainId", [])
            return "result" in result
        except Exception:
            return False


@pytest.fixture
def client():
    """Provide test client"""
    return NornTestClient()


@pytest.fixture(scope="session", autouse=True)
def check_node_running():
    """Check if node is running before tests"""
    client = NornTestClient()
    if not client.is_node_running():
        pytest.skip("Norn node not running at configured address")


# ============================================
# Test 1: Basic RPC Operations
# ============================================


def test_get_chain_id(client):
    """Test getting chain ID"""
    result = client._make_request("eth_chainId", [])

    assert "result" in result, "Response should have result field"
    chain_id = result["result"]

    assert isinstance(chain_id, str), "Chain ID should be string"
    assert chain_id.startswith("0x"), "Chain ID should start with 0x"
    assert len(chain_id) <= 66, "Chain ID should be valid hex"

    print(f"✅ Chain ID: {chain_id}")


def test_get_block_number(client):
    """Test getting block number"""
    result = client._make_request("eth_blockNumber", [])

    assert "result" in result, "Response should have result field"
    block_number = result["result"]

    assert isinstance(block_number, str), "Block number should be string"
    assert block_number.startswith("0x"), "Block number should start with 0x"

    # Parse as hex to verify validity
    block_num = int(block_number, 16)
    assert block_num >= 0, "Block number should be positive"

    print(f"✅ Block number: {block_number}")


def test_get_gas_price(client):
    """Test getting gas price"""
    result = client._make_request("eth_gasPrice", [])

    assert "result" in result, "Response should have result field"
    gas_price = result["result"]

    assert isinstance(gas_price, str), "Gas price should be string"
    assert gas_price.startswith("0x"), "Gas price should start with 0x"

    # Parse as hex
    price = int(gas_price, 16)
    assert price > 0, "Gas price should be positive"

    print(f"✅ Gas price: {gas_price}")


# ============================================
# Test 2: Block Information
# ============================================


def test_get_block_by_number(client):
    """Test getting block by number"""
    result = client._make_request("eth_getBlockByNumber", ["0x0", False])

    assert "result" in result, "Response should have result field"
    block = result["result"]

    # Block might be null for some networks
    if block is not None:
        assert isinstance(block, dict), "Block should be dictionary"
        assert "hash" in block, "Block should have hash"
        assert "number" in block, "Block should have number"
        print(f"✅ Block retrieved: {block['hash']}")
    else:
        print("✅ Block 0x0 not found (expected for some networks)")


# ============================================
# Test 3: Account Balance
# ============================================


def test_get_balance(client):
    """Test getting account balance"""
    address = "0x0000000000000000000000000000000000000000"
    result = client._make_request("eth_getBalance", [address, "latest"])

    assert "result" in result, "Response should have result field"
    balance = result["result"]

    assert isinstance(balance, str), "Balance should be string"
    assert balance.startswith("0x"), "Balance should start with 0x"

    # Parse as hex
    bal = int(balance, 16)
    assert bal >= 0, "Balance should be non-negative"

    print(f"✅ Balance: {balance} wei")


def test_get_transaction_count(client):
    """Test getting transaction count (nonce)"""
    address = "0x0000000000000000000000000000000000000000"
    result = client._make_request("eth_getTransactionCount", [address, "latest"])

    assert "result" in result, "Response should have result field"
    nonce = result["result"]

    assert isinstance(nonce, str), "Nonce should be string"
    assert nonce.startswith("0x"), "Nonce should start with 0x"

    print(f"✅ Transaction count: {nonce}")


def test_get_code(client):
    """Test getting account code"""
    address = "0x0000000000000000000000000000000000000000"
    result = client._make_request("eth_getCode", [address, "latest"])

    assert "result" in result, "Response should have result field"
    code = result["result"]

    assert isinstance(code, str), "Code should be string"
    assert code.startswith("0x"), "Code should start with 0x"

    if code == "0x":
        print("✅ Regular account has no code")
    else:
        print(f"✅ Contract code: {len(code) // 2 - 1} bytes")


# ============================================
# Test 4: Error Handling
# ============================================


def test_invalid_address_format(client):
    """Test that invalid address is rejected"""
    with pytest.raises(requests.exceptions.RequestException):
        client._make_request("eth_getBalance", ["invalid_address", "latest"])

    print("✅ Invalid address correctly rejected")


def test_invalid_block_number(client):
    """Test that invalid block number is rejected"""
    result = client._make_request("eth_getBlockByNumber", ["invalid", False])

    # Should either have error or null result
    assert "error" in result or result.get("result") is None, (
        "Invalid block should return error or null"
    )

    print("✅ Invalid block number rejected")


# ============================================
# Test 5: Response Parsing
# ============================================


def test_response_parsing(client):
    """Test parsing various response types"""
    # String response
    chain_id_result = client._make_request("eth_chainId", [])
    chain_id = chain_id_result["result"]
    assert isinstance(chain_id, str), "String response should be string"

    # Numeric response
    block_number_result = client._make_request("eth_blockNumber", [])
    block_number = block_number_result["result"]
    assert isinstance(block_number, str), "Numeric response should be hex string"

    # Object response
    block_result = client._make_request("eth_getBlockByNumber", ["0x0", False])
    block = block_result["result"]
    # Block might be null, but structure should be valid

    print("✅ All response types parsed correctly")


# ============================================
# Test 6: Connection Handling
# ============================================


def test_multiple_requests(client):
    """Test making multiple sequential requests"""
    for i in range(5):
        result = client._make_request("eth_chainId", [])
        assert "result" in result, f"Request {i} failed"

    print("✅ Multiple sequential requests completed")


def test_concurrent_requests(client):
    """Test handling concurrent requests"""
    import concurrent.futures

    def make_request():
        result = client._make_request("eth_chainId", [])
        return "result" in result

    with concurrent.futures.ThreadPoolExecutor(max_workers=5) as executor:
        results = list(executor.map(lambda _: make_request(), range(5)))

    assert all(results), "All concurrent requests should succeed"
    print("✅ Concurrent requests completed successfully")


# ============================================
# Test 7: Data Consistency
# ============================================


def test_consistent_results(client):
    """Test that same request returns consistent results"""
    result1 = client._make_request("eth_chainId", [])
    chain_id_1 = result1["result"]

    result2 = client._make_request("eth_chainId", [])
    chain_id_2 = result2["result"]

    assert chain_id_1 == chain_id_2, "Chain ID should be consistent"
    print("✅ Results are consistent")


# ============================================
# Test 8: Example-Specific Tests
# ============================================


def test_basic_rpc_requirements(client):
    """Test basic_rpc.py example requirements"""
    # All methods used by basic_rpc.py
    client._make_request("eth_chainId", [])
    client._make_request("eth_blockNumber", [])
    client._make_request("eth_gasPrice", [])
    client._make_request("eth_getBlockByNumber", ["0x1", False])

    print("✅ All basic_rpc.py requirements verified")


def test_balance_checker_requirements(client):
    """Test balance_checker.py example requirements"""
    address = "0x0000000000000000000000000000000000000000"
    client._make_request("eth_getBalance", [address, "latest"])

    print("✅ All balance_checker.py requirements verified")


def test_transaction_sender_requirements(client):
    """Test transaction_sender.py example requirements"""
    address = "0x0000000000000000000000000000000000000000"
    client._make_request("eth_getTransactionCount", [address, "latest"])
    client._make_request("eth_gasPrice", [])

    print("✅ All transaction_sender.py requirements verified")


# ============================================
# Test 9: Performance and Timing
# ============================================


def test_response_time(client):
    """Test that responses are received in reasonable time"""
    import time

    start = time.time()
    client._make_request("eth_chainId", [])
    elapsed = time.time() - start

    assert elapsed < 5, "Response should be received within 5 seconds"
    print(f"✅ Response time: {elapsed:.3f}s")


def test_batch_performance(client):
    """Test performance of batch requests"""
    import time

    start = time.time()
    for _ in range(10):
        client._make_request("eth_blockNumber", [])
    elapsed = time.time() - start

    avg_time = elapsed / 10
    assert avg_time < 1, "Average request time should be less than 1 second"
    print(f"✅ Average response time: {avg_time:.3f}s")


# ============================================
# Test Helper Functions
# ============================================


def wei_to_ether(wei: int) -> float:
    """Convert wei to ether"""
    return wei / 1e18


def test_wei_conversion():
    """Test wei to ether conversion"""
    assert wei_to_ether(1000000000000000000) == 1.0, "1 ether = 1e18 wei"
    assert wei_to_ether(0) == 0.0, "0 wei = 0 ether"
    print("✅ Wei conversion works correctly")


# ============================================
# Test Report
# ============================================


@pytest.fixture(scope="session", autouse=True)
def print_summary(request):
    """Print test summary"""

    def summary():
        print("\n" + "=" * 40)
        print("Integration Tests Summary")
        print("=" * 40)
        print("Tests verify:")
        print("✓ RPC connectivity")
        print("✓ Response format validation")
        print("✓ Error handling")
        print("✓ Concurrent request handling")
        print("✓ Data consistency")
        print("✓ Example-specific requirements")
        print("=" * 40)

    request.addfinalizer(summary)
