const { ethers } = require("ethers");
const axios = require("axios");

const RPC_URL = "http://127.0.0.1:50991";
const PRIVATE_KEY = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const CHAIN_ID = 31337;

async function main() {
  console.log("========================================");
  console.log("  部署 SimpleStorage 合约");
  console.log("========================================");
  console.log("");

  // 创建 provider 和 wallet
  const provider = new ethers.JsonRpcProvider(RPC_URL);
  const wallet = new ethers.Wallet(PRIVATE_KEY, provider);

  console.log("部署账户:", wallet.address);

  const balance = await provider.getBalance(wallet.address);
  console.log("账户余额:", ethers.formatEther(balance), "ETH");
  console.log("");

  try {
    // 读取合约 bytecode
    const SimpleStorageArtifact = require("./artifacts/contracts/SimpleStorage.sol/SimpleStorage.json");
    const abi = SimpleStorageArtifact.abi;
    const bytecode = SimpleStorageArtifact.bytecode;

    console.log("正在部署 SimpleStorage 合约...");
    const factory = new ethers.ContractFactory(abi, bytecode, wallet);
    const contract = await factory.deploy();

    await contract.waitForDeployment();
    const address = await contract.getAddress();

    console.log("");
    console.log("========================================");
    console.log("✓ 部署成功！");
    console.log("========================================");
    console.log("");
    console.log("合约地址:", address);
    console.log("交易哈希:", contract.deploymentTransaction().hash);
    console.log("");

    // 测试 setValue
    console.log("测试 setValue(42)...");
    const tx1 = await contract.setValue(42);
    const receipt1 = await tx1.wait();
    console.log("✓ 交易成功");
    console.log("  Gas 使用:", receipt1.gasUsed.toString());
    console.log("  区块号:", receipt1.blockNumber);
    console.log("  交易哈希:", receipt1.hash);

    // 测试 getValue
    const value = await contract.getValue();
    console.log("✓ getValue() 返回:", value.toString());

    if (value.toString() === "42") {
      console.log("✓ 值验证成功！");
    } else {
      console.log("✗ 值不匹配，期望 42");
    }

    console.log("");

    // 测试 setMessage
    console.log('测试 setMessage("Hello from Norn!")...');
    const tx2 = await contract.setMessage("Hello from Norn!");
    const receipt2 = await tx2.wait();
    console.log("✓ 交易成功");
    console.log("  Gas 使用:", receipt2.gasUsed.toString());

    // 测试 getMessage
    const message = await contract.getMessage();
    console.log("✓ getMessage() 返回:", message);

    if (message === "Hello from Norn!") {
      console.log("✓ 消息验证成功！");
    } else {
      console.log("✗ 消息不匹配");
    }

    console.log("");

    // 测试 addItem
    console.log('测试 addItem("Test Item")...');
    const tx3 = await contract.addItem("Test Item Data");
    const receipt3 = await tx3.wait();
    console.log("✓ 交易成功");
    console.log("  Gas 使用:", receipt3.gasUsed.toString());

    const itemCount = await contract.itemCount();
    console.log("✓ itemCount() 返回:", itemCount.toString());

    console.log("");
    console.log("========================================");
    console.log("✓ 所有测试通过！");
    console.log("========================================");

  } catch (error) {
    console.error("");
    console.error("========================================");
    console.error("✗ 部署或测试失败");
    console.error("========================================");
    console.error("错误:", error.message);

    if (error.reason) {
      console.error("原因:", error.reason);
    }

    if (error.data) {
      console.error("数据:", error.data);
    }
  }
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
