/**
 * Norn Solidity SDK
 *
 * This SDK provides a high-level interface for deploying and interacting with
 * Solidity smart contracts on the Norn blockchain via the Ethereum JSON-RPC API.
 *
 * Features:
 * - Compile Solidity contracts (requires solc or artifact files)
 * - Deploy contracts to the Norn blockchain
 * - Call contract functions with type-safe encoding
 * - Decode contract return values
 * - Listen to contract events
 *
 * Prerequisites:
 * - A running Norn node with Ethereum JSON-RPC enabled
 * - ethers.js v6
 * - A compiled contract artifact (JSON) or solc compiler installed
 */

require('dotenv').config();
const { ethers } = require('ethers');
const fs = require('fs');
const path = require('path');

/**
 * NornSoliditySDK - Main SDK class for Solidity contract interaction
 */
class NornSoliditySDK {
    /**
     * Create a new SDK instance
     * @param {string} rpcUrl - Ethereum JSON-RPC endpoint URL
     * @param {string} privateKey - Private key for signing transactions (optional)
     */
    constructor(rpcUrl, privateKey) {
        this.rpcUrl = rpcUrl || process.env.NORN_ETH_RPC_URL || 'http://localhost:9545';
        this.privateKey = privateKey || process.env.PRIVATE_KEY;
        this.chainId = parseInt(process.env.CHAIN_ID) || 31337;

        // Create ethers provider and wallet
        this.provider = new ethers.JsonRpcProvider(this.rpcUrl);
        this.wallet = this.privateKey
            ? new ethers.Wallet(this.privateKey, this.provider)
            : null;

        // Cache for deployed contracts
        this.contracts = new Map();
    }

    /**
     * Get the connected signer (wallet)
     */
    getSigner() {
        if (!this.wallet) {
            throw new Error('No private key configured. Set PRIVATE_KEY environment variable.');
        }
        return this.wallet;
    }

    /**
     * Get the provider for read-only operations
     */
    getProvider() {
        return this.provider;
    }

    /**
     * Load a compiled contract artifact
     * @param {string} artifactPath - Path to the JSON artifact file
     * @returns {Object} Parsed artifact
     */
    loadArtifact(artifactPath) {
        const fullPath = path.resolve(artifactPath);
        if (!fs.existsSync(fullPath)) {
            throw new Error(`Artifact file not found: ${fullPath}`);
        }

        const artifact = JSON.parse(fs.readFileSync(fullPath, 'utf8'));
        return artifact;
    }

    /**
     * Deploy a contract from a compiled artifact
     * @param {Object} artifact - Compiled contract artifact
     * @param {Array} constructorArgs - Constructor arguments
     * @param {Object} options - Deployment options (gasLimit, value, etc.)
     * @returns {Promise<Contract>} Deployed contract instance
     */
    async deploy(artifact, constructorArgs = [], options = {}) {
        if (!this.wallet) {
            throw new Error('No private key configured. Cannot deploy contracts.');
        }

        const bytecode = artifact.bytecode || artifact.deployment_bytecode;
        if (!bytecode) {
            throw new Error('Artifact missing bytecode');
        }

        // Get the ABI
        const abi = artifact.abi || artifact.abi_items;
        if (!abi || abi.length === 0) {
            throw new Error('Artifact missing ABI');
        }

        console.log(`Deploying contract: ${artifact.name || 'Unknown'}`);
        console.log(`  Bytecode length: ${bytecode.length / 2} bytes`);
        console.log(`  Constructor args: ${constructorArgs.length} parameters`);

        // Create contract factory
        const factory = new ethers.ContractFactory(abi, bytecode, this.getSigner());

        // Estimate gas
        let gasLimit = options.gasLimit;
        if (!gasLimit) {
            try {
                gasLimit = await factory.getDeployTransaction(...constructorArgs).then(tx => tx.gasLimit);
                gasLimit = Math.ceil(gasLimit * 1.2); // Add 20% buffer
            } catch (e) {
                gasLimit = 5_000_000; // Default fallback
            }
        }

        // Deploy
        const contract = await factory.deploy(...constructorArgs, {
            gasLimit: gasLimit,
            ...options,
        });

        console.log(`  Deployment transaction: ${contract.deploymentTransaction().hash}`);
        console.log(`  Waiting for confirmation...`);

        await contract.waitForDeployment();
        const address = await contract.getAddress();

        console.log(`  Contract deployed at: ${address}`);

        // Cache the contract
        this.contracts.set(address, contract);

        return contract;
    }

    /**
     * Attach to an already-deployed contract
     * @param {string} address - Contract address
     * @param {Object} artifact - Compiled contract artifact
     * @returns {Contract} Contract instance
     */
    attach(address, artifact) {
        const abi = artifact.abi || artifact.abi_items;
        if (!abi || abi.length === 0) {
            throw new Error('Artifact missing ABI');
        }

        const contract = new ethers.Contract(address, abi, this.getSigner());
        this.contracts.set(address, contract);
        return contract;
    }

    /**
     * Call a read-only function on a contract (eth_call)
     * @param {string} address - Contract address
     * @param {string} functionName - Function name
     * @param {Array} params - Function parameters
     * @param {Object} artifact - Contract artifact (for ABI)
     * @returns {Promise<*>} Decoded return value(s)
     */
    async call(address, functionName, params = [], artifact = null) {
        const contract = this.contracts.get(address);

        if (!contract && artifact) {
            // Auto-attach if we have the artifact
            this.attach(address, artifact);
        }

        const targetContract = this.contracts.get(address);
        if (!targetContract) {
            throw new Error(`Contract not found at ${address}. Call attach() first.`);
        }

        try {
            const result = await targetContract[functionName](...params);
            return result;
        } catch (error) {
            console.error(`Call failed: ${functionName}(${params.join(', ')})`);
            throw error;
        }
    }

    /**
     * Send a transaction to a contract function
     * @param {string} address - Contract address
     * @param {string} functionName - Function name
     * @param {Array} params - Function parameters
     * @param {Object} options - Transaction options
     * @returns {Promise<TransactionReceipt>} Transaction receipt
     */
    async send(address, functionName, params = [], options = {}) {
        const contract = this.contracts.get(address);
        if (!contract) {
            throw new Error(`Contract not found at ${address}. Call attach() first.`);
        }

        try {
            const tx = await contract[functionName](...params, options);
            console.log(`  Transaction: ${tx.hash}`);

            const receipt = await tx.wait();
            console.log(`  Block: ${receipt.blockNumber}, Gas used: ${receipt.gasUsed.toString()}`);

            return receipt;
        } catch (error) {
            console.error(`Transaction failed: ${functionName}(${params.join(', ')})`);
            throw error;
        }
    }

    /**
     * Encode function call data manually
     * @param {string} abi - Contract ABI
     * @param {string} functionName - Function name
     * @param {Array} params - Parameters
     * @returns {string} Encoded call data (hex)
     */
    encodeFunctionCall(abi, functionName, params) {
        const iface = new ethers.Interface(abi);
        return iface.encodeFunctionData(functionName, params);
    }

    /**
     * Decode function return data
     * @param {string} abi - Contract ABI
     * @param {string} functionName - Function name
     * @param {string} data - Return data (hex)
     * @returns {Array} Decoded values
     */
    decodeFunctionResult(abi, functionName, data) {
        const iface = new ethers.Interface(abi);
        return iface.decodeFunctionResult(functionName, data);
    }

    /**
     * Get contract event logs
     * @param {string} address - Contract address
     * @param {string} eventName - Event name
     * @param {Object} filter - Event filter options
     * @returns {Array} Array of event logs
     */
    async getEvents(address, eventName, filter = {}) {
        const contract = this.contracts.get(address);
        if (!contract) {
            throw new Error(`Contract not found at ${address}`);
        }

        const fromBlock = filter.fromBlock || 0;
        const toBlock = filter.toBlock || 'latest';

        const events = await contract.queryFilter(eventName, fromBlock, toBlock);
        return events.map(e => ({
            blockNumber: e.blockNumber,
            transactionHash: e.transactionHash,
            args: e.args,
            ...e,
        }));
    }

    /**
     * Get the balance of an address
     * @param {string} address - Address to check
     * @returns {Promise<string>} Balance in wei (hex string)
     */
    async getBalance(address) {
        const balance = await this.provider.getBalance(address);
        return balance.toString();
    }

    /**
     * Get transaction count (nonce) for an address
     * @param {string} address - Address to check
     * @returns {Promise<number>} Transaction count
     */
    async getTransactionCount(address) {
        return await this.provider.getTransactionCount(address);
    }

    /**
     * Get the latest block number
     * @returns {Promise<number>} Block number
     */
    async getBlockNumber() {
        return await this.provider.getBlockNumber();
    }

    /**
     * Send ETH to an address
     * @param {string} to - Recipient address
     * @param {string} amount - Amount in ETH (string)
     * @returns {Promise<TransactionReceipt>} Transaction receipt
     */
    async sendETH(to, amount) {
        if (!this.wallet) {
            throw new Error('No private key configured.');
        }

        const tx = await this.getSigner().sendTransaction({
            to,
            value: ethers.parseEther(amount),
        });

        console.log(`  Transaction: ${tx.hash}`);
        const receipt = await tx.wait();
        console.log(`  Block: ${receipt.blockNumber}`);
        return receipt;
    }
}

module.exports = { NornSoliditySDK };
