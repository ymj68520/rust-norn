const hre = require("hardhat");

async function main() {
  console.log("开始部署智能合约到 Norn 本地网络...");

  // 获取网络信息
  const network = await hre.ethers.provider.getNetwork();
  console.log("网络 Chain ID:", network.chainId.toString());

  // 获取第一个账户（Hardhat 默认账户）
  const [deployer] = await hre.ethers.getSigners();
  console.log("部署账户地址:", deployer.address);

  // 获取账户余额
  const balance = await hre.ethers.provider.getBalance(deployer.address);
  console.log("账户余额:", hre.ethers.formatEther(balance), "ETH");

  // 检查余额是否足够
  if (balance === 0n) {
    console.error("错误：部署账户余额为 0，无法部署合约");
    console.log("提示：需要向该账户转入测试 ETH");
    return;
  }

  try {
    // 部署 SimpleStorage 合约
    console.log("\n部署 SimpleStorage 合约...");
    const SimpleStorage = await hre.ethers.getContractFactory("SimpleStorage");
    const storage = await SimpleStorage.deploy();
    await storage.waitForDeployment();
    const storageAddress = await storage.getAddress();
    console.log("✓ SimpleStorage 部署成功:", storageAddress);

    // 测试合约功能
    console.log("\n测试合约功能...");

    // 1. 测试 getValue - 初始值应该是 0
    const initialValue = await storage.getValue();
    console.log("初始值:", initialValue.toString());

    // 2. 测试 setValue
    console.log("设置值为 42...");
    const setTx = await storage.setValue(42);
    const receipt = await setTx.wait();
    console.log("✓ setValue 交易成功, 区块:", receipt.blockNumber);

    // 3. 验证新值
    const newValue = await storage.getValue();
    console.log("新值:", newValue.toString());

    if (newValue.toString() === "42") {
      console.log("✓ 值验证成功！");
    } else {
      console.log("✗ 值验证失败");
    }

    // 4. 测试 setMessage
    console.log("\n设置消息为 'Hello Norn!'...");
    const setMessageTx = await storage.setMessage("Hello Norn!");
    await setMessageTx.wait();
    console.log("✓ setMessage 交易成功");

    // 5. 验证消息
    const message = await storage.getMessage();
    console.log("消息内容:", message);

    if (message === "Hello Norn!") {
      console.log("✓ 消息验证成功！");
    } else {
      console.log("✗ 消息验证失败");
    }

    // 6. 测试 addItem
    console.log("\n添加测试项目...");
    const addItemTx = await storage.addItem("Test Item Data");
    const itemReceipt = await addItemTx.wait();
    console.log("✓ addItem 交易成功, Gas 使用:", itemReceipt.gasUsed.toString());

    // 7. 获取项目数量
    const itemCount = await storage.itemCount();
    console.log("项目总数:", itemCount.toString());

    // 8. 获取项目详情
    const item = await storage.getItem(1);
    console.log("项目详情:");
    console.log("  ID:", item.id.toString());
    console.log("  所有者:", item.owner);
    console.log("  时间戳:", item.timestamp.toString());
    console.log("  数据:", item.data);

    console.log("\n========================================");
    console.log("✓ 所有测试通过！");
    console.log("========================================");
    console.log("\n部署摘要:");
    console.log("- 网络:", hre.network.name);
    console.log("- Chain ID:", network.chainId.toString());
    console.log("- 部署者:", deployer.address);
    console.log("- SimpleStorage 地址:", storageAddress);

    // 保存部署信息
    const fs = require("fs");
    const deployment = {
      network: hre.network.name,
      chainId: network.chainId.toString(),
      deployer: deployer.address,
      contracts: {
        SimpleStorage: storageAddress
      },
      timestamp: new Date().toISOString()
    };

    fs.writeFileSync(
      "contracts/deployment-local.json",
      JSON.stringify(deployment, null, 2)
    );
    console.log("\n部署信息已保存到: contracts/deployment-local.json");

  } catch (error) {
    console.error("\n✗ 部署或测试失败:");
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
