require("@nomicfoundation/hardhat-toolbox");
require("dotenv").config();

/** @type import('hardhat/config').HardhatUserConfig */
module.exports = {
  solidity: {
    version: "0.8.20",
    settings: {
      optimizer: {
        enabled: true,
        runs: 200
      }
    }
  },
  networks: {
    norn: {
      url: process.env.NORN_RPC_URL || "http://localhost:8545",
      chainId: 31337,
      accounts: process.env.PRIVATE_KEY ? [process.env.PRIVATE_KEY] : [],
      gasPrice: 1000000000, // 1 Gwei
      gas: "auto"
    },
    norn_local: {
      url: process.env.NORN_RPC_URL || "http://127.0.0.1:50991",
      chainId: 31337,
      accounts: [],
      gasPrice: 1000000000, // 1 Gwei
      gas: "auto"
    }
  },
  paths: {
    sources: "./contracts",
    tests: "./contracts/test",
    cache: "./cache",
    artifacts: "./artifacts"
  },
  mocha: {
    timeout: 40000
  }
};
