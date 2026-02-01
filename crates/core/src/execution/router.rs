//! Transaction Router
//!
//! Routes transactions to the appropriate executor based on transaction type.

use crate::evm::{EVMExecutor, EVMContext, EVMExecutionResult};
use norn_common::types::{Transaction, TransactionType, Address, Hash};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Result of transaction execution (unified for both Native and EVM)
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Transaction hash
    pub tx_hash: Hash,

    /// Whether execution succeeded
    pub success: bool,

    /// Error message if failed
    pub error: Option<String>,

    /// Gas used
    pub gas_used: u64,

    /// Returned data
    pub return_data: Vec<u8>,

    /// Logs emitted (for EVM transactions)
    pub logs: Vec<LogEntry>,
}

/// Log entry emitted during transaction execution
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Contract address
    pub address: Address,

    /// Topics
    pub topics: Vec<Hash>,

    /// Data
    pub data: Vec<u8>,
}

/// Transaction router that dispatches to appropriate executor
pub struct TransactionRouter {
    /// EVM executor
    evm_executor: Option<Arc<EVMExecutor>>,

    /// Current block number
    block_number: Arc<RwLock<u64>>,

    /// Current block timestamp
    block_timestamp: Arc<RwLock<u64>>,

    /// Block gas limit
    block_gas_limit: u64,

    /// Block coinbase (block proposer address)
    block_coinbase: Arc<RwLock<Address>>,

    /// Base fee for EIP-1559
    base_fee: Arc<RwLock<u64>>,
}

impl TransactionRouter {
    /// Create a new transaction router
    pub fn new(
        evm_executor: Option<Arc<EVMExecutor>>,
        block_gas_limit: u64,
    ) -> Self {
        Self {
            evm_executor,
            block_number: Arc::new(RwLock::new(0)),
            block_timestamp: Arc::new(RwLock::new(0)),
            block_gas_limit,
            block_coinbase: Arc::new(RwLock::new(Address::default())),
            base_fee: Arc::new(RwLock::new(0)),
        }
    }

    /// Set block number
    pub async fn set_block_number(&self, number: u64) {
        let mut block_number = self.block_number.write().await;
        *block_number = number;
    }

    /// Set block timestamp
    pub async fn set_block_timestamp(&self, timestamp: u64) {
        let mut block_timestamp = self.block_timestamp.write().await;
        *block_timestamp = timestamp;
    }

    /// Set block coinbase (block proposer address)
    pub async fn set_block_coinbase(&self, coinbase: Address) {
        let mut block_coinbase = self.block_coinbase.write().await;
        *block_coinbase = coinbase;
    }

    /// Set base fee for EIP-1559
    pub async fn set_base_fee(&self, base_fee: u64) {
        let mut fee = self.base_fee.write().await;
        *fee = base_fee;
    }

    /// Calculate effective gas price from transaction
    fn calculate_gas_price(&self, tx: &Transaction) -> u64 {
        // EIP-1559 logic: effective gas price is min(max_fee_per_gas, base_fee + max_priority_fee_per_gas)
        if let Some(max_fee) = tx.body.max_fee_per_gas {
            let base_fee = *self.base_fee.blocking_read();
            let priority_fee = tx.body.max_priority_fee_per_gas.unwrap_or(0);
            std::cmp::min(max_fee, base_fee.saturating_add(priority_fee))
        } else if let Some(gas_price) = tx.body.gas_price {
            // Legacy transaction with explicit gas price
            gas_price
        } else {
            // Fallback to default
            1_000_000_000
        }
    }

    /// Execute transaction based on its type
    pub async fn execute_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<ExecutionResult, String> {
        let tx_type = tx.body.tx_type;

        debug!(
            "Routing transaction: hash={:?}, type={:?}",
            tx.body.hash, tx_type
        );

        match tx_type {
            TransactionType::Native => {
                // For now, native transactions are not handled by EVM router
                // They should be handled by the existing native executor
                warn!("Native transaction execution not implemented in router");
                Err("Native transactions should use native executor".to_string())
            }

            TransactionType::EVM => {
                self.execute_evm_transaction(tx).await
            }
        }
    }

    /// Execute EVM transaction
    async fn execute_evm_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<ExecutionResult, String> {
        let evm_executor = self.evm_executor.as_ref()
            .ok_or("EVM executor not configured")?;

        // Create EVM context
        let block_number = *self.block_number.read().await;
        let block_timestamp = *self.block_timestamp.read().await;
        let block_coinbase = *self.block_coinbase.read().await;
        let tx_gas_price = self.calculate_gas_price(tx);

        let ctx = EVMContext {
            block_number,
            block_timestamp,
            block_coinbase,
            block_gas_limit: self.block_gas_limit,
            tx_gas_price,
        };

        // Execute transaction
        let result = evm_executor.execute(tx, &ctx)
            .await
            .map_err(|e| format!("EVM execution failed: {:?}", e))?;

        // Convert EVM result to unified result
        Ok(ExecutionResult {
            tx_hash: tx.body.hash,
            success: result.success,
            error: result.error,
            gas_used: result.gas_used,
            return_data: result.output,
            logs: result.logs.into_iter().map(|log| LogEntry {
                address: log.address,
                topics: log.topics,
                data: log.data,
            }).collect(),
        })
    }

    /// Get EVM executor reference
    pub fn evm_executor(&self) -> Option<&Arc<EVMExecutor>> {
        self.evm_executor.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evm::EVMConfig;
    use crate::state::account::{AccountStateManager, AccountStateConfig};
    use num_bigint::BigUint;
    use norn_common::types::TransactionBody;

    fn create_test_evm_transaction() -> Transaction {
        Transaction {
            body: TransactionBody {
                hash: Hash([1u8; 32]),
                address: Address([2u8; 20]),
                receiver: Address([3u8; 20]),
                gas: 100_000,
                nonce: 0,
                event: Vec::new(),
                opt: Vec::new(),
                state: Vec::new(),
                data: Vec::new(),
                expire: 0,
                height: 0,
                index: 0,
                block_hash: Hash::default(),
                timestamp: 0,
                public: norn_common::types::PublicKey::default(),
                signature: Vec::new(),
                tx_type: TransactionType::EVM,
                chain_id: Some(31337),
                value: Some("1000000000000000000".to_string()),
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                access_list: None,
                gas_price: None,
            },
        }
    }

    #[tokio::test]
    async fn test_router_creation() {
        let router = TransactionRouter::new(None, 30_000_000);
        assert_eq!(router.block_gas_limit, 30_000_000);

        // Test setting block info
        router.set_block_number(100).await;
        router.set_block_timestamp(12345).await;

        assert_eq!(*router.block_number.read().await, 100);
        assert_eq!(*router.block_timestamp.read().await, 12345);
    }

    #[tokio::test]
    async fn test_evm_transaction_routing() {
        let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
        let evm_config = EVMConfig::default();
        let evm_executor = Arc::new(EVMExecutor::new(Arc::clone(&state_manager), evm_config));

        // Setup: Give sender account sufficient balance
        let sender = Address([2u8; 20]);
        state_manager.update_balance(&sender, BigUint::from(2_000_000_000_000_000_000u128)).await.unwrap(); // 2 ETH

        let router = TransactionRouter::new(Some(evm_executor), 30_000_000);

        let tx = create_test_evm_transaction();
        let result = router.execute_transaction(&tx).await;

        assert!(result.is_ok());
        let exec_result = result.unwrap();
        assert!(exec_result.success);
        assert_eq!(exec_result.gas_used, 21_000); // Base gas for transfer
    }

    #[tokio::test]
    async fn test_native_transaction_rejection() {
        let router = TransactionRouter::new(None, 30_000_000);

        let mut tx = create_test_evm_transaction();
        tx.body.tx_type = TransactionType::Native;

        let result = router.execute_transaction(&tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_no_evm_executor() {
        // Router without EVM executor
        let router = TransactionRouter::new(None, 30_000_000);

        let tx = create_test_evm_transaction();
        let result = router.execute_transaction(&tx).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "EVM executor not configured");
    }
}
