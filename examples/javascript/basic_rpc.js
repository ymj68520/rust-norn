require('dotenv').config();
const axios = require('axios');

const RPC_URL = process.env.NORN_RPC_URL || 'http://127.0.0.1:50051';

const client = axios.create({
  baseURL: RPC_URL,
  timeout: 30000,
});

let requestId = 0;

async function makeRequest(method, params = []) {
  requestId++;
  const payload = {
    jsonrpc: '2.0',
    id: requestId,
    method,
    params,
  };

  try {
    const response = await client.post('', payload);
    return response.data;
  } catch (error) {
    console.error(`RPC request failed: ${error.message}`);
    throw error;
  }
}

async function getChainId() {
  const response = await makeRequest('eth_chainId');
  return response.result;
}

async function getBlockNumber() {
  const response = await makeRequest('eth_blockNumber');
  return response.result;
}

async function getBlockByNumber(blockNumber, fullTx = false) {
  const response = await makeRequest('eth_getBlockByNumber', [blockNumber, fullTx]);
  return response.result;
}

async function getGasPrice() {
  const response = await makeRequest('eth_gasPrice');
  return response.result;
}

async function main() {
  console.log('=== Basic RPC Example ===\n');

  try {
    console.log('1. Get Chain ID');
    const chainId = await getChainId();
    console.log(`   Chain ID: ${chainId}\n`);

    console.log('2. Get Latest Block Number');
    const blockNum = await getBlockNumber();
    console.log(`   Block Number: ${blockNum}\n`);

    console.log('3. Get Block Information');
    try {
      const block = await getBlockByNumber('0x1', false);
      if (block) {
        console.log(`   Block Hash: ${block.hash}`);
        console.log(`   Miner: ${block.miner}`);
        console.log(`   Timestamp: ${block.timestamp}\n`);
      } else {
        console.log('   Block not found\n');
      }
    } catch (error) {
      console.log(`   Block not available yet\n`);
    }

    console.log('4. Get Gas Price');
    const gasPrice = await getGasPrice();
    const gasPriceWei = parseInt(gasPrice, 16);
    console.log(`   Gas Price: ${gasPriceWei} wei\n`);

    console.log('✅ Basic RPC example completed!');
  } catch (error) {
    console.error('❌ Error:', error.message);
  }
}

main();
