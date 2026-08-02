use norn_common::chain_context::ChainContext;
use norn_common::types::{Block, TransactionV2};
use norn_common::utils::codec;
use norn_core::blockchain::Blockchain;
use norn_core::txpool_v2::TransactionV2Pool;
use norn_crypto::transaction::verify_transaction_v2;
use norn_network::service::NetworkEvent;
use norn_network::NetworkService;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, warn};

pub struct PeerManager {
    chain: Arc<Blockchain>,
    tx_pool_v2: Arc<TransactionV2Pool>,
    chain_context: ChainContext,
    max_transaction_bytes: usize,
    verification_slots: Arc<Semaphore>,
    #[allow(dead_code)]
    network: Arc<NetworkService>,
}

impl PeerManager {
    pub fn new(
        chain: Arc<Blockchain>,
        tx_pool_v2: Arc<TransactionV2Pool>,
        network: Arc<NetworkService>,
        chain_context: ChainContext,
        max_transaction_bytes: usize,
        max_verification_tasks: usize,
    ) -> Self {
        assert!(
            max_verification_tasks > 0,
            "verification task limit must be non-zero"
        );
        Self {
            chain,
            tx_pool_v2,
            network,
            chain_context,
            max_transaction_bytes,
            verification_slots: Arc::new(Semaphore::new(max_verification_tasks)),
        }
    }

    pub async fn handle_network_event(&self, event: NetworkEvent) {
        match event {
            NetworkEvent::Listening(_)
            | NetworkEvent::PeerConnected(_)
            | NetworkEvent::DialFailed { .. }
            | NetworkEvent::PeerAuthenticated { .. }
            | NetworkEvent::PeerDisconnected(_) => {}
            NetworkEvent::BlockReceived(data) => {
                self.handle_block(data).await;
            }
            NetworkEvent::TransactionReceived(data) => {
                self.handle_transaction(data).await;
            }
            NetworkEvent::ConsensusMessageReceived(data) => {
                self.handle_consensus_message(data).await;
            }
        }
    }

    async fn handle_block(&self, data: Vec<u8>) {
        match codec::deserialize::<Block>(&data) {
            Ok(block) => {
                info!("Received block height={}", block.header.height);

                // Validate block before adding to chain
                if self.validate_block(&block).await {
                    // Add to chain (Buffer handles validation/ordering)
                    self.chain.add_block(block).await;
                    info!("Block added to chain successfully");
                } else {
                    warn!("Block validation failed, rejecting");
                }
            }
            Err(e) => {
                warn!("Failed to deserialize block: {}", e);
            }
        }
    }

    async fn handle_transaction(&self, data: Vec<u8>) {
        let Ok(_permit) = self.verification_slots.clone().acquire_owned().await else {
            warn!("TransactionV2 verification service is shutting down");
            return;
        };
        if data.len() > self.max_transaction_bytes {
            warn!(
                "Rejected TransactionV2 above Genesis byte limit: {} > {}",
                data.len(),
                self.max_transaction_bytes
            );
            return;
        }
        match TransactionV2::decode_and_validate(&data, &self.chain_context) {
            Ok(tx) => {
                info!("Received transaction from network");

                // Verify transaction before adding to pool
                match verify_transaction_v2(&tx) {
                    Ok(()) => match self.tx_pool_v2.add(tx) {
                        Ok(()) => info!("TransactionV2 added to pool"),
                        Err(e) => warn!("TransactionV2 pool rejected transaction: {}", e),
                    },
                    Err(e) => {
                        warn!("TransactionV2 verification failed: {}", e);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to decode TransactionV2: {}", e);
            }
        }
    }

    async fn handle_consensus_message(&self, data: Vec<u8>) {
        info!("Received consensus message");
        // TODO: Implement consensus message handling
        // This could include VDF proofs, voting messages, etc.
        _ = data; // Suppress unused warning for now
    }

    async fn validate_block(&self, block: &Block) -> bool {
        // Basic block validation
        if block.header.height <= 0 {
            return false;
        }

        // TODO: Add more comprehensive block validation:
        // 1. Verify block hash
        // 2. Verify merkle root
        // 3. Verify VDF
        // 4. Verify signatures
        // 5. Check gas limits
        // 6. Validate all transactions in the block

        true // For now, accept all blocks
    }
}
