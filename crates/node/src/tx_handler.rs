use norn_common::chain_context::ChainContext;
use norn_common::types::TransactionV2;
use norn_core::txpool_v2::TransactionV2Pool;
use norn_crypto::transaction::verify_transaction_v2;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, warn};

pub struct TxHandler {
    pool: Arc<TransactionV2Pool>,
    context: ChainContext,
    max_transaction_bytes: usize,
    verification_slots: Arc<Semaphore>,
}

impl TxHandler {
    pub fn new(
        pool: Arc<TransactionV2Pool>,
        context: ChainContext,
        max_transaction_bytes: usize,
        max_verification_tasks: usize,
    ) -> Self {
        assert!(
            max_verification_tasks > 0,
            "verification task limit must be non-zero"
        );
        Self {
            pool,
            context,
            max_transaction_bytes,
            verification_slots: Arc::new(Semaphore::new(max_verification_tasks)),
        }
    }

    pub async fn handle_tx_data(&self, data: Vec<u8>) {
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
        match TransactionV2::decode_and_validate(&data, &self.context) {
            Ok(tx) => {
                if let Err(e) = verify_transaction_v2(&tx) {
                    warn!("Rejected TransactionV2 signature: {}", e);
                    return;
                }
                let id = tx.transaction_id;
                match self.pool.add(tx) {
                    Ok(()) => info!("Received TransactionV2 id={:?}", id),
                    Err(e) => warn!("Rejected TransactionV2 from pool: {}", e),
                }
            }
            Err(e) => {
                warn!("Failed to decode TransactionV2: {}", e);
            }
        }
    }
}
