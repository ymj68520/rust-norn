//! Simple test to debug the hanging issue

use norn_core::txpool_enhanced::EnhancedTxPool;
use norn_common::types::{Transaction, Address, Hash};
use std::sync::Arc;

#[tokio::test]
async fn test_simple_add() {
    println!("Test: Starting simple add test");
    let pool = EnhancedTxPool::new();
    println!("Test: Pool created");

    let mut tx1 = Transaction::default();
    tx1.body.hash.0[0] = 1;
    tx1.body.gas_price = Some(100);

    println!("Test: Adding tx1...");
    pool.add(tx1.clone()).await.unwrap();
    println!("Test: tx1 added");

    let stats = pool.stats().await;
    println!("Test: Pool size = {}", stats.size);
    assert_eq!(stats.size, 1);
    println!("Test: PASSED");
}

#[tokio::test]
async fn test_simple_package() {
    println!("Test: Starting simple package test");
    let pool = EnhancedTxPool::new();
    println!("Test: Pool created");

    let mut tx1 = Transaction::default();
    tx1.body.hash.0[0] = 1;
    tx1.body.gas_price = Some(100);

    println!("Test: Adding tx1...");
    pool.add(tx1.clone()).await.unwrap();
    println!("Test: tx1 added");

    struct MockChain;
    #[async_trait::async_trait]
    impl norn_core::txpool::ChainReader for MockChain {
        async fn get_transaction_by_hash(&self, _hash: &Hash) -> Option<Transaction> {
            println!("MockChain: get_transaction_by_hash called");
            None
        }
    }

    println!("Test: Calling package...");
    let chain = MockChain;
    let packaged = pool.package(&chain).await;
    println!("Test: Package returned {} transactions", packaged.len());
    assert_eq!(packaged.len(), 1);
    println!("Test: PASSED");
}
