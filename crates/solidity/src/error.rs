//! Error types for Solidity compiler integration

use thiserror::Error;
use serde::{Serialize, Deserialize};

/// Result type for Solidity operations
pub type SolidityResult<T> = Result<T, SolidityError>;

/// Compilation error from solc
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationError {
    /// Error type (e.g., "TypeError", "ParserError")
    #[serde(rename = "type")]
    pub error_type: String,

    /// Component (e.g., "general", "semantic")
    pub component: String,

    /// Error message
    pub message: String,

    /// Source location
    pub source_location: Option<SourceLocation>,

    /// Whether this is a warning
    #[serde(default)]
    pub is_warning: bool,
}

/// Source location of an error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    /// File path
    pub file: String,

    /// Start line
    pub start: u32,

    /// End line
    pub end: u32,

    /// Start column
    pub start_column: u32,

    /// End column
    pub end_column: u32,
}

/// Errors that can occur during Solidity compilation
#[derive(Error, Debug, Clone)]
pub enum SolidityError {
    /// solc compiler not found on the system
    #[error("Solidity compiler (solc) not found. Install it via: pip install solc-select && solc-select install 0.8.20")]
    SolcNotFound,

    /// solc found at path but not executable
    #[error("Solidity compiler at '{path}' is not executable")]
    SolcNotExecutable { path: String },

    /// Compilation failed
    #[error("Compilation failed: {message}")]
    CompilationFailed { message: String },

    /// Compilation failed with errors from solc
    #[error("Compilation errors: {errors:?}")]
    CompilationErrors { errors: Vec<CompilationError> },

    /// Version mismatch
    #[error("Version mismatch: found {found}, required {required}")]
    VersionMismatch { found: String, required: String },

    /// Invalid Solidity source code
    #[error("Invalid Solidity source: {message}")]
    InvalidSource { message: String },

    /// Contract not found in compilation output
    #[error("Contract '{contract}' not found in compilation output")]
    ContractNotFound { contract: String },

    /// Failed to parse compiler output
    #[error("Failed to parse compiler output: {message}")]
    ParseError { message: String },

    /// I/O error
    #[error("I/O error: {message}")]
    IoError { message: String },

    /// JSON parsing error
    #[error("JSON parsing error: {message}")]
    JsonError { message: String },

    /// Unsupported feature
    #[error("Unsupported Solidity feature: {feature}")]
    UnsupportedFeature { feature: String },

    /// Configuration error
    #[error("Configuration error: {message}")]
    ConfigError { message: String },
}

impl From<std::io::Error> for SolidityError {
    fn from(err: std::io::Error) -> Self {
        SolidityError::IoError {
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for SolidityError {
    fn from(err: serde_json::Error) -> Self {
        SolidityError::JsonError {
            message: err.to_string(),
        }
    }
}

impl From<anyhow::Error> for SolidityError {
    fn from(err: anyhow::Error) -> Self {
        SolidityError::CompilationFailed {
            message: err.to_string(),
        }
    }
}
