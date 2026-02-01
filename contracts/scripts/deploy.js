const hre = require("hardhat");

async function main() {
  console.log("Deploying contracts to network:", hre.network.name);

  const [deployer] = await hre.ethers.getSigners();
  console.log("Deploying contracts with the account:", deployer.address);

  // Get account balance
  const balance = await hre.ethers.provider.getBalance(deployer.address);
  console.log("Account balance:", hre.ethers.formatEther(balance), "ETH");

  // Deploy NornToken
  console.log("\nDeploying NornToken...");
  const NornToken = await hre.ethers.getContractFactory("NornToken");
  const token = await NornToken.deploy();
  await token.waitForDeployment();
  const tokenAddress = await token.getAddress();
  console.log("NornToken deployed to:", tokenAddress);

  // Deploy SimpleStorage
  console.log("\nDeploying SimpleStorage...");
  const SimpleStorage = await hre.ethers.getContractFactory("SimpleStorage");
  const storage = await SimpleStorage.deploy();
  await storage.waitForDeployment();
  const storageAddress = await storage.getAddress();
  console.log("SimpleStorage deployed to:", storageAddress);

  // Deploy Ballot with sample proposals
  console.log("\nDeploying Ballot...");
  const proposals = ["Proposal 1", "Proposal 2", "Proposal 3"];
  const Ballot = await hre.ethers.getContractFactory("Ballot");
  const ballot = await Ballot.deploy(proposals);
  await ballot.waitForDeployment();
  const ballotAddress = await ballot.getAddress();
  console.log("Ballot deployed to:", ballotAddress);

  // Log deployment summary
  console.log("\n=== Deployment Summary ===");
  console.log("Network:", hre.network.name);
  console.log("Deployer:", deployer.address);
  console.log("\nContracts:");
  console.log("- NornToken:", tokenAddress);
  console.log("- SimpleStorage:", storageAddress);
  console.log("- Ballot:", ballotAddress);

  // Verify contracts
  console.log("\n=== Verification ===");
  console.log("NornToken total supply:", await hre.ethers.formatEther(await token.totalSupply()));
  console.log("SimpleStorage initial value:", await storage.getValue());
  console.log("Ballot proposal count:", await ballot.getAllProposalsLength ? await ballot.getAllProposalsLength() : proposals.length);

  // Save deployment addresses to file
  const fs = require("fs");
  const deployment = {
    network: hre.network.name,
    chainId: (await hre.ethers.provider.getNetwork()).chainId.toString(),
    deployer: deployer.address,
    contracts: {
      NornToken: tokenAddress,
      SimpleStorage: storageAddress,
      Ballot: ballotAddress
    },
    timestamp: new Date().toISOString()
  };

  fs.writeFileSync(
    "contracts/deploy/deployment.json",
    JSON.stringify(deployment, null, 2)
  );
  console.log("\nDeployment info saved to contracts/deploy/deployment.json");
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
