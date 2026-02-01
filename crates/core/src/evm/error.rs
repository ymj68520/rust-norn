//! EVM-specific error types
//!
//! This module defines all error types that can occur during EVM execution.

use anyhow::Result;
use thiserror::Error;

/// EVM execution errors
#[derive(Error, Debug)]
pub enum EVMError {
    /// General execution error
    #[error("EVM execution error: {0}")]
    Execution(String),

    /// Database access error
    #[error("Database error: {0}")]
    Database(#[from] anyhow::Error),

    /// State access error
    #[error("State access error: {0}")]
    StateAccess(String),

    /// Gas-related error
    #[error("Gas error: {0}")]
    Gas(String),

    /// Out of gas
    #[error("Out of gas")]
    OutOfGas,

    /// Invalid transaction
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    /// Revert from smart contract
    #[error("Contract reverted: {0}")]
    Revert(String),

    /// Contract creation failed
    #[error("Contract creation failed: {0}")]
    ContractCreationFailed(String),

    /// Precompile execution failed
    #[error("Precompile execution failed: {0}")]
    PrecompileFailed(String),

    /// Invalid bytecode
    #[error("Invalid bytecode: {0}")]
    InvalidBytecode(String),

    /// Stack overflow/underflow
    #[error("Stack error: {0}")]
    Stack(String),

    /// Unsupported operation
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),
}

impl EVMError {
    /// Create an execution error with a message
    pub fn execution(msg: impl Into<String>) -> Self {
        Self::Execution(msg.into())
    }

    /// Create a state access error
    pub fn state_access(msg: impl Into<String>) -> Self {
        Self::StateAccess(msg.into())
    }

    /// Create a gas error
    pub fn gas(msg: impl Into<String>) -> Self {
        Self::Gas(msg.into())
    }

    /// Create an invalid transaction error
    pub fn invalid_tx(msg: impl Into<String>) -> Self {
        Self::InvalidTransaction(msg.into())
    }

    /// Create a revert error
    pub fn revert(msg: impl Into<String>) -> Self {
        Self::Revert(msg.into())
    }
}

/// Result type for EVM operations
pub type EVMResult<T> = Result<T, EVMError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = EVMError::execution("test error");
        assert_eq!(err.to_string(), "EVM execution error: test error");

        let err = EVMError::gas("out of gas");
        assert_eq!(err.to_string(), "Gas error: out of gas");

        let err = EVMError::revert("insufficient balance");
        assert_eq!(err.to_string(), "Contract reverted: insufficient balance");
    }
}
