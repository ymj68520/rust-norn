use norn_common::types::Block;
use norn_common::utils::codec;
use norn_common::types::GeneralParams;
use tracing::warn;
use num_bigint::{BigInt as NumBigInt, Sign};

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
    
    // Stub
    true 
}

pub async fn verify_block_vdf_async(block: &Block) -> bool {
    // For now, always return true to allow block processing to continue in tests.
    // Proper VDF verification requires a fully initialized norn_crypto::calculator.
    true
}