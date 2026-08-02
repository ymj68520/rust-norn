//! Solidity compiler extension for norn EVM
//!
//! This module provides `SolidityCompilerExt` which extends the standalone
//! `norn_solidity::SolidityCompiler` with norn-specific functionality.
//!
//! It converts solc's raw JSON output into norn-core-compatible artifacts
//! with norn-core ABI types for use with the EVM executor.

use super::*;
use crate::evm::abi::ABIType;
use keccak_hash::keccak256;

/// Extension trait for the SolidityCompiler to integrate with norn EVM
pub trait SolidityCompilerExt {
    /// Compile Solidity source and return norn-compatible deployment info
    fn compile_for_deployment(
        &self,
        source: &str,
        contract_name: Option<&str>,
    ) -> SolidityResult<Vec<NornContractArtifact>>;

    /// Compile Solidity files and return norn-compatible deployment info
    fn compile_files_for_deployment(
        &self,
        files: &[std::path::PathBuf],
        contract_name: Option<&str>,
    ) -> SolidityResult<Vec<NornContractArtifact>>;
}

impl SolidityCompilerExt for SolidityCompiler {
    fn compile_for_deployment(
        &self,
        source: &str,
        contract_name: Option<&str>,
    ) -> SolidityResult<Vec<NornContractArtifact>> {
        let result = self.compile_source(source, contract_name)?;

        result
            .contracts
            .into_iter()
            .map(|contract| {
                Ok(NornContractArtifact::from_compiled_contract(
                    contract,
                    self.version(),
                ))
            })
            .collect()
    }

    fn compile_files_for_deployment(
        &self,
        files: &[std::path::PathBuf],
        contract_name: Option<&str>,
    ) -> SolidityResult<Vec<NornContractArtifact>> {
        let result = self.compile_files(files, contract_name)?;

        result
            .contracts
            .into_iter()
            .map(|contract| {
                Ok(NornContractArtifact::from_compiled_contract(
                    contract,
                    self.version(),
                ))
            })
            .collect()
    }
}

/// Norn-compatible contract artifact
///
/// Contains everything needed to deploy and interact with a Solidity contract
/// on the norn blockchain, using norn-core's ABI types for encoding/decoding.
#[derive(Debug, Clone)]
pub struct NornContractArtifact {
    /// Contract name
    pub name: String,

    /// Full deployment bytecode (constructor + runtime)
    pub bytecode: Vec<u8>,

    /// Runtime bytecode (the code stored on chain)
    pub runtime_bytecode: Vec<u8>,

    /// Raw ABI from solc (for reference / JSON serialization)
    pub raw_abi: Vec<norn_solidity::AbiItem>,

    /// Norn-core compatible ABI items (for encoding/decoding)
    pub abi_items: Vec<crate::evm::abi::ABIItem>,

    /// Function selectors computed from the ABI
    pub selectors: Vec<(String, [u8; 4])>,

    /// Solidity compiler version
    pub compiler_version: String,
}

impl NornContractArtifact {
    /// Create from a compiled contract
    fn from_compiled_contract(contract: CompiledContract, compiler_version: &str) -> Self {
        let raw_abi = contract.abi.clone();
        let abi_items: Vec<crate::evm::abi::ABIItem> = raw_abi
            .iter()
            .filter_map(|item| match item {
                norn_solidity::AbiItem::Function {
                    name,
                    inputs,
                    outputs,
                    ..
                } => {
                    let core_inputs: Vec<crate::evm::abi::ABIParamType> = inputs
                        .iter()
                        .map(|p| crate::evm::abi::ABIParamType {
                            name: Some(p.name.clone()),
                            ty: parse_abi_type(&p.param_type),
                            indexed: p.indexed,
                        })
                        .collect();

                    let core_outputs: Vec<crate::evm::abi::ABIParamType> = outputs
                        .iter()
                        .map(|p| crate::evm::abi::ABIParamType {
                            name: Some(p.name.clone()),
                            ty: parse_abi_type(&p.param_type),
                            indexed: false,
                        })
                        .collect();

                    Some(crate::evm::abi::ABIItem::Function {
                        name: name.clone(),
                        inputs: core_inputs,
                        outputs: core_outputs,
                    })
                }
                norn_solidity::AbiItem::Event { name, inputs, .. } => {
                    let core_inputs: Vec<crate::evm::abi::ABIParamType> = inputs
                        .iter()
                        .map(|p| crate::evm::abi::ABIParamType {
                            name: Some(p.name.clone()),
                            ty: parse_abi_type(&p.param_type),
                            indexed: p.indexed,
                        })
                        .collect();

                    Some(crate::evm::abi::ABIItem::Event {
                        name: name.clone(),
                        inputs: core_inputs,
                    })
                }
                _ => None,
            })
            .collect();

        // Compute function selectors
        let selectors: Vec<(String, [u8; 4])> = abi_items
            .iter()
            .filter_map(|item| {
                if let crate::evm::abi::ABIItem::Function { name, inputs, .. } = item {
                    let types: Vec<String> =
                        inputs.iter().map(|p| abi_type_to_string(&p.ty)).collect();
                    let sig = format!("{}({})", name, types.join(","));
                    let output = keccak256_bytes(sig.as_bytes());
                    let mut sel = [0u8; 4];
                    sel.copy_from_slice(&output[..4]);
                    Some((name.clone(), sel))
                } else {
                    None
                }
            })
            .collect();

        Self {
            name: contract.contract_name.clone(),
            bytecode: contract.evm.bytecode.object.as_bytes(),
            runtime_bytecode: contract.evm.deployed_bytecode.object.as_bytes(),
            raw_abi,
            abi_items,
            selectors,
            compiler_version: compiler_version.to_string(),
        }
    }

    /// Get a function selector for the given function name and input types
    pub fn function_selector(&self, name: &str, input_types: &[&str]) -> [u8; 4] {
        let signature = format!("{}({})", name, input_types.join(","));
        let output = keccak256_bytes(signature.as_bytes());
        let mut selector = [0u8; 4];
        selector.copy_from_slice(&output[..4]);
        selector
    }

    /// Encode a function call using the ABI
    pub fn encode_function_call(
        &self,
        function_name: &str,
        params: &[ABIParam],
    ) -> EVMResult<Vec<u8>> {
        ABI::encode_function_call(function_name, params)
    }

    /// Get the contract address for a given deployer and nonce
    pub fn deployment_address(&self, deployer: Address, nonce: u64) -> Address {
        use crate::evm::code_storage::CodeStorage;
        CodeStorage::calculate_create_address(deployer, nonce)
    }
}

// Conversion implementations from norn_solidity types to norn-core types

impl From<&norn_solidity::AbiParam> for crate::evm::abi::ABIParam {
    fn from(p: &norn_solidity::AbiParam) -> Self {
        crate::evm::abi::ABIParam {
            name: Some(p.name.clone()),
            value: parse_abi_value(&p.param_type),
        }
    }
}

/// Parse a Solidity ABI type string into an ABIValue placeholder
fn parse_abi_value(type_str: &str) -> crate::evm::abi::ABIValue {
    // Default to zero-value for encoding purposes
    // The actual value is provided by the caller
    crate::evm::abi::ABIValue::Uint(0, 256)
}

/// Parse a Solidity ABI type string into norn-core ABIType
fn parse_abi_type(type_str: &str) -> crate::evm::abi::ABIType {
    use crate::evm::abi::ABIType;

    if type_str.starts_with("uint") || type_str.starts_with("int") {
        return if let Some(bits_str) = type_str
            .strip_prefix("uint")
            .or_else(|| type_str.strip_prefix("int"))
        {
            if let Ok(bits) = bits_str.parse::<u16>() {
                if type_str.starts_with("uint") {
                    ABIType::Uint(bits)
                } else {
                    ABIType::Int(bits)
                }
            } else if bits_str.is_empty() {
                // uint256, int256 (default)
                if type_str.starts_with("uint") {
                    ABIType::Uint(256)
                } else {
                    ABIType::Int(256)
                }
            } else {
                ABIType::Uint(256)
            }
        } else {
            ABIType::Uint(256)
        };
    }

    match type_str {
        "address" => ABIType::Address,
        "bool" => ABIType::Bool,
        "bytes" | "string" => ABIType::Bytes,
        _ => {
            // Check for fixed bytes (bytes1, bytes32, etc.)
            if type_str.starts_with("bytes") {
                if let Ok(size) = type_str[5..].parse::<u8>() {
                    return ABIType::FixedBytes(size);
                }
                return ABIType::Bytes;
            }
            // Check for arrays
            if type_str.contains('[') {
                let base = type_str.split('[').next().unwrap_or("bytes");
                let inner = parse_abi_type(base);
                return ABIType::Array(Box::new(inner));
            }
            // Check for tuple types
            if type_str.starts_with("tuple") {
                return ABIType::Tuple(Vec::new());
            }
            // Default: treat as dynamic bytes
            ABIType::Bytes
        }
    }
}

/// Convert an ABIType to its string representation for function selectors
fn abi_type_to_string(ty: &crate::evm::abi::ABIType) -> String {
    use crate::evm::abi::ABIType;
    match ty {
        ABIType::Uint(bits) => format!("uint{}", bits),
        ABIType::Int(bits) => format!("int{}", bits),
        ABIType::Address => "address".to_string(),
        ABIType::Bool => "bool".to_string(),
        ABIType::Bytes => "bytes".to_string(),
        ABIType::FixedBytes(size) => format!("bytes{}", size),
        ABIType::String => "string".to_string(),
        ABIType::Array(inner) => format!("{}[]", abi_type_to_string(inner)),
        ABIType::FixedArray(inner, size) => format!("{}[{}]", abi_type_to_string(inner), size),
        ABIType::Tuple(_) => "tuple".to_string(),
    }
}

/// Compute keccak256 hash of data and return the 32-byte result
fn keccak256_bytes(data: &[u8]) -> [u8; 32] {
    let mut buf = [0u8; 32];
    // Copy data into buffer (pad with zeros if data < 32 bytes)
    let len = data.len().min(32);
    buf[..len].copy_from_slice(&data[..len]);
    // Compute keccak256 hash in-place
    keccak_hash::keccak256(&mut buf);
    buf
}
