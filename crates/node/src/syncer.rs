use std::sync::Arc;
use norn_core::blockchain::Blockchain;
use norn_network::NetworkService;
use tracing::info;

pub struct BlockSyncer {
    #[allow(dead_code)]
    chain: Arc<Blockchain>,
    #[allow(dead_code)]
    network: Arc<NetworkService>,
}

impl BlockSyncer {
    pub fn new(chain: Arc<Blockchain>, network: Arc<NetworkService>) -> Self {
        Self { chain, network }
    }

    pub async fn start(&self) {
        // TODO: Implement periodic sync request
        // 1. Get local height
        // 2. Request status from peers
        // 3. If behind, request blocks
        info!("BlockSyncer started (stub)");
    }
}
