import requests
import json
import os
from typing import Dict, Optional, Tuple
from dotenv import load_dotenv

load_dotenv()


def wei_to_ether(wei: int) -> float:
    return wei / 1e18


def ether_to_wei(ether: float) -> int:
    return int(ether * 1e18)


class BalanceChecker:
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

    def get_balance(self, address: str, block: str = "latest") -> Tuple[int, float]:
        result = self._make_request("eth_getBalance", [address, block])
        balance_hex = result.get("result", "0x0")
        balance_wei = int(balance_hex, 16)
        balance_ether = wei_to_ether(balance_wei)
        return balance_wei, balance_ether

    def check_multiple_accounts(self, addresses: list) -> Dict:
        results = {}
        for address in addresses:
            try:
                wei, ether = self.get_balance(address)
                results[address] = {"wei": wei, "ether": ether, "status": "✅"}
            except Exception as e:
                results[address] = {"error": str(e), "status": "❌"}
        return results

    def track_balance_history(self, address: str) -> Dict:
        history = {}
        for block in ["0x0", "0x1", "latest"]:
            try:
                wei, ether = self.get_balance(address, block)
                history[block] = {"wei": wei, "ether": ether}
            except:
                history[block] = None
        return history


def main():
    print("=== Balance Checker Example ===\n")

    checker = BalanceChecker()
    account = os.getenv("ACCOUNT_ADDRESS", "0x0000000000000000000000000000000000000000")

    print(f"Checking balance for: {account}\n")

    print("1. Current Balance")
    try:
        wei, ether = checker.get_balance(account, "latest")
        print(f"   Balance (wei): {wei:,}")
        print(f"   Balance (ether): {ether:.18f}\n")
    except Exception as e:
        print(f"   Error: {e}\n")

    print("2. Balance History")
    try:
        history = checker.track_balance_history(account)
        for block, balance in history.items():
            if balance:
                print(f"   Block {block}: {balance['ether']:.18f} ether")
            else:
                print(f"   Block {block}: N/A")
        print()
    except Exception as e:
        print(f"   Error: {e}\n")

    print("3. Multiple Accounts")
    accounts = [
        account,
        "0x1111111111111111111111111111111111111111",
        "0x2222222222222222222222222222222222222222",
    ]
    try:
        results = checker.check_multiple_accounts(accounts)
        for addr, data in results.items():
            if "error" not in data:
                ether = data.get("ether", 0)
                print(f"   {addr}: {ether:.6f} ether {data['status']}")
            else:
                print(f"   {addr}: {data['error']} {data['status']}")
        print()
    except Exception as e:
        print(f"   Error: {e}\n")

    print("✅ Balance checker example completed!")


if __name__ == "__main__":
    main()
