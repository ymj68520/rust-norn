#!/usr/bin/env node

/**
 * 检查测试账户余额
 */

const axios = require('axios');
const ethers = require('ethers');

const RPC_URL = process.env.NORN_RPC_URL || 'http://127.0.0.1:50991';

// Hardhat 默认测试账户
const TEST_ACCOUNT = '0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266';

async function checkBalance() {
  console.log("========================================");
  console.log("  Norn 账户余额检查");
  console.log("========================================");
  console.log("");

  console.log("测试账户: " + TEST_ACCOUNT);
  console.log("");

  // 查询余额
  const accountWithoutPrefix = TEST_ACCOUNT.toLowerCase().replace('0x', '');

  try {
    const response = await axios.post(RPC_URL, {
      jsonrpc: '2.0',
      method: 'eth_getBalance',
      params: [accountWithoutPrefix, 'latest'],
      id: 1
    });

    const balanceHex = response.data.result;
    const balanceWei = BigInt(balanceHex || 0);
    const balanceEther = Number(ethers.formatEther(balanceWei));

    console.log("余额: " + balanceEther + " ETH");
    console.log("");

    if (balanceEther === 0) {
      console.log("========================================");
      console.log("⚠️  账户余额为 0");
      console.log("========================================");
      console.log("");
      console.log("由于测试账户没有 ETH，无法部署智能合约。");
      console.log("");
      console.log("建议解决方案:");
      console.log("");
      console.log("1. 最简单 - 在 state manager 中添加充值功能");
      console.log("2. 或者 - 修改创世块预分配余额");
      console.log("3. 或者 - 添加 dev RPC 方法（如 dev_impersonateAccount）");
      console.log("");

      console.log("是否现在添加快速充值功能？(y/n)");

    } else {
      console.log("账户有余额，可以部署合约！");
    }

  } catch (error) {
    console.error("查询失败:", error.message);
  }
}

checkBalance();
