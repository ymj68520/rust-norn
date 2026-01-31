"""
Batch RPC Requests Example

This example demonstrates how to efficiently batch multiple RPC calls
into a single request, which is much faster than making separate requests.

Benefits of batching:
- Single HTTP round trip instead of multiple
- Better performance for sequential operations
- Atomicity for reading state at the same block height
- Reduced latency in network calls

Use cases:
- Getting balances for multiple addresses
- Fetching multiple blocks' data
- Reading multiple contract states
- Pre-flight checks before transaction submission
"""

import os
import asyncio
import aiohttp
from dotenv import load_dotenv
from typing import List, Dict, Any, Tuple
import json

load_dotenv()


class BatchRpcClient:
    """Client for making batch RPC requests"""

    def __init__(self, rpc_url: str):
        self.rpc_url = rpc_url

    async def batch_request(
        self, requests: List[Dict[str, Any]]
    ) -> List[Dict[str, Any]]:
        """
        Execute a batch of RPC requests.
        Returns results in the same order as the requests.
        """
        async with aiohttp.ClientSession() as session:
            async with session.post(self.rpc_url, json=requests) as response:
                results = await response.json()
        return results

    async def get_balances_batch(self, addresses: List[str]) -> List[Tuple[str, str]]:
        """
        Batch request to get balances for multiple addresses.
        """
        requests = []
        for index, address in enumerate(addresses):
            requests.append(
                {
                    "jsonrpc": "2.0",
                    "method": "eth_getBalance",
                    "params": [address, "latest"],
                    "id": index + 1,
                }
            )

        responses = await self.batch_request(requests)

        balances = []
        for index, address in enumerate(addresses):
            if "result" in responses[index]:
                balance = responses[index]["result"]
                balances.append((address, balance))

        return balances

    async def get_blocks_batch(self, block_numbers: List[str]) -> List[Dict[str, Any]]:
        """
        Batch request to get block details for multiple block numbers.
        """
        requests = []
        for index, block_num in enumerate(block_numbers):
            requests.append(
                {
                    "jsonrpc": "2.0",
                    "method": "eth_getBlockByNumber",
                    "params": [block_num, False],
                    "id": index + 1,
                }
            )

        responses = await self.batch_request(requests)

        blocks = []
        for response in responses:
            if "result" in response:
                blocks.append(response["result"])

        return blocks

    async def get_transactions_batch(
        self, tx_hashes: List[str]
    ) -> List[Dict[str, Any]]:
        """
        Batch request to get transaction details for multiple hashes.
        """
        requests = []
        for index, tx_hash in enumerate(tx_hashes):
            requests.append(
                {
                    "jsonrpc": "2.0",
                    "method": "eth_getTransactionByHash",
                    "params": [tx_hash],
                    "id": index + 1,
                }
            )

        responses = await self.batch_request(requests)

        transactions = []
        for response in responses:
            if "result" in response:
                transactions.append(response["result"])

        return transactions

    async def get_storage_batch(
        self, contract_address: str, positions: List[str]
    ) -> List[str]:
        """
        Batch request to check multiple storage slots.
        """
        requests = []
        for index, position in enumerate(positions):
            requests.append(
                {
                    "jsonrpc": "2.0",
                    "method": "eth_getStorageAt",
                    "params": [contract_address, position, "latest"],
                    "id": index + 1,
                }
            )

        responses = await self.batch_request(requests)

        storage_values = []
        for response in responses:
            if "result" in response:
                value = response["result"]
                storage_values.append(value)

        return storage_values

    async def mixed_batch_request(
        self,
        chain_id: bool = False,
        gas_price: bool = False,
        block_number: bool = False,
    ) -> List[Dict[str, Any]]:
        """
        Mixed batch request - combines different RPC methods.
        """
        requests = []
        request_id = 1

        if chain_id:
            requests.append(
                {
                    "jsonrpc": "2.0",
                    "method": "eth_chainId",
                    "params": [],
                    "id": request_id,
                }
            )
            request_id += 1

        if gas_price:
            requests.append(
                {
                    "jsonrpc": "2.0",
                    "method": "eth_gasPrice",
                    "params": [],
                    "id": request_id,
                }
            )
            request_id += 1

        if block_number:
            requests.append(
                {
                    "jsonrpc": "2.0",
                    "method": "eth_blockNumber",
                    "params": [],
                    "id": request_id,
                }
            )
            request_id += 1

        responses = await self.batch_request(requests)
        return responses

    @staticmethod
    def hex_to_decimal(hex_str: str) -> int:
        """Convert hex string to decimal"""
        return int(hex_str, 16)

    @staticmethod
    def wei_to_ether(wei_hex: str) -> float:
        """Format wei to ether"""
        wei = int(wei_hex, 16)
        return wei / 1e18


async def main():
    """Main example demonstrating batch RPC requests"""
    rpc_url = os.getenv("RPC_URL", "http://localhost:8545")
    client = BatchRpcClient(rpc_url)

    print("=== Batch RPC Requests Examples ===\n")

    # Example 1: Batch balance queries
    print("1. Batch Balance Queries:")
    print("   Querying balances for multiple addresses in one request...")
    addresses = [
        "0x742d35Cc6634C0532925a3b844Bc9e7595f32D23",
        "0x0000000000000000000000000000000000000000",
        "0x1111111111111111111111111111111111111111",
    ]

    try:
        balances = await client.get_balances_batch(addresses)
        print("   Results:")
        for address, balance in balances:
            balance_decimal = client.hex_to_decimal(balance)
            balance_ether = client.wei_to_ether(balance)
            print(f"   {address} -> {balance_decimal} Wei ({balance_ether} ETH)")
    except Exception as e:
        print(f"   Error: {e}")

    # Example 2: Batch block queries
    print("\n2. Batch Block Queries:")
    print("   Fetching multiple blocks in one request...")
    block_numbers = ["0x1", "0x2", "0x3"]

    try:
        blocks = await client.get_blocks_batch(block_numbers)
        print(f"   Fetched {len(blocks)} blocks successfully")
        for block in blocks:
            if block and "number" in block:
                miner = block.get("miner", "unknown")
                print(f"   Block {block['number']}: miner {miner}")
    except Exception as e:
        print(f"   Error: {e}")

    # Example 3: Batch storage queries
    print("\n3. Batch Storage Queries:")
    print("   Reading multiple storage slots from a contract...")
    contract = "0x0000000000000000000000000000000000000001"
    positions = ["0x0", "0x1", "0x2"]

    try:
        values = await client.get_storage_batch(contract, positions)
        print("   Results from contract storage:")
        for idx, value in enumerate(values):
            print(f"   Position {idx}: {value}")
    except Exception as e:
        print(f"   Error: {e}")

    # Example 4: Mixed batch request
    print("\n4. Mixed Batch Request:")
    print("   Combining different RPC methods in one batch...")

    try:
        results = await client.mixed_batch_request(
            chain_id=True, gas_price=True, block_number=True
        )
        print("   Results:")
        for result in results:
            if "result" in result:
                print(f"   {result['result']}")
    except Exception as e:
        print(f"   Error: {e}")

    # Example 5: Educational information
    print("\n5. Batch Request Performance Benefits:")
    print("   ✓ Single HTTP connection for multiple calls")
    print("   ✓ Results atomic at the same block height")
    print("   ✓ Reduced round-trip latency")
    print("   ✓ Better for reading multiple state snapshots")
    print("   ✓ Can batch up to 100+ requests (depends on node)")

    print("\n6. Batch Request Patterns:")
    print("   Pattern 1: Get state before transaction")
    print("     - Query nonce, gas price, balances all at once")
    print("   Pattern 2: Multi-address monitoring")
    print("     - Check balances/nonces for multiple accounts")
    print("   Pattern 3: Contract audit")
    print("     - Read multiple storage slots at once")
    print("   Pattern 4: Historical data fetching")
    print("     - Get multiple blocks' data in parallel")

    print("\n=== Key Points ===")
    print("✓ Batch requests must be sent as JSON array, not individual objects")
    print("✓ Results are returned in the same order as requests")
    print("✓ Each request in the batch must have unique 'id'")
    print("✓ All requests in a batch are executed at the same block height")
    print("✓ Error in one request doesn't affect others in the batch")


if __name__ == "__main__":
    asyncio.run(main())
