use anyhow::{Result, anyhow};
use norn_common::types::{Block, Hash, GeneralParams, Address};
use norn_crypto::transaction::verify_transaction;
use norn_crypto::vdf::VDFCalculator;
use rs_merkle::{MerkleTree, algorithms::Sha256 as MerkleSha256};
use sha2::{Sha256, Digest};
use chrono::Utc;
use tracing::{debug, warn};
use curve25519_dalek::{
    ristretto::RistrettoPoint,
    scalar::Scalar,
};
use crate::state::AccountStateManager;

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
    #[error("Invalid proof: {0}")]
    InvalidProof(String),
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
        // Use build_mode to determine defaults
        let verify_crypto = !norn_common::build_mode::IS_TEST_MODE;

        Self {
            max_timestamp_drift: 300, // 5 minutes
            min_block_interval: 1,    // 1 second
            max_gas_limit: 10_000_000,
            max_tx_per_block: 10_000,
            max_block_size: 10 * 1024 * 1024, // 10MB
            verify_vdf: verify_crypto,  // Skip in test mode for speed
            verify_vrf: verify_crypto,  // Skip in test mode for speed
        }
    }
}

impl ValidationConfig {
    /// Create config for testing (lenient validation)
    pub fn test_config() -> Self {
        Self {
            verify_vdf: false,
            verify_vrf: false,
            ..Default::default()
        }
    }

    /// Create config for production (strict validation)
    pub fn production_config() -> Self {
        Self {
            verify_vdf: true,
            verify_vrf: true,
            ..Default::default()
        }
    }
}

/// Validate a block according to consensus rules
pub async fn validate_block(
    block: &Block,
    previous_block: Option<&Block>,
    config: &ValidationConfig,
    state_manager: Option<&AccountStateManager>,
) -> Result<()> {
    debug!("Validating block at height {}", block.header.height);

    // 1. Basic header validation
    validate_header(block, previous_block, config)?;

    // 2. Validate all transactions (with balance/nonce checks if state manager available)
    validate_transactions(block, config, state_manager).await?;

    // 3. Validate merkle root
    validate_merkle_root(block)?;

    // 4. Validate block hash
    validate_block_hash(block)?;

    // 5. Validate consensus proofs
    if config.verify_vdf {
        validate_vdf(block).await?;
    }

    if config.verify_vrf {
        verify_block_vrf(block)?;
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
async fn validate_transactions(
    block: &Block,
    config: &ValidationConfig,
    state_manager: Option<&AccountStateManager>,
) -> Result<()> {
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

        // Additional gas validation: check for overflow
        if tx.body.gas > i64::MAX - 100_000 {
            return Err(anyhow!(ValidationError::InvalidTransaction {
                index,
                reason: "Gas value too large".to_string(),
            }));
        }

        // Check gas is reasonable (not negative, within limits)
        if tx.body.gas < 21000 && !is_contract_creation(tx) {
            // Minimum gas for normal transactions is 21000
            return Err(anyhow!(ValidationError::InvalidTransaction {
                index,
                reason: "Gas below minimum (21000)".to_string(),
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

        // Validate transaction value if present
        if let Some(ref value_str) = tx.body.value {
            // Validate value format (should be a valid number string)
            if value_str.is_empty() || value_str.parse::<u128>().is_err() {
                return Err(anyhow!(ValidationError::InvalidTransaction {
                    index,
                    reason: "Invalid value format".to_string(),
                }));
            }

            // Check sender has sufficient balance (if state manager available)
            if let Some(state_mgr) = state_manager {
                let sender_balance = state_mgr.get_balance(&tx.body.address).await?;
                let value_u256: num_bigint::BigUint = value_str.parse().unwrap_or_else(|_| num_bigint::BigUint::from(0u32));

                if sender_balance < value_u256 {
                    return Err(anyhow!(ValidationError::InvalidTransaction {
                        index,
                        reason: format!("Insufficient balance: have {}, need {}", sender_balance, value_str),
                    }));
                }
            }
        }

        // Validate max_fee_per_gas if present (EIP-1559)
        let max_fee = tx.body.max_fee_per_gas;

        if let Some(fee) = max_fee {
            if fee == 0 {
                return Err(anyhow!(ValidationError::InvalidTransaction {
                    index,
                    reason: "Max fee per gas cannot be zero".to_string(),
                }));
            }
        }

        // Validate max_priority_fee_per_gas if present (EIP-1559)
        if let Some(max_priority_fee) = tx.body.max_priority_fee_per_gas {
            if max_priority_fee > max_fee.unwrap_or(max_priority_fee) {
                return Err(anyhow!(ValidationError::InvalidTransaction {
                    index,
                    reason: "Priority fee exceeds max fee".to_string(),
                }));
            }
        }

        // Validate nonce (if state manager available)
        if let Some(state_mgr) = state_manager {
            let current_nonce = state_mgr.get_nonce(&tx.body.address).await? as i64;
            if tx.body.nonce < current_nonce {
                return Err(anyhow!(ValidationError::InvalidTransaction {
                    index,
                    reason: format!("Nonce too low: current {}, got {}", current_nonce, tx.body.nonce),
                }));
            }
            // Note: We allow nonce to be higher than current for future transactions,
            // but consensus rules might want to enforce exact match
        }
    }

    // Check total gas doesn't exceed block gas limit
    if total_gas > block.header.gas_limit {
        return Err(anyhow!(ValidationError::GasLimitExceeded));
    }

    Ok(())
}

/// Check if transaction is a contract creation (no receiver or data but no receiver)
fn is_contract_creation(tx: &norn_common::types::Transaction) -> bool {
    // Contract creation if receiver is zero address OR data is non-empty with zero receiver
    tx.body.receiver == Address::default() ||
    (tx.body.receiver == Address([0u8; 20]) && !tx.body.data.is_empty())
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
    block.header.calculate_hash().unwrap_or_default()
}

/// Validate VDF proof (Legacy no-op, VDF is isolated from consensus finality)
async fn validate_vdf(_block: &Block) -> Result<()> {
    Ok(())
}

/// Extract iteration count from GeneralParams
fn extract_iterations_from_params(_params: &GeneralParams) -> u64 {
    1000
}

/// Validate VRF proof (VRF is verified in state machine)
pub fn verify_block_vrf(_block: &Block) -> Result<()> {
    Ok(())
}

/// Check if public key format is valid
fn is_valid_public_key_format(key_bytes: &[u8]) -> bool {
    // Basic validation: non-zero, reasonable length
    if key_bytes.len() < 32 {
        return false;
    }

    // Check not all zeros
    !key_bytes.iter().all(|&b| b == 0)
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
    use norn_common::types::BlockHeader;

    fn create_test_block(height: i64, prev_hash: Hash, timestamp: i64) -> Block {
        let mut block = Block {
            header: BlockHeader {
                protocol_version: norn_common::types::ProtocolVersion(2),
                chain_id: norn_common::types::ChainId(Hash([1u8; 32])),
                height,
                epoch: 1,
                round: 0,
                timestamp,
                prev_block_hash: prev_hash,
                block_hash: Hash::default(),
                merkle_root: Hash::default(),
                state_root: Hash::default(),
                proposer: norn_common::types::ValidatorId([0u8; 32]),
                stake_snapshot_hash: norn_common::types::StakeSnapshotHash::default(),
                parent_randomness: Hash::default(),
                gas_limit: 1000000,
                base_fee: 1_000_000_000,
                consensus_data_hash: Hash::default(),
            },
            transactions: vec![],
        };
        // Calculate and set the block hash
        block.header.block_hash = calculate_block_hash(&block);
        block
    }

    #[tokio::test]
    async fn test_basic_block_validation() {
        // Use config without VDF/VRF verification for testing
        let config = ValidationConfig {
            verify_vdf: false,
            verify_vrf: false,
            ..Default::default()
        };
        let genesis = create_test_block(0, Hash::default(), Utc::now().timestamp());

        // Genesis block should validate
        assert!(validate_block(&genesis, None, &config, None).await.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_height() {
        let config = ValidationConfig {
            verify_vdf: false,
            verify_vrf: false,
            ..Default::default()
        };
        let block = create_test_block(-1, Hash::default(), Utc::now().timestamp());

        // Negative height should fail validation
        let result = validate_block(&block, None, &config, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_block_sequence() {
        // Use config without VDF/VRF verification for testing
        let config = ValidationConfig {
            verify_vdf: false,
            verify_vrf: false,
            ..Default::default()
        };
        let base_time = Utc::now().timestamp();
        let genesis = create_test_block(0, Hash::default(), base_time);
        let genesis_hash = genesis.header.block_hash;

        // Create block2 with timestamp > genesis.timestamp + min_block_interval
        let block2 = create_test_block(1, genesis_hash, base_time + 2);

        // Should validate with correct previous
        assert!(validate_block(&block2, Some(&genesis), &config, None).await.is_ok());

        // Should fail with wrong previous hash
        let block2_wrong = create_test_block(1, Hash::default(), base_time + 3);
        // block2_wrong has wrong prev_block_hash so it should fail
        assert!(validate_block(&block2_wrong, Some(&genesis), &config, None).await.is_err());
    }

    #[tokio::test]
    async fn test_validation_with_state_manager() {
        // Test that validation works with state manager (balance/nonce checks)
        let config = ValidationConfig {
            verify_vdf: false,
            verify_vrf: false,
            ..Default::default()
        };
        let state_manager = AccountStateManager::default();

        // Empty block should validate even with state manager
        let genesis = create_test_block(0, Hash::default(), Utc::now().timestamp());
        assert!(validate_block(&genesis, None, &config, Some(&state_manager)).await.is_ok());
    }

    #[tokio::test]
    async fn test_validation_without_state_manager() {
        // Test that validation works without state manager (backward compatibility)
        let config = ValidationConfig {
            verify_vdf: false,
            verify_vrf: false,
            ..Default::default()
        };

        // Empty block should validate without state manager
        let genesis = create_test_block(0, Hash::default(), Utc::now().timestamp());
        assert!(validate_block(&genesis, None, &config, None).await.is_ok());
    }
}