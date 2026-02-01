//! EVM (Ethereum Virtual Machine) integration for norn blockchain
//!
//! This module provides EVM compatibility, allowing the norn blockchain to execute
//! Ethereum smart contracts and interact with the Ethereum ecosystem.

// Module exports
mod error;
mod runtime; // Fixed with SyncStateManager bridging layer
mod executor;
mod code_storage;
mod logging;
mod receipt;
mod precompiles;
mod eip1559;
mod access_list;
mod gas;
mod blockhash;
mod abi;
mod benchmarks;
mod real_contracts;

pub use error::{EVMError, EVMResult};
pub use runtime::NornDatabaseAdapter; // Fixed with SyncStateManager bridging layer
pub use executor::{EVMExecutor, EVMExecutionResult, ExecutionLog};
pub use code_storage::CodeStorage;
pub use logging::{EventLog, LogManager};
pub use receipt::{Receipt, ReceiptDB, ReceiptLog, Bloom};
pub use precompiles::{
    is_precompile, execute as execute_precompile, PrecompileResult,
    ECRECOVER_ADDRESS, SHA256_ADDRESS, RIPEMD160_ADDRESS,
    IDENTITY_ADDRESS, MODEXP_ADDRESS, ECADD_ADDRESS,
    ECMUL_ADDRESS, ECPAIRING_ADDRESS, BLAKE2F_ADDRESS,
};
pub use eip1559::{EIP1559FeeCalculator, EIP1559Config};
pub use access_list::{
    AccessListTracker, EIP2930Utils, AccessType,
    COLD_ACCOUNT_ACCESS_COST, COLD_SLOAD_COST,
    WARM_ACCOUNT_ACCESS_COST, WARM_SLOAD_COST,
    ACCESS_LIST_ADDRESS_COST, ACCESS_LIST_STORAGE_KEY_COST,
};
pub use gas::{GasCalculator, costs as gas_costs};
pub use blockhash::{BlockHistory, MAX_BLOCK_HASH_HISTORY};
pub use abi::{
    ABI, ABIParam, ABIValue, ABIType, ABIItem, ABIParamType,
    HumanReadableABI,
};
pub use benchmarks::{BenchmarkSuite, BenchmarkResult};
#[cfg(feature = "real_contracts_test")]
pub use real_contracts::ContractTester;

// Future modules (to be implemented):
// mod precompiles;  // Precompiled contracts

/// EVM configuration
#[derive(Debug, Clone)]
pub struct EVMConfig {
    /// Chain ID for EIP-155 replay protection
    pub chain_id: u64,

    /// Block gas limit
    pub block_gas_limit: u64,

    /// Maximum contract size in bytes (EIP-170: 24KB)
    pub max_contract_size: usize,

    /// Maximum call depth (to prevent stack overflow)
    pub max_call_depth: usize,

    /// Enable/disable precompiled contracts
    pub enable_precompiles: bool,

    /// EIP-1559 fee market configuration
    pub eip1559_config: EIP1559Config,
}

impl Default for EVMConfig {
    fn default() -> Self {
        Self {
            chain_id: 31337, // Default testnet chain ID
            block_gas_limit: 30_000_000,
            max_contract_size: 24_576, // EIP-170 limit
            max_call_depth: 1024,
            enable_precompiles: true,
            eip1559_config: EIP1559Config::default(),
        }
    }
}

/// EVM execution context
#[derive(Debug, Clone)]
pub struct EVMContext {
    /// Block number
    pub block_number: u64,

    /// Block timestamp
    pub block_timestamp: u64,

    /// Block proposer (coinbase)
    pub block_coinbase: norn_common::types::Address,

    /// Block gas limit
    pub block_gas_limit: u64,

    /// Transaction gas price
    pub tx_gas_price: u64,
}

impl Default for EVMContext {
    fn default() -> Self {
        Self {
            block_number: 0,
            block_timestamp: chrono::Utc::now().timestamp() as u64,
            block_coinbase: norn_common::types::Address::default(),
            block_gas_limit: 30_000_000,
            tx_gas_price: 1_000_000_000, // 1 Gwei
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = EVMConfig::default();
        assert_eq!(config.chain_id, 31337);
        assert_eq!(config.block_gas_limit, 30_000_000);
        assert_eq!(config.max_contract_size, 24_576);
        assert_eq!(config.max_call_depth, 1024);
        assert!(config.enable_precompiles);
    }

    #[test]
    fn test_context_default() {
        let ctx = EVMContext::default();
        assert_eq!(ctx.block_number, 0);
        assert_eq!(ctx.block_gas_limit, 30_000_000);
        assert_eq!(ctx.tx_gas_price, 1_000_000_000);
    }
}
