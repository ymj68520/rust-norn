pub mod merkle;
pub mod txpool;
pub mod block_buffer;
pub mod blockchain;
pub mod data_processor;
pub mod config;
pub mod consensus;
pub mod metrics;
pub mod validation;
pub mod state;
pub mod execution;
pub mod fee;
pub mod wallet;
pub mod events;
pub mod evm;

// Re-export commonly used types
pub use txpool::{TxPool, TransactionPool, TxPoolStats};
pub mod txpool_enhanced;  // New: Enhanced transaction pool
pub use txpool_enhanced::{EnhancedTxPool, PrioritizedTransaction, TxPoolError};
