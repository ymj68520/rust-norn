//! Transaction execution module
//!
//! Provides transaction execution and gas management.

pub mod router;

pub use router::{TransactionRouter, ExecutionResult, LogEntry};

// TODO: Fix imports in the following modules and re-enable them:
// pub mod gas;
// pub mod executor;
// pub mod nonce;