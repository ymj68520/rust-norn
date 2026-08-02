//! Transaction execution module
//!
//! Provides transaction execution and gas management.

pub mod gas;
pub mod overlay;
pub mod router;

pub use gas::{GasCalculator, GasConfig, GasSchedule, GasUsage};
pub use overlay::{
    calculate_v2_execution_data_hash, execute_v2_block, ExecutionOverlay, OverlayError,
    OverlayWrite, V2BlockExecution, V2ExecutionContext, V2ExecutionResult,
};
pub use router::{ExecutionResult, LogEntry, TransactionRouter};
