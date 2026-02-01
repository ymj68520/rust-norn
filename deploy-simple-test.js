const hre = require("hardhat");

async function main() {
  console.log("========================================");
  console.log("  部署 SimpleStorage 合约");
  console.log("========================================");
  console.log("");

  const [deployer] = await hre.ethers.getSigners();
  console.log("部署账户:", deployer.address);

  const balance = await hre.ethers.provider.getBalance(deployer.address);
  console.log("账户余额:", hre.ethers.formatEther(balance), "ETH");
  console.log("");

  try {
    // 部署 SimpleStorage
    console.log("正在部署 SimpleStorage 合约...");
    const SimpleStorage = await hre.ethers.getContractFactory("SimpleStorage");
    const storage = await SimpleStorage.deploy();
    await storage.waitForDeployment();
    const address = await storage.getAddress();

    console.log("");
    console.log("========================================");
    console.log("✓ 部署成功！");
    console.log("========================================");
    console.log("");
    console.log("合约地址:", address);
    console.log("");

    // 测试 setValue
    console.log("测试 setValue(42)...");
    const tx1 = await storage.setValue(42);
    const receipt1 = await tx1.wait();
    console.log("✓ 交易成功, Gas 使用:", receipt1.gasUsed.toString());

    // 测试 getValue
    const value = await storage.getValue();
    console.log("✓ getValue() 返回:", value.toString());

    if (value.toString() === "42") {
      console.log("✓ 值验证成功！");
    } else {
      console.log("✗ 值不匹配，期望 42");
    }

    console.log("");

    // 测试 setMessage
    console.log('测试 setMessage("Hello from Norn!")...');
    const tx2 = await storage.setMessage("Hello from Norn!");
    const receipt2 = await tx2.wait();
    console.log("✓ 交易成功, Gas 使用:", receipt2.gasUsed.toString());

    // 测试 getMessage
    const message = await storage.getMessage();
    console.log("✓ getMessage() 返回:", message);

    if (message === "Hello from Norn!") {
      console.log("✓ 消息验证成功！");
    } else {
      console.log("✗ 消息不匹配");
    }

    console.log("");
    console.log("========================================");
    console.log("✓ 所有测试通过！");
    console.log("========================================");

  } catch (error) {
    console.error("");
    console.error("========================================");
    console.error("✗ 部署或测试失败");
    console.error("========================================");
    console.error(error.message);
    if (error.reason) {
      console.error("原因:", error.reason);
    }
  }
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
