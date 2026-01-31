"""
Health Check and Monitoring Example

This example demonstrates how to monitor the health of a blockchain node
and implement various health checks:

Health Checks:
- Node connectivity (can we reach the RPC endpoint?)
- Chain synchronization (is the node syncing?)
- Latest block info (what's the current height?)
- Gas price trends (how does gas vary?)
- Peer count (how many peers are connected?)
- Transaction pool size (how many pending transactions?)

Monitoring patterns:
- Periodic health checks
- Alert triggers for anomalies
- Performance metrics collection
- Network condition tracking
"""

import os
import asyncio
import aiohttp
import time
from dotenv import load_dotenv
from typing import Dict, Any, Optional
from dataclasses import dataclass

load_dotenv()


@dataclass
class HealthStatus:
    """Health status of the node"""

    is_connected: bool
    is_syncing: bool
    latest_block: int
    latest_block_time: int
    gas_price: str
    peer_count: int
    pending_transactions: int
    network_id: str
    client_version: str
    timestamp: int


class MonitoringClient:
    """RPC client with health check capabilities"""

    def __init__(self, rpc_url: str):
        self.rpc_url = rpc_url

    async def check_connectivity(self) -> bool:
        """Basic connectivity check"""
        try:
            payload = {
                "jsonrpc": "2.0",
                "method": "web3_clientVersion",
                "params": [],
                "id": 1,
            }

            async with aiohttp.ClientSession() as session:
                async with session.post(
                    self.rpc_url, json=payload, timeout=aiohttp.ClientTimeout(total=5)
                ) as response:
                    return response.status == 200
        except Exception:
            return False

    async def check_sync_status(self) -> bool:
        """Check if node is syncing (False = synced, True = syncing)"""
        try:
            payload = {"jsonrpc": "2.0", "method": "eth_syncing", "params": [], "id": 1}

            async with aiohttp.ClientSession() as session:
                async with session.post(self.rpc_url, json=payload) as response:
                    result = await response.json()

            if "result" in result:
                # False means synced, dict means syncing
                return result["result"] is not False
            return True
        except Exception:
            return True

    async def get_latest_block(self) -> int:
        """Get latest block number"""
        try:
            payload = {
                "jsonrpc": "2.0",
                "method": "eth_blockNumber",
                "params": [],
                "id": 1,
            }

            async with aiohttp.ClientSession() as session:
                async with session.post(self.rpc_url, json=payload) as response:
                    result = await response.json()

            if "result" in result:
                return int(result["result"], 16)
            return 0
        except Exception:
            return 0

    async def get_block_timestamp(self, block_num: int) -> int:
        """Get block timestamp"""
        try:
            block_hex = hex(block_num)
            payload = {
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [block_hex, False],
                "id": 1,
            }

            async with aiohttp.ClientSession() as session:
                async with session.post(self.rpc_url, json=payload) as response:
                    result = await response.json()

            if "result" in result and result["result"]:
                return int(result["result"]["timestamp"], 16)
            return 0
        except Exception:
            return 0

    async def get_gas_price(self) -> str:
        """Get current gas price"""
        try:
            payload = {
                "jsonrpc": "2.0",
                "method": "eth_gasPrice",
                "params": [],
                "id": 1,
            }

            async with aiohttp.ClientSession() as session:
                async with session.post(self.rpc_url, json=payload) as response:
                    result = await response.json()

            if "result" in result:
                return result["result"]
            return "0x0"
        except Exception:
            return "0x0"

    async def get_peer_count(self) -> int:
        """Get number of peers connected"""
        try:
            payload = {
                "jsonrpc": "2.0",
                "method": "net_peerCount",
                "params": [],
                "id": 1,
            }

            async with aiohttp.ClientSession() as session:
                async with session.post(self.rpc_url, json=payload) as response:
                    result = await response.json()

            if "result" in result:
                return int(result["result"], 16)
            return 0
        except Exception:
            return 0

    async def get_pending_tx_count(self) -> int:
        """Get pending transactions count"""
        try:
            payload = {
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": ["pending", False],
                "id": 1,
            }

            async with aiohttp.ClientSession() as session:
                async with session.post(self.rpc_url, json=payload) as response:
                    result = await response.json()

            if "result" in result and result["result"]:
                return len(result["result"].get("transactions", []))
            return 0
        except Exception:
            return 0

    async def get_network_id(self) -> str:
        """Get network ID"""
        try:
            payload = {"jsonrpc": "2.0", "method": "net_version", "params": [], "id": 1}

            async with aiohttp.ClientSession() as session:
                async with session.post(self.rpc_url, json=payload) as response:
                    result = await response.json()

            if "result" in result:
                return result["result"]
            return "unknown"
        except Exception:
            return "unknown"

    async def get_client_version(self) -> str:
        """Get client version"""
        try:
            payload = {
                "jsonrpc": "2.0",
                "method": "web3_clientVersion",
                "params": [],
                "id": 1,
            }

            async with aiohttp.ClientSession() as session:
                async with session.post(self.rpc_url, json=payload) as response:
                    result = await response.json()

            if "result" in result:
                return result["result"]
            return "unknown"
        except Exception:
            return "unknown"

    async def perform_health_check(self) -> HealthStatus:
        """Perform comprehensive health check"""
        is_connected = await self.check_connectivity()
        is_syncing = await self.check_sync_status()
        latest_block = await self.get_latest_block()
        latest_block_time = await self.get_block_timestamp(latest_block)
        gas_price = await self.get_gas_price()
        peer_count = await self.get_peer_count()
        pending_transactions = await self.get_pending_tx_count()
        network_id = await self.get_network_id()
        client_version = await self.get_client_version()

        return HealthStatus(
            is_connected=is_connected,
            is_syncing=is_syncing,
            latest_block=latest_block,
            latest_block_time=latest_block_time,
            gas_price=gas_price,
            peer_count=peer_count,
            pending_transactions=pending_transactions,
            network_id=network_id,
            client_version=client_version,
            timestamp=int(time.time()),
        )

    @staticmethod
    def gas_price_to_gwei(gas_price_hex: str) -> float:
        """Convert hex gas price to gwei"""
        wei = int(gas_price_hex, 16)
        return wei / 1e9


async def main():
    """Main example demonstrating health checks and monitoring"""
    rpc_url = os.getenv("RPC_URL", "http://localhost:8545")
    client = MonitoringClient(rpc_url)

    print("=== Health Check and Monitoring Examples ===\n")

    # Perform health check
    print("Performing health check...\n")

    try:
        health = await client.perform_health_check()

        print("=== Health Status ===")
        print(f"Connected: {'✓ YES' if health.is_connected else '✗ NO'}")
        print(f"Syncing: {'⚠ YES' if health.is_syncing else '✓ NO'}")
        print(f"Latest Block: {health.latest_block}")
        print(f"Block Time: {health.latest_block_time} ({health.timestamp})")
        print(
            f"Gas Price: {health.gas_price} ({client.gas_price_to_gwei(health.gas_price):.2f} Gwei)"
        )
        print(f"Peers Connected: {health.peer_count}")
        print(f"Pending Transactions: {health.pending_transactions}")
        print(f"Network ID: {health.network_id}")
        print(f"Client Version: {health.client_version}")
        print(f"Timestamp: {health.timestamp}")

        # Health indicators
        print("\n=== Health Indicators ===")
        if health.is_connected:
            print("✓ Node is reachable")
        else:
            print("✗ Node is unreachable - cannot connect to RPC endpoint")

        if not health.is_syncing and health.is_connected:
            print("✓ Node is fully synced")
        elif health.is_syncing:
            print("⚠ Node is syncing - may have delayed data")

        if health.peer_count > 0:
            print(f"✓ Node has {health.peer_count} peer(s) connected")
        else:
            print("✗ No peers connected - node may be isolated")

        if health.pending_transactions > 0:
            print(f"ℹ {health.pending_transactions} transactions in mempool")

    except Exception as e:
        print(f"Health check failed: {e}")

    # Monitoring examples
    print("\n=== Monitoring Patterns ===")

    print("\n1. Periodic Health Checks:")
    print("   - Check connectivity every 30 seconds")
    print("   - Alert if node becomes unreachable")
    print("   - Track syncing status changes")

    print("\n2. Performance Metrics:")
    print("   - Track block time trends")
    print("   - Monitor gas price variations")
    print("   - Count peer connections over time")

    print("\n3. Anomaly Detection:")
    print("   - Alert if block time > 30 seconds")
    print("   - Alert if gas price spikes > 2x baseline")
    print("   - Alert if peer count drops to 0")

    print("\n4. Threshold-based Alerts:")
    print("   - Warning: peer_count < 3")
    print("   - Critical: peer_count == 0")
    print("   - Warning: pending_transactions > 1000")

    print("\n=== Recommended Health Check Intervals ===")
    print("   - Basic connectivity: Every 10-30 seconds")
    print("   - Full health check: Every 1-5 minutes")
    print("   - Historical metrics: Every 1 hour (collect aggregates)")


if __name__ == "__main__":
    asyncio.run(main())
