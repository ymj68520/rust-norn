/**
 * Contract Deployment Example
 *
 * This script demonstrates how to:
 * 1. Connect to a Norn node
 * 2. Load a compiled contract artifact
 * 3. Deploy the contract to the blockchain
 * 4. Verify the deployment
 *
 * Usage:
 *   npm run deploy
 *
 * Prerequisites:
 * - Running Norn node with Ethereum JSON-RPC enabled
 * - PRIVATE_KEY set in .env
 * - CONTRACT_ARTIFACT set in .env
 */

require('dotenv').config();
const { NornSoliditySDK } = require('./index');
const fs = require('fs');
const path = require('path');

async function main() {
    console.log('=== Norn Contract Deployment Example ===\n');

    // Load environment variables
    const artifactPath = process.env.CONTRACT_ARTIFACT || './artifacts/SimpleStorage.json';
    const constructorArgs = process.env.CONSTRUCTOR_ARGS
        ? JSON.parse(process.env.CONSTRUCTOR_ARGS)
        : [];

    // Initialize SDK
    const sdk = new NornSoliditySDK();
    console.log('1. SDK initialized');
    console.log(`   RPC: ${sdk.rpcUrl}`);
    console.log(`   Chain ID: ${sdk.chainId}`);

    // Check connection
    try {
        const blockNumber = await sdk.getBlockNumber();
        console.log(`   Connected! Latest block: ${blockNumber}`);
    } catch (error) {
        console.error('   Failed to connect to Norn node:', error.message);
        console.error('   Make sure the node is running with --rpc-url flag');
        process.exit(1);
    }

    // Check wallet
    if (!sdk.wallet) {
        console.error('   ERROR: No private key configured. Set PRIVATE_KEY in .env');
        process.exit(1);
    }

    const deployerAddress = await sdk.getSigner().getAddress();
    const balance = await sdk.getBalance(deployerAddress);
    console.log(`   Deployer: ${deployerAddress}`);
    console.log(`   Balance: ${ethers.formatEther(balance)} ETH`);

    // Load artifact
    console.log('\n2. Loading contract artifact...');
    let artifact;
    try {
        artifact = sdk.loadArtifact(artifactPath);
        console.log(`   Contract: ${artifact.name || 'Unknown'}`);
        console.log(`   Bytecode: ${((artifact.bytecode || '').length / 2)} bytes`);
        console.log(`   ABI items: ${(artifact.abi || []).length}`);
    } catch (error) {
        console.error('   Failed to load artifact:', error.message);
        console.error('   Set CONTRACT_ARTIFACT in .env to point to your artifact JSON');
        process.exit(1);
    }

    // Deploy contract
    console.log('\n3. Deploying contract...');
    let contract;
    try {
        contract = await sdk.deploy(artifact, constructorArgs, {
            gasLimit: 5_000_000,
        });
    } catch (error) {
        console.error('   Deployment failed:', error.message);
        process.exit(1);
    }

    // Verify deployment
    console.log('\n4. Verifying deployment...');
    const address = await contract.getAddress();
    console.log(`   Contract address: ${address}`);

    // Check contract code exists
    const code = await sdk.provider.getCode(address);
    if (code === '0x') {
        console.error('   WARNING: No code found at contract address!');
    } else {
        console.log(`   Contract code: ${code.length / 2 - 1} bytes`);
    }

    // Try calling a view function (if the contract has one)
    console.log('\n5. Testing contract interaction...');
    try {
        // Try common view functions
        const viewFunctions = ['get', 'value', 'count', 'balance', 'name', 'symbol', 'totalSupply'];
        for (const fn of viewFunctions) {
            try {
                const result = await sdk.call(address, fn, []);
                console.log(`   ${fn}() = ${JSON.stringify(result)}`);
                break;
            } catch (e) {
                // Function doesn't exist, try next
            }
        }
    } catch (error) {
        console.log(`   (Contract call test skipped: ${error.message})`);
    }

    // Save deployment info
    const deploymentInfo = {
        name: artifact.name,
        address: address,
        deployer: deployerAddress,
        blockNumber: await sdk.getBlockNumber(),
        timestamp: new Date().toISOString(),
        constructorArgs: constructorArgs,
    };

    const outputPath = path.join(path.dirname(artifactPath), 'deployment.json');
    fs.writeFileSync(outputPath, JSON.stringify(deploymentInfo, null, 2));
    console.log(`\n6. Deployment info saved to: ${outputPath}`);

    console.log('\n=== Deployment Complete ===');
    console.log(`Contract: ${artifact.name}`);
    console.log(`Address: ${address}`);
    console.log(`Deployer: ${deployerAddress}`);
    console.log(`Network: Chain ID ${sdk.chainId}`);
}

main().catch(error => {
    console.error('Fatal error:', error);
    process.exit(1);
});
