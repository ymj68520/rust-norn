require('dotenv').config();
const axios = require('axios');

const RPC_URL = process.env.NORN_RPC_URL || 'http://127.0.0.1:50051';
const ACCOUNT = process.env.ACCOUNT_ADDRESS || '0x0000000000000000000000000000000000000000';

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

function weiToEther(wei) {
  return parseInt(wei, 16) / 1e18;
}

function etherToWei(ether) {
  return (ether * 1e18).toString(16);
}

async function getBalance(address, block = 'latest') {
  const response = await makeRequest('eth_getBalance', [address, block]);
  return response.result;
}

async function checkBalance(address) {
  const balanceHex = await getBalance(address, 'latest');
  const balanceWei = parseInt(balanceHex, 16);
  const balanceEther = weiToEther(balanceHex);
  return { balanceWei, balanceEther };
}

async function checkMultipleAccounts(addresses) {
  const results = {};
  for (const address of addresses) {
    try {
      const { balanceWei, balanceEther } = await checkBalance(address);
      results[address] = {
        wei: balanceWei,
        ether: balanceEther,
        status: '✅',
      };
    } catch (error) {
      results[address] = {
        error: error.message,
        status: '❌',
      };
    }
  }
  return results;
}

async function trackBalanceHistory(address) {
  const history = {};
  const blocks = ['0x0', '0x1', 'latest'];

  for (const block of blocks) {
    try {
      const balanceHex = await getBalance(address, block);
      history[block] = {
        wei: parseInt(balanceHex, 16),
        ether: weiToEther(balanceHex),
      };
    } catch (error) {
      history[block] = null;
    }
  }

  return history;
}

async function main() {
  console.log('=== Balance Checker Example ===\n');

  try {
    console.log(`Checking balance for: ${ACCOUNT}\n`);

    console.log('1. Current Balance');
    const { balanceWei, balanceEther } = await checkBalance(ACCOUNT);
    console.log(`   Balance (wei): ${balanceWei.toLocaleString()}`);
    console.log(`   Balance (ether): ${balanceEther.toFixed(18)}\n`);

    console.log('2. Balance History');
    const history = await trackBalanceHistory(ACCOUNT);
    for (const [block, balance] of Object.entries(history)) {
      if (balance) {
        console.log(`   Block ${block}: ${balance.ether.toFixed(18)} ether`);
      } else {
        console.log(`   Block ${block}: N/A`);
      }
    }
    console.log();

    console.log('3. Multiple Accounts');
    const accounts = [
      ACCOUNT,
      '0x1111111111111111111111111111111111111111',
      '0x2222222222222222222222222222222222222222',
    ];
    const balances = await checkMultipleAccounts(accounts);
    for (const [addr, data] of Object.entries(balances)) {
      if (data.error) {
        console.log(`   ${addr}: ${data.error} ${data.status}`);
      } else {
        console.log(`   ${addr}: ${data.ether.toFixed(6)} ether ${data.status}`);
      }
    }
    console.log();

    console.log('✅ Balance checker example completed!');
  } catch (error) {
    console.error('❌ Error:', error.message);
  }
}

main();
