use std::sync::Arc;
use norn_core::txpool::TxPool;
use norn_common::types::Transaction;
use norn_common::utils::codec;
use tracing::{warn, info};

pub struct TxHandler {
    pool: Arc<TxPool>,
}

impl TxHandler {
    pub fn new(pool: Arc<TxPool>) -> Self {
        Self { pool }
    }

    pub async fn handle_tx_data(&self, data: Vec<u8>) {
        match codec::deserialize::<Transaction>(&data) {
            Ok(tx) => {
                info!("Received tx hash={}", tx.body.hash);
                self.pool.add(tx);
            }
            Err(e) => {
                warn!("Failed to deserialize tx: {}", e);
            }
        }
    }
}
