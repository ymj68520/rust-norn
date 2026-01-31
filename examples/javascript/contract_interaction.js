/**
 * Smart Contract Interaction Example
 *
 * This example demonstrates how to interact with smart contracts:
 * - Encoding contract function calls using ABI
 * - Reading from contracts (eth_call)
 * - Calling contract functions (eth_sendRawTransaction)
 * - Decoding return values
 * - Handling ERC-20 tokens as a real-world example
 *
 * In production, use ethers.js or web3.js for automatic ABI encoding/decoding.
 * This example shows the concepts for educational purposes.
 */

require('dotenv').config();

/**
 * Client for interacting with smart contracts via RPC
 */
class ContractClient {
  constructor(rpcUrl) {
    this.rpcUrl = rpcUrl;
  }

  /**
   * Call a contract function (read-only, no gas cost).
   * Uses eth_call for view/pure functions.
   */
  async callFunction(fromAddress, contractAddress, data) {
    const payload = {
      jsonrpc: '2.0',
      method: 'eth_call',
      params: [
        {
          from: fromAddress,
          to: contractAddress,
          data: data
        },
        'latest'
      ],
      id: 1
    };

    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });

      const result = await response.json();

      if (result.result !== undefined) {
        return result.result;
      } else if (result.error) {
        throw new Error(`RPC Error: ${JSON.stringify(result.error)}`);
      } else {
        throw new Error('Unexpected response format');
      }
    } catch (error) {
      throw error;
    }
  }

  /**
   * Get the balance of an ERC-20 token for an address.
   * Encodes: balanceOf(address)
   * Selector: 0x70a08231
   */
  async getErc20Balance(tokenAddress, accountAddress) {
    // Pad address to 32 bytes
    const paddedAddress = accountAddress
      .toLowerCase()
      .replace('0x', '')
      .padStart(64, '0');
    const data = `0x70a08231${paddedAddress}`;

    const result = await this.callFunction(
      '0x0000000000000000000000000000000000000000',
      tokenAddress,
      data
    );

    return result;
  }

  /**
   * Encode a transfer call for an ERC-20 token.
   * Returns the encoded data to be used in a transaction.
   */
  encodeErc20Transfer(recipient, amountWei) {
    // Selector for transfer(address,uint256)
    let data = '0xa9059cbb';

    // Encode recipient address (pad to 32 bytes)
    const paddedRecipient = recipient
      .toLowerCase()
      .replace('0x', '')
      .padStart(64, '0');
    data += paddedRecipient;

    // Encode amount (pad to 32 bytes)
    const amountInt = BigInt(amountWei);
    const amountHex = amountInt.toString(16).padStart(64, '0');
    data += amountHex;

    return data;
  }

  /**
   * Read contract storage at a specific position.
   * Useful for reading state variables directly.
   */
  async getStorageAt(contractAddress, position) {
    const payload = {
      jsonrpc: '2.0',
      method: 'eth_getStorageAt',
      params: [contractAddress, position, 'latest'],
      id: 1
    };

    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });

      const result = await response.json();

      if (result.result !== undefined) {
        return result.result;
      } else if (result.error) {
        throw new Error(`RPC Error: ${JSON.stringify(result.error)}`);
      } else {
        throw new Error('Unexpected response format');
      }
    } catch (error) {
      throw error;
    }
  }

  /**
   * Get the bytecode of a contract.
   * Returns '0x' if address is not a contract.
   */
  async getCode(contractAddress) {
    const payload = {
      jsonrpc: '2.0',
      method: 'eth_getCode',
      params: [contractAddress, 'latest'],
      id: 1
    };

    try {
      const response = await fetch(this.rpcUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });

      const result = await response.json();

      if (result.result !== undefined) {
        return result.result;
      } else if (result.error) {
        throw new Error(`RPC Error: ${JSON.stringify(result.error)}`);
      } else {
        throw new Error('Unexpected response format');
      }
    } catch (error) {
      throw error;
    }
  }

  /**
   * Decode uint256 from hex string (big-endian)
   */
  static decodeUint256(hexStr) {
    const cleaned = hexStr.replace('0x', '');
    return BigInt(`0x${cleaned}`);
  }

  /**
   * Format wei to readable token amount (default 18 decimals like ETH)
   */
  static formatTokenAmount(wei, decimals = 18) {
    const divisor = BigInt(10 ** decimals);
    return Number(wei) / Number(divisor);
  }
}

/**
 * Main example demonstrating contract interactions
 */
async function main() {
  const rpcUrl = process.env.RPC_URL || 'http://localhost:8545';
  const client = new ContractClient(rpcUrl);

  console.log('=== Smart Contract Interaction Examples ===\n');

  // Example 1: Verify if an address is a contract
  const exampleContract = '0x0000000000000000000000000000000000000001';
  console.log('1. Checking if address is a contract:');
  console.log(`   Address: ${exampleContract}`);
  try {
    const code = await client.getCode(exampleContract);
    if (code === '0x') {
      console.log('   Result: Not a contract (EOA or empty)');
    } else {
      console.log(
        `   Result: Contract found (code length: ${(code.length - 2) / 2} bytes)`
      );
    }
  } catch (error) {
    console.log(`   Error: ${error.message}`);
  }

  // Example 2: Encode ERC-20 transfer call
  console.log('\n2. Encoding ERC-20 transfer call:');
  const recipient = '0x742d35Cc6634C0532925a3b844Bc9e7595f32D23';
  const amountWei = '1000000000000000000'; // 1 token (18 decimals)

  try {
    const encodedData = client.encodeErc20Transfer(recipient, amountWei);
    console.log(`   Recipient: ${recipient}`);
    console.log(`   Amount: ${amountWei} wei`);
    console.log(`   Encoded data: ${encodedData}`);
    console.log('   (This data would be used in eth_sendRawTransaction)');
  } catch (error) {
    console.log(`   Error: ${error.message}`);
  }

  // Example 3: Demonstrate storage access pattern
  console.log('\n3. Contract Storage Access Pattern:');
  console.log('   To read contract state, use eth_getStorageAt');
  console.log('   - Position 0: Often total supply for ERC-20');
  console.log('   - Position 1: Often owner address');
  console.log('   - Position 2+: Depends on contract design');
  console.log('   Storage slots are 32 bytes (256 bits)');

  // Example 4: Educational explanation of ABI encoding
  console.log('\n4. Understanding ABI Encoding:');
  console.log('   For function: transfer(address to, uint256 amount)');
  console.log(
    "   - Selector (first 4 bytes): keccak256('transfer(address,uint256)')[0:4]"
  );
  console.log('   - Parameter 1 (address): Padded to 32 bytes');
  console.log('   - Parameter 2 (uint256): Padded to 32 bytes');
  console.log('   - Total: 4 + 32 + 32 = 68 bytes (136 hex chars)');

  // Example 5: Common contract addresses format
  console.log('\n5. Working with Contract Addresses:');
  const testAddresses = [
    ['EOA Example', '0x742d35Cc6634C0532925a3b844Bc9e7595f32D23'],
    ['Contract Example', '0x0000000000000000000000000000000000000001'],
    ['Zero Address', '0x0000000000000000000000000000000000000000']
  ];

  for (const [name, addr] of testAddresses) {
    console.log(`   ${name}: ${addr}`);
  }

  console.log('\n=== Key Points ===');
  console.log('✓ Use eth_call for read-only contract calls (no gas, no state changes)');
  console.log(
    '✓ Use eth_sendRawTransaction for state-changing calls (costs gas)'
  );
  console.log('✓ ABI encoding is deterministic - same call always produces same data');
  console.log('✓ Always verify you\'re calling the correct contract address');
  console.log('✓ In production, use ethers.js or web3.js for automatic ABI encoding');
}

main().catch(console.error);
