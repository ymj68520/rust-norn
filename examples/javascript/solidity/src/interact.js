/**
 * Contract Interaction Example
 *
 * This script demonstrates how to interact with a deployed Solidity contract:
 * - Read contract state (view/pure functions)
 * - Send transactions (state-changing functions)
 * - Decode return values
 * - Listen for events
 *
 * Usage:
 *   node src/interact.js [command] [args...]
 *
 * Commands:
 *   call <address> <function> [args...]  - Call a view function
 *   send <address> <function> [args...]  - Send a transaction
 *   events <address> [event]             - Get event logs
 *   deploy                                 - Deploy a new contract
 *
 * Prerequisites:
 * - Running Norn node with Ethereum JSON-RPC enabled
 * - PRIVATE_KEY set in .env (for send transactions)
 */

require('dotenv').config();
const { ethers } = require('ethers');
const { NornSoliditySDK } = require('./index');
const fs = require('fs');

async function main() {
    const args = process.argv.slice(2);
    const command = args[0] || 'help';

    // Initialize SDK
    const sdk = new NornSoliditySDK();
    console.log(`Norn SDK - RPC: ${sdk.rpcUrl}\n`);

    switch (command) {
        case 'deploy':
            await deployContract(sdk);
            break;

        case 'call':
            await callFunction(sdk, args[1], args[2], args.slice(3));
            break;

        case 'send':
            await sendTransaction(sdk, args[1], args[2], args.slice(3));
            break;

        case 'events':
            await getEvents(sdk, args[1], args[2]);
            break;

        case 'balance':
            await checkBalance(sdk, args[1]);
            break;

        case 'help':
        default:
            printHelp();
            break;
    }
}

/**
 * Deploy a new contract
 */
async function deployContract(sdk) {
    console.log('=== Deploy Contract ===\n');

    const artifactPath = process.env.CONTRACT_ARTIFACT;
    if (!artifactPath || !fs.existsSync(artifactPath)) {
        console.error('Error: Set CONTRACT_ARTIFACT in .env to a valid artifact JSON file');
        process.exit(1);
    }

    const artifact = sdk.loadArtifact(artifactPath);
    const contract = await sdk.deploy(artifact, [], { gasLimit: 5_000_000 });
    const address = await contract.getAddress();

    console.log(`\nContract deployed at: ${address}`);
    console.log(`\nTo interact with this contract, run:`);
    console.log(`  node src/interact.js call ${address} <function> [args...]`);
    console.log(`  node src/interact.js send ${address} <function> [args...]`);
}

/**
 * Call a view/pure function
 */
async function callFunction(sdk, address, functionName, params) {
    console.log(`=== Call: ${functionName} ===\n`);

    if (!address || !functionName) {
        console.error('Usage: call <address> <function> [args...]');
        process.exit(1);
    }

    try {
        // Parse params (try to parse as numbers/addresses)
        const parsedParams = params.map(p => {
            if (p.startsWith('0x')) return p;
            if (!isNaN(p)) return ethers.parseUnits(p, 'ether').toBigInt();
            return p;
        });

        console.log(`Contract: ${address}`);
        console.log(`Function: ${functionName}(${parsedParams.join(', ')})`);

        const result = await sdk.call(address, functionName, parsedParams);

        // Format result
        if (result && result.toHexString) {
            console.log(`Result: ${result.toHexString()}`);
        } else if (Array.isArray(result)) {
            console.log(`Result: [${result.map(r => r.toString()).join(', ')}]`);
        } else if (result && typeof result === 'object') {
            console.log(`Result: ${JSON.stringify(result, null, 2)}`);
        } else {
            console.log(`Result: ${result}`);
        }
    } catch (error) {
        console.error(`Error: ${error.message}`);
        process.exit(1);
    }
}

/**
 * Send a transaction
 */
async function sendTransaction(sdk, address, functionName, params) {
    console.log(`=== Send Transaction: ${functionName} ===\n`);

    if (!address || !functionName) {
        console.error('Usage: send <address> <function> [args...]');
        process.exit(1);
    }

    try {
        // Parse params
        const parsedParams = params.map(p => {
            if (p.startsWith('0x')) return p;
            if (!isNaN(p)) return ethers.parseUnits(p, 'ether').toBigInt();
            return p;
        });

        console.log(`Contract: ${address}`);
        console.log(`Function: ${functionName}(${parsedParams.join(', ')})`);

        if (!sdk.wallet) {
            console.error('Error: No private key configured. Set PRIVATE_KEY in .env');
            process.exit(1);
        }

        const receipt = await sdk.send(address, functionName, parsedParams);
        console.log(`\nTransaction confirmed in block ${receipt.blockNumber}`);
    } catch (error) {
        console.error(`Error: ${error.message}`);
        process.exit(1);
    }
}

/**
 * Get event logs
 */
async function getEvents(sdk, address, eventName) {
    console.log('=== Contract Events ===\n');

    if (!address) {
        console.error('Usage: events <address> [event_name]');
        process.exit(1);
    }

    try {
        const events = await sdk.getEvents(address, eventName);

        if (events.length === 0) {
            console.log('No events found.');
            return;
        }

        console.log(`Found ${events.length} event(s):\n`);
        for (const event of events) {
            console.log(`Block ${event.blockNumber}:`);
            console.log(`  Transaction: ${event.transactionHash}`);
            if (event.args) {
                console.log(`  Args: ${JSON.stringify(event.args, (k, v) =>
                    typeof v === 'object' && v !== null && v._isBigNumber
                        ? v.toString()
                        : v
                )}`);
            }
            console.log();
        }
    } catch (error) {
        console.error(`Error: ${error.message}`);
        process.exit(1);
    }
}

/**
 * Check balance of an address
 */
async function checkBalance(sdk, address) {
    if (!address) {
        console.error('Usage: balance <address>');
        process.exit(1);
    }

    const balance = await sdk.getBalance(address);
    console.log(`Address: ${address}`);
    console.log(`Balance: ${ethers.formatEther(balance)} ETH`);
}

/**
 * Print help message
 */
function printHelp() {
    console.log(`
Norn Contract Interaction Tool

Commands:
  deploy                              Deploy a new contract
  call <address> <fn> [args...]       Call a view/pure function
  send <address> <fn> [args...]       Send a state-changing transaction
  events <address> [event_name]       Get event logs
  balance <address>                   Check address balance

Environment Variables:
  NORN_ETH_RPC_URL    Ethereum JSON-RPC endpoint (default: http://localhost:9545)
  PRIVATE_KEY         Private key for signing transactions
  CONTRACT_ARTIFACT   Path to compiled contract artifact JSON
  CHAIN_ID            Chain ID (default: 31337)

Examples:
  # Deploy a contract
  npm run deploy

  # Call a view function
  node src/interact.js call 0x1234... getValue

  # Send a transaction
  node src/interact.js send 0x1234... setValue 42

  # Get events
  node src/interact.js events 0x1234... Transfer
`);
}

main().catch(error => {
    console.error('Fatal error:', error);
    process.exit(1);
});
