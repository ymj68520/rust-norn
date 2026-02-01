//! Performance benchmarks for enhanced features
//!
//! Run with: cargo bench --bench enhanced_features

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use norn_core::txpool_enhanced::EnhancedTxPool;
use norn_common::types::{Transaction, Address};

fn bench_txpool_add(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("txpool_add");
    for size in [100, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let pool = EnhancedTxPool::new();
                rt.block_on(async {
                    for i in 0..size {
                        let mut tx = Transaction::default();
                        tx.body.hash.0[0] = i as u8;
                        tx.body.gas_price = Some(100);
                        tx.body.address = Address([i as u8; 20]);
                        tx.body.nonce = i as i64;
                        black_box(pool.add(tx).await.ok());
                    }
                });
            });
        });
    }
    group.finish();
}

fn bench_txpool_package(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Setup: Add transactions
    let pool = EnhancedTxPool::new();
    rt.block_on(async {
        for i in 0..1000 {
            let mut tx = Transaction::default();
            tx.body.hash.0[0] = i as u8;
            tx.body.gas_price = Some((i % 100) * 10);
            tx.body.address = Address([i as u8; 20]);
            tx.body.nonce = i as i64;
            pool.add(tx).await.ok();
        }
    });

    c.bench_function("txpool_package_1000", |b| {
        b.iter(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                struct MockChain;
                #[async_trait::async_trait]
                impl norn_core::txpool::ChainReader for MockChain {
                    async fn get_transaction_by_hash(&self, _: &norn_common::types::Hash) -> Option<Transaction> {
                        None
                    }
                }
                black_box(pool.package(&MockChain).await)
            });
        });
    });
}

fn bench_txpool_stats(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Setup: Add transactions
    let pool = EnhancedTxPool::new();
    rt.block_on(async {
        for i in 0..5000 {
            let mut tx = Transaction::default();
            tx.body.hash.0[0] = i as u8;
            tx.body.gas_price = Some(100);
            pool.add(tx).await.ok();
        }
    });

    c.bench_function("txpool_stats_5000", |b| {
        b.iter(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                black_box(pool.stats().await)
            });
        });
    });
}

criterion_group!(benches, bench_txpool_add, bench_txpool_package, bench_txpool_stats);
criterion_main!(benches);
