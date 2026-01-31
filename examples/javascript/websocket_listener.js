require('dotenv').config();
const WebSocket = require('ws');

const WS_URL = process.env.NORN_WS_URL || 'ws://127.0.0.1:50052';

class WebSocketListener {
  constructor(wsUrl = WS_URL) {
    this.wsUrl = wsUrl;
    this.requestId = 0;
    this.subscriptions = {};
  }

  async connectAndListen() {
    const ws = new WebSocket(this.wsUrl);

    ws.on('open', () => {
      console.log(`✅ Connected to WebSocket: ${this.wsUrl}\n`);
      this.subscribeToBlocks(ws);
      this.subscribeToTransactions(ws);
    });

    ws.on('message', (message) => {
      this.handleMessage(JSON.parse(message));
    });

    ws.on('close', () => {
      console.log('\n\n✅ WebSocket connection closed');
    });

    ws.on('error', (error) => {
      console.error('❌ WebSocket error:', error.message);
    });
  }

  subscribeToBlocks(ws) {
    this.requestId++;
    const subscription = {
      jsonrpc: '2.0',
      id: this.requestId,
      method: 'eth_subscribe',
      params: ['newHeads'],
    };
    ws.send(JSON.stringify(subscription));
    console.log('📡 Subscription request sent for newHeads');
  }

  subscribeToTransactions(ws) {
    this.requestId++;
    const subscription = {
      jsonrpc: '2.0',
      id: this.requestId,
      method: 'eth_subscribe',
      params: ['newPendingTransactions'],
    };
    ws.send(JSON.stringify(subscription));
    console.log('📡 Subscription request sent for newPendingTransactions\n');
  }

  handleMessage(msg) {
    if (msg.id) {
      if (msg.id === 1) {
        console.log(`✅ Subscribed to newHeads with ID: ${msg.result}`);
        this.subscriptions['newHeads'] = msg.result;
      } else if (msg.id === 2) {
        console.log(`✅ Subscribed to newPendingTransactions with ID: ${msg.result}`);
        this.subscriptions['newPendingTransactions'] = msg.result;
      }
      return;
    }

    if (msg.method === 'eth_subscription') {
      const { subscription, result } = msg.params;

      if (subscription === this.subscriptions['newHeads']) {
        this.printBlockInfo(result);
      } else if (subscription === this.subscriptions['newPendingTransactions']) {
        this.printTransactionInfo(result);
      }
    }
  }

  printBlockInfo(block) {
    console.log('\n🔗 [Block] New block received');
    console.log(`   Height: ${block.number}`);
    console.log(`   Miner: ${block.miner}`);
    console.log(`   Timestamp: ${block.timestamp}`);
  }

  printTransactionInfo(txHash) {
    console.log(`💰 [Tx] Pending transaction: ${txHash}`);
  }
}

async function main() {
  console.log('=== WebSocket Listener Example ===\n');

  const listener = new WebSocketListener();

  try {
    await listener.connectAndListen();
    console.log('\nListening for events (press Ctrl+C to stop)...\n');

    await new Promise((resolve) => {
      process.on('SIGINT', () => {
        resolve();
      });
    });
  } catch (error) {
    console.error('❌ Error:', error.message);
  }
}

main();
