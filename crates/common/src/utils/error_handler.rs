use crate::error::{NornError, Result};
use tracing::{error, warn, debug, instrument};

/// Error handling utilities for the norn blockchain
pub struct ErrorHandler;

impl ErrorHandler {
    /// Handle and log errors appropriately
    #[instrument(skip_all)]
    pub fn handle_error<T>(result: Result<T>, operation: &str) -> Option<T> {
        match result {
            Ok(value) => {
                debug!("Operation '{}' completed successfully", operation);
                Some(value)
            }
            Err(err) => {
                Self::log_error(&err, operation);
                None
            }
        }
    }

    /// Handle errors that should be propagated
    #[instrument(skip_all)]
    pub fn handle_error_propagate<T>(result: Result<T>, operation: &str) -> Result<T> {
        match result {
            Ok(value) => {
                debug!("Operation '{}' completed successfully", operation);
                Ok(value)
            }
            Err(err) => {
                Self::log_error(&err, operation);
                Err(err)
            }
        }
    }

    /// Log errors with appropriate level based on error type
    fn log_error(error: &NornError, operation: &str) {
        match error {
            NornError::Database(db_err) => {
                error!("Database error in '{}': {}", operation, db_err);
            }
            NornError::Network(net_err) => {
                warn!("Network error in '{}': {}", operation, net_err);
            }
            NornError::Crypto(crypto_err) => {
                error!("Cryptographic error in '{}': {}", operation, crypto_err);
            }
            NornError::Validation(validation_err) => {
                warn!("Validation error in '{}': {}", operation, validation_err);
            }
            NornError::ConsensusError(consensus_err) => {
                warn!("Consensus error in '{}': {}", operation, consensus_err);
            }
            NornError::Config(config_err) => {
                error!("Configuration error in '{}': {}", operation, config_err);
            }
            NornError::Io(io_err) => {
                error!("I/O error in '{}': {}", operation, io_err);
            }
            NornError::Serialization(serial_err) => {
                error!("Serialization error in '{}': {}", operation, serial_err);
            }
            NornError::Internal(internal_err) => {
                error!("Internal error in '{}': {}", operation, internal_err);
            }
        }
    }

    /// Create a context-aware error
    pub fn context_error(message: &str) -> NornError {
        NornError::Internal(message.to_string())
    }

    /// Check if error is recoverable
    pub fn is_recoverable(error: &NornError) -> bool {
        match error {
            NornError::Network(_) => true,
            NornError::Database(crate::error::DatabaseError::ConnectionFailed(_)) => true,
            NornError::Database(crate::error::DatabaseError::TransactionFailed(_)) => true,
            NornError::Io(_) => true,
            _ => false,
        }
    }

    /// Check if error should trigger a retry
    pub fn should_retry(error: &NornError) -> bool {
        match error {
            NornError::Network(crate::error::NetworkError::Timeout(_)) => true,
            NornError::Network(crate::error::NetworkError::ConnectionFailed(_)) => true,
            NornError::Database(crate::error::DatabaseError::ConnectionFailed(_)) => true,
            NornError::Io(io_err) if io_err.kind() == std::io::ErrorKind::TimedOut => true,
            _ => false,
        }
    }
}

/// Macro for convenient error handling
#[macro_export]
macro_rules! handle_error {
    ($expr:expr, $operation:expr) => {
        $crate::utils::error_handler::ErrorHandler::handle_error($expr, $operation)
    };
}

#[macro_export]
macro_rules! handle_error_propagate {
    ($expr:expr, $operation:expr) => {
        $crate::utils::error_handler::ErrorHandler::handle_error_propagate($expr, $operation)
    };
}

#[macro_export]
macro_rules! context_error {
    ($msg:expr) => {
        $crate::utils::error_handler::ErrorHandler::context_error($msg)
    };
}

/// Retry mechanism for recoverable errors
pub async fn retry_async<F, T, Fut>(
    mut operation: F,
    max_retries: usize,
    operation_name: &str,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut attempts = 0;
    
    loop {
        attempts += 1;
        match operation().await {
            Ok(result) => {
                if attempts > 1 {
                    debug!("Operation '{}' succeeded after {} attempts", operation_name, attempts);
                }
                return Ok(result);
            }
            Err(error) => {
                if attempts >= max_retries || !ErrorHandler::should_retry(&error) {
                    error!("Operation '{}' failed after {} attempts: {}", operation_name, attempts, error);
                    return Err(error);
                }
                
                warn!("Operation '{}' failed (attempt {}): {}, retrying...", operation_name, attempts, error);
                
                // Exponential backoff
                let delay = std::time::Duration::from_millis(100 * (2_u64.pow(attempts as u32 - 1)));
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{DatabaseError, NetworkError};

    #[test]
    fn test_error_recovery() {
        let recoverable_error = NornError::Network(NetworkError::ConnectionFailed("test".to_string()));
        assert!(ErrorHandler::is_recoverable(&recoverable_error));

        let non_recoverable_error = NornError::Crypto(crate::error::CryptoError::InvalidSignature("test".to_string()));
        assert!(!ErrorHandler::is_recoverable(&non_recoverable_error));
    }

    #[test]
    fn test_retry_logic() {
        let retryable_error = NornError::Network(NetworkError::Timeout("test".to_string()));
        assert!(ErrorHandler::should_retry(&retryable_error));

        let non_retryable_error = NornError::Validation(crate::error::ValidationError::InvalidBlock("test".to_string()));
        assert!(!ErrorHandler::should_retry(&non_retryable_error));
    }

    #[tokio::test]
    async fn test_retry_success() {
        let mut call_count = 0;
        let result = retry_async(
            || {
                call_count += 1;
                async move {
                    if call_count < 3 {
                        Err(NornError::Network(NetworkError::Timeout("test".to_string())))
                    } else {
                        Ok("success")
                    }
                }
            },
            5,
            "test_operation",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(call_count, 3);
    }
}