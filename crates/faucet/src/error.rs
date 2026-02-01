//! Error types for the faucet service

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Faucet service errors
#[derive(Error, Debug)]
pub enum FaucetError {
    #[error("Rate limit exceeded: try again in {0} seconds")]
    RateLimitExceeded(u64),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    #[error("Insufficient funds in faucet")]
    InsufficientFunds,

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] sled::Error),

    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl IntoResponse for FaucetError {
    fn into_response(self) -> Response {
        let (status, error_message, error_code) = match self {
            FaucetError::RateLimitExceeded(seconds) => (
                StatusCode::TOO_MANY_REQUESTS,
                format!("Rate limit exceeded. Try again in {} seconds", seconds),
                "RATE_LIMIT_EXCEEDED",
            ),
            FaucetError::InvalidAddress(msg) => (
                StatusCode::BAD_REQUEST,
                format!("Invalid address: {}", msg),
                "INVALID_ADDRESS",
            ),
            FaucetError::InvalidAmount(msg) => (
                StatusCode::BAD_REQUEST,
                format!("Invalid amount: {}", msg),
                "INVALID_AMOUNT",
            ),
            FaucetError::InsufficientFunds => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Faucet is out of funds. Please try again later.".to_string(),
                "INSUFFICIENT_FUNDS",
            ),
            FaucetError::TransactionFailed(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Transaction failed: {}", msg),
                "TRANSACTION_FAILED",
            ),
            FaucetError::DatabaseError(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", err),
                "DATABASE_ERROR",
            ),
            FaucetError::RpcError(msg) => (
                StatusCode::BAD_GATEWAY,
                format!("RPC error: {}", msg),
                "RPC_ERROR",
            ),
            FaucetError::InternalError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal error: {}", msg),
                "INTERNAL_ERROR",
            ),
        };

        let body = Json(json!({
            "error": error_code,
            "message": error_message,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }));

        (status, body).into_response()
    }
}

pub type FaucetResult<T> = Result<T, FaucetError>;
