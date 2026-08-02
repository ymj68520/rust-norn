use dashmap::DashMap;
use norn_common::types::{TransactionId, TransactionV2};
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_V2_TX_POOL_SIZE: usize = 20_480;

#[derive(Debug, thiserror::Error)]
pub enum TransactionV2PoolError {
    #[error("invalid TransactionV2: {0}")]
    InvalidTransaction(String),
    #[error("TransactionV2 pool is full")]
    Full,
}

/// Context-independent storage for already context-bound and cryptographically
/// verified V2 transactions. Packaging order is derived from the transaction
/// ID, never from concurrent map iteration order.
#[derive(Debug)]
pub struct TransactionV2Pool {
    txs: DashMap<TransactionId, TransactionV2>,
    count: AtomicUsize,
    max_size: usize,
}

impl TransactionV2Pool {
    pub fn new() -> Self {
        Self::new_with_capacity(MAX_V2_TX_POOL_SIZE)
    }

    pub fn new_with_capacity(max_size: usize) -> Self {
        assert!(
            max_size > 0,
            "V2 transaction pool capacity must be non-zero"
        );
        Self {
            txs: DashMap::new(),
            count: AtomicUsize::new(0),
            max_size,
        }
    }

    pub fn len(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn add(&self, tx: TransactionV2) -> Result<(), TransactionV2PoolError> {
        tx.validate()
            .map_err(|e| TransactionV2PoolError::InvalidTransaction(e.to_string()))?;
        let id = tx.transaction_id;
        if self.txs.contains_key(&id) {
            return Ok(());
        }
        self.count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < self.max_size).then_some(count + 1)
            })
            .map_err(|_| TransactionV2PoolError::Full)?;

        // Reserve the bounded slot before inserting so concurrent producers
        // cannot exceed the protocol pool ceiling. A duplicate that raced the
        // initial lookup returns the reservation.
        if let dashmap::mapref::entry::Entry::Vacant(entry) = self.txs.entry(id) {
            entry.insert(tx);
        } else {
            self.count.fetch_sub(1, Ordering::AcqRel);
        }
        Ok(())
    }

    pub fn get(&self, id: &TransactionId) -> Option<TransactionV2> {
        self.txs.get(id).map(|tx| tx.clone())
    }

    pub fn remove(&self, id: &TransactionId) -> Option<TransactionV2> {
        let removed = self.txs.remove(id).map(|(_, tx)| tx);
        if removed.is_some() {
            self.count.fetch_sub(1, Ordering::AcqRel);
        }
        removed
    }

    /// Package at most `limit` transactions in canonical ID order.
    pub fn package(&self, limit: usize) -> Vec<TransactionV2> {
        let selected = self.select(limit);
        let mut result = Vec::with_capacity(selected.len());
        for tx in selected {
            if self.remove(&tx.transaction_id).is_some() {
                result.push(tx);
            }
        }
        result
    }

    /// Select at most `limit` transactions without removing them.  A block
    /// proposal is not finality: transactions stay available until a commit
    /// path explicitly removes them, so a rejected or crashed proposal cannot
    /// make a valid transaction disappear.
    pub fn select(&self, limit: usize) -> Vec<TransactionV2> {
        let mut candidates: Vec<(TransactionId, TransactionV2)> = self
            .txs
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();
        candidates.sort_by_key(|(id, _)| id.0 .0);
        candidates
            .into_iter()
            .take(limit)
            .map(|(_, tx)| tx)
            .collect()
    }
}

impl Default for TransactionV2Pool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::{Address, ChainId, Hash, ProtocolVersion, PublicKey, TransactionType};

    fn tx(byte: u8) -> TransactionV2 {
        let mut tx = TransactionV2 {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(Hash([1; 32])),
            nonce: byte as u64,
            sender: Address([2; 20]),
            receiver: Some(Address([3; 20])),
            value: 1,
            gas_limit: 21_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 1,
            data: Vec::new(),
            event: Vec::new(),
            opt: Vec::new(),
            state: Vec::new(),
            expire: None,
            timestamp: 1,
            tx_type: TransactionType::Native,
            access_list: Vec::new(),
            public_key: PublicKey([4; 33]),
            signature: [byte.max(1); 64],
            transaction_id: TransactionId::default(),
        };
        tx.transaction_id = tx.calculate_id().unwrap();
        tx
    }

    #[test]
    fn v2_pool_packages_by_id_and_deduplicates() {
        let pool = TransactionV2Pool::new();
        pool.add(tx(1)).unwrap();
        pool.add(tx(2)).unwrap();
        pool.add(tx(1)).unwrap();
        assert_eq!(pool.len(), 2);

        let packaged = pool.package(2);
        assert_eq!(packaged.len(), 2);
        assert!(packaged[0].transaction_id.0 .0 <= packaged[1].transaction_id.0 .0);
        assert!(pool.is_empty());
    }

    #[test]
    fn v2_pool_selection_is_non_destructive_until_commit() {
        let pool = TransactionV2Pool::new();
        let transaction = tx(1);
        let id = transaction.transaction_id;
        pool.add(transaction).unwrap();

        let selected = pool.select(1);
        assert_eq!(selected.len(), 1);
        assert_eq!(pool.len(), 1);
        assert!(pool.get(&id).is_some());
    }

    #[test]
    fn v2_pool_enforces_configured_capacity() {
        let pool = TransactionV2Pool::new_with_capacity(1);
        pool.add(tx(1)).unwrap();
        assert!(matches!(pool.add(tx(2)), Err(TransactionV2PoolError::Full)));
    }
}
