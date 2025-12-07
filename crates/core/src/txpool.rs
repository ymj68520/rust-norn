use dashmap::DashMap;
use norn_common::types::{Hash, Transaction};
use std::sync::atomic::{AtomicUsize, Ordering};
use async_trait::async_trait;
use tracing::{debug};

// Trait to decouple TxPool from Blockchain
#[async_trait]
pub trait ChainReader: Send + Sync {
    async fn get_transaction_by_hash(&self, hash: &Hash) -> Option<Transaction>;
}

const MAX_TX_POOL_SIZE: usize = 20480;
const MAX_TX_PACKAGE_COUNT: usize = 10000;

#[derive(Debug)]
pub struct TxPool {
    txs: DashMap<Hash, Transaction>,
    count: AtomicUsize,
}

impl TxPool {
    pub fn new() -> Self {
        Self {
            txs: DashMap::new(),
            count: AtomicUsize::new(0),
        }
    }

    pub fn add(&self, tx: Transaction) {
        if self.count.load(Ordering::Relaxed) >= MAX_TX_POOL_SIZE {
            return;
        }
        
        let hash = tx.body.hash;
        if self.txs.contains_key(&hash) {
            return;
        }

        self.txs.insert(hash, tx);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn remove(&self, hash: &Hash) {
        if self.txs.remove(hash).is_some() {
            self.count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn contains(&self, hash: &Hash) -> bool {
        self.txs.contains_key(hash)
    }

    pub fn get(&self, hash: &Hash) -> Option<Transaction> {
        self.txs.get(hash).map(|t| t.clone())
    }

    pub async fn package<C: ChainReader>(&self, chain: &C) -> Vec<Transaction> {
        debug!("Start package transaction...");
        let mut result = Vec::with_capacity(MAX_TX_PACKAGE_COUNT);
        let mut to_remove = Vec::new();
        
        // Iterating DashMap is synchronous.
        // But checking chain is async.
        // We can collect candidates first, then check async.
        
        let candidates: Vec<(Hash, Transaction)> = self.txs
            .iter()
            .take(MAX_TX_PACKAGE_COUNT * 2) // Take more to filter?
            .map(|r| (*r.key(), r.value().clone()))
            .collect();

        for (hash, tx) in candidates {
            if result.len() >= MAX_TX_PACKAGE_COUNT {
                break;
            }

            if chain.get_transaction_by_hash(&hash).await.is_some() {
                debug!("Transaction already in database.");
                to_remove.push(hash);
            } else {
                result.push(tx);
                to_remove.push(hash); // Remove from pool as it is being packaged?
                // Go code removes it from pool when packaging!
                // "pool.txs.Delete(txHash)"
            }
        }
        
        for hash in to_remove {
            self.remove(&hash);
        }

        result
    }
}

impl Default for TxPool {

    fn default() -> Self {

        Self::new()

    }

}



#[cfg(test)]

mod tests {

    use super::*;

    use norn_common::types::{Hash, Transaction};

    use async_trait::async_trait;



    struct MockChain;



    #[async_trait]

    impl ChainReader for MockChain {

        async fn get_transaction_by_hash(&self, _hash: &Hash) -> Option<Transaction> {

            None

        }

    }



    fn create_tx(byte: u8) -> Transaction {

        let mut tx = Transaction::default();

        tx.body.hash.0[0] = byte;

        tx

    }



    #[test]

    fn test_txpool_add_remove() {

        let pool = TxPool::new();

        let tx = create_tx(1);

        let hash = tx.body.hash;



        pool.add(tx.clone());

        assert!(pool.contains(&hash));

        assert!(pool.get(&hash).is_some());



        pool.remove(&hash);

        assert!(!pool.contains(&hash));

        assert!(pool.get(&hash).is_none());

    }



    #[tokio::test]

    async fn test_txpool_package() {

        let pool = TxPool::new();

        let tx1 = create_tx(1);

        let tx2 = create_tx(2);

        

        pool.add(tx1.clone());

        pool.add(tx2.clone());

        

        let chain = MockChain;

        let txs = pool.package(&chain).await;

        

        assert_eq!(txs.len(), 2);

        assert!(!pool.contains(&tx1.body.hash));

        assert!(!pool.contains(&tx2.body.hash));

    }

}
