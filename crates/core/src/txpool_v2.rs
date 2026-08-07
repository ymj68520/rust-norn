use dashmap::DashMap;
use norn_common::types::{Address, TransactionId, TransactionV2};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicUsize, Ordering},
};

const MAX_V2_TX_POOL_SIZE: usize = 50_000;

#[derive(Debug, thiserror::Error)]
pub enum TransactionV2PoolError {
    #[error("invalid TransactionV2: {0}")]
    InvalidTransaction(String),
    #[error("TransactionV2 pool is full")]
    Full,
}

/// Context-independent storage for already context-bound and cryptographically
/// verified V2 transactions. Packaging order is deterministic and preserves
/// per-sender nonce order, never relying on concurrent map iteration order.
#[derive(Debug)]
pub struct TransactionV2Pool {
    txs: DashMap<TransactionId, TransactionV2>,
    next_nonce: DashMap<Address, u64>,
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
            next_nonce: DashMap::new(),
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

    pub fn contains(&self, id: &TransactionId) -> bool {
        self.txs.contains_key(id)
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

    /// Atomically reserve capacity for a verified batch before publishing any
    /// of its new transactions. Duplicate IDs are idempotent and do not
    /// consume additional pool slots.
    pub fn add_batch(
        &self,
        transactions: &[TransactionV2],
    ) -> Result<usize, TransactionV2PoolError> {
        let mut seen = HashSet::with_capacity(transactions.len());
        let mut new_transactions = Vec::with_capacity(transactions.len());
        for tx in transactions {
            tx.validate()
                .map_err(|error| TransactionV2PoolError::InvalidTransaction(error.to_string()))?;
            if seen.insert(tx.transaction_id) && !self.txs.contains_key(&tx.transaction_id) {
                new_transactions.push(tx.clone());
            }
        }
        let requested = new_transactions.len();
        if requested == 0 {
            return Ok(0);
        }
        self.count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count
                    .checked_add(requested)
                    .filter(|next| *next <= self.max_size)
            })
            .map_err(|_| TransactionV2PoolError::Full)?;

        let mut inserted = 0usize;
        for tx in new_transactions {
            if let dashmap::mapref::entry::Entry::Vacant(entry) = self.txs.entry(tx.transaction_id)
            {
                entry.insert(tx);
                inserted += 1;
            }
        }
        self.count.fetch_sub(requested - inserted, Ordering::AcqRel);
        Ok(inserted)
    }

    pub fn add_bypass_validation(&self, tx: TransactionV2) {
        let id = tx.transaction_id;
        if !self.txs.contains_key(&id) {
            self.count.fetch_add(1, Ordering::AcqRel);
            self.txs.insert(id, tx);
        }
    }

    pub fn get(&self, id: &TransactionId) -> Option<TransactionV2> {
        self.txs.get(id).map(|tx| tx.clone())
    }

    /// Resolve a set of relay short IDs in one bounded pool scan. Ambiguous
    /// prefixes are deliberately omitted so consensus falls back to the full
    /// block instead of guessing a transaction body.
    pub fn get_by_relay_short_ids(&self, wanted: &HashSet<u64>) -> HashMap<u64, TransactionV2> {
        let mut found = HashMap::with_capacity(wanted.len());
        let mut ambiguous = HashSet::new();
        for entry in self.txs.iter() {
            let short_id = entry.key().relay_short_id();
            if !wanted.contains(&short_id) || ambiguous.contains(&short_id) {
                continue;
            }
            if found.insert(short_id, entry.value().clone()).is_some() {
                found.remove(&short_id);
                ambiguous.insert(short_id);
            }
        }
        found
    }

    pub fn remove(&self, id: &TransactionId) -> Option<TransactionV2> {
        let removed = self.txs.remove(id).map(|(_, tx)| tx);
        if let Some(transaction) = &removed {
            self.next_nonce
                .entry(transaction.sender)
                .and_modify(|next| *next = (*next).max(transaction.nonce.saturating_add(1)))
                .or_insert_with(|| transaction.nonce.saturating_add(1));
            self.count.fetch_sub(1, Ordering::AcqRel);
        }
        removed
    }

    /// Advance a sender's canonical nonce after finality and discard every
    /// now-obsolete transaction for that sender.  Admission is keyed by
    /// transaction ID, so distinct signed transactions can still race with
    /// the same nonce. Once one of them is finalized, the alternatives can
    /// never execute and must not keep occupying bounded pool capacity.
    pub fn remove_committed(&self, transaction: &TransactionV2) -> usize {
        let next_nonce = transaction.nonce.saturating_add(1);
        self.next_nonce
            .entry(transaction.sender)
            .and_modify(|next| *next = (*next).max(next_nonce))
            .or_insert(next_nonce);

        let stale_ids = self
            .txs
            .iter()
            .filter_map(|entry| {
                (entry.value().sender == transaction.sender && entry.value().nonce < next_nonce)
                    .then_some(*entry.key())
            })
            .collect::<Vec<_>>();
        let mut removed = 0;
        for id in stale_ids {
            if self.txs.remove(&id).is_some() {
                self.count.fetch_sub(1, Ordering::AcqRel);
                removed += 1;
            }
        }
        removed
    }

    /// Advance all finalized sender nonces and prune stale pool entries in a
    /// single pass. Calling `remove_committed` once per block transaction is
    /// quadratic (`block_len * pool_len`) and delayed state activation by
    /// several seconds on Raspberry Pi under large blocks.
    pub fn remove_committed_batch(&self, transactions: &[TransactionV2]) -> usize {
        if transactions.is_empty() {
            return 0;
        }
        let mut finalized_next = HashMap::<Address, u64>::new();
        for transaction in transactions {
            let next = transaction.nonce.saturating_add(1);
            finalized_next
                .entry(transaction.sender)
                .and_modify(|current| *current = (*current).max(next))
                .or_insert(next);
        }
        for (sender, next) in &finalized_next {
            self.next_nonce
                .entry(*sender)
                .and_modify(|current| *current = (*current).max(*next))
                .or_insert(*next);
        }

        let stale_ids = self
            .txs
            .iter()
            .filter_map(|entry| {
                finalized_next
                    .get(&entry.value().sender)
                    .filter(|next| entry.value().nonce < **next)
                    .map(|_| *entry.key())
            })
            .collect::<Vec<_>>();
        let mut removed = 0;
        for id in stale_ids {
            if self.txs.remove(&id).is_some() {
                self.count.fetch_sub(1, Ordering::AcqRel);
                removed += 1;
            }
        }
        removed
    }

    /// Drop a sender suffix that cannot become valid at the current canonical
    /// state (for example, an unfunded nonce lane). Unlike finality removal,
    /// this deliberately does not advance the sender nonce frontier.
    pub fn discard_sender_from_nonce(&self, sender: Address, first_nonce: u64) -> usize {
        let invalid_ids = self
            .txs
            .iter()
            .filter_map(|entry| {
                (entry.value().sender == sender && entry.value().nonce >= first_nonce)
                    .then_some(*entry.key())
            })
            .collect::<Vec<_>>();
        let mut removed = 0;
        for id in invalid_ids {
            if self.txs.remove(&id).is_some() {
                self.count.fetch_sub(1, Ordering::AcqRel);
                removed += 1;
            }
        }
        removed
    }

    /// Package at most `limit` transactions in deterministic sender/nonce order.
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
        self.select_with_base_nonces(limit, &HashMap::new())
    }

    /// Select against canonical account nonces supplied by the block
    /// producer. This makes restart recovery and out-of-order gossip use the
    /// finalized state frontier rather than guessing from arrival order.
    pub fn select_with_base_nonces(
        &self,
        limit: usize,
        base_nonces: &HashMap<Address, u64>,
    ) -> Vec<TransactionV2> {
        if limit == 0 {
            return Vec::new();
        }
        // A single global sort made proposal production O(pool_len log
        // pool_len) and consumed several seconds on Cortex-A53 once the pool
        // held tens of thousands of transactions. Preserve the exact same
        // deterministic `(sender, nonce, transaction_id)` order while sorting
        // only the small sender list and each sender's own nonce lane.
        let mut by_sender = HashMap::<Address, Vec<(TransactionId, TransactionV2)>>::new();
        for entry in self.txs.iter() {
            by_sender
                .entry(entry.value().sender)
                .or_default()
                .push((*entry.key(), entry.value().clone()));
        }
        let mut senders = by_sender.keys().copied().collect::<Vec<_>>();
        senders.sort_by(|left, right| left.0.cmp(&right.0));
        let mut expected_nonce = base_nonces.clone();
        for entry in self.next_nonce.iter() {
            expected_nonce
                .entry(*entry.key())
                .and_modify(|nonce| *nonce = (*nonce).max(*entry.value()))
                .or_insert(*entry.value());
        }
        let mut selected = Vec::with_capacity(limit.min(self.len()));
        for sender in senders {
            let Some(mut candidates) = by_sender.remove(&sender) else {
                continue;
            };
            candidates.sort_by(|(left_id, left), (right_id, right)| {
                left.nonce
                    .cmp(&right.nonce)
                    .then_with(|| left_id.0 .0.cmp(&right_id.0 .0))
            });
            for (_, tx) in candidates {
                // An unseen sender starts at canonical nonce zero. Treating
                // the first gossip nonce as the frontier lets out-of-order
                // delivery (for example nonce 178 arriving before nonce 0)
                // create a proposal that deterministic execution must reject.
                // Finality advances this map for established senders.
                let expected = expected_nonce.entry(tx.sender).or_insert(0);
                if tx.nonce != *expected {
                    continue;
                }
                *expected = expected.saturating_add(1);
                selected.push(tx);
                if selected.len() == limit {
                    return selected;
                }
            }
        }
        selected
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
        pool.add(tx(0)).unwrap();
        pool.add(tx(1)).unwrap();
        pool.add(tx(0)).unwrap();
        assert_eq!(pool.len(), 2);

        let packaged = pool.package(2);
        assert_eq!(packaged.len(), 2);
        assert_eq!(
            packaged
                .iter()
                .map(|transaction| transaction.nonce)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert!(pool.is_empty());
    }

    #[test]
    fn v2_pool_selection_is_non_destructive_until_commit() {
        let pool = TransactionV2Pool::new();
        let transaction = tx(0);
        let id = transaction.transaction_id;
        pool.add(transaction).unwrap();

        let selected = pool.select(1);
        assert_eq!(selected.len(), 1);
        assert_eq!(pool.len(), 1);
        assert!(pool.get(&id).is_some());
    }

    #[test]
    fn v2_pool_preserves_nonce_order_for_a_sender() {
        let pool = TransactionV2Pool::new();
        pool.add(tx(2)).unwrap();
        pool.add(tx(0)).unwrap();
        pool.add(tx(1)).unwrap();

        let packaged = pool.select(3);
        let nonces: Vec<u64> = packaged
            .into_iter()
            .map(|transaction| transaction.nonce)
            .collect();
        assert_eq!(nonces, vec![0, 1, 2]);
    }

    #[test]
    fn finality_evicts_conflicting_nonce_alternatives() {
        let pool = TransactionV2Pool::new();
        let committed = tx(0);
        let mut conflicting = tx(0);
        conflicting.timestamp = 2;
        conflicting.transaction_id = conflicting.calculate_id().unwrap();
        let successor = tx(1);

        pool.add(committed.clone()).unwrap();
        pool.add(conflicting).unwrap();
        pool.add(successor.clone()).unwrap();
        assert_eq!(pool.len(), 3);

        assert_eq!(pool.remove_committed(&committed), 2);
        assert_eq!(pool.len(), 1);
        assert_eq!(pool.select(1)[0].transaction_id, successor.transaction_id);
    }

    #[test]
    fn batch_finality_prunes_each_sender_frontier_in_one_operation() {
        let pool = TransactionV2Pool::new();
        let sender_a = (0..4).map(tx).collect::<Vec<_>>();
        let sender_b = (0..4)
            .map(|nonce| {
                let mut transaction = tx(nonce);
                transaction.sender = Address([9; 20]);
                transaction.transaction_id = transaction.calculate_id().unwrap();
                transaction
            })
            .collect::<Vec<_>>();
        for transaction in sender_a.iter().chain(&sender_b) {
            pool.add(transaction.clone()).unwrap();
        }

        assert_eq!(
            pool.remove_committed_batch(&[sender_a[1].clone(), sender_b[2].clone()]),
            5
        );
        assert_eq!(pool.len(), 3);
        assert_eq!(
            pool.select(3)
                .iter()
                .map(|transaction| (transaction.sender, transaction.nonce))
                .collect::<Vec<_>>(),
            vec![
                (sender_a[2].sender, 2),
                (sender_a[3].sender, 3),
                (sender_b[3].sender, 3),
            ]
        );
    }

    #[test]
    fn v2_pool_enforces_configured_capacity() {
        let pool = TransactionV2Pool::new_with_capacity(1);
        pool.add(tx(1)).unwrap();
        assert!(matches!(pool.add(tx(2)), Err(TransactionV2PoolError::Full)));
    }
}
