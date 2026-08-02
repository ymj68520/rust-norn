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
    ABIParam, ABIParamType, ABIType, CodeStorage, EVMConfig, EVMContext, EVMError,
    EVMExecutionResult, EVMExecutor, EVMResult, ExecutionLog, ABI,
};
use norn_common::types::{Address, Hash};
use num_bigint::BigUint;
use std::sync::Arc;
use tracing::{debug, info, warn};

// Re-export the solidity compiler types for convenience
pub use norn_solidity::{
    CompileResult, CompiledContract, EvmVersion, OptimizationSettings, SolcConfig,
    SolidityCompiler, SolidityError, SolidityResult,
};

pub mod bindings;
pub mod compiler;
pub mod deployment;

pub use bindings::{CallResult, ContractBindings};
pub use compiler::{NornContractArtifact, SolidityCompilerExt};
pub use deployment::ContractDeployer;
