import requests
import json
import os
from typing import Any, Dict, Optional
from dotenv import load_dotenv

load_dotenv()


class NornRPCClient:
    def __init__(self, rpc_url: Optional[str] = None):
        self.rpc_url = rpc_url or os.getenv("NORN_RPC_URL", "http://127.0.0.1:50051")
        self.session = requests.Session()
        self.request_id = 0

    def _make_request(self, method: str, params: list) -> Dict[str, Any]:
        self.request_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params,
        }

        try:
            response = self.session.post(self.rpc_url, json=payload, timeout=30)
            response.raise_for_status()
            return response.json()
        except requests.exceptions.RequestException as e:
            print(f"RPC request failed: {e}")
            raise

    def get_chain_id(self) -> str:
        result = self._make_request("eth_chainId", [])
        return result.get("result", "")

    def get_block_number(self) -> str:
        result = self._make_request("eth_blockNumber", [])
        return result.get("result", "")

    def get_block_by_number(self, block_number: str, full_tx: bool = False) -> Dict:
        result = self._make_request("eth_getBlockByNumber", [block_number, full_tx])
        return result.get("result", {})

    def get_gas_price(self) -> str:
        result = self._make_request("eth_gasPrice", [])
        return result.get("result", "")

    def get_balance(self, address: str, block: str = "latest") -> str:
        result = self._make_request("eth_getBalance", [address, block])
        return result.get("result", "")

    def get_transaction_count(self, address: str, block: str = "latest") -> str:
        result = self._make_request("eth_getTransactionCount", [address, block])
        return result.get("result", "")

    def send_raw_transaction(self, signed_tx: str) -> str:
        result = self._make_request("eth_sendRawTransaction", [signed_tx])
        return result.get("result", "")

    def get_transaction_receipt(self, tx_hash: str) -> Dict:
        result = self._make_request("eth_getTransactionReceipt", [tx_hash])
        return result.get("result", {})


def main():
    print("=== Norn RPC Client Example ===\n")

    client = NornRPCClient()

    print("1. Get Chain ID")
    try:
        chain_id = client.get_chain_id()
        print(f"   Chain ID: {chain_id}\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    print("2. Get Latest Block Number")
    try:
        block_num = client.get_block_number()
        print(f"   Block Number: {block_num}\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    print("3. Get Block Information")
    try:
        block = client.get_block_by_number("0x1", False)
        if block:
            print(f"   Block Hash: {block.get('hash', 'N/A')}")
            print(f"   Miner: {block.get('miner', 'N/A')}")
            print(f"   Timestamp: {block.get('timestamp', 'N/A')}\n")
        else:
            print("   Block not found\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    print("4. Get Gas Price")
    try:
        gas_price = client.get_gas_price()
        gas_price_wei = int(gas_price, 16) if gas_price else 0
        print(f"   Gas Price: {gas_price_wei} wei\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    account = os.getenv("ACCOUNT_ADDRESS", "0x0000000000000000000000000000000000000000")

    print(f"5. Get Account Balance for {account}")
    try:
        balance_hex = client.get_balance(account, "latest")
        balance_wei = int(balance_hex, 16) if balance_hex else 0
        balance_ether = balance_wei / 1e18
        print(f"   Balance (wei): {balance_wei}")
        print(f"   Balance (ether): {balance_ether:.18f}\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    print("6. Get Transaction Count (Nonce)")
    try:
        nonce_hex = client.get_transaction_count(account, "latest")
        nonce = int(nonce_hex, 16) if nonce_hex else 0
        print(f"   Nonce: {nonce}\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    print("✅ RPC Client example completed!")


if __name__ == "__main__":
    main()
