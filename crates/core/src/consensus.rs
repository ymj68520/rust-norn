use norn_common::types::{Block, GeneralParams};
use norn_common::utils::codec;
use num_bigint::BigInt;
use hex;
use tracing::{warn, info};

// Wrapper for VDF verification
pub fn verify_block_vdf(block: &Block) -> bool {
    // 1. Deserialize Params
    let params: GeneralParams = match codec::deserialize(&block.header.params) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to deserialize block params for VDF check: {}", e);
            return false;
        }
    };

    // Check if we have a calculator available
    if let Some(calculator) = norn_crypto::calculator::get_calculator() {
        // Convert hash to BigInt (using hex string)
        let hash_hex = hex::encode(block.header.prev_block_hash.0);
        let seed = match BigInt::parse_bytes(hash_hex.as_bytes(), 16) {
            Some(s) => s,
            None => {
                warn!("Failed to parse prev_block_hash as BigInt");
                return false;
            }
        };

        // Convert proof bytes to BigInt
        let proof = BigInt::from_bytes_be(num_bigint::Sign::Plus, &params.proof);

        // For now, just return true without async verification
        // TODO: Implement proper async VDF verification with proper error handling
        true // For now, return true immediately - this should be improved for production
    } else {
        warn!("VDF calculator not initialized, skipping verification");
        false
    }
}

pub async fn verify_block_vdf_async(block: &Block) -> bool {
    // 1. Deserialize Params
    let params: GeneralParams = match codec::deserialize(&block.header.params) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to deserialize block params for VDF check: {}", e);
            return false;
        }
    };

    // Check if we have a calculator available
    if let Some(calculator) = norn_crypto::calculator::get_calculator() {
        // Convert hash to BigInt (using hex string)
        let hash_hex = hex::encode(block.header.prev_block_hash.0);
        let seed = match BigInt::parse_bytes(hash_hex.as_bytes(), 16) {
            Some(s) => s,
            None => {
                warn!("Failed to parse prev_block_hash as BigInt");
                return false;
            }
        };

        // Convert proof bytes to BigInt
        let proof = BigInt::from_bytes_be(num_bigint::Sign::Plus, &params.proof);

        // Perform actual VDF verification
        calculator.verify_block_vdf(&seed, &proof).await
    } else {
        warn!("VDF calculator not initialized, skipping verification");
        false
    }
}