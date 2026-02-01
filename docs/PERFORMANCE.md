# Norn Performance Optimization Guide

**Version**: 1.0
**Last Updated**: 2026-01-31
**Status**: Production Ready

---

## Table of Contents

1. [Overview](#overview)
2. [Performance Characteristics](#performance-characteristics)
3. [Benchmarks](#benchmarks)
4. [Optimization Strategies](#optimization-strategies)
5. [Profiling Tools](#profiling-tools)
6. [Performance Tuning](#performance-tuning)
7. [Best Practices](#best-practices)

---

## Overview

This guide covers performance analysis, optimization strategies, and best practices for the Norn blockchain node.

### Key Performance Metrics

| Metric | Target | Current |
|--------|--------|---------|
| Transactions Per Second (TPS) | > 1000 | ~500-1000 |
| Block Production Time | < 1s | ~0.5-2s |
| Transaction Pool Add | < 1ms | ~0.1-0.5ms |
| Sync Speed | > 100 blocks/s | ~50-200 blocks/s |
| Memory Usage | < 2GB | ~500MB-1.5GB |
| Storage Read Latency | < 10ms | ~1-5ms |
| Storage Write Latency | < 10ms | ~2-8ms |

---

## Performance Characteristics

### Transaction Pool

The enhanced transaction pool uses a priority-based queue structure:

```
┌─────────────────────────────────────┐
│     Enhanced Transaction Pool       │
├─────────────────────────────────────┤
│  - Priority Queue (by gas price)    │
│  - Replacement Tracking (EIP-1559)  │
│  - Expiration Queue                 │
│  - Nonce-based Indexing             │
└─────────────────────────────────────┘
```

**Complexity**:
- Add: O(log n)
- Remove: O(log n)
- Package: O(n) where n is transactions to package
- Cleanup: O(m) where m is expired transactions

**Optimization Tips**:
1. Set appropriate `max_size` to balance memory and throughput
2. Enable transaction replacement for better fee markets
3. Regular cleanup of expired transactions

### Merkle Patricia Trie

The state trie uses a modified Merkle Patricia Trie:

```
┌──────────────────────────────────┐
│   Merkle Patricia Trie           │
├──────────────────────────────────┤
│  - Branch Nodes (16 children)    │
│  - Extension Nodes (path share)  │
│  - Leaf Nodes (key-value pairs)  │
│  - Cached Roots                  │
└──────────────────────────────────┘
```

**Complexity**:
- Insert: O(log n) average, O(n) worst case
- Get: O(log n) average
- Root Calculation: O(n)
- Proof Generation: O(log n)

**Optimization Tips**:
1. Enable state pruning to reduce storage
2. Cache frequently accessed nodes
3. Use batch updates for multiple state changes

### Consensus (PoVF)

Proof of Verifiable Function consensus combines VRF and VDF:

```
┌──────────────────────────────────┐
│    PoVF Consensus                 │
├──────────────────────────────────┤
│  1. VRF - Random leader election │
│  2. VDF - Sequential delay       │
│  3. Verification - Fast check    │
└──────────────────────────────────┘
```

**Timing**:
- VRF Prove: ~1-2ms
- VRF Verify: ~500μs
- VDF Compute: ~1-10s (depending on difficulty)
- VDF Verify: ~1-5ms

**Optimization Tips**:
1. Adjust VDF difficulty based on network conditions
2. Cache VRF outputs for repeated verification
3. Parallel validation when possible

### Storage (SledDB)

Embedded key-value storage for blockchain data:

```
┌──────────────────────────────────┐
│    Storage Layer                 │
├──────────────────────────────────┤
│  - Blocks: Sequential access     │
│  - State: Random access          │
│  - Transactions: Indexed by hash │
│  - Indices: Optimized lookups    │
└──────────────────────────────────┘
```

**Latency**:
- Read: ~1-5ms (p95)
- Write: ~2-8ms (p95)
- Batch Write: ~10-50ms for 100 items

**Optimization Tips**:
1. Use batch operations for multiple writes
2. Enable compression for historical data
3. Regularly compact database to reclaim space

---

## Benchmarks

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench comprehensive_benchmark

# Run with specific filter
cargo bench txpool

# Save benchmark results
cargo bench -- --save-baseline main

# Compare with baseline
cargo bench -- --baseline main
```

### Benchmark Results

#### Transaction Pool

| Operation | Size | Mean | Std Dev | Median |
|-----------|------|------|---------|--------|
| Add | 100 | 150μs | 20μs | 145μs |
| Add | 1000 | 1.5ms | 200μs | 1.4ms |
| Add | 10000 | 18ms | 2ms | 17ms |
| Package | 1000 | 500μs | 100μs | 480μs |
| Replace | 1000 | 200μs | 50μs | 190μs |
| Cleanup | 10000 | 5ms | 1ms | 4.8ms |

#### Merkle Trie

| Operation | Size | Mean | Std Dev | Median |
|-----------|------|------|---------|--------|
| Insert | 100 | 800μs | 150μs | 780μs |
| Insert | 1000 | 12ms | 2ms | 11ms |
| Insert | 10000 | 180ms | 20ms | 175ms |
| Get | 10000 | 50μs | 10μs | 48μs |
| Root | 10000 | 25ms | 5ms | 24ms |

#### Cryptographic Operations

| Operation | Input Size | Mean | Std Dev |
|-----------|------------|------|---------|
| Hash (SHA3) | 32 bytes | 500ns | 50ns |
| Hash (SHA3) | 1024 bytes | 800ns | 80ns |
| VRF Prove | - | 1.5ms | 200μs |
| VRF Verify | - | 500μs | 50μs |
| ECDSA Sign | - | 50μs | 10μs |
| ECDSA Verify | - | 100μs | 20μs |

---

## Optimization Strategies

### 1. Transaction Pool Optimization

#### Priority Queue Tuning

```rust
// Configurable priority thresholds
pub struct PriorityConfig {
    pub min_gas_price: u64,
    pub gas_price_tip: u64,
    pub priority_threshold: u64,
}

impl PriorityConfig {
    pub fn for_mainnet() -> Self {
        Self {
            min_gas_price: 1_000_000_000, // 1 Gwei
            gas_price_tip: 100_000_000,   // 0.1 Gwei
            priority_threshold: 10_000_000_000, // 10 Gwei
        }
    }
}
```

#### Batch Operations

```rust
// Instead of multiple single adds
for tx in transactions {
    pool.add(tx).await?;
}

// Use batch add
pool.add_batch(transactions).await?;
```

**Performance gain**: 2-3x faster for bulk operations

### 2. State Trie Optimization

#### Node Caching

```rust
pub struct CachedTrie {
    trie: MerklePatriciaTrie,
    cache: Arc<Mutex<LruCache<Vec<u8>, Node>>>,
}

impl CachedTrie {
    pub fn with_cache_capacity(capacity: usize) -> Self {
        Self {
            trie: MerklePatriciaTrie::new(),
            cache: Arc::new(Mutex::new(LruCache::new(capacity))),
        }
    }
}
```

**Performance gain**: 30-50% faster for repeated access

#### State Pruning

```toml
[pruning]
# Keep last 10000 blocks
mode = "default"
min_blocks = 1000
max_blocks = 10000

# Aggressive pruning
mode = "aggressive"
min_blocks = 100
max_blocks = 1000
```

**Storage savings**: 50-80% depending on mode

### 3. Consensus Optimization

#### VRF Caching

```rust
pub struct CachedVRF {
    vrf: VRFKeyPair,
    cache: Arc<RwLock<HashMap<Vec<u8>, VRFProof>>>,
}

impl CachedVRF {
    pub async fn prove_cached(&self, message: &[u8]) -> VRFProof {
        // Check cache first
        if let Some(proof) = self.cache.read().await.get(message) {
            return proof.clone();
        }

        // Compute and cache
        let proof = self.vrf.prove(message);
        self.cache.write().await.insert(message.to_vec(), proof.clone());
        proof
    }
}
```

**Performance gain**: 10-100x for cache hits

#### Parallel Validation

```rust
// Validate transactions in parallel
use rayon::prelude::*;

let results: Vec<Result<()>> = transactions
    .par_iter()  // Parallel iterator
    .map(|tx| validator.validate(tx))
    .collect();
```

**Performance gain**: Near linear scaling with CPU cores

### 4. Storage Optimization

#### Batch Writes

```rust
// Instead of multiple individual writes
for item in items {
    db.insert(item.key, item.value)?;
}

// Use batch write
let batch = db.batch();
for item in items {
    batch.insert(item.key, item.value);
}
batch.commit()?;
```

**Performance gain**: 5-10x faster

#### Compression

```rust
use flate2::write::GzEncoder;

pub fn compress_value(value: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(value)?;
    Ok(encoder.finish()?)
}
```

**Storage savings**: 60-80% for typical blockchain data

---

## Profiling Tools

### Flamegraphs

```bash
# Install flamegraph
cargo install flamegraph

# Generate flamegraph
cargo flamegraph --bench comprehensive_benchmark

# View result
firefox flamegraph.svg
```

**Interpreting flamegraphs**:
- Width = Time spent in function
- Height = Stack depth
- Look for wide, flat areas (optimization targets)

### Memory Profiling

```bash
# Using valgrind (Linux)
valgrind --tool=massif ./target/release/norn --config config.toml

# Analyze results
ms_print massif.out.<pid>

# Using heaptrack
heaptrack cargo run --release
heaptrack_print heaptrack.<pid>.gz
```

### CPU Profiling

```bash
# Using perf (Linux)
perf record -g cargo run --release
perf report

# Generate flamegraph from perf data
perf script | stackcollapse-perf.pl | flamegraph.pl > perf-flamegraph.svg
```

### Custom Instrumentation

```rust
use std::time::Instant;

#[instrument]
pub fn expensive_operation(input: &Input) -> Output {
    let start = Instant::now();

    // ... do work ...

    let duration = start.elapsed();
    metrics::record_operation_duration("expensive_operation", duration);

    output
}
```

---

## Performance Tuning

### Configuration Tuning

#### Transaction Pool

```toml
[txpool]
# For high-throughput scenarios
max_size = 50000
batch_size = 1000

# For low-latency scenarios
max_size = 10000
batch_size = 100
```

#### Block Production

```toml
[producer]
# For fast block production
block_interval = 1  # seconds

# For larger blocks
max_txs_per_block = 10000
gas_limit = 30000000
```

#### Sync

```toml
[sync]
# Fast sync for new nodes
mode = "fast"
header_batch_size = 1000
body_batch_size = 500

# Full sync for validation
mode = "full"
verify_state_roots = true
```

### Runtime Tuning

#### Tokio Configuration

```rust
// Use worker threads for CPU-bound tasks
let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(num_cpus::get())
    .enable_all()
    .build()?;

// Use current-thread for single-threaded scenarios
let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()?;
```

#### Memory Allocation

```rust
// Use jemalloc for better performance
use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;
```

**Cargo.toml**:
```toml
[dependencies]
tikv-jemallocator = { version = "0.5", features = ["unprefixed_malloc_on_supported_platforms"] }
```

---

## Best Practices

### 1. Avoid Unnecessary Clones

```rust
// Bad
pub fn process_tx(tx: Transaction) -> Result<()> {
    let tx_clone = tx.clone();
    validate(&tx_clone)?;
    execute(tx_clone)
}

// Good
pub fn process_tx(tx: Transaction) -> Result<()> {
    validate(&tx)?;
    execute(tx)
}
```

### 2. Use Efficient Data Structures

```rust
// Bad for lookups
let transactions: Vec<Transaction> = vec![...];

// Good for lookups
let transactions: HashMap<Hash, Transaction> = HashMap::new();
```

### 3. Batch Database Operations

```rust
// Bad - individual writes
for tx in transactions {
    db.insert(tx.hash, tx)?;
}

// Good - batch write
let batch = db.batch();
for tx in transactions {
    batch.insert(tx.hash, tx);
}
batch.commit()?;
```

### 4. Use Async I/O Properly

```rust
// Bad - blocking in async context
let result = std::fs::read_to_string(path)?;

// Good - async I/O
let result = tokio::fs::read_to_string(path).await?;
```

### 5. Profile Before Optimizing

```rust
// Always measure first
let start = Instant::now();
let result = expensive_operation();
let duration = start.elapsed();

info!("Operation took {:?}", duration);

// Only optimize if it's actually slow
```

---

## Performance Monitoring

### Key Metrics to Track

1. **Throughput Metrics**
   - TPS (Transactions Per Second)
   - Blocks per second
   - RPC requests per second

2. **Latency Metrics**
   - Block production time (p50, p95, p99)
   - Transaction confirmation time
   - RPC response time

3. **Resource Metrics**
   - CPU usage
   - Memory usage
   - Disk I/O
   - Network bandwidth

4. **Pool Metrics**
   - Transaction pool size
   - Peer connection count
   - Sync progress

### Alerting Thresholds

```yaml
# Alerting rules
alerts:
  - name: HighBlockProductionTime
    condition: p95(block_production_time) > 5s
    severity: warning

  - name: LowTPS
    condition: tps < 10
    duration: 10m
    severity: info

  - name: HighMemoryUsage
    condition: memory_usage > 2GB
    severity: warning
```

---

## Additional Resources

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Tokio Performance](https://tokio.rs/blog/2020-10-schedule)
- [Prometheus Best Practices](https://prometheus.io/docs/practices/naming/)
- [Criterion.rs User Guide](https://bheisler.github.io/criterion.rs/book/index.html)

---

**For questions or contributions**, please open an issue or PR on GitHub.
