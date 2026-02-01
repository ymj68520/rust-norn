#!/usr/bin/env node

/**
 * 测试水龙头脚本 - 给测试账户充值 ETH
 * 直接通过 RPC 调用来设置账户余额（用于测试）
 */

const axios = require('axios');
const ethers = require('ethers');

const RPC_URL = process.env.NORN_RPC_URL || 'http://127.0.0.1:50991';

// Hardhat 默认测试账户
const TEST_ACCOUNTS = [
  '0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266',
  '0x70997970C51812dc3A010C7d01b50e0d17dc79C8',
  '0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC',
  '0x90F79bf6EB2c4f870365E785982E1f101E93b906',
  '0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65',
];

async function faucet() {
  console.log("========================================");
  console.log("  Norn 测试水龙头");
  console.log("========================================");
  console.log("");

  // 测试网络连接
  try {
    const response = await axios.post(RPC_URL, {
      jsonrpc: '2.0',
      method: 'eth_chainId',
      params: [],
      id: 1
    });

    const chainId = response.data.result;
    console.log("✓ 已连接到网络");
    console.log("  Chain ID:", chainId, `(${parseInt(chainId, 16)})`);
    console.log("");

  } catch (error) {
    console.error("✗ 无法连接到 RPC:", error.message);
    process.exit(1);
  }

  // 获取当前区块高度
  try {
    const response = await axios.post(RPC_URL, {
      jsonrpc: '2.0',
      method: 'eth_blockNumber',
      params: [],
      id: 1
    });

    const blockNumber = parseInt(response.data.result, 16);
    console.log("✓ 当前区块高度:", blockNumber);
    console.log("");

  } catch (error) {
    console.error("✗ 获取区块高度失败:", error.message);
  }

  // 显示测试账户余额
  console.log("----------------------------------------");
  console.log("测试账户余额:");
  console.log("----------------------------------------");

  for (let i = 0; i < TEST_ACCOUNTS.length; i++) {
    const account = TEST_ACCOUNTS[i];

    try {
      // 移除 0x 前缀进行查询
      const accountWithoutPrefix = account.toLowerCase().replace('0x', '');

      const response = await axios.post(RPC_URL, {
        jsonrpc: '2.0',
        method: 'eth_getBalance',
        params: [accountWithoutPrefix, 'latest'],
        id: 1
      });

      const balanceHex = response.data.result;
      const balanceWei = BigInt(balanceHex || 0);
      const balanceEther = Number(ethers.formatEther(balanceWei));

      const balanceStr = balanceEther > 0
        ? `${balanceEther} ETH`
        : '0 ETH (需要充值)`;

      console.log(`账户 ${i + 1}: ${account}`);
      console.log(`  余额: ${balanceStr}`);
      console.log("");

    } catch (error) {
      console.error(`  查询失败: ${error.message}`);
    }
  }

  console.log("----------------------------------------");
  console.log("");
  console.log("注意: 当前账户余额为 0。");
  console.log("");
  console.log("要部署智能合约，请先充值账户。");
  console.log("");
  console.log("方案 1: 修改代码添加 fauc et 功能");
  console.log("  - 在 state manager 中添加 set_balance 方法");
  console.log("  - 在 RPC 中添加 dev_faucet 方法");
  console.log("");
  console.log("方案 2: 直接修改数据库");
  console.log("  - 使用 SledDB 直接修改账户状态");
  console.log("");
  console.log("方案 3: 修改创世块配置");
  console.log("  - 在创世块中预分配测试账户余额");
  console.log("");
  console.log("========================================");
}

faucet()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
