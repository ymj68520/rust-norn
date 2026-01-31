import requests
import json
import os
import time
from typing import Dict, Optional
from dotenv import load_dotenv

load_dotenv()


class TransactionSender:
    def __init__(self, rpc_url: Optional[str] = None):
        self.rpc_url = rpc_url or os.getenv("NORN_RPC_URL", "http://127.0.0.1:50051")
        self.request_id = 0

    def _make_request(self, method: str, params: list) -> Dict:
        self.request_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params,
        }

        response = requests.post(self.rpc_url, json=payload, timeout=30)
        response.raise_for_status()
        return response.json()

    def get_nonce(self, address: str) -> int:
        result = self._make_request("eth_getTransactionCount", [address, "latest"])
        nonce_hex = result.get("result", "0x0")
        return int(nonce_hex, 16)

    def get_gas_price(self) -> int:
        result = self._make_request("eth_gasPrice", [])
        gas_price_hex = result.get("result", "0x0")
        return int(gas_price_hex, 16)

    def send_raw_transaction(self, signed_tx: str) -> str:
        result = self._make_request("eth_sendRawTransaction", [signed_tx])
        if "error" in result:
            raise Exception(f"Transaction failed: {result['error']}")
        return result.get("result", "")

    def get_transaction_receipt(self, tx_hash: str) -> Optional[Dict]:
        result = self._make_request("eth_getTransactionReceipt", [tx_hash])
        return result.get("result")

    def wait_for_receipt(
        self, tx_hash: str, max_retries: int = 30, retry_delay: int = 1
    ) -> Optional[Dict]:
        for attempt in range(max_retries):
            receipt = self.get_transaction_receipt(tx_hash)
            if receipt:
                return receipt
            print(f"   Waiting for receipt... (attempt {attempt + 1}/{max_retries})")
            time.sleep(retry_delay)
        return None


def create_transaction_example() -> Dict:
    return {
        "from": os.getenv(
            "ACCOUNT_ADDRESS", "0x0000000000000000000000000000000000000000"
        ),
        "to": os.getenv(
            "RECIPIENT_ADDRESS", "0x1111111111111111111111111111111111111111"
        ),
        "value": int(os.getenv("TRANSACTION_VALUE", 1000000000000000000)),
        "gasPrice": int(os.getenv("GAS_PRICE", 1000000000)),
        "gasLimit": int(os.getenv("GAS_LIMIT", 21000)),
        "data": "0x",
    }


def print_transaction_structure():
    print("\n=== Transaction Structure ===")
    tx = create_transaction_example()
    for key, value in tx.items():
        print(f"   {key}: {value}")
    print()


def main():
    print("=== Transaction Sender Example ===\n")

    sender = TransactionSender()
    account = os.getenv("ACCOUNT_ADDRESS", "0x0000000000000000000000000000000000000000")

    print("1. Get Current Nonce")
    try:
        nonce = sender.get_nonce(account)
        print(f"   Account: {account}")
        print(f"   Nonce: {nonce}\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    print("2. Get Gas Price")
    try:
        gas_price = sender.get_gas_price()
        print(f"   Gas Price: {gas_price} wei\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    print("3. Transaction Information")
    print_transaction_structure()

    print("4. Transaction Sending Steps")
    print("   ✓ Create transaction object")
    print("   ✓ Sign with private key (ECDSA)")
    print("   ✓ Encode as RLP")
    print("   ✓ Send via eth_sendRawTransaction\n")

    print("NOTE: To send actual transactions:")
    print("1. Implement transaction signing")
    print("2. Use Web3.py or similar library for signing")
    print("3. Send the signed transaction\n")

    print("Example with Web3.py (not included in this example):")
    print("```python")
    print("from web3 import Web3")
    print("w3 = Web3(Web3.HTTPProvider('http://127.0.0.1:50051'))")
    print("tx_hash = w3.eth.send_transaction({")
    print("    'from': account,")
    print("    'to': recipient,")
    print("    'value': Web3.to_wei(1, 'ether'),")
    print("    'gas': 21000,")
    print("    'gasPrice': w3.eth.gas_price,")
    print("})")
    print("```\n")

    print("✅ Transaction sender example completed!")


if __name__ == "__main__":
    main()
