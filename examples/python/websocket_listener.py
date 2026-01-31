import asyncio
import json
import os
import websockets
from typing import Optional
from dotenv import load_dotenv

load_dotenv()


class WebSocketListener:
    def __init__(self, ws_url: Optional[str] = None):
        self.ws_url = ws_url or os.getenv("NORN_WS_URL", "ws://127.0.0.1:50052")
        self.request_id = 0
        self.subscriptions = {}

    async def connect_and_listen(self):
        async with websockets.connect(self.ws_url) as websocket:
            print(f"✅ Connected to WebSocket: {self.ws_url}\n")

            await self.subscribe_to_blocks(websocket)
            await self.subscribe_to_transactions(websocket)

            await self.listen_for_events(websocket)

    async def subscribe_to_blocks(self, websocket):
        self.request_id += 1
        subscription = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "eth_subscribe",
            "params": ["newHeads"],
        }
        await websocket.send(json.dumps(subscription))
        print("📡 Subscription request sent for newHeads")

    async def subscribe_to_transactions(self, websocket):
        self.request_id += 1
        subscription = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "eth_subscribe",
            "params": ["newPendingTransactions"],
        }
        await websocket.send(json.dumps(subscription))
        print("📡 Subscription request sent for newPendingTransactions\n")

    async def listen_for_events(self, websocket):
        block_count = 0
        tx_count = 0

        print("Listening for events (Ctrl+C to stop)...\n")

        try:
            async for message in websocket:
                data = json.loads(message)
                block_count, tx_count = await self.handle_message(
                    data, block_count, tx_count
                )
        except asyncio.CancelledError:
            print("\n\nListener stopped")
            print(f"Total blocks received: {block_count}")
            print(f"Total transactions received: {tx_count}")

    async def handle_message(self, msg: dict, block_count: int, tx_count: int):
        if "id" in msg:
            if msg["id"] == 1:
                subscription_id = msg.get("result")
                print(f"✅ Subscribed to newHeads with ID: {subscription_id}")
                self.subscriptions["newHeads"] = subscription_id
            elif msg["id"] == 2:
                subscription_id = msg.get("result")
                print(
                    f"✅ Subscribed to newPendingTransactions with ID: {subscription_id}"
                )
                self.subscriptions["newPendingTransactions"] = subscription_id
            return block_count, tx_count

        if msg.get("method") == "eth_subscription":
            params = msg.get("params", {})
            subscription_id = params.get("subscription")
            result = params.get("result")

            if subscription_id == self.subscriptions.get("newHeads"):
                block_count += 1
                self.print_block_info(result, block_count)
            elif subscription_id == self.subscriptions.get("newPendingTransactions"):
                tx_count += 1
                self.print_transaction_info(result, tx_count)

        return block_count, tx_count

    @staticmethod
    def print_block_info(block: dict, count: int):
        print(f"\n🔗 [Block #{count}] New block received")
        print(f"   Height: {block.get('number', 'N/A')}")
        print(f"   Miner: {block.get('miner', 'N/A')}")
        print(f"   Timestamp: {block.get('timestamp', 'N/A')}")

    @staticmethod
    def print_transaction_info(tx_hash: str, count: int):
        print(f"💰 [Tx #{count}] Pending transaction: {tx_hash}")


async def main():
    print("=== WebSocket Listener Example ===\n")

    listener = WebSocketListener()

    try:
        await listener.connect_and_listen()
    except KeyboardInterrupt:
        print("\n\n✅ WebSocket listener stopped")
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    asyncio.run(main())
