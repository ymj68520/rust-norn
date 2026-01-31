/**
 * Health Check and Monitoring Example
 *
 * This example demonstrates how to monitor the health of a blockchain node
 * and implement various health checks:
 *
 * Health Checks:
 * - Node connectivity (can we reach the RPC endpoint?)
 * - Chain synchronization (is the node syncing?)
 * - Latest block info (what's the current height?)
 * - Gas price trends (how does gas vary?)
 * - Peer count (how many peers are connected?)
 * - Transaction pool size (how many pending transactions?)
 *
 * Monitoring patterns:
 * - Periodic health checks
 * - Alert triggers for anomalies
 * - Performance metrics collection
 * - Network condition tracking
 */

require('dotenv').config();

/**
 * Health status of the node
 */
class HealthStatus {
  constructor(data) {
    this.isConnected = data.isConnected;
    this.isSyncing = data.isSyncing;
    this.latestBlock = data.latestBlock;
    this.latestBlockTime = data.latestBlockTime;
    this.gasPrice = data.gasPrice;
    this.peerCount = data.peerCount;
    this.pendingTransactions = data.pendingTransactions;
    this.networkId = data.networkId;
    this.clientVersion = data.clientVersion;
    this.timestamp = data.timestamp;
  }
}

/**
 * RPC client with health check capabilities
 */
class MonitoringClient {
  constructor(rpcUrl) {
    this.rpcUrl = rpcUrl;
  }

  /**
   * Basic connectivity check
   */
  async checkConnectivity() {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'web3_clientVersion',
          params: [],
          id: 1
        }),
        timeout: 5000
      });

      return response.status === 200;
    } catch (error) {
      return false;
    }
  }

  /**
   * Check if node is syncing (false = synced, true = syncing)
   */
  async checkSyncStatus() {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'eth_syncing',
          params: [],
          id: 1
        })
      });

      const result = await response.json();

      if (result.result !== undefined) {
        // false means synced, object means syncing
        return result.result !== false;
      }
      return true;
    } catch (error) {
      return true;
    }
  }

  /**
   * Get latest block number
   */
  async getLatestBlock() {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'eth_blockNumber',
          params: [],
          id: 1
        })
      });

      const result = await response.json();

      if (result.result !== undefined) {
        return parseInt(result.result, 16);
      }
      return 0;
    } catch (error) {
      return 0;
    }
  }

  /**
   * Get block timestamp
   */
  async getBlockTimestamp(blockNum) {
    try {
      const blockHex = '0x' + blockNum.toString(16);

      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'eth_getBlockByNumber',
          params: [blockHex, false],
          id: 1
        })
      });

      const result = await response.json();

      if (result.result && result.result.timestamp) {
        return parseInt(result.result.timestamp, 16);
      }
      return 0;
    } catch (error) {
      return 0;
    }
  }

  /**
   * Get current gas price
   */
  async getGasPrice() {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'eth_gasPrice',
          params: [],
          id: 1
        })
      });

      const result = await response.json();

      if (result.result !== undefined) {
        return result.result;
      }
      return '0x0';
    } catch (error) {
      return '0x0';
    }
  }

  /**
   * Get number of peers connected
   */
  async getPeerCount() {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'net_peerCount',
          params: [],
          id: 1
        })
      });

      const result = await response.json();

      if (result.result !== undefined) {
        return parseInt(result.result, 16);
      }
      return 0;
    } catch (error) {
      return 0;
    }
  }

  /**
   * Get pending transactions count
   */
  async getPendingTxCount() {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'eth_getBlockByNumber',
          params: ['pending', false],
          id: 1
        })
      });

      const result = await response.json();

      if (result.result && result.result.transactions) {
        return result.result.transactions.length;
      }
      return 0;
    } catch (error) {
      return 0;
    }
  }

  /**
   * Get network ID
   */
  async getNetworkId() {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'net_version',
          params: [],
          id: 1
        })
      });

      const result = await response.json();

      if (result.result !== undefined) {
        return result.result;
      }
      return 'unknown';
    } catch (error) {
      return 'unknown';
    }
  }

  /**
   * Get client version
   */
  async getClientVersion() {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          method: 'web3_clientVersion',
          params: [],
          id: 1
        })
      });

      const result = await response.json();

      if (result.result !== undefined) {
        return result.result;
      }
      return 'unknown';
    } catch (error) {
      return 'unknown';
    }
  }

  /**
   * Perform comprehensive health check
   */
  async performHealthCheck() {
    const [
      isConnected,
      isSyncing,
      latestBlock,
      gasPrice,
      peerCount,
      pendingTransactions,
      networkId,
      clientVersion
    ] = await Promise.all([
      this.checkConnectivity(),
      this.checkSyncStatus(),
      this.getLatestBlock(),
      this.getGasPrice(),
      this.getPeerCount(),
      this.getPendingTxCount(),
      this.getNetworkId(),
      this.getClientVersion()
    ]);

    const latestBlockTime = await this.getBlockTimestamp(latestBlock);

    return new HealthStatus({
      isConnected,
      isSyncing,
      latestBlock,
      latestBlockTime,
      gasPrice,
      peerCount,
      pendingTransactions,
      networkId,
      clientVersion,
      timestamp: Math.floor(Date.now() / 1000)
    });
  }

  /**
   * Convert hex gas price to gwei
   */
  static gasPriceToGwei(gasPriceHex) {
    const wei = BigInt(gasPriceHex);
    const divisor = BigInt(10 ** 9);
    return Number(wei) / Number(divisor);
  }
}

/**
 * Main example demonstrating health checks and monitoring
 */
async function main() {
  const rpcUrl = process.env.RPC_URL || 'http://localhost:8545';
  const client = new MonitoringClient(rpcUrl);

  console.log('=== Health Check and Monitoring Examples ===\n');

  // Perform health check
  console.log('Performing health check...\n');

  try {
    const health = await client.performHealthCheck();

    console.log('=== Health Status ===');
    console.log(`Connected: ${health.isConnected ? '✓ YES' : '✗ NO'}`);
    console.log(`Syncing: ${health.isSyncing ? '⚠ YES' : '✓ NO'}`);
    console.log(`Latest Block: ${health.latestBlock}`);
    console.log(
      `Block Time: ${health.latestBlockTime} (${health.timestamp})`
    );
    console.log(
      `Gas Price: ${health.gasPrice} (${MonitoringClient.gasPriceToGwei(health.gasPrice).toFixed(2)} Gwei)`
    );
    console.log(`Peers Connected: ${health.peerCount}`);
    console.log(`Pending Transactions: ${health.pendingTransactions}`);
    console.log(`Network ID: ${health.networkId}`);
    console.log(`Client Version: ${health.clientVersion}`);
    console.log(`Timestamp: ${health.timestamp}`);

    // Health indicators
    console.log('\n=== Health Indicators ===');
    if (health.isConnected) {
      console.log('✓ Node is reachable');
    } else {
      console.log(
        '✗ Node is unreachable - cannot connect to RPC endpoint'
      );
    }

    if (!health.isSyncing && health.isConnected) {
      console.log('✓ Node is fully synced');
    } else if (health.isSyncing) {
      console.log('⚠ Node is syncing - may have delayed data');
    }

    if (health.peerCount > 0) {
      console.log(`✓ Node has ${health.peerCount} peer(s) connected`);
    } else {
      console.log('✗ No peers connected - node may be isolated');
    }

    if (health.pendingTransactions > 0) {
      console.log(`ℹ ${health.pendingTransactions} transactions in mempool`);
    }
  } catch (error) {
    console.log(`Health check failed: ${error.message}`);
  }

  // Monitoring examples
  console.log('\n=== Monitoring Patterns ===');

  console.log('\n1. Periodic Health Checks:');
  console.log('   - Check connectivity every 30 seconds');
  console.log('   - Alert if node becomes unreachable');
  console.log('   - Track syncing status changes');

  console.log('\n2. Performance Metrics:');
  console.log('   - Track block time trends');
  console.log('   - Monitor gas price variations');
  console.log('   - Count peer connections over time');

  console.log('\n3. Anomaly Detection:');
  console.log('   - Alert if block time > 30 seconds');
  console.log('   - Alert if gas price spikes > 2x baseline');
  console.log('   - Alert if peer count drops to 0');

  console.log('\n4. Threshold-based Alerts:');
  console.log('   - Warning: peer_count < 3');
  console.log('   - Critical: peer_count == 0');
  console.log('   - Warning: pending_transactions > 1000');

  console.log('\n=== Recommended Health Check Intervals ===');
  console.log('   - Basic connectivity: Every 10-30 seconds');
  console.log('   - Full health check: Every 1-5 minutes');
  console.log('   - Historical metrics: Every 1 hour (collect aggregates)');
}

main().catch(console.error);
