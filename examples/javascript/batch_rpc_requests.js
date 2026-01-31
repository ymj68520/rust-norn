/**
 * Batch RPC Requests Example
 *
 * This example demonstrates how to efficiently batch multiple RPC calls
 * into a single request, which is much faster than making separate requests.
 *
 * Benefits of batching:
 * - Single HTTP round trip instead of multiple
 * - Better performance for sequential operations
 * - Atomicity for reading state at the same block height
 * - Reduced latency in network calls
 *
 * Use cases:
 * - Getting balances for multiple addresses
 * - Fetching multiple blocks' data
 * - Reading multiple contract states
 * - Pre-flight checks before transaction submission
 */

require('dotenv').config();

/**
 * Client for making batch RPC requests
 */
class BatchRpcClient {
  constructor(rpcUrl) {
    this.rpcUrl = rpcUrl;
  }

  /**
   * Execute a batch of RPC requests.
   * Returns results in the same order as the requests.
   */
  async batchRequest(requests) {
    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(requests)
      });

      const results = await response.json();
      return results;
    } catch (error) {
      throw error;
    }
  }

  /**
   * Batch request to get balances for multiple addresses.
   */
  async getBalancesBatch(addresses) {
    const requests = addresses.map((address, index) => ({
      jsonrpc: '2.0',
      method: 'eth_getBalance',
      params: [address, 'latest'],
      id: index + 1
    }));

    const responses = await this.batchRequest(requests);

    const balances = [];
    for (let index = 0; index < addresses.length; index++) {
      if ('result' in responses[index]) {
        const balance = responses[index].result;
        balances.push([addresses[index], balance]);
      }
    }

    return balances;
  }

  /**
   * Batch request to get block details for multiple block numbers.
   */
  async getBlocksBatch(blockNumbers) {
    const requests = blockNumbers.map((blockNum, index) => ({
      jsonrpc: '2.0',
      method: 'eth_getBlockByNumber',
      params: [blockNum, false],
      id: index + 1
    }));

    const responses = await this.batchRequest(requests);

    const blocks = [];
    for (const response of responses) {
      if ('result' in response) {
        blocks.push(response.result);
      }
    }

    return blocks;
  }

  /**
   * Batch request to get transaction details for multiple hashes.
   */
  async getTransactionsBatch(txHashes) {
    const requests = txHashes.map((txHash, index) => ({
      jsonrpc: '2.0',
      method: 'eth_getTransactionByHash',
      params: [txHash],
      id: index + 1
    }));

    const responses = await this.batchRequest(requests);

    const transactions = [];
    for (const response of responses) {
      if ('result' in response) {
        transactions.push(response.result);
      }
    }

    return transactions;
  }

  /**
   * Batch request to check multiple storage slots.
   */
  async getStorageBatch(contractAddress, positions) {
    const requests = positions.map((position, index) => ({
      jsonrpc: '2.0',
      method: 'eth_getStorageAt',
      params: [contractAddress, position, 'latest'],
      id: index + 1
    }));

    const responses = await this.batchRequest(requests);

    const storageValues = [];
    for (const response of responses) {
      if ('result' in response) {
        const value = response.result;
        storageValues.push(value);
      }
    }

    return storageValues;
  }

  /**
   * Mixed batch request - combines different RPC methods.
   */
  async mixedBatchRequest(options = {}) {
    const { chainId = false, gasPrice = false, blockNumber = false } = options;

    const requests = [];
    let requestId = 1;

    if (chainId) {
      requests.push({
        jsonrpc: '2.0',
        method: 'eth_chainId',
        params: [],
        id: requestId++
      });
    }

    if (gasPrice) {
      requests.push({
        jsonrpc: '2.0',
        method: 'eth_gasPrice',
        params: [],
        id: requestId++
      });
    }

    if (blockNumber) {
      requests.push({
        jsonrpc: '2.0',
        method: 'eth_blockNumber',
        params: [],
        id: requestId++
      });
    }

    const responses = await this.batchRequest(requests);
    return responses;
  }

  /**
   * Convert hex string to decimal
   */
  static hexToDecimal(hexStr) {
    return BigInt(hexStr);
  }

  /**
   * Format wei to ether
   */
  static weiToEther(weiHex) {
    const wei = BigInt(weiHex);
    const divisor = BigInt(10 ** 18);
    return Number(wei) / Number(divisor);
  }
}

/**
 * Main example demonstrating batch RPC requests
 */
async function main() {
  const rpcUrl = process.env.RPC_URL || 'http://localhost:8545';
  const client = new BatchRpcClient(rpcUrl);

  console.log('=== Batch RPC Requests Examples ===\n');

  // Example 1: Batch balance queries
  console.log('1. Batch Balance Queries:');
  console.log('   Querying balances for multiple addresses in one request...');
  const addresses = [
    '0x742d35Cc6634C0532925a3b844Bc9e7595f32D23',
    '0x0000000000000000000000000000000000000000',
    '0x1111111111111111111111111111111111111111'
  ];

  try {
    const balances = await client.getBalancesBatch(addresses);
    console.log('   Results:');
    for (const [address, balance] of balances) {
      const balanceDecimal = BatchRpcClient.hexToDecimal(balance);
      const balanceEther = BatchRpcClient.weiToEther(balance);
      console.log(
        `   ${address} -> ${balanceDecimal} Wei (${balanceEther} ETH)`
      );
    }
  } catch (error) {
    console.log(`   Error: ${error.message}`);
  }

  // Example 2: Batch block queries
  console.log('\n2. Batch Block Queries:');
  console.log('   Fetching multiple blocks in one request...');
  const blockNumbers = ['0x1', '0x2', '0x3'];

  try {
    const blocks = await client.getBlocksBatch(blockNumbers);
    console.log(`   Fetched ${blocks.length} blocks successfully`);
    for (const block of blocks) {
      if (block && block.number) {
        const miner = block.miner || 'unknown';
        console.log(`   Block ${block.number}: miner ${miner}`);
      }
    }
  } catch (error) {
    console.log(`   Error: ${error.message}`);
  }

  // Example 3: Batch storage queries
  console.log('\n3. Batch Storage Queries:');
  console.log('   Reading multiple storage slots from a contract...');
  const contract = '0x0000000000000000000000000000000000000001';
  const positions = ['0x0', '0x1', '0x2'];

  try {
    const values = await client.getStorageBatch(contract, positions);
    console.log('   Results from contract storage:');
    for (let idx = 0; idx < values.length; idx++) {
      console.log(`   Position ${idx}: ${values[idx]}`);
    }
  } catch (error) {
    console.log(`   Error: ${error.message}`);
  }

  // Example 4: Mixed batch request
  console.log('\n4. Mixed Batch Request:');
  console.log('   Combining different RPC methods in one batch...');

  try {
    const results = await client.mixedBatchRequest({
      chainId: true,
      gasPrice: true,
      blockNumber: true
    });
    console.log('   Results:');
    for (const result of results) {
      if ('result' in result) {
        console.log(`   ${result.result}`);
      }
    }
  } catch (error) {
    console.log(`   Error: ${error.message}`);
  }

  // Example 5: Educational information
  console.log('\n5. Batch Request Performance Benefits:');
  console.log('   ✓ Single HTTP connection for multiple calls');
  console.log('   ✓ Results atomic at the same block height');
  console.log('   ✓ Reduced round-trip latency');
  console.log('   ✓ Better for reading multiple state snapshots');
  console.log('   ✓ Can batch up to 100+ requests (depends on node)');

  console.log('\n6. Batch Request Patterns:');
  console.log('   Pattern 1: Get state before transaction');
  console.log('     - Query nonce, gas price, balances all at once');
  console.log('   Pattern 2: Multi-address monitoring');
  console.log('     - Check balances/nonces for multiple accounts');
  console.log('   Pattern 3: Contract audit');
  console.log('     - Read multiple storage slots at once');
  console.log('   Pattern 4: Historical data fetching');
  console.log('     - Get multiple blocks\' data in parallel');

  console.log('\n=== Key Points ===');
  console.log('✓ Batch requests must be sent as JSON array, not individual objects');
  console.log('✓ Results are returned in the same order as requests');
  console.log('✓ Each request in the batch must have unique \'id\'');
  console.log('✓ All requests in a batch are executed at the same block height');
  console.log('✓ Error in one request doesn\'t affect others in the batch');
}

main().catch(console.error);
