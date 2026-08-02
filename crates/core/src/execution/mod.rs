//! Transaction execution module
//!
//! Provides transaction execution and gas management.

pub mod router;
pub mod gas;

pub use router::{TransactionRouter, ExecutionResult, LogEntry};
pub use gas::{GasCalculator, GasConfig, GasSchedule, GasUsage};