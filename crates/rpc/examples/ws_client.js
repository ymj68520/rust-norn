/**
 * WebSocket Client Example for Norn Blockchain (JavaScript)
 *
 * This example demonstrates how to connect to the Norn WebSocket API
 * and subscribe to real-time blockchain events.
 *
 * Run with: node ws_client.js
 */

const WebSocket = require('ws');

// Connect to WebSocket server
const url = 'ws://localhost:8545/ws';
const ws = new WebSocket(url);

ws.on('open', () => {
    console.log('✅ Connected to Norn WebSocket API');

    // Subscribe to new blocks
    const subscribeBlocks = {
        jsonrpc: '2.0',
        id: 1,
        method: 'eth_subscribe',
        params: ['newHeads']
    };

    console.log('📡 Subscribing to new blocks...');
    ws.send(JSON.stringify(subscribeBlocks));

    // Subscribe to pending transactions
    const subscribeTxs = {
        jsonrpc: '2.0',
        id: 2,
        method: 'eth_subscribe',
        params: ['newPendingTransactions']
    };

    console.log('📡 Subscribing to pending transactions...');
    ws.send(JSON.stringify(subscribeTxs));

    // Subscribe to sync status
    const subscribeSync = {
        jsonrpc: '2.0',
        id: 3,
        method: 'eth_subscribe',
        params: ['syncing']
    };

    console.log('📡 Subscribing to sync status...');
    ws.send(JSON.stringify(subscribeSync));
});

ws.on('message', (data) => {
    try {
        const message = JSON.parse(data);

        // Handle subscription notifications
        if (message.subscription && message.result) {
            const subId = message.subscription;
            const result = message.result;

            // Format output based on subscription type
            if (result.hash && result.number !== undefined) {
                // New block
                console.log(`🧱 New Block #${result.number}`);
                console.log(`   Hash: ${result.hash}`);
                console.log(`   Parent Hash: ${result.parentHash}`);
                console.log(`   Transactions: ${result.transactions}`);
                console.log(`   Timestamp: ${result.timestamp}`);
                console.log('');
            } else if (typeof result === 'string' && result.startsWith('0x')) {
                // Pending transaction
                console.log(`📝 Pending Transaction: ${result}`);
            } else if (result.syncing !== undefined) {
                // Sync status
                console.log(`🔄 Sync Status: ${result.syncing ? 'Syncing' : 'Synced'}`);
                if (result.syncing) {
                    console.log(`   Starting Block: ${result.startingBlock}`);
                    console.log(`   Current Block: ${result.currentBlock}`);
                    console.log(`   Highest Block: ${result.highestBlock}`);
                }
                console.log('');
            }
        }
        // Handle subscription confirmation
        else if (message.result && typeof message.result === 'string') {
            console.log(`✅ Subscription created: ${message.result}`);
        }
        // Handle errors
        else if (message.error) {
            console.error(`❌ Error: ${message.error.message} (Code: ${message.error.code})`);
        }
    } catch (e) {
        console.error('Failed to parse message:', e);
        console.log('Raw:', data.toString());
    }
});

ws.on('error', (error) => {
    console.error('WebSocket error:', error);
});

ws.on('close', () => {
    console.log('🔌 Connection closed');
});

// Handle graceful shutdown
process.on('SIGINT', () => {
    console.log('\n👋 Shutting down...');
    ws.close();
    process.exit(0);
});
