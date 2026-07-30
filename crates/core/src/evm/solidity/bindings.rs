//! Type-safe contract bindings for the norn EVM
//!
//! This module provides `ContractBindings` which wraps ABI encoding/decoding
//! for type-safe interaction with deployed Solidity contracts.

use super::*;
use keccak_hash::keccak256;

/// Result of a contract function call
#[derive(Debug, Clone)]
pub struct CallResult {
    /// Whether the call succeeded
    pub success: bool,

    /// Raw return data
    pub return_data: Vec<u8>,

    /// Decoded return values
    pub decoded: Vec<crate::evm::abi::ABIParam>,

    /// Gas used
    pub gas_used: u64,

    /// Error message if failed
    pub error: Option<String>,

    /// Logs emitted during execution
    pub logs: Vec<crate::evm::ExecutionLog>,
}

impl CallResult {
    /// Check if the call succeeded
    pub fn is_ok(&self) -> bool {
        self.success
    }

    /// Get the return data as hex string
    pub fn return_data_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.return_data))
    }
}

/// Type-safe contract bindings
///
/// Provides a convenient interface for interacting with a deployed Solidity contract.
/// Created from a compiled artifact and optionally an already-deployed contract address.
pub struct ContractBindings {
    /// Contract name
    name: String,

    /// Contract address (None if not deployed yet)
    address: Option<Address>,

    /// Norn-core ABI items
    abi_items: Vec<crate::evm::abi::ABIItem>,

    /// Raw solc ABI (for reference)
    raw_abi: Vec<norn_solidity::AbiItem>,

    /// Contract deployer reference
    deployer: Arc<ContractDeployer>,
}

impl ContractBindings {
    /// Create new contract bindings from an artifact
    pub fn new(artifact: &NornContractArtifact, address: Option<Address>) -> Self {
        Self {
            name: artifact.name.clone(),
            address,
            abi_items: artifact.abi_items.clone(),
            raw_abi: artifact.raw_abi.clone(),
            deployer: Arc::new(ContractDeployer::dummy()),
        }
    }

    /// Create bindings for an already-deployed contract
    pub fn deployed(name: &str, address: Address, artifact: &NornContractArtifact) -> Self {
        let mut bindings = Self::new(artifact, Some(address));
        bindings.name = name.to_string();
        bindings
    }

    /// Get the contract address (requires deployment first)
    pub fn address(&self) -> EVMResult<Address> {
        self.address.ok_or_else(|| EVMError::Execution(
            "Contract not yet deployed".to_string()
        ))
    }

    /// Encode a function call data
    pub fn encode_call(
        &self,
        function_name: &str,
        params: &[ABIParam],
    ) -> EVMResult<Vec<u8>> {
        ABI::encode_function_call(function_name, params)
    }

    /// Encode constructor arguments
    pub fn encode_constructor(&self, params: &[ABIParam]) -> EVMResult<Vec<u8>> {
        ABI::encode_function_call("constructor", params)
    }

    /// Check if the contract has a function with the given name
    pub fn has_function(&self, name: &str) -> bool {
        self.abi_items.iter().any(|item| {
            if let crate::evm::abi::ABIItem::Function { name: n, .. } = item {
                n == name
            } else {
                false
            }
        })
    }

    /// Get the function selector for a given function name
    pub fn function_selector(&self, name: &str, input_types: &[&str]) -> [u8; 4] {
        let signature = format!("{}({})", name, input_types.join(","));
        let mut output = [0u8; 32];
        // Copy signature into buffer and hash in-place
        let sig_bytes = signature.as_bytes();
        let len = sig_bytes.len().min(32);
        output[..len].copy_from_slice(&sig_bytes[..len]);
        keccak_hash::keccak256(&mut output);
        let mut selector = [0u8; 4];
        selector.copy_from_slice(&output[..4]);
        selector
    }
}
