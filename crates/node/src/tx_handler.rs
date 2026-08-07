use norn_common::chain_context::ChainContext;
use norn_common::types::{TransactionId, TransactionV2, TransactionV2Batch};
use norn_core::txpool_v2::TransactionV2Pool;
use norn_crypto::transaction::{verify_transaction_v2, verify_transactions_v2_ingress};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, info, warn};

pub struct TxHandler {
    work_tx: mpsc::Sender<Vec<u8>>,
    cache_tx: mpsc::Sender<Vec<u8>>,
    relay_cache: Arc<RelayTransactionCache>,
}

struct RelayCacheInner {
    transactions: HashMap<TransactionId, TransactionV2>,
    insertion_order: VecDeque<TransactionId>,
}

/// Bounded cache of context-bound, self-committed transaction bodies seen on
/// the gossip topic. Entries are not trusted as signature-verified: compact
/// proposal reconstruction always passes the resulting block through normal
/// consensus validation before a vote can be cast.
pub struct RelayTransactionCache {
    inner: Mutex<RelayCacheInner>,
    capacity: usize,
}

impl RelayTransactionCache {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(RelayCacheInner {
                transactions: HashMap::with_capacity(capacity),
                insertion_order: VecDeque::with_capacity(capacity),
            }),
            capacity,
        }
    }

    fn insert_batch(&self, transactions: &[TransactionV2]) {
        let mut inner = self.inner.lock().expect("relay transaction cache poisoned");
        for transaction in transactions {
            if inner.transactions.contains_key(&transaction.transaction_id) {
                continue;
            }
            while inner.transactions.len() >= self.capacity {
                let Some(oldest) = inner.insertion_order.pop_front() else {
                    break;
                };
                inner.transactions.remove(&oldest);
            }
            inner.insertion_order.push_back(transaction.transaction_id);
            inner
                .transactions
                .insert(transaction.transaction_id, transaction.clone());
        }
    }

    pub fn get(&self, id: &TransactionId) -> Option<TransactionV2> {
        self.inner
            .lock()
            .expect("relay transaction cache poisoned")
            .transactions
            .get(id)
            .cloned()
    }

    pub fn get_by_relay_short_ids(&self, wanted: &HashSet<u64>) -> HashMap<u64, TransactionV2> {
        let inner = self.inner.lock().expect("relay transaction cache poisoned");
        let mut found = HashMap::with_capacity(wanted.len());
        let mut ambiguous = HashSet::new();
        for (transaction_id, transaction) in &inner.transactions {
            let short_id = transaction_id.relay_short_id();
            if !wanted.contains(&short_id) || ambiguous.contains(&short_id) {
                continue;
            }
            if found.insert(short_id, transaction.clone()).is_some() {
                found.remove(&short_id);
                ambiguous.insert(short_id);
            }
        }
        found
    }
}

#[derive(Clone)]
struct TxVerificationWork {
    pool: Arc<TransactionV2Pool>,
    context: ChainContext,
    max_transaction_bytes: usize,
    max_gossip_bytes: usize,
}

impl TxHandler {
    pub fn new(
        pool: Arc<TransactionV2Pool>,
        context: ChainContext,
        max_transaction_bytes: usize,
        max_gossip_bytes: usize,
        max_verification_tasks: usize,
    ) -> Self {
        assert!(
            max_verification_tasks > 0,
            "verification task limit must be non-zero"
        );
        let work = TxVerificationWork {
            pool,
            context,
            max_transaction_bytes,
            max_gossip_bytes,
        };
        let relay_cache = Arc::new(RelayTransactionCache::new(50_000));
        // Keep transaction verification behind a bounded queue. A peer can
        // advertise a burst much faster than an ARM validator can verify it;
        // accepting unbounded spawned tasks lets that burst monopolize Tokio
        // workers and delay consensus messages. The receiver deliberately
        // waits for a permit before taking more work, so `try_enqueue` applies
        // backpressure without ever blocking the consensus event loop.
        // Genesis permits a large verification parallelism ceiling, but this
        // ingress worker intentionally runs one batch at a time to protect BFT.
        // Multiplying that ceiling by 64 previously buffered thousands of
        // batches and kept an ARM validator CPU/SD card busy long after the
        // client burst ended. Bound cryptographic debt to at most 32 batches;
        // compact proposals repair any bodies deliberately dropped here.
        let verification_queue_capacity = max_verification_tasks.clamp(8, 32);
        // Structural relay caching is much cheaper and helps compact proposal
        // reconstruction, so give it a modestly larger but still bounded lane.
        let relay_cache_queue_capacity = max_verification_tasks.saturating_mul(2).clamp(32, 128);
        let (work_tx, mut work_rx) = mpsc::channel(verification_queue_capacity);
        let (cache_tx, mut cache_rx) = mpsc::channel::<Vec<u8>>(relay_cache_queue_capacity);
        let cache = relay_cache.clone();
        // Structural decoding for compact reconstruction must never run on the
        // node event loop. A single bounded decoder is fast enough to stay
        // ahead of ARM signature verification, while `try_send` preserves
        // consensus priority under an abusive burst.
        std::thread::Builder::new()
            .name("norn-relay-cache".to_owned())
            .spawn(move || {
                while let Some(data) = cache_rx.blocking_recv() {
                    if let Ok(Some(batch)) =
                        TransactionV2Batch::decode_and_validate(&data, &context)
                    {
                        if batch.transactions.iter().all(|transaction| {
                            bincode::serialized_size(transaction)
                                .map(|size| size as usize <= max_transaction_bytes)
                                .unwrap_or(false)
                        }) {
                            cache.insert_batch(&batch.transactions);
                        }
                    } else if data.len() <= max_transaction_bytes {
                        if let Ok(transaction) = TransactionV2::decode_and_validate(&data, &context)
                        {
                            cache.insert_batch(std::slice::from_ref(&transaction));
                        }
                    }
                }
            })
            .expect("failed to start relay cache decoder");
        // A batch already uses the dedicated bounded cryptographic pool. Only
        // dispatch one gossip work item at a time so multiple batches cannot
        // multiply CPU use and starve consensus.
        let verification_slots = Arc::new(Semaphore::new(1));
        tokio::spawn(async move {
            while let Some(data) = work_rx.recv().await {
                let Ok(permit) = verification_slots.clone().acquire_owned().await else {
                    break;
                };
                let work = work.clone();
                tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    work.process(data);
                });
            }
        });
        Self {
            work_tx,
            cache_tx,
            relay_cache,
        }
    }

    /// Queue a network transaction without delaying consensus dispatch.
    /// `false` means the bounded verification queue is full or shutting down;
    /// callers can safely drop gossip because the transaction is still held by
    /// the originating validator and proposals carry their own transactions.
    pub fn try_enqueue(&self, data: Vec<u8>) -> bool {
        let _ = self.cache_tx.try_send(data.clone());
        self.work_tx.try_send(data).is_ok()
    }

    pub fn relay_cache(&self) -> Arc<RelayTransactionCache> {
        self.relay_cache.clone()
    }
}

impl TxVerificationWork {
    fn process(&self, data: Vec<u8>) {
        if data.len() > self.max_gossip_bytes {
            warn!(
                "Rejected transaction gossip above Genesis block byte limit: {} > {}",
                data.len(),
                self.max_gossip_bytes
            );
            return;
        }
        match TransactionV2Batch::decode(&data) {
            Ok(Some(batch)) => {
                if let Err(error) = self.process_batch(batch.transactions) {
                    warn!("Rejected TransactionV2 batch: {error}");
                }
                return;
            }
            Ok(None) => {}
            Err(error) => {
                warn!("Failed to decode TransactionV2 batch: {error}");
                return;
            }
        }
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

    fn process_batch(&self, mut transactions: Vec<TransactionV2>) -> anyhow::Result<()> {
        for tx in &transactions {
            if tx.protocol_version != self.context.protocol_version
                || tx.chain_id != self.context.chain_id
            {
                anyhow::bail!("transaction context does not match this chain");
            }
            let encoded_len = bincode::serialized_size(tx)? as usize;
            if encoded_len > self.max_transaction_bytes {
                anyhow::bail!("transaction exceeds Genesis byte limit");
            }
        }
        // Gossipsub and compact-body repair can race to deliver the same
        // transactions. Do not spend an Ed25519 verification on an ID already
        // held in the cryptographically verified pool.
        let received = transactions.len();
        transactions.retain(|tx| !self.pool.contains(&tx.transaction_id));
        if transactions.is_empty() {
            debug!("Ignored duplicate TransactionV2 batch count={}", received);
            return Ok(());
        }
        verify_transactions_v2_ingress(&transactions)
            .map_err(|error| anyhow::anyhow!("invalid batch signature: {error}"))?;
        let admitted = self.pool.add_batch(&transactions)?;
        debug!(
            "Received TransactionV2 batch count={} new={} admitted={}",
            received,
            transactions.len(),
            admitted
        );
        Ok(())
    }
}
