use std::sync::Arc;
use norn_core::blockchain::Blockchain;
use norn_network::service::NetworkEvent;
use norn_network::NetworkService;
use norn_common::types::Block;
use norn_common::utils::codec;
use tracing::{info, warn, error};

pub struct PeerManager {
    chain: Arc<Blockchain>,
    network: Arc<NetworkService>,
}

impl PeerManager {
    pub fn new(chain: Arc<Blockchain>, network: Arc<NetworkService>) -> Self {
        Self { chain, network }
    }

    pub async fn handle_network_event(&self, event: NetworkEvent) {
        match event {
            NetworkEvent::BlockReceived(data) => {
                self.handle_block(data).await;
            }
            NetworkEvent::TransactionReceived(data) => {
                // TODO: Dispatch to TxHandler
            }
            NetworkEvent::ConsensusMessageReceived(_) => {
                // TODO: Handle consensus
            }
        }
    }

    async fn handle_block(&self, data: Vec<u8>) {
        match codec::deserialize::<Block>(&data) {
            Ok(block) => {
                info!("Received block height={}", block.header.height);
                // Append to Buffer (which handles validation/ordering)
                self.chain.add_block(block).await;
            }
            Err(e) => {
                warn!("Failed to deserialize block: {}", e);
            }
        }
    }
}
