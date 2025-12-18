use thiserror::Error;

/// Common error types for the norn blockchain
#[derive(Error, Debug)]
pub enum NornError {
    /// Database related errors
    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),

    /// Network related errors
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    /// Cryptographic errors
    #[error("Cryptographic error: {0}")]
    Crypto(#[from] CryptoError),

    /// Validation errors
    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),

    /// Consensus errors
    #[error("Consensus error: {0}")]
    ConsensusError(String),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Generic errors
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Database specific errors
#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Database corruption detected")]
    Corruption,

    #[error("Database connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Batch operation failed: {0}")]
    BatchFailed(String),
}

/// Network specific errors
#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Message serialization failed: {0}")]
    SerializationFailed(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Timeout occurred: {0}")]
    Timeout(String),

    #[error("Peer banned: {0}")]
    PeerBanned(String),
}

/// Cryptographic specific errors
#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Invalid key: {0}")]
    InvalidKey(String),

    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("VRF verification failed: {0}")]
    VrfVerificationFailed(String),

    #[error("VDF computation failed: {0}")]
    VdfComputationFailed(String),

    #[error("Random number generation failed")]
    RandomGenerationFailed,
}

/// Validation specific errors
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Invalid block: {0}")]
    InvalidBlock(String),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Invalid proof: {0}")]
    InvalidProof(String),

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    #[error("Invalid height: {0}")]
    InvalidHeight(String),

    #[error("Invalid hash: {0}")]
    InvalidHash(String),

    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Gas limit exceeded: {0}")]
    GasLimitExceeded(String),

    #[error("Nonce mismatch: {0}")]
    NonceMismatch(String),
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, NornError>;

/// Error conversion traits
impl From<serde_json::Error> for NornError {
    fn from(err: serde_json::Error) -> Self {
        NornError::Serialization(err.to_string())
    }
}

impl From<toml::de::Error> for NornError {
    fn from(err: toml::de::Error) -> Self {
        NornError::Config(format!("TOML parsing error: {}", err))
    }
}

impl From<config::ConfigError> for NornError {
    fn from(err: config::ConfigError) -> Self {
        NornError::Config(format!("Configuration error: {}", err))
    }
}

/// Error context helper
pub trait ErrorContext<T> {
    fn with_context(self, context: &str) -> Result<T>;
}

impl<T, E> ErrorContext<T> for std::result::Result<T, E>
where
    E: Into<NornError>,
{
    fn with_context(self, context: &str) -> Result<T> {
        self.map_err(|e| {
            let norn_err = e.into();
            match norn_err {
                NornError::Internal(msg) => NornError::Internal(format!("{}: {}", context, msg)),
                NornError::Database(db_err) => NornError::Database(DatabaseError::TransactionFailed(format!("{}: {}", context, db_err))),
                _ => NornError::Internal(format!("{}: {}", context, norn_err)),
            }
        })
    }
}