//! Solidity integration for the norn EVM
//!
//! This module bridges the standalone `norn_solidity` compiler crate with the
//! norn EVM execution layer, providing high-level APIs for:
//!
//! 1. Compiling Solidity source to EVM bytecode
//! 2. Deploying compiled contracts to the norn blockchain
//! 3. Type-safe contract interaction (encoded calls + result decoding)
//!
//! # Architecture
//!
//! ```text
//!  Solidity Source (.sol)
//!         │
//!         ▼
//!  ┌─────────────────┐
//!  │ norn_solidity   │  ← Standalone crate (wraps solc)
//!  │  ::compiler      │
//!  └────────┬────────┘
//!           │ CompiledContract (bytecode + raw ABI)
//!           ▼
//!  ┌───────────────────────────┐
//!  │ norn_core::evm::solidity  │  ← This module
//!  │  ::deployment             │
//!  └────────┬──────────────────┘
//!           │ Contract address + state
//!           ▼
//!  ┌─────────────────────┐
//!  │ norn_core::evm      │  ← EVM execution (revm)
//!  │  ::executor         │
//!  └─────────────────────┘
//! ```

use crate::evm::{
    ABI, ABIParam, ABIType, ABIParamType, EVMConfig, EVMContext, EVMError,
    EVMExecutor, EVMResult, EVMExecutionResult, ExecutionLog,
    CodeStorage,
};
use norn_common::types::{Address, Hash};
use num_bigint::BigUint;
use std::sync::Arc;
use tracing::{debug, info, warn};

// Re-export the solidity compiler types for convenience
pub use norn_solidity::{
    SolidityCompiler, SolcConfig, OptimizationSettings, EvmVersion,
    CompiledContract, CompileResult, SolidityError, SolidityResult,
};

pub mod compiler;
pub mod deployment;
pub mod bindings;

pub use compiler::{SolidityCompilerExt, NornContractArtifact};
pub use deployment::ContractDeployer;
pub use bindings::{ContractBindings, CallResult};
