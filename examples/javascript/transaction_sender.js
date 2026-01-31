require('dotenv').config();
const axios = require('axios');

const RPC_URL = process.env.NORN_RPC_URL || 'http://127.0.0.1:50051';
const ACCOUNT = process.env.ACCOUNT_ADDRESS || '0x0000000000000000000000000000000000000000';
const RECIPIENT = process.env.RECIPIENT_ADDRESS || '0x1111111111111111111111111111111111111111';

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

  const response = await client.post('', payload);
  return response.data;
}

async function getTransactionCount(address, block = 'latest') {
  const response = await makeRequest('eth_getTransactionCount', [address, block]);
  return parseInt(response.result, 16);
}

async function getGasPrice() {
  const response = await makeRequest('eth_gasPrice');
  return parseInt(response.result, 16);
}

async function sendRawTransaction(signedTx) {
  const response = await makeRequest('eth_sendRawTransaction', [signedTx]);
  if (response.error) {
    throw new Error(`Transaction failed: ${response.error.message}`);
  }
  return response.result;
}

async function getTransactionReceipt(txHash) {
  const response = await makeRequest('eth_getTransactionReceipt', [txHash]);
  return response.result;
}

async function waitForReceipt(txHash, maxRetries = 30, retryDelay = 1000) {
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    const receipt = await getTransactionReceipt(txHash);
    if (receipt) {
      return receipt;
    }
    console.log(`   Waiting for receipt... (attempt ${attempt + 1}/${maxRetries})`);
    await new Promise(resolve => setTimeout(resolve, retryDelay));
  }
  return null;
}

function printTransactionExample(from, to, nonce) {
  console.log('\n=== Example Transaction Structure ===');
  console.log(`From: ${from}`);
  console.log(`To: ${to}`);
  console.log(`Nonce: ${nonce}`);
  console.log(`Value: 1000000000000000000 wei (1 ether)`);
  console.log(`Gas Price: 1000000000 wei`);
  console.log(`Gas Limit: 21000`);
  console.log(`Data: 0x (empty for value transfer)`);
  console.log('\nTo send this transaction:');
  console.log('1. Create the transaction structure');
  console.log('2. Sign it with your private key using ECDSA');
  console.log('3. Encode it as RLP');
  console.log('4. Send via eth_sendRawTransaction with 0x prefix');
}

async function main() {
  console.log('=== Transaction Sender Example ===\n');

  try {
    console.log(`From: ${ACCOUNT}`);
    console.log(`To: ${RECIPIENT}\n`);

    console.log('1. Get Current Nonce');
    const nonce = await getTransactionCount(ACCOUNT, 'latest');
    console.log(`   Nonce: ${nonce}\n`);

    console.log('2. Get Gas Price');
    const gasPrice = await getGasPrice();
    console.log(`   Gas Price: ${gasPrice} wei\n`);

    console.log('3. Transaction Information');
    printTransactionExample(ACCOUNT, RECIPIENT, nonce);

    console.log('\n4. Transaction Sending Steps');
    console.log('   ✓ Create transaction object');
    console.log('   ✓ Sign with private key (ECDSA)');
    console.log('   ✓ Encode as RLP');
    console.log('   ✓ Send via eth_sendRawTransaction\n');

    console.log('NOTE: To send actual transactions:');
    console.log('1. Implement transaction signing');
    console.log('2. Use ethers.js or web3.js for signing');
    console.log('3. Send the signed transaction\n');

    console.log('Example with ethers.js (not included in this example):');
    console.log('```javascript');
    console.log("const ethers = require('ethers');");
    console.log("const provider = new ethers.providers.JsonRpcProvider(RPC_URL);");
    console.log("const wallet = new ethers.Wallet(PRIVATE_KEY, provider);");
    console.log('const tx = {');
    console.log(`  to: '${RECIPIENT}',`);
    console.log(`  value: ethers.utils.parseEther('1.0'),`);
    console.log('};');
    console.log('const txResponse = await wallet.sendTransaction(tx);');
    console.log('const receipt = await txResponse.wait();');
    console.log('```\n');

    console.log('✅ Transaction sender example completed!');
  } catch (error) {
    console.error('❌ Error:', error.message);
  }
}

main();
