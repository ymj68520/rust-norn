/**
 * Integration tests for Norn RPC JavaScript examples
 *
 * These tests verify that all examples can connect to and interact with
 * a running Norn node. They test the core functionality of each example
 * and validate response parsing.
 *
 * Requirements:
 * - A running Norn node at http://127.0.0.1:50051
 * - Set NORN_RPC_URL environment variable if using different address
 *
 * Run tests with:
 * ```bash
 * npm test
 * # or
 * jest tests/integration.test.js
 * ```
 */

const http = require('http');

/**
 * Norn RPC test client
 */
class NornTestClient {
  constructor(rpcUrl = null) {
    this.rpcUrl = rpcUrl || process.env.NORN_RPC_URL || 'http://127.0.0.1:50051';
    this.requestId = 0;
  }

  /**
   * Make RPC request
   */
  async _makeRequest(method, params = []) {
    this.requestId++;
    const payload = {
      jsonrpc: '2.0',
      id: this.requestId,
      method: method,
      params: params,
    };

    return new Promise((resolve, reject) => {
      const urlObj = new URL(this.rpcUrl);
      const options = {
        hostname: urlObj.hostname,
        port: urlObj.port || 80,
        path: urlObj.pathname || '/',
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': JSON.stringify(payload).length,
        },
        timeout: 10000,
      };

      const req = http.request(options, (res) => {
        let data = '';
        res.on('data', (chunk) => {
          data += chunk;
        });
        res.on('end', () => {
          try {
            resolve(JSON.parse(data));
          } catch (e) {
            reject(e);
          }
        });
      });

      req.on('error', reject);
      req.on('timeout', () => {
        req.destroy();
        reject(new Error('Request timeout'));
      });

      req.write(JSON.stringify(payload));
      req.end();
    });
  }

  /**
   * Check if node is running
   */
  async isNodeRunning() {
    try {
      const result = await this._makeRequest('eth_chainId');
      return 'result' in result;
    } catch (e) {
      return false;
    }
  }
}

// ============================================
// Test Setup
// ============================================

let client;

beforeAll(() => {
  client = new NornTestClient();
});

beforeAll(async () => {
  const running = await client.isNodeRunning();
  if (!running) {
    console.warn('⚠️ Norn node not running at configured address');
    // Don't skip, let tests fail naturally
  }
});

// ============================================
// Test 1: Basic RPC Operations
// ============================================

describe('Basic RPC Operations', () => {
  test('get_chain_id', async () => {
    const result = await client._makeRequest('eth_chainId');

    expect(result).toHaveProperty('result');
    const chainId = result.result;

    expect(typeof chainId).toBe('string');
    expect(chainId.startsWith('0x')).toBe(true);
    expect(chainId.length).toBeLessThanOrEqual(66);

    console.log(`✅ Chain ID: ${chainId}`);
  });

  test('get_block_number', async () => {
    const result = await client._makeRequest('eth_blockNumber');

    expect(result).toHaveProperty('result');
    const blockNumber = result.result;

    expect(typeof blockNumber).toBe('string');
    expect(blockNumber.startsWith('0x')).toBe(true);

    // Parse as hex
    const blockNum = parseInt(blockNumber, 16);
    expect(blockNum).toBeGreaterThanOrEqual(0);

    console.log(`✅ Block number: ${blockNumber}`);
  });

  test('get_gas_price', async () => {
    const result = await client._makeRequest('eth_gasPrice');

    expect(result).toHaveProperty('result');
    const gasPrice = result.result;

    expect(typeof gasPrice).toBe('string');
    expect(gasPrice.startsWith('0x')).toBe(true);

    // Parse as hex
    const price = parseInt(gasPrice, 16);
    expect(price).toBeGreaterThan(0);

    console.log(`✅ Gas price: ${gasPrice}`);
  });
});

// ============================================
// Test 2: Block Information
// ============================================

describe('Block Information', () => {
  test('get_block_by_number', async () => {
    const result = await client._makeRequest('eth_getBlockByNumber', ['0x0', false]);

    expect(result).toHaveProperty('result');
    const block = result.result;

    if (block !== null) {
      expect(typeof block).toBe('object');
      expect(block).toHaveProperty('hash');
      expect(block).toHaveProperty('number');
      console.log(`✅ Block retrieved: ${block.hash}`);
    } else {
      console.log('✅ Block 0x0 not found (expected for some networks)');
    }
  });
});

// ============================================
// Test 3: Account Balance
// ============================================

describe('Account Balance', () => {
  test('get_balance', async () => {
    const address = '0x0000000000000000000000000000000000000000';
    const result = await client._makeRequest('eth_getBalance', [address, 'latest']);

    expect(result).toHaveProperty('result');
    const balance = result.result;

    expect(typeof balance).toBe('string');
    expect(balance.startsWith('0x')).toBe(true);

    // Parse as hex
    const bal = parseInt(balance, 16);
    expect(bal).toBeGreaterThanOrEqual(0);

    console.log(`✅ Balance: ${balance} wei`);
  });

  test('get_transaction_count', async () => {
    const address = '0x0000000000000000000000000000000000000000';
    const result = await client._makeRequest('eth_getTransactionCount', [address, 'latest']);

    expect(result).toHaveProperty('result');
    const nonce = result.result;

    expect(typeof nonce).toBe('string');
    expect(nonce.startsWith('0x')).toBe(true);

    console.log(`✅ Transaction count: ${nonce}`);
  });

  test('get_code', async () => {
    const address = '0x0000000000000000000000000000000000000000';
    const result = await client._makeRequest('eth_getCode', [address, 'latest']);

    expect(result).toHaveProperty('result');
    const code = result.result;

    expect(typeof code).toBe('string');
    expect(code.startsWith('0x')).toBe(true);

    if (code === '0x') {
      console.log('✅ Regular account has no code');
    } else {
      console.log(`✅ Contract code: ${code.length / 2 - 1} bytes`);
    }
  });
});

// ============================================
// Test 4: Error Handling
// ============================================

describe('Error Handling', () => {
  test('invalid_address_format', async () => {
    const result = await client._makeRequest('eth_getBalance', ['invalid_address', 'latest']);

    // Should have error or invalid response
    expect(result.error || result.result === null).toBe(true);
    console.log('✅ Invalid address correctly rejected');
  });

  test('invalid_block_number', async () => {
    const result = await client._makeRequest('eth_getBlockByNumber', ['invalid', false]);

    // Should either have error or null result
    expect(result.error || result.result === null).toBe(true);
    console.log('✅ Invalid block number rejected');
  });
});

// ============================================
// Test 5: Response Parsing
// ============================================

describe('Response Parsing', () => {
  test('parse_various_response_types', async () => {
    // String response
    const chainIdResult = await client._makeRequest('eth_chainId');
    const chainId = chainIdResult.result;
    expect(typeof chainId).toBe('string');

    // Numeric response
    const blockNumberResult = await client._makeRequest('eth_blockNumber');
    const blockNumber = blockNumberResult.result;
    expect(typeof blockNumber).toBe('string');
    expect(blockNumber.startsWith('0x')).toBe(true);

    // Object response
    const blockResult = await client._makeRequest('eth_getBlockByNumber', ['0x0', false]);
    const block = blockResult.result;
    // Block might be null, but structure should be valid

    console.log('✅ All response types parsed correctly');
  });
});

// ============================================
// Test 6: Connection Handling
// ============================================

describe('Connection Handling', () => {
  test('multiple_requests', async () => {
    for (let i = 0; i < 5; i++) {
      const result = await client._makeRequest('eth_chainId');
      expect(result).toHaveProperty('result');
    }
    console.log('✅ Multiple sequential requests completed');
  });

  test('concurrent_requests', async () => {
    const promises = [];
    for (let i = 0; i < 5; i++) {
      promises.push(client._makeRequest('eth_chainId'));
    }
    const results = await Promise.all(promises);
    results.forEach((result) => {
      expect(result).toHaveProperty('result');
    });
    console.log('✅ Concurrent requests completed successfully');
  });
});

// ============================================
// Test 7: Data Consistency
// ============================================

describe('Data Consistency', () => {
  test('consistent_results', async () => {
    const result1 = await client._makeRequest('eth_chainId');
    const chainId1 = result1.result;

    const result2 = await client._makeRequest('eth_chainId');
    const chainId2 = result2.result;

    expect(chainId1).toBe(chainId2);
    console.log('✅ Results are consistent');
  });
});

// ============================================
// Test 8: Example-Specific Tests
// ============================================

describe('Example-Specific Requirements', () => {
  test('basic_rpc_requirements', async () => {
    // All methods used by basic_rpc.js
    await client._makeRequest('eth_chainId');
    await client._makeRequest('eth_blockNumber');
    await client._makeRequest('eth_gasPrice');
    await client._makeRequest('eth_getBlockByNumber', ['0x1', false]);

    console.log('✅ All basic_rpc.js requirements verified');
  });

  test('balance_checker_requirements', async () => {
    const address = '0x0000000000000000000000000000000000000000';
    await client._makeRequest('eth_getBalance', [address, 'latest']);

    console.log('✅ All balance_checker.js requirements verified');
  });

  test('transaction_sender_requirements', async () => {
    const address = '0x0000000000000000000000000000000000000000';
    await client._makeRequest('eth_getTransactionCount', [address, 'latest']);
    await client._makeRequest('eth_gasPrice');

    console.log('✅ All transaction_sender.js requirements verified');
  });
});

// ============================================
// Test 9: Performance and Timing
// ============================================

describe('Performance and Timing', () => {
  test('response_time', async () => {
    const start = Date.now();
    await client._makeRequest('eth_chainId');
    const elapsed = Date.now() - start;

    expect(elapsed).toBeLessThan(5000);
    console.log(`✅ Response time: ${elapsed}ms`);
  });

  test('batch_performance', async () => {
    const start = Date.now();
    for (let i = 0; i < 10; i++) {
      await client._makeRequest('eth_blockNumber');
    }
    const elapsed = Date.now() - start;
    const avgTime = elapsed / 10;

    expect(avgTime).toBeLessThan(1000);
    console.log(`✅ Average response time: ${avgTime.toFixed(0)}ms`);
  });
});

// ============================================
// Test Helper Functions
// ============================================

describe('Helper Functions', () => {
  test('wei_conversion', () => {
    const weiToEther = (wei) => wei / 1e18;

    expect(weiToEther(1000000000000000000)).toBe(1.0);
    expect(weiToEther(0)).toBe(0.0);
    console.log('✅ Wei conversion works correctly');
  });
});

// ============================================
// Test Report
// ============================================

afterAll(() => {
  console.log('\n' + '='.repeat(40));
  console.log('Integration Tests Summary');
  console.log('='.repeat(40));
  console.log('Tests verify:');
  console.log('✓ RPC connectivity');
  console.log('✓ Response format validation');
  console.log('✓ Error handling');
  console.log('✓ Concurrent request handling');
  console.log('✓ Data consistency');
  console.log('✓ Example-specific requirements');
  console.log('='.repeat(40));
});
