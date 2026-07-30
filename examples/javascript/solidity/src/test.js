/**
 * End-to-End Contract Test
 *
 * This script demonstrates the full lifecycle:
 * 1. Compile a Solidity contract
 * 2. Deploy it to the blockchain
 * 3. Interact with it (read/write)
 * 4. Verify state changes
 *
 * Usage:
 *   node src/test.js
 */

require('dotenv').config();
const { NornSoliditySDK } = require('./index');
const fs = require('fs');
const path = require('path');

// Sample Solidity contract for testing
const SAMPLE_CONTRACT = `
pragma solidity ^0.8.20;

contract SimpleStorage {
    uint256 private value;

    event ValueChanged(address indexed by, uint256 newValue);

    function set(uint256 x) public {
        value = x;
        emit ValueChanged(msg.sender, x);
    }

    function get() public view returns (uint256) {
        return value;
    }

    function increment() public {
        value = value + 1;
    }
}
`;

async function main() {
    console.log('=== Norn Smart Contract E2E Test ===\n');

    const sdk = new NornSoliditySDK();
    console.log('1. SDK initialized');
    console.log(`   RPC: ${sdk.rpcUrl}`);

    // Check connection
    try {
        const blockNumber = await sdk.getBlockNumber();
        console.log(`   Connected! Block: ${blockNumber}`);
    } catch (error) {
        console.error('   Failed to connect to Norn node:', error.message);
        process.exit(1);
    }

    // Check wallet
    if (!sdk.wallet) {
        console.error('   ERROR: No private key configured. Set PRIVATE_KEY in .env');
        process.exit(1);
    }

    const deployer = await sdk.getSigner().getAddress();
    const balance = await sdk.getBalance(deployer);
    console.log(`   Deployer: ${deployer}`);
    console.log(`   Balance: ${ethers.formatEther(balance)} ETH`);

    // Compile contract
    console.log('\n2. Compiling contract...');
    const { SolidityCompiler } = require('../../crates/solidity');
    const compiler = new SolidityCompiler({ min_version: '0.8.0' });
    const artifacts = compiler.compile_source(SAMPLE_CONTRACT, 'SimpleStorage');

    if (!artifacts.isSuccess()) {
        console.error('Compilation failed:', artifacts.errors);
        process.exit(1);
    }

    const artifact = artifacts.contracts[0];
    console.log(`   Contract: ${artifact.name}`);
    console.log(`   Bytecode: ${artifact.bytecode.length / 2} bytes`);
    console.log(`   Functions: ${artifact.abi.filter(a => a.type === 'function').length}`);

    // Deploy contract
    console.log('\n3. Deploying contract...');
    const contract = await sdk.deploy(artifact, [], { gasLimit: 5_000_000 });
    const address = await contract.getAddress();
    console.log(`   Address: ${address}`);

    // Test 1: Call get() - should return 0
    console.log('\n4. Testing initial state...');
    let value = await sdk.call(address, 'get');
    console.log(`   get() = ${value}`);
    assert(value === 0n, 'Initial value should be 0');

    // Test 2: Set value
    console.log('\n5. Setting value to 42...');
    await sdk.send(address, 'set', [42n]);
    value = await sdk.call(address, 'get');
    console.log(`   get() = ${value}`);
    assert(value === 42n, 'Value should be 42 after set');

    // Test 3: Increment
    console.log('\n6. Incrementing value...');
    await sdk.send(address, 'increment');
    value = await sdk.call(address, 'get');
    console.log(`   get() = ${value}`);
    assert(value === 43n, 'Value should be 43 after increment');

    // Test 4: Listen for events
    console.log('\n7. Checking events...');
    const events = await sdk.getEvents(address, 'ValueChanged');
    console.log(`   ValueChanged events: ${events.length}`);
    if (events.length > 0) {
        console.log(`   Last event: by=${events[events.length - 1].args[0]}, value=${events[events.length - 1].args[1]}`);
    }

    // Summary
    console.log('\n=== Test Results ===');
    console.log('All tests passed!');
    console.log(`\nContract: ${artifact.name}`);
    console.log(`Address: ${address}`);
    console.log(`Network: Chain ID ${sdk.chainId}`);
}

function assert(condition, message) {
    if (!condition) {
        console.error(`   FAIL: ${message}`);
        process.exit(1);
    }
    console.log(`   PASS: ${message}`);
}

main().catch(error => {
    console.error('Fatal error:', error);
    process.exit(1);
});
