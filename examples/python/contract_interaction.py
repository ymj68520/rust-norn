"""
Smart Contract Interaction Example

This example demonstrates how to interact with smart contracts:
- Encoding contract function calls using ABI
- Reading from contracts (eth_call)
- Calling contract functions (eth_sendRawTransaction)
- Decoding return values
- Handling ERC-20 tokens as a real-world example

In production, use web3.py for automatic ABI encoding/decoding.
This example shows the concepts for educational purposes.
"""

import os
import asyncio
import aiohttp
from dotenv import load_dotenv
from typing import Dict, Any, Optional
import json

load_dotenv()


class ContractClient:
    """Client for interacting with smart contracts via RPC"""

    def __init__(self, rpc_url: str):
        self.rpc_url = rpc_url

    async def call_function(
        self, from_address: str, contract_address: str, data: str
    ) -> str:
        """
        Call a contract function (read-only, no gas cost).
        Uses eth_call for view/pure functions.
        """
        payload = {
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [
                {"from": from_address, "to": contract_address, "data": data},
                "latest",
            ],
            "id": 1,
        }

        async with aiohttp.ClientSession() as session:
            async with session.post(self.rpc_url, json=payload) as response:
                result = await response.json()

        if "result" in result:
            return result["result"]
        elif "error" in result:
            raise Exception(f"RPC Error: {result['error']}")
        else:
            raise Exception("Unexpected response format")

    async def get_erc20_balance(self, token_address: str, account_address: str) -> str:
        """
        Get the balance of an ERC-20 token for an address.
        Encodes: balanceOf(address)
        Selector: 0x70a08231
        """
        # Pad address to 32 bytes
        padded_address = account_address.lower().replace("0x", "").zfill(64)
        data = f"0x70a08231{padded_address}"

        result = await self.call_function(
            "0x0000000000000000000000000000000000000000", token_address, data
        )

        return result

    def encode_erc20_transfer(self, recipient: str, amount_wei: str) -> str:
        """
        Encode a transfer call for an ERC-20 token.
        Returns the encoded data to be used in a transaction.
        """
        # Selector for transfer(address,uint256)
        data = "0xa9059cbb"

        # Encode recipient address (pad to 32 bytes)
        padded_recipient = recipient.lower().replace("0x", "").zfill(64)
        data += padded_recipient

        # Encode amount (pad to 32 bytes)
        amount_int = int(amount_wei)
        amount_hex = hex(amount_int)[2:].zfill(64)
        data += amount_hex

        return data

    async def get_storage_at(self, contract_address: str, position: str) -> str:
        """
        Read contract storage at a specific position.
        Useful for reading state variables directly.
        """
        payload = {
            "jsonrpc": "2.0",
            "method": "eth_getStorageAt",
            "params": [contract_address, position, "latest"],
            "id": 1,
        }

        async with aiohttp.ClientSession() as session:
            async with session.post(self.rpc_url, json=payload) as response:
                result = await response.json()

        if "result" in result:
            return result["result"]
        elif "error" in result:
            raise Exception(f"RPC Error: {result['error']}")
        else:
            raise Exception("Unexpected response format")

    async def get_code(self, contract_address: str) -> str:
        """
        Get the bytecode of a contract.
        Returns '0x' if address is not a contract.
        """
        payload = {
            "jsonrpc": "2.0",
            "method": "eth_getCode",
            "params": [contract_address, "latest"],
            "id": 1,
        }

        async with aiohttp.ClientSession() as session:
            async with session.post(self.rpc_url, json=payload) as response:
                result = await response.json()

        if "result" in result:
            return result["result"]
        elif "error" in result:
            raise Exception(f"RPC Error: {result['error']}")
        else:
            raise Exception("Unexpected response format")

    @staticmethod
    def decode_uint256(hex_str: str) -> int:
        """Decode uint256 from hex string (big-endian)"""
        cleaned = hex_str.replace("0x", "")
        return int(cleaned, 16) if cleaned else 0

    @staticmethod
    def format_token_amount(wei: int, decimals: int = 18) -> float:
        """Format wei to readable token amount (default 18 decimals like ETH)"""
        return wei / (10**decimals)


async def main():
    """Main example demonstrating contract interactions"""
    rpc_url = os.getenv("RPC_URL", "http://localhost:8545")
    client = ContractClient(rpc_url)

    print("=== Smart Contract Interaction Examples ===\n")

    # Example 1: Verify if an address is a contract
    example_contract = "0x0000000000000000000000000000000000000001"
    print("1. Checking if address is a contract:")
    print(f"   Address: {example_contract}")
    try:
        code = await client.get_code(example_contract)
        if code == "0x":
            print("   Result: Not a contract (EOA or empty)")
        else:
            print(
                f"   Result: Contract found (code length: {(len(code) - 2) // 2} bytes)"
            )
    except Exception as e:
        print(f"   Error: {e}")

    # Example 2: Encode ERC-20 transfer call
    print("\n2. Encoding ERC-20 transfer call:")
    recipient = "0x742d35Cc6634C0532925a3b844Bc9e7595f32D23"
    amount_wei = "1000000000000000000"  # 1 token (18 decimals)

    try:
        encoded_data = client.encode_erc20_transfer(recipient, amount_wei)
        print(f"   Recipient: {recipient}")
        print(f"   Amount: {amount_wei} wei")
        print(f"   Encoded data: {encoded_data}")
        print("   (This data would be used in eth_sendRawTransaction)")
    except Exception as e:
        print(f"   Error: {e}")

    # Example 3: Demonstrate storage access pattern
    print("\n3. Contract Storage Access Pattern:")
    print("   To read contract state, use eth_getStorageAt")
    print("   - Position 0: Often total supply for ERC-20")
    print("   - Position 1: Often owner address")
    print("   - Position 2+: Depends on contract design")
    print("   Storage slots are 32 bytes (256 bits)")

    # Example 4: Educational explanation of ABI encoding
    print("\n4. Understanding ABI Encoding:")
    print("   For function: transfer(address to, uint256 amount)")
    print("   - Selector (first 4 bytes): keccak256('transfer(address,uint256)')[0:4]")
    print("   - Parameter 1 (address): Padded to 32 bytes")
    print("   - Parameter 2 (uint256): Padded to 32 bytes")
    print("   - Total: 4 + 32 + 32 = 68 bytes (136 hex chars)")

    # Example 5: Common contract addresses format
    print("\n5. Working with Contract Addresses:")
    test_addresses = [
        ("EOA Example", "0x742d35Cc6634C0532925a3b844Bc9e7595f32D23"),
        ("Contract Example", "0x0000000000000000000000000000000000000001"),
        ("Zero Address", "0x0000000000000000000000000000000000000000"),
    ]

    for name, addr in test_addresses:
        print(f"   {name}: {addr}")

    print("\n=== Key Points ===")
    print("✓ Use eth_call for read-only contract calls (no gas, no state changes)")
    print("✓ Use eth_sendRawTransaction for state-changing calls (costs gas)")
    print("✓ ABI encoding is deterministic - same call always produces same data")
    print("✓ Always verify you're calling the correct contract address")
    print("✓ In production, use web3.py for automatic ABI encoding")


if __name__ == "__main__":
    asyncio.run(main())
