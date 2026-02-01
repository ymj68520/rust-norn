//! Comprehensive Performance Benchmarks for Norn Blockchain
//!
//! This benchmark suite covers:
//! - Transaction pool operations
//! - Block production
//! - State management
//! - Merkle tree operations
//! - Storage operations
//! - Consensus operations (VRF/VDF)
//!
//! Run with: cargo bench --bench comprehensive_benchmark
//!
//! For flamegraph profiling:
//! cargo install flamegraph
//! cargo flamegraph --bench comprehensive_benchmark

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use norn_core::txpool_enhanced::EnhancedTxPool;
use norn_core::state::MerklePatriciaTrie;
use norn_common::types::{Transaction, Address, Hash};
use std::time::Duration;

// ============================================
// Transaction Pool Benchmarks
// ============================================

fn bench_txpool_add_batch(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("txpool_add_batch");
    group.sample_size(20);

    for size in [100, 500, 1000, 5000, 10000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let pool = EnhancedTxPool::new();
                rt.block_on(async {
                    for i in 0..size {
                        let mut tx = Transaction::default();
                        tx.body.hash.0[0] = (i % 256) as u8;
                        tx.body.hash.0[1] = ((i >> 8) % 256) as u8;
                        tx.body.gas_price = Some(100 + (i % 100) * 10);
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

fn bench_txpool_replacement(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("txpool_replacement");

    // Benchmark with different pool sizes
    for size in [100, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let pool = EnhancedTxPool::new();
                rt.block_on(async {
                    // Add initial transactions
                    for i in 0..size {
                        let mut tx = Transaction::default();
                        tx.body.hash.0[0] = i as u8;
                        tx.body.gas_price = Some(100);
                        tx.body.address = Address([i as u8; 20]);
                        tx.body.nonce = i as i64;
                        pool.add(tx).await.ok();
                    }

                    // Replace first transaction
                    let mut replacement = Transaction::default();
                    replacement.body.hash.0[0] = 255; // unique hash
                    replacement.body.gas_price = Some(120); // higher price
                    replacement.body.address = Address([0; 20]);
                    replacement.body.nonce = 0;
                    black_box(pool.add(replacement).await.ok());
                });
            });
        });
    }
    group.finish();
}

fn bench_txpool_cleanup(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("txpool_cleanup");

    for size in [1000, 5000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let pool = EnhancedTxPool::new();
                rt.block_on(async {
                    // Add transactions
                    for i in 0..size {
                        let mut tx = Transaction::default();
                        tx.body.hash.0[0] = i as u8;
                        tx.body.gas_price = Some(100);
                        pool.add(tx).await.ok();
                    }

                    // Cleanup expired
                    black_box(pool.cleanup_expired().await);
                });
            });
        });
    }
    group.finish();
}

// ============================================
// Merkle Tree Benchmarks
// ============================================

fn bench_merkle_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_insert");

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let mut trie = MerklePatriciaTrie::new();
                for i in 0..size {
                    let key = format!("key{:0>8}", i);
                    let value = format!("value{:0>8}", i);
                    black_box(trie.insert(key.into_bytes(), value.into_bytes()));
                }
            });
        });
    }
    group.finish();
}

fn bench_merkle_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_get");

    // Setup: Create trie with data
    let mut trie = MerklePatriciaTrie::new();
    let size = 10000;
    for i in 0..size {
        let key = format!("key{:0>8}", i);
        let value = format!("value{:0>8}", i);
        trie.insert(key.into_bytes(), value.into_bytes());
    }

    group.bench_function("merkle_get_10000", |b| {
        b.iter(|| {
            let key = format!("key{:0>8}", black_box(5000));
            black_box(trie.get(&key.into_bytes()));
        });
    });

    group.finish();
}

fn bench_merkle_root(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_root");

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let mut trie = MerklePatriciaTrie::new();
                for i in 0..size {
                    let key = format!("key{:0>8}", i);
                    let value = format!("value{:0>8}", i);
                    trie.insert(key.into_bytes(), value.into_bytes());
                }
                black_box(trie.root());
            });
        });
    }
    group.finish();
}

// ============================================
// Hashing Benchmarks
// ============================================

fn bench_hashing(c: &mut Criterion) {
    use norn_crypto::hash::hash;

    let mut group = c.benchmark_group("hashing");

    for size in [32, 256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let data = vec![0u8; size];
            b.iter(|| {
                black_box(hash(&data));
            });
        });
    }
    group.finish();
}

// ============================================
// Signature Benchmarks
// ============================================

fn bench_signing(c: &mut Criterion) {
    use norn_crypto::signature::{Keypair, Signer};
    use rand::Rng;

    let mut group = c.benchmark_group("signing");

    let keypair = Keypair::generate();
    let message = b"Hello, Norn Blockchain!";

    group.bench_function("ecdsa_sign", |b| {
        b.iter(|| {
            black_box(keypair.sign(message));
        });
    });

    group.bench_function("ecdsa_verify", |b| {
        let signature = keypair.sign(message);
        b.iter(|| {
            black_box(keypair.verify(message, &signature));
        });
    });

    group.finish();
}

// ============================================
// VRF Benchmarks
// ============================================

#[cfg(feature = "vrf")]
fn bench_vrf(c: &mut Criterion) {
    use norn_crypto::vrf::{VRFKeyPair, VRFProducer};

    let mut group = c.benchmark_group("vrf");
    group.measurement_time(Duration::from_secs(10));

    let keypair = VRFKeyPair::generate();
    let message = b"block_producer_selection_data";

    group.bench_function("vrf_prove", |b| {
        b.iter(|| {
            black_box(keypair.prove(message));
        });
    });

    group.bench_function("vrf_verify", |b| {
        let proof = keypair.prove(message);
        let public_key = keypair.public_key();
        b.iter(|| {
            black_box(VRFProducer::verify(message, &proof, public_key));
        });
    });

    group.finish();
}

// ============================================
// Comparison Benchmarks
// ============================================

fn bench_data_structures(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_structures");

    // Vec lookup vs HashMap lookup
    let size = 10000;
    let vec_data: Vec<Vec<u8>> = (0..size).map(|i| format!("key{:0>8}", i).into_bytes()).collect();
    let map_data: std::collections::HashMap<String, Vec<u8>> =
        (0..size).map(|i| (format!("key{:0>8}", i), vec![i as u8; 32])).collect();

    group.bench_function("vec_lookup_10000", |b| {
        b.iter(|| {
            let key = format!("key{:0>8}", black_box(5000));
            black_box(vec_data.iter().find(|v| v.starts_with(key.as_bytes())));
        });
    });

    group.bench_function("hashmap_lookup_10000", |b| {
        b.iter(|| {
            let key = format!("key{:0>8}", black_box(5000));
            black_box(map_data.get(&key));
        });
    });

    group.finish();
}

// ============================================
// Memory Allocation Benchmarks
// ============================================

fn bench_memory_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory");

    group.bench_function("tx_creation", |b| {
        b.iter(|| {
            let mut tx = Transaction::default();
            tx.body.hash = Hash::default();
            tx.body.gas_price = Some(100);
            tx.body.address = Address::default();
            black_box(tx);
        });
    });

    group.bench_function("hash_creation", |b| {
        b.iter(|| {
            black_box(Hash::default());
        });
    });

    group.finish();
}

// ============================================
// Register all benchmarks
// ============================================

criterion_group!(
    benches,
    // Transaction pool
    bench_txpool_add_batch,
    bench_txpool_replacement,
    bench_txpool_cleanup,
    // Merkle tree
    bench_merkle_insert,
    bench_merkle_get,
    bench_merkle_root,
    // Crypto
    bench_hashing,
    bench_signing,
    // Data structures
    bench_data_structures,
    // Memory
    bench_memory_allocation
);

#[cfg(feature = "vrf")]
criterion_group!(vrf_benches, bench_vrf);

criterion_main!(benches);
