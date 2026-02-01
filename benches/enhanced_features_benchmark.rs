//! Performance benchmarks for enhanced features
//!
//! Run with: cargo bench --features production --bench enhanced_features_benchmark

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::time::Duration;

// Note: These are mock benchmarks since we can't compile the full workspace
// due to pre-existing errors. In production, these would import:
// use norn_core::txpool_enhanced::{EnhancedTxPool, PrioritizedTransaction};
// use norn_common::types::{Transaction, Hash, Address};

/// Mock transaction for benchmarking
#[derive(Clone)]
struct MockTransaction {
    hash: [u8; 32],
    gas_price: u64,
    nonce: i64,
    address: [u8; 20],
}

impl MockTransaction {
    fn new(gas_price: u64, nonce: i64) -> Self {
        Self {
            hash: [nonce as u8; 32],
            gas_price,
            nonce,
            address: [0u8; 20],
        }
    }
}

/// Benchmark: Transaction pool add operations
fn bench_txpool_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("txpool_add");

    for size in [100, 500, 1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Simulate transaction pool operations
            let mut pool: Vec<MockTransaction> = Vec::with_capacity(size);

            b.iter(|| {
                // Clear and refill pool
                pool.clear();
                for i in 0..size {
                    pool.push(MockTransaction::new(100 + (i % 100), i as i64));
                }

                // Simulate priority insertion
                pool.sort_by(|a, b| b.gas_price.cmp(&a.gas_price));

                black_box(&pool);
            });
        });
    }

    group.finish();
}

/// Benchmark: Transaction pool package (sorting)
fn bench_txpool_package(c: &mut Criterion) {
    let mut group = c.benchmark_group("txpool_package");

    for size in [100, 500, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let mut pool: Vec<MockTransaction> = Vec::with_capacity(size);
            for i in 0..size {
                pool.push(MockTransaction::new(100 + (i % 100), i as i64));
            }

            b.iter(|| {
                // Simulate packaging operation
                let mut sorted = pool.clone();
                sorted.sort_by(|a, b| b.gas_price.cmp(&a.gas_price));
                sorted.sort_by_key(|tx| tx.nonce);
                black_box(&sorted);
            });
        });
    }

    group.finish();
}

/// Benchmark: Transaction replacement check
fn bench_txpool_replacement(c: &mut Criterion) {
    let mut group = c.benchmark_group("txpool_replacement");

    group.bench_function("check_replacement", |b| {
        let existing_gas = 100u64;
        let new_gas = 115u64; // 15% higher

        b.iter(|| {
            let price_increase = new_gas.saturating_sub(existing_gas);
            let should_replace = price_increase >= (existing_gas / 10);
            black_box(should_replace);
        });
    });

    group.finish();
}

/// Benchmark: BinaryHeap operations (priority queue)
fn bench_priority_queue(c: &mut Criterion) {
    use std::collections::BinaryHeap;

    let mut group = c.benchmark_group("priority_queue");

    for size in [100, 500, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let mut heap = BinaryHeap::new();
                for i in 0..size {
                    heap.push(100 + (i % 100)); // Gas prices
                }

                // Pop all (simulating packaging)
                let mut result = Vec::with_capacity(size);
                while let Some(price) = heap.pop() {
                    result.push(price);
                }

                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark: Transaction expiration check
fn bench_expiration_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("expiration");

    let now = chrono::Utc::now().timestamp();

    group.bench_function("check_expired", |b| {
        let txs: Vec<i64> = (0..1000)
            .map(|i| now - i * 10) // Various timestamps
            .collect();

        b.iter(|| {
            let expired_count = txs.iter()
                .filter(|&&timestamp| (now - timestamp) > 3600)
                .count();
            black_box(expired_count);
        });
    });

    group.finish();
}

/// Benchmark: Fast sync batch processing
fn bench_fast_sync_batches(c: &mut Criterion) {
    let mut group = c.benchmark_group("fast_sync");

    for batch_size in [100, 500, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(batch_size), batch_size, |b, &batch_size| {
            b.iter(|| {
                // Simulate batch download and processing
                let headers: Vec<u64> = (0..batch_size).collect();
                let bodies: Vec<u64> = (0..batch_size).collect();

                // Simulate processing
                let processed: Vec<_> = headers.iter()
                    .zip(bodies.iter())
                    .map(|(&h, &b)| h + b)
                    .collect();

                black_box(processed);
            });
        });
    }

    group.finish();
}

/// Benchmark: Checkpoint verification
fn bench_checkpoint_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("checkpoint");

    group.bench_function("verify_checkpoint", |b| {
        // Simulate state root calculation
        let state_data: Vec<u8> = (0..10000).map(|i| i as u8).collect();

        b.iter(|| {
            use sha3::{Digest, Keccak256};
            let hash = Keccak256::digest(&state_data);
            black_box(hash);
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(5))
        .measurement_time(Duration::from_secs(10))
        .sample_size(100);
    targets =
        bench_txpool_add,
        bench_txpool_package,
        bench_txpool_replacement,
        bench_priority_queue,
        bench_expiration_check,
        bench_fast_sync_batches,
        bench_checkpoint_verification
}

criterion_main!(benches);
