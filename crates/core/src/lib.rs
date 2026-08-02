pub mod block_buffer;
pub mod blockchain;
pub mod config;
pub mod consensus;
pub mod data_processor;
pub mod events;
pub mod evm;
pub mod execution;
pub mod fee;
pub mod finality;
pub mod merkle;
pub mod metrics;
pub mod state;
pub mod txpool;
pub mod txpool_v2;
pub mod validation;
pub mod wallet;

// Re-export commonly used types
pub use txpool::{TransactionPool, TxPool, TxPoolStats};
pub use txpool_v2::{TransactionV2Pool, TransactionV2PoolError};
pub mod txpool_enhanced; // New: Enhanced transaction pool
pub use txpool_enhanced::{EnhancedTxPool, PrioritizedTransaction, TxPoolError};
