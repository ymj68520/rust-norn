//! Solidity compiler artifact types
//!
//! These types represent the output of solc compilation, mirroring the
//! standard JSON artifact format used by Hardhat, Foundry, and ethers.js.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tiny_keccak::{Hasher, Keccak};

// ============ Top-level solc output ============

/// Complete output from a solc compilation (standard JSON format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolcOutput {
    /// Solidity compiler version
    pub version: String,

    /// Long version string
    #[serde(default)]
    pub long_version: String,

    /// Source code information (key: file path)
    pub sources: HashMap<String, SourceInfo>,

    /// Compiled contract artifacts (key: source path -> (key: contract name -> artifact))
    pub contracts: HashMap<String, HashMap<String, CompiledContract>>,

    /// Compiler errors and warnings
    #[serde(default)]
    pub errors: Vec<CompilationError>,
}

/// Source file information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    /// Source ID
    pub id: u32,
}

// ============ Contract artifact ============

/// Compiled contract artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledContract {
    /// Contract name
    pub contract_name: String,

    /// Contract ABI (Ethereum Contract ABI Specification)
    pub abi: Vec<AbiItem>,

    /// EVM bytecode output
    pub evm: EvmOutput,

    /// Solidity compiler metadata (JSON string)
    #[serde(default)]
    pub metadata: String,

    /// User documentation (NatSpec for users)
    #[serde(default)]
    pub userdoc: Userdoc,

    /// Developer documentation (NatSpec for developers)
    #[serde(default)]
    pub devdoc: Devdoc,
}

impl CompiledContract {
    /// Get runtime bytecode as Vec<u8>
    pub fn runtime_bytecode(&self) -> Vec<u8> {
        self.evm.deployed_bytecode.object.as_bytes()
    }

    /// Get deployment (constructor) bytecode as Vec<u8>
    pub fn deployment_bytecode(&self) -> Vec<u8> {
        self.evm.bytecode.object.as_bytes()
    }

    /// Get runtime bytecode as hex string
    pub fn runtime_bytecode_hex(&self) -> String {
        format!("0x{}", hex::encode(self.runtime_bytecode()))
    }

    /// Get deployment bytecode as hex string
    pub fn deployment_bytecode_hex(&self) -> String {
        format!("0x{}", hex::encode(self.deployment_bytecode()))
    }

    /// Check if the contract has runtime bytecode
    pub fn has_runtime_bytecode(&self) -> bool {
        !self.evm.deployed_bytecode.object.is_empty()
    }

    /// Get all function ABI items
    pub fn functions(&self) -> Vec<&AbiItem> {
        self.abi
            .iter()
            .filter_map(|item| {
                if let AbiItem::Function { .. } = item {
                    Some(item)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all event ABI items
    pub fn events(&self) -> Vec<&AbiItem> {
        self.abi
            .iter()
            .filter_map(|item| {
                if let AbiItem::Event { .. } = item {
                    Some(item)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get a function by name
    pub fn get_function(&self, name: &str) -> Option<&AbiItem> {
        self.abi.iter().find(|item| {
            if let AbiItem::Function { name: n, .. } = item {
                n == name
            } else {
                false
            }
        })
    }

    /// Get all method identifiers (selector -> signature)
    pub fn method_identifiers(&self) -> &HashMap<String, String> {
        &self.evm.deployed_bytecode.method_identifiers
    }
}

/// EVM bytecode output (contains both deployment and runtime)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmOutput {
    /// Deployment (constructor) bytecode
    pub bytecode: EvmBytecode,

    /// Runtime (deployed) bytecode
    #[serde(rename = "deployedBytecode")]
    pub deployed_bytecode: EvmBytecode,
}

/// EVM bytecode object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmBytecode {
    /// Hex-encoded bytecode
    pub object: BytecodeObject,

    /// Source map
    #[serde(default)]
    pub source_map: String,

    /// Function identifiers
    #[serde(default)]
    pub method_identifiers: HashMap<String, String>,

    /// Link references
    #[serde(default)]
    pub link_references: serde_json::Value,
}

impl EvmBytecode {
    /// Convert to bytes
    pub fn as_bytes(&self) -> Vec<u8> {
        let hex_str = self
            .object
            .code
            .strip_prefix("0x")
            .unwrap_or(&self.object.code);
        hex::decode(hex_str).unwrap_or_default()
    }

    /// Check if the bytecode is empty
    pub fn is_empty(&self) -> bool {
        self.object.code.is_empty() || self.object.code == "0x"
    }
}

/// Bytecode object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BytecodeObject {
    /// Hex-encoded bytecode
    pub code: String,
}

impl BytecodeObject {
    /// Convert hex string to bytes
    pub fn as_bytes(&self) -> Vec<u8> {
        let hex_str = self.code.strip_prefix("0x").unwrap_or(&self.code);
        hex::decode(hex_str).unwrap_or_default()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.code.is_empty() || self.code == "0x"
    }
}

// ============ ABI Types ============

/// ABI item (function, event, error, or state variable)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AbiItem {
    /// Function declaration
    Function {
        /// Function name
        name: String,

        /// Full signature string
        #[serde(rename = "signature")]
        signature: String,

        /// Input parameters
        inputs: Vec<AbiParam>,

        /// Output parameters
        outputs: Vec<AbiParam>,

        /// State mutability
        #[serde(default)]
        state_mutability: StateMutability,

        /// Whether the function is payable
        #[serde(default)]
        payable: bool,

        /// Whether this is an anonymous event
        #[serde(default)]
        anonymous: bool,
    },

    /// Event declaration
    Event {
        /// Event name
        name: String,

        /// Whether the event is anonymous
        #[serde(default)]
        anonymous: bool,

        /// Input parameters
        inputs: Vec<AbiParam>,
    },

    /// Custom error declaration (Solidity 0.8.4+)
    Error {
        /// Error name
        name: String,

        /// Input parameters
        inputs: Vec<AbiParam>,
    },
}

impl AbiItem {
    /// Get the function selector (first 4 bytes of keccak256 of the signature)
    pub fn selector(&self) -> [u8; 4] {
        let signature = match self {
            AbiItem::Function { name, inputs, .. } => {
                let types: Vec<String> = inputs.iter().map(|p| p.type_signature()).collect();
                format!("{}({})", name, types.join(","))
            }
            _ => return [0u8; 4],
        };

        let mut hasher = Keccak::v256();
        let mut output = [0u8; 32];
        hasher.update(signature.as_bytes());
        hasher.finalize(&mut output);
        let mut selector = [0u8; 4];
        selector.copy_from_slice(&output[..4]);
        selector
    }

    /// Get the human-readable signature
    pub fn signature(&self) -> &str {
        match self {
            AbiItem::Function { signature, .. } => signature,
            _ => "",
        }
    }
}

/// ABI parameter type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiParam {
    /// Parameter name
    pub name: String,

    /// Parameter type string (e.g., "uint256", "address", "bytes32")
    #[serde(rename = "type")]
    pub param_type: String,

    /// Internal Solidity type
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub internal_type: String,

    /// Whether this parameter is indexed (for events)
    #[serde(default)]
    pub indexed: bool,

    /// Components for tuple types
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<AbiParam>,
}

impl AbiParam {
    /// Get the full type signature including nested components
    pub fn type_signature(&self) -> String {
        if self.components.is_empty() {
            self.param_type.clone()
        } else {
            let components: Vec<String> =
                self.components.iter().map(|c| c.type_signature()).collect();
            format!("({})", components.join(","))
        }
    }
}

/// State mutability of a function
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StateMutability {
    #[default]
    Nonpayable,
    Payable,
    Pure,
    View,
}

impl std::fmt::Display for StateMutability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateMutability::Nonpayable => write!(f, "nonpayable"),
            StateMutability::Payable => write!(f, "payable"),
            StateMutability::Pure => write!(f, "pure"),
            StateMutability::View => write!(f, "view"),
        }
    }
}

// ============ Documentation ============

/// User documentation (NatSpec)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Userdoc {
    pub details: String,
    pub methods: HashMap<String, String>,
}

/// Developer documentation (NatSpec)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Devdoc {
    pub details: String,
    pub methods: HashMap<String, String>,
    #[serde(default)]
    pub state_variables: HashMap<String, String>,
}

// Re-export compilation error types from error module
pub use crate::error::{CompilationError, SourceLocation};

// ============ Compilation result ============

/// Compilation result
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// Successfully compiled contracts
    pub contracts: Vec<CompiledContract>,

    /// Compiler warnings
    pub warnings: Vec<String>,

    /// Compiler errors
    pub errors: Vec<CompilationError>,
}

impl CompileResult {
    /// Whether compilation succeeded (no errors)
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get a contract by name
    pub fn get_contract(&self, name: &str) -> Option<&CompiledContract> {
        self.contracts.iter().find(|c| c.contract_name == name)
    }

    /// Get contracts by source file path
    pub fn get_contracts_by_source(&self, source_path: &str) -> Vec<&CompiledContract> {
        self.contracts
            .iter()
            .filter(|c| c.metadata.contains(source_path))
            .collect()
    }
}
