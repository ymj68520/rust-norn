/**
 * Solidity Compiler CLI
 *
 * This script compiles Solidity source files to EVM bytecode and ABI artifacts
 * using the solc compiler.
 *
 * Usage:
 *   node src/compile.js <input.sol> [output_dir] [contract_name]
 *
 * Or with environment variables:
 *   SOLIDITY_SOURCE=./contracts/MyContract.sol
 *   OUTPUT_DIR=./artifacts
 *   CONTRACT_NAME=MyContract
 */

require('dotenv').config();
const { SolidityCompiler, SolcConfig } = require('../../crates/solidity');
const fs = require('fs');
const path = require('path');

async function main() {
    const args = process.argv.slice(2);

    // Get arguments
    const inputFile = args[0] || process.env.SOLIDITY_SOURCE;
    const outputDir = args[1] || process.env.OUTPUT_DIR || './artifacts';
    const contractName = args[2] || process.env.CONTRACT_NAME;

    if (!inputFile) {
        console.error('Usage: compile.js <input.sol> [output_dir] [contract_name]');
        console.error('   or: Set SOLIDITY_SOURCE environment variable');
        process.exit(1);
    }

    console.log('=== Solidity Compiler ===\n');
    console.log(`Input:  ${inputFile}`);
    console.log(`Output: ${outputDir}`);
    if (contractName) console.log(`Filter: ${contractName}`);

    // Read source file
    const sourcePath = path.resolve(inputFile);
    if (!fs.existsSync(sourcePath)) {
        console.error(`Error: Source file not found: ${sourcePath}`);
        process.exit(1);
    }

    const source = fs.readFileSync(sourcePath, 'utf8');
    console.log(`Source: ${source.length} bytes\n`);

    // Create compiler
    let compiler;
    try {
        const config = SolcConfig.default();
        compiler = new SolidityCompiler(config);
        console.log(`Compiler: solc ${compiler.version()}`);
    } catch (error) {
        console.error('Failed to initialize compiler:', error.message);
        console.error('\nInstall solc:');
        console.error('  pip install solc-select');
        console.error('  solc-select install 0.8.20');
        console.error('  solc-select use 0.8.20');
        process.exit(1);
    }

    // Compile
    console.log('\nCompiling...');
    let result;
    try {
        result = compiler.compile_source(source, contractName);
    } catch (error) {
        console.error('Compilation failed:', error.message);
        process.exit(1);
    }

    if (!result.isSuccess()) {
        console.error('\nCompilation errors:');
        for (const err of result.errors) {
            console.error(`  ${err.type}: ${err.message}`);
        }
        process.exit(1);
    }

    // Output results
    console.log(`\nCompiled ${result.contracts.length} contract(s)`);

    // Create output directory
    const outputPath = path.resolve(outputDir);
    if (!fs.existsSync(outputPath)) {
        fs.mkdirSync(outputPath, { recursive: true });
    }

    // Write artifacts
    for (const contract of result.contracts) {
        const artifactName = contract.contractName || 'Contract';
        const artifactFile = path.join(outputPath, `${artifactName}.json`);

        // Create artifact in ethers-compatible format
        const artifact = {
            contractName: contract.contract_name,
            abi: contract.abi,
            bytecode: contract.evm.bytecode.object.code,
            deployedBytecode: contract.evm.deployed_bytecode.object.code,
            metadata: contract.metadata,
            compiler: {
                name: 'solc',
                version: compiler.version(),
            },
        };

        fs.writeFileSync(artifactFile, JSON.stringify(artifact, null, 2));
        console.log(`  ${artifactName}: ${artifactFile}`);
        console.log(`    Bytecode: ${contract.evm.bytecode.object.code.length / 2} bytes`);
        console.log(`    Runtime:  ${contract.evm.deployed_bytecode.object.code.length / 2} bytes`);
        console.log(`    ABI:      ${contract.abi.length} items`);
    }

    // Write warnings
    if (result.warnings.length > 0) {
        console.log('\nWarnings:');
        for (const warning of result.warnings) {
            console.log(`  ${warning}`);
        }
    }

    console.log('\nCompilation complete!');
    console.log(`\nArtifacts written to: ${outputPath}`);
    console.log('\nTo deploy, run:');
    console.log(`  node src/deploy.js --artifact ${path.join(outputPath, result.contracts[0].contract_name + '.json')}`);
}

main().catch(error => {
    console.error('Fatal error:', error);
    process.exit(1);
});
