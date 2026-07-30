//! Solidity Compiler Integration for Norn Blockchain
//!
//! This crate provides Solidity smart contract compilation capabilities by wrapping
//! the `solc` (Solidity compiler) command-line tool.

pub mod artifact;
pub mod compiler;
pub mod config;
pub mod error;

// Re-export main types
pub use artifact::{AbiItem, AbiParam, StateMutability, CompiledContract, CompileResult, SolcOutput};
pub use compiler::SolidityCompiler;
pub use config::{EvmVersion, OptimizationSettings, SolcConfig};
pub use error::{CompilationError, SolidityError, SolidityResult};
