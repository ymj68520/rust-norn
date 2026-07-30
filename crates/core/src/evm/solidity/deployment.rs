//! Contract deployment for the norn EVM
//!
//! This module provides high-level APIs for deploying Solidity-compiled
//! contracts to the norn blockchain.

use super::*;
use crate::evm::abi::ABI;
use norn_common::types::Transaction;

/// Contract deployer
///
/// Provides high-level methods for deploying Solidity contracts to the norn
/// blockchain, including:
/// - Deploying from compiled artifacts
/// - Deploying with constructor arguments
/// - Getting contract address before deployment
pub struct ContractDeployer {
    executor: Arc<EVMExecutor>,
}

impl ContractDeployer {
    /// Create a new contract deployer
    pub fn new(executor: Arc<EVMExecutor>) -> Self {
        Self { executor }
    }

    /// Create a dummy deployer (for bindings without full initialization)
    pub fn dummy() -> Self {
        // Create with a dummy executor - this is only for type binding purposes
        // The deployer should always be initialized with a real executor
        Self {
            executor: Arc::new(EVMExecutor::dummy()),
        }
    }

    /// Get reference to the EVM executor
    pub fn executor(&self) -> &Arc<EVMExecutor> {
        &self.executor
    }

    /// Deploy a contract from compiled bytecode
    ///
    /// # Arguments
    /// * `sender` - Address of the contract deployer
    /// * `nonce` - Sender's transaction nonce
    /// * `bytecode` - Full contract bytecode (constructor + runtime)
    /// * `value` - ETH value to send with the transaction (in wei)
    /// * `gas_limit` - Gas limit for deployment
    ///
    /// # Returns
    /// (contract_address, execution_result)
    pub async fn deploy(
        &self,
        sender: Address,
        nonce: u64,
        bytecode: Vec<u8>,
        value: u128,
        gas_limit: u64,
    ) -> EVMResult<(Address, EVMExecutionResult)> {
        info!(
            "Deploying contract: sender={:?}, bytecode_len={}, value={}",
            sender,
            bytecode.len(),
            value
        );

        // Use EVMExecutor's create_contract
        self.executor
            .create_contract(sender, nonce, bytecode, value, gas_limit)
            .await
    }

    /// Deploy a contract with encoded constructor arguments
    ///
    /// # Arguments
    /// * `sender` - Address of the contract deployer
    /// * `nonce` - Sender's transaction nonce
    /// * `artifact` - Compiled contract artifact
    /// * `constructor_args` - ABI-encoded constructor arguments
    /// * `value` - ETH value to send with the transaction (in wei)
    /// * `gas_limit` - Gas limit for deployment
    pub async fn deploy_with_args(
        &self,
        sender: Address,
        nonce: u64,
        artifact: &NornContractArtifact,
        constructor_args: Vec<u8>,
        value: u128,
        gas_limit: u64,
    ) -> EVMResult<(Address, EVMExecutionResult)> {
        info!(
            "Deploying contract '{}' with constructor args ({} bytes)",
            artifact.name,
            constructor_args.len()
        );

        // Combine deployment bytecode with constructor arguments
        let mut full_bytecode = artifact.bytecode.clone();
        full_bytecode.extend(constructor_args);

        self.deploy(sender, nonce, full_bytecode, value, gas_limit)
            .await
    }

    /// Deploy a compiled contract artifact
    ///
    /// This is the most convenient method - it combines compilation and deployment.
    ///
    /// # Arguments
    /// * `sender` - Address of the contract deployer
    /// * `nonce` - Sender's transaction nonce
    /// * `artifact` - Compiled contract artifact
    /// * `value` - ETH value to send with the transaction (in wei)
    /// * `gas_limit` - Gas limit for deployment
    pub async fn deploy_artifact(
        &self,
        sender: Address,
        nonce: u64,
        artifact: &NornContractArtifact,
        value: u128,
        gas_limit: u64,
    ) -> EVMResult<(Address, EVMExecutionResult)> {
        self.deploy_with_args(sender, nonce, artifact, Vec::new(), value, gas_limit)
            .await
    }

    /// Deploy a contract using a CREATE2-style deterministic address
    ///
    /// # Arguments
    /// * `sender` - Address of the contract deployer
    /// * `salt` - 32-byte salt for address calculation
    /// * `init_code` - Contract initialization code (bytecode + constructor args)
    /// * `value` - ETH value to send with the transaction (in wei)
    /// * `gas_limit` - Gas limit for deployment
    pub async fn deploy_create2(
        &self,
        sender: Address,
        salt: [u8; 32],
        init_code: Vec<u8>,
        value: u128,
        gas_limit: u64,
    ) -> EVMResult<(Address, EVMExecutionResult)> {
        info!(
            "Deploying contract (CREATE2): sender={:?}, init_code_len={}",
            sender,
            init_code.len()
        );

        // Use the EVMExecutor's create2_contract method
        self.executor
            .create2_contract(sender, salt, init_code, value, gas_limit)
            .await
    }

    /// Call a contract function and decode the result
    ///
    /// # Arguments
    /// * `from` - Address of the caller
    /// * `contract_address` - Address of the contract to call
    /// * `artifact` - Compiled contract artifact (for ABI)
    /// * `function_name` - Name of the function to call
    /// * `params` - ABI-encoded parameters
    /// * `value` - ETH value to send
    /// * `gas_limit` - Gas limit
    pub async fn call_function(
        &self,
        from: Address,
        contract_address: Address,
        artifact: &NornContractArtifact,
        function_name: &str,
        params: &[ABIParam],
        value: u128,
        gas_limit: u64,
    ) -> EVMResult<CallResult> {
        // Encode the function call
        let call_data = ABI::encode_function_call(function_name, params)?;

        // Execute the call
        let result = self.executor
            .call_contract(from, contract_address, value, call_data, gas_limit)
            .await?;

        // Find the function in the ABI to determine return types
        let return_types: Vec<ABIType> = artifact.abi_items.iter()
            .filter_map(|item| {
                if let crate::evm::abi::ABIItem::Function { name, outputs, .. } = item {
                    if name == function_name {
                        Some(outputs.iter().map(|p| p.ty.clone()).collect::<Vec<ABIType>>())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        // Decode the return value
        let decoded = if !result.output.is_empty() && !return_types.is_empty() {
            ABI::decode_function_return(&result.output, &return_types)?
        } else {
            Vec::new()
        };

        Ok(CallResult {
            success: result.success,
            return_data: result.output,
            decoded,
            gas_used: result.gas_used,
            error: result.error,
            logs: result.logs,
        })
    }

    /// Perform a static call (read-only, no state changes)
    pub async fn static_call(
        &self,
        from: Address,
        contract_address: Address,
        artifact: &NornContractArtifact,
        function_name: &str,
        params: &[ABIParam],
        gas_limit: u64,
    ) -> EVMResult<CallResult> {
        let call_data = ABI::encode_function_call(function_name, params)?;

        let result = self.executor
            .static_call(from, contract_address, call_data, gas_limit)
            .await?;

        let return_types: Vec<ABIType> = artifact.abi_items.iter()
            .filter_map(|item| {
                if let crate::evm::abi::ABIItem::Function { name, outputs, .. } = item {
                    if name == function_name {
                        Some(outputs.iter().map(|p| p.ty.clone()).collect::<Vec<ABIType>>())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        let decoded = if !result.output.is_empty() && !return_types.is_empty() {
            ABI::decode_function_return(&result.output, &return_types)?
        } else {
            Vec::new()
        };

        Ok(CallResult {
            success: result.success,
            return_data: result.output,
            decoded,
            gas_used: result.gas_used,
            error: result.error,
            logs: result.logs,
        })
    }

    /// Estimate gas for a contract deployment
    pub async fn estimate_deploy_gas(
        &self,
        bytecode: &[u8],
    ) -> EVMResult<u64> {
        // Estimate gas based on bytecode length
        // Base cost for CREATE: 32000
        // Memory expansion cost
        // Code deposit cost: 200 gas per byte
        let base_cost = 32_000u64;
        let memory_cost = ((bytecode.len() as u64 + 31) / 32) * 3;
        let code_cost = bytecode.len() as u64 * 200;

        Ok(base_cost + memory_cost + code_cost)
    }

    /// Estimate gas for a contract function call
    pub async fn estimate_call_gas(
        &self,
        _from: Address,
        _contract_address: Address,
        _data: Vec<u8>,
        _value: u128,
    ) -> EVMResult<u64> {
        // Simple gas estimation for contract calls
        // Base CALL cost + data cost
        let base_cost = 21_000u64;
        let data_cost = _data.len() as u64 * 16;
        Ok(base_cost + data_cost)
    }
}
