use anyhow::{Result, anyhow};
use norn_common::types::{Block, Transaction, TransactionBody, Hash};
use norn_crypto::transaction::verify_transaction;
use rs_merkle::{MerkleTree, algorithms::Sha256 as MerkleSha256};
use sha2::{Sha256, Digest};
use chrono::{DateTime, Utc};
use tracing::{debug, warn};

/// Block validation errors
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Invalid block height")]
    InvalidHeight,
    #[error("Invalid timestamp")]
    InvalidTimestamp,
    #[error("Invalid previous block hash")]
    InvalidPreviousHash,
    #[error("Invalid merkle root")]
    InvalidMerkleRoot,
    #[error("Invalid block hash")]
    InvalidBlockHash,
    #[error("Invalid transaction at index {index}: {reason}")]
    InvalidTransaction { index: usize, reason: String },
    #[error("Invalid VDF proof")]
    InvalidVDF,
    #[error("Invalid VRF proof")]
    InvalidVRF,
    #[error("Gas limit exceeded")]
    GasLimitExceeded,
    #[error("Block too large")]
    BlockTooLarge,
}

/// Configuration for block validation
pub struct ValidationConfig {
    /// Maximum allowed timestamp drift from current time (seconds)
    pub max_timestamp_drift: i64,
    /// Minimum timestamp interval between blocks (seconds)
    pub min_block_interval: i64,
    /// Maximum gas limit per block
    pub max_gas_limit: i64,
    /// Maximum number of transactions per block
    pub max_tx_per_block: usize,
    /// Maximum block size in bytes
    pub max_block_size: usize,
    /// Whether to verify VDF proofs
    pub verify_vdf: bool,
    /// Whether to verify VRF proofs
    pub verify_vrf: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_timestamp_drift: 300, // 5 minutes
            min_block_interval: 1,    // 1 second
            max_gas_limit: 10_000_000,
            max_tx_per_block: 10_000,
            max_block_size: 10 * 1024 * 1024, // 10MB
            verify_vdf: true,
            verify_vrf: true,
        }
    }
}

/// Validate a block according to consensus rules
pub async fn validate_block(
    block: &Block,
    previous_block: Option<&Block>,
    config: &ValidationConfig,
) -> Result<()> {
    debug!("Validating block at height {}", block.header.height);

    // 1. Basic header validation
    validate_header(block, previous_block, config)?;

    // 2. Validate all transactions
    validate_transactions(block, config)?;

    // 3. Validate merkle root
    validate_merkle_root(block)?;

    // 4. Validate block hash
    validate_block_hash(block)?;

    // 5. Validate consensus proofs
    if config.verify_vdf {
        validate_vdf(block).await?;
    }

    if config.verify_vrf {
        validate_vrf(block).await?;
    }

    // 6. Validate block size
    validate_block_size(block, config)?;

    debug!("Block validation successful for height {}", block.header.height);
    Ok(())
}

/// Validate block header
fn validate_header(
    block: &Block,
    previous_block: Option<&Block>,
    config: &ValidationConfig,
) -> Result<()> {
    // Check height
    if block.header.height < 0 {
        return Err(anyhow!(ValidationError::InvalidHeight));
    }

    // Check previous block hash
    if let Some(prev) = previous_block {
        if block.header.height != prev.header.height + 1 {
            warn!(
                "Invalid height: expected {}, got {}",
                prev.header.height + 1,
                block.header.height
            );
            return Err(anyhow!(ValidationError::InvalidHeight));
        }

        if block.header.prev_block_hash != prev.header.block_hash {
            warn!(
                "Invalid previous hash: expected {}, got {}",
                hex::encode(prev.header.block_hash.0),
                hex::encode(block.header.prev_block_hash.0)
            );
            return Err(anyhow!(ValidationError::InvalidPreviousHash));
        }
    } else if block.header.height > 0 {
        // No previous block but height > 0 (not genesis)
        return Err(anyhow!(ValidationError::InvalidPreviousHash));
    }

    // Check timestamp
    let now = Utc::now().timestamp();
    let timestamp = block.header.timestamp;

    if timestamp > now + config.max_timestamp_drift {
        return Err(anyhow!(ValidationError::InvalidTimestamp));
    }

    if let Some(prev) = previous_block {
        if timestamp < prev.header.timestamp + config.min_block_interval {
            return Err(anyhow!(ValidationError::InvalidTimestamp));
        }
    }

    // Check gas limit
    if block.header.gas_limit > config.max_gas_limit {
        return Err(anyhow!(ValidationError::GasLimitExceeded));
    }

    Ok(())
}

/// Validate all transactions in a block
fn validate_transactions(block: &Block, config: &ValidationConfig) -> Result<()> {
    if block.transactions.len() > config.max_tx_per_block {
        return Err(anyhow!(ValidationError::BlockTooLarge));
    }

    let mut total_gas = 0i64;

    for (index, tx) in block.transactions.iter().enumerate() {
        // Verify transaction structure and signature
        if let Err(e) = verify_transaction(tx) {
            return Err(anyhow!(ValidationError::InvalidTransaction {
                index,
                reason: e.to_string(),
            }));
        }

        // Check gas
        total_gas += tx.body.gas;
        if tx.body.gas <= 0 {
            return Err(anyhow!(ValidationError::InvalidTransaction {
                index,
                reason: "Non-positive gas".to_string(),
            }));
        }

        // Check expiration
        if tx.body.expire > 0 && tx.body.expire < block.header.timestamp {
            return Err(anyhow!(ValidationError::InvalidTransaction {
                index,
                reason: "Transaction expired".to_string(),
            }));
        }

        // Verify block height and transaction index match
        if tx.body.height != block.header.height || tx.body.index != index as i64 {
            return Err(anyhow!(ValidationError::InvalidTransaction {
                index,
                reason: "Height/index mismatch".to_string(),
            }));
        }

        // Verify block hash
        if tx.body.block_hash != block.header.block_hash {
            return Err(anyhow!(ValidationError::InvalidTransaction {
                index,
                reason: "Block hash mismatch".to_string(),
            }));
        }
    }

    // Check total gas doesn't exceed block gas limit
    if total_gas > block.header.gas_limit {
        return Err(anyhow!(ValidationError::GasLimitExceeded));
    }

    Ok(())
}

/// Validate merkle root of transactions
fn validate_merkle_root(block: &Block) -> Result<()> {
    if block.transactions.is_empty() {
        // Empty block should have empty merkle root
        if block.header.merkle_root != Hash::default() {
            return Err(anyhow!(ValidationError::InvalidMerkleRoot));
        }
        return Ok(());
    }

    // Build merkle tree from transactions
    let mut tx_hashes = Vec::new();
    for tx in &block.transactions {
        tx_hashes.push(tx.body.hash.0);
    }

    let merkle_tree = MerkleTree::<MerkleSha256>::from_leaves(&tx_hashes);
    let calculated_root = merkle_tree.root_hex().unwrap_or_default();

    // Convert hex string back to Hash for comparison
    let calculated_hash = match hex::decode(&calculated_root) {
        Ok(bytes) => {
            let mut hash = Hash::default();
            let len = std::cmp::min(bytes.len(), 32);
            hash.0[..len].copy_from_slice(&bytes[..len]);
            hash
        }
        Err(_) => {
            warn!("Failed to decode calculated merkle root");
            return Err(anyhow!(ValidationError::InvalidMerkleRoot));
        }
    };

    if calculated_hash != block.header.merkle_root {
        warn!(
            "Merkle root mismatch: expected {}, got {}",
            hex::encode(block.header.merkle_root.0),
            calculated_root
        );
        return Err(anyhow!(ValidationError::InvalidMerkleRoot));
    }

    Ok(())
}

/// Validate block hash
fn validate_block_hash(block: &Block) -> Result<()> {
    let calculated_hash = calculate_block_hash(block);

    if calculated_hash != block.header.block_hash {
        warn!(
            "Block hash mismatch: expected {}, got {}",
            hex::encode(block.header.block_hash.0),
            hex::encode(calculated_hash.0)
        );
        return Err(anyhow!(ValidationError::InvalidBlockHash));
    }

    Ok(())
}

/// Calculate block hash from header fields
fn calculate_block_hash(block: &Block) -> Hash {
    let mut hasher = Sha256::new();

    // Hash all header fields except the block hash itself
    hasher.update(block.header.timestamp.to_le_bytes());
    hasher.update(block.header.prev_block_hash.0);
    hasher.update(block.header.merkle_root.0);
    hasher.update(block.header.height.to_le_bytes());
    hasher.update(block.header.public_key.0);
    hasher.update(&block.header.params);
    hasher.update(block.header.gas_limit.to_le_bytes());

    let result = hasher.finalize();
    let mut hash = Hash::default();
    hash.0.copy_from_slice(&result);
    hash
}

/// Validate VDF proof
async fn validate_vdf(block: &Block) -> Result<()> {
    // This would integrate with the VDF verification from consensus module
    // For now, we'll call the existing verification function
    if !crate::consensus::verify_block_vdf(block) {
        return Err(anyhow!(ValidationError::InvalidVDF));
    }
    Ok(())
}

/// Validate VRF proof
async fn validate_vrf(block: &Block) -> Result<()> {
    // VRF validation would go here
    // This would verify that the block proposer was selected via VRF
    // For now, we'll assume it's valid
    Ok(())
}

/// Validate block size
fn validate_block_size(block: &Block, config: &ValidationConfig) -> Result<()> {
    // Serialize block to check size
    let serialized = norn_common::utils::codec::serialize(block)
        .map_err(|_| anyhow!("Failed to serialize block for size check"))?;

    if serialized.len() > config.max_block_size {
        return Err(anyhow!(ValidationError::BlockTooLarge));
    }

    Ok(())
}

/// Quick validation for gossip/p2p propagation (less strict)
pub async fn validate_block_for_propagation(block: &Block) -> Result<()> {
    let config = ValidationConfig {
        verify_vdf: false, // Skip expensive VDF verification during propagation
        verify_vrf: false, // Skip expensive VRF verification during propagation
        ..Default::default()
    };

    // Only do basic validation
    validate_header(block, None, &config)?;
    validate_merkle_root(block)?;
    validate_block_hash(block)?;
    validate_block_size(block, &config)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::{BlockHeader, GenesisParams};

    fn create_test_block(height: i64, prev_hash: Hash) -> Block {
        Block {
            header: BlockHeader {
                timestamp: Utc::now().timestamp(),
                prev_block_hash: prev_hash,
                block_hash: Hash::default(),
                merkle_root: Hash::default(),
                height,
                public: norn_common::types::PublicKey::default(),
                params: vec![],
                gas_limit: 1000000,
            },
            transactions: vec![],
        }
    }

    #[tokio::test]
    async fn test_basic_block_validation() {
        let config = ValidationConfig::default();
        let genesis = create_test_block(0, Hash::default());

        // Genesis block should validate
        assert!(validate_block(&genesis, None, &config).await.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_height() {
        let config = ValidationConfig::default();
        let block = create_test_block(-1, Hash::default());

        assert!(matches!(
            validate_block(&block, None, &config).await.unwrap_err().downcast(),
            Ok(ValidationError::InvalidHeight)
        ));
    }

    #[tokio::test]
    async fn test_block_sequence() {
        let config = ValidationConfig::default();
        let genesis = create_test_block(0, Hash::default());

        // Set proper hash for genesis
        let genesis_hash = calculate_block_hash(&genesis);
        let mut genesis = genesis;
        genesis.header.block_hash = genesis_hash;

        let block2 = create_test_block(1, genesis_hash);

        // Should validate with correct previous
        assert!(validate_block(&block2, Some(&genesis), &config).await.is_ok());

        // Should fail with wrong previous hash
        let mut block2_wrong = block2.clone();
        block2_wrong.header.prev_block_hash = Hash::default();
        assert!(validate_block(&block2_wrong, Some(&genesis), &config).await.is_err());
    }
}