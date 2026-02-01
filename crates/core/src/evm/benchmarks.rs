//! Performance benchmarks for EVM execution
//!
//! This module provides comprehensive performance testing for the EVM,
//! measuring execution speed, gas consumption, and throughput.

use crate::evm::executor::EVMExecutor;
use crate::evm::EVMConfig;
use crate::state::account::{AccountStateManager, AccountStateConfig, AccountState, AccountType};
use norn_common::types::{Address, Hash};
use std::sync::Arc;
use std::time::{Duration, Instant};
use num_bigint::BigUint;

/// Performance benchmark result
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Benchmark name
    pub name: String,

    /// Number of operations
    pub operations: u64,

    /// Total time taken
    pub total_time: Duration,

    /// Average time per operation
    pub avg_time_per_op: Duration,

    /// Operations per second
    pub ops_per_second: f64,

    /// Total gas used
    pub total_gas_used: u64,

    /// Average gas per operation
    pub avg_gas_per_op: u64,
}

/// Performance benchmark suite
pub struct BenchmarkSuite {
    /// EVM executor
    executor: EVMExecutor,

    /// Test state manager
    state_manager: Arc<AccountStateManager>,
}

impl BenchmarkSuite {
    /// Create a new benchmark suite
    pub fn new() -> Self {
        let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
        let config = EVMConfig::default();
        let executor = EVMExecutor::new(Arc::clone(&state_manager), config);

        Self {
            executor,
            state_manager,
        }
    }

    /// Run all benchmarks
    pub fn run_all(&self) -> Vec<BenchmarkResult> {
        let mut results = Vec::new();

        println!("=== EVM Performance Benchmarks ===\n");

        // Benchmark 1: Simple transfers
        results.push(self.benchmark_simple_transfers());

        // Benchmark 2: Contract calls
        results.push(self.benchmark_contract_calls());

        // Benchmark 3: Storage operations
        results.push(self.benchmark_storage_operations());

        // Benchmark 4: Complex contracts
        results.push(self.benchmark_complex_contracts());

        // Benchmark 5: Batch execution
        results.push(self.benchmark_batch_execution());

        // Print summary
        self.print_summary(&results);

        results
    }

    /// Benchmark simple ETH transfers
    fn benchmark_simple_transfers(&self) -> BenchmarkResult {
        println!("📊 Benchmarking: Simple ETH Transfers");

        const NUM_TRANSFERS: u64 = 1000;

        // Setup accounts
        let sender = Address([1u8; 20]);
        let receiver = Address([2u8; 20]);

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async {
                self.state_manager.update_balance(
                    &sender,
                    BigUint::from(NUM_TRANSFERS) * BigUint::from(1_000_000_000_000_000_000u128)
                ).await.unwrap();
            });

        let start = Instant::now();
        let mut total_gas = 0u64;

        let rt = tokio::runtime::Runtime::new().unwrap();

        for i in 0..NUM_TRANSFERS {
            let result = rt.block_on(async {
                self.executor.execute_with_revm(
                    sender,
                    Some(receiver),
                    1_000_000_000_000_000_000u128, // 1 ETH
                    Vec::new(),
                    100_000,
                    &Default::default(),
                ).await
            });

            if let Ok(exec_result) = result {
                total_gas += exec_result.gas_used;
            }
        }

        let elapsed = start.elapsed();

        let result = BenchmarkResult {
            name: "Simple Transfers".to_string(),
            operations: NUM_TRANSFERS,
            total_time: elapsed,
            avg_time_per_op: elapsed / NUM_TRANSFERS as u32,
            ops_per_second: NUM_TRANSFERS as f64 / elapsed.as_secs_f64(),
            total_gas_used: total_gas,
            avg_gas_per_op: total_gas / NUM_TRANSFERS,
        };

        self.print_benchmark_result(&result);
        result
    }

    /// Benchmark contract calls
    fn benchmark_contract_calls(&self) -> BenchmarkResult {
        println!("📊 Benchmarking: Contract Calls");

        const NUM_CALLS: u64 = 500;

        // Deploy a simple contract
        let creator = Address([1u8; 20]);
        let contract_code = vec![
            0x60, 0x00, // PUSH1 0
            0x60, 0x00, // PUSH1 0
            0x54,       // SLOAD
            0x60, 0x01, // PUSH1 1
            0x01,       // ADD
            0x60, 0x00, // PUSH1 0
            0x55,       // SSTORE
            0x60, 0x00, // PUSH1 0
            0x52,       // MSTORE
            0x60, 0x20, // PUSH1 32
            0x60, 0x00, // PUSH1 0
            0xF3,       // RETURN
        ];

        let rt = tokio::runtime::Runtime::new().unwrap();

        let contract_address = rt.block_on(async {
            let (addr, _) = self.executor.create_contract(
                creator,
                0,
                contract_code,
                0,
                1_000_000,
            ).await.unwrap();
            addr
        });

        let start = Instant::now();
        let mut total_gas = 0u64;

        for _ in 0..NUM_CALLS {
            let result = rt.block_on(async {
                self.executor.execute_with_revm(
                    creator,
                    Some(contract_address),
                    0,
                    Vec::new(),
                    200_000,
                    &Default::default(),
                ).await
            });

            if let Ok(exec_result) = result {
                total_gas += exec_result.gas_used;
            }
        }

        let elapsed = start.elapsed();

        let result = BenchmarkResult {
            name: "Contract Calls".to_string(),
            operations: NUM_CALLS,
            total_time: elapsed,
            avg_time_per_op: elapsed / NUM_CALLS as u32,
            ops_per_second: NUM_CALLS as f64 / elapsed.as_secs_f64(),
            total_gas_used: total_gas,
            avg_gas_per_op: total_gas / NUM_CALLS,
        };

        self.print_benchmark_result(&result);
        result
    }

    /// Benchmark storage operations
    fn benchmark_storage_operations(&self) -> BenchmarkResult {
        println!("📊 Benchmarking: Storage Operations");

        const NUM_OPS: u64 = 200;

        // Create contract with heavy storage usage
        let creator = Address([1u8; 20]);
        let mut contract_code = vec![];

        // Generate code that does 10 SSTOREs
        for _ in 0..10 {
            contract_code.extend_from_slice(&[
                0x60, 0x01, // PUSH1 1
                0x60, 0x00, // PUSH1 0
                0x55,       // SSTORE
            ]);
        }

        contract_code.extend_from_slice(&[
            0x60, 0x00, // PUSH1 0
            0x52,       // MSTORE
            0x60, 0x20, // PUSH1 32
            0x60, 0x00, // PUSH1 0
            0xF3,       // RETURN
        ]);

        let rt = tokio::runtime::Runtime::new().unwrap();

        let contract_address = rt.block_on(async {
            let (addr, _) = self.executor.create_contract(
                creator,
                0,
                contract_code,
                0,
                5_000_000,
            ).await.unwrap();
            addr
        });

        let start = Instant::now();
        let mut total_gas = 0u64;

        for _ in 0..NUM_OPS {
            let result = rt.block_on(async {
                self.executor.execute_with_revm(
                    creator,
                    Some(contract_address),
                    0,
                    Vec::new(),
                    1_000_000,
                    &Default::default(),
                ).await
            });

            if let Ok(exec_result) = result {
                total_gas += exec_result.gas_used;
            }
        }

        let elapsed = start.elapsed();

        let result = BenchmarkResult {
            name: "Storage Operations".to_string(),
            operations: NUM_OPS,
            total_time: elapsed,
            avg_time_per_op: elapsed / NUM_OPS as u32,
            ops_per_second: NUM_OPS as f64 / elapsed.as_secs_f64(),
            total_gas_used: total_gas,
            avg_gas_per_op: total_gas / NUM_OPS,
        };

        self.print_benchmark_result(&result);
        result
    }

    /// Benchmark complex contracts
    fn benchmark_complex_contracts(&self) -> BenchmarkResult {
        println!("📊 Benchmarking: Complex Contracts");

        const NUM_CALLS: u64 = 100;

        // Create a more complex contract (ERC20-like)
        let creator = Address([1u8; 20]);

        // Simplified ERC20 transfer function
        let erc20_code = vec![
            // CONTRACT START
            0x60, 0x80, 0x60, 0x40, 0x52, 0x60, 0x20, 0x60, 0x18, 0xF3, // code copy
            // ... (simplified - actual ERC20 would be much longer)
            0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xF3, // return
        ];

        let rt = tokio::runtime::Runtime::new().unwrap();

        let contract_address = rt.block_on(async {
            let (addr, _) = self.executor.create_contract(
                creator,
                0,
                erc20_code,
                0,
                10_000_000,
            ).await.unwrap();
            addr
        });

        // Simulate transfer call data
        let mut call_data = vec![0u8; 36]; // function selector + args
        call_data[0..4].copy_from_slice(&[0xa9, 0x05, 0x9c, 0xbb]); // transfer(address,uint256)

        let start = Instant::now();
        let mut total_gas = 0u64;

        for _ in 0..NUM_CALLS {
            let result = rt.block_on(async {
                self.executor.execute_with_revm(
                    creator,
                    Some(contract_address),
                    0,
                    call_data.clone(),
                    500_000,
                    &Default::default(),
                ).await
            });

            if let Ok(exec_result) = result {
                total_gas += exec_result.gas_used;
            }
        }

        let elapsed = start.elapsed();

        let result = BenchmarkResult {
            name: "Complex Contracts".to_string(),
            operations: NUM_CALLS,
            total_time: elapsed,
            avg_time_per_op: elapsed / NUM_CALLS as u32,
            ops_per_second: NUM_CALLS as f64 / elapsed.as_secs_f64(),
            total_gas_used: total_gas,
            avg_gas_per_op: total_gas / NUM_CALLS,
        };

        self.print_benchmark_result(&result);
        result
    }

    /// Benchmark batch execution
    fn benchmark_batch_execution(&self) -> BenchmarkResult {
        println!("📊 Benchmarking: Batch Execution");

        const BATCH_SIZE: usize = 100;

        let rt = tokio::runtime::Runtime::new().unwrap();

        // Setup multiple accounts
        let mut addresses = Vec::new();
        for i in 0..BATCH_SIZE {
            let addr = Address([i as u8; 20]);
            addresses.push(addr);

            let final_addr = addr.clone();
            rt.block_on(async {
                self.state_manager.update_balance(
                    &final_addr,
                    BigUint::from(1_000_000_000_000_000_000u128)
                ).await.unwrap();
            });
        }

        let start = Instant::now();
        let mut total_gas = 0u64;

        // Execute transfers in batch
        for i in 0..BATCH_SIZE {
            let sender = addresses[i];
            let receiver = addresses[(i + 1) % BATCH_SIZE];

            let result = rt.block_on(async {
                self.executor.execute_with_revm(
                    sender,
                    Some(receiver),
                    1_000_000_000u128, // 1000 gwei
                    Vec::new(),
                    100_000,
                    &Default::default(),
                ).await
            });

            if let Ok(exec_result) = result {
                total_gas += exec_result.gas_used;
            }
        }

        let elapsed = start.elapsed();

        let result = BenchmarkResult {
            name: "Batch Execution".to_string(),
            operations: BATCH_SIZE as u64,
            total_time: elapsed,
            avg_time_per_op: elapsed / BATCH_SIZE as u32,
            ops_per_second: BATCH_SIZE as f64 / elapsed.as_secs_f64(),
            total_gas_used: total_gas,
            avg_gas_per_op: total_gas / BATCH_SIZE as u64,
        };

        self.print_benchmark_result(&result);
        result
    }

    /// Print a single benchmark result
    fn print_benchmark_result(&self, result: &BenchmarkResult) {
        println!("  ├─ Name: {}", result.name);
        println!("  ├─ Operations: {}", result.operations);
        println!("  ├─ Total Time: {:?}", result.total_time);
        println!("  ├─ Avg Time/Op: {:?}", result.avg_time_per_op);
        println!("  ├─ Ops/Second: {:.2}", result.ops_per_second);
        println!("  ├─ Total Gas: {}", result.total_gas_used);
        println!("  ├─ Avg Gas/Op: {}", result.avg_gas_per_op);
        println!("  └───────────────────────────────────────────");
    }

    /// Print benchmark summary
    fn print_summary(&self, results: &[BenchmarkResult]) {
        println!("\n=== Benchmark Summary ===");

        println!("\n📈 Throughput (Ops/Second):");
        for result in results {
            println!("  {:<25}: {:.2} ops/s", result.name, result.ops_per_second);
        }

        println!("\n⏱️  Average Latency:");
        for result in results {
            println!("  {:<25}: {:?}", result.name, result.avg_time_per_op);
        }

        println!("\n⛽ Average Gas/Op:");
        for result in results {
            println!("  {:<25}: {}", result.name, result.avg_gas_per_op);
        }

        // Calculate overall stats
        let total_ops: u64 = results.iter().map(|r| r.operations).sum();
        let total_time: Duration = results.iter().map(|r| r.total_time).sum();
        let overall_ops_per_sec = total_ops as f64 / total_time.as_secs_f64();

        println!("\n🎯 Overall Performance:");
        println!("  Total Operations: {}", total_ops);
        println!("  Total Time: {:?}", total_time);
        println!("  Overall Throughput: {:.2} ops/s", overall_ops_per_sec);
        println!("─────────────────────────────────────────────\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_suite() {
        let suite = BenchmarkSuite::new();
        let results = suite.run_all();

        // Verify all benchmarks completed
        assert_eq!(results.len(), 5, "Should have 5 benchmark results");

        // Verify reasonable throughput
        for result in &results {
            assert!(result.ops_per_second > 0.0, "Should have positive throughput");
            assert!(result.total_gas_used > 0, "Should consume gas");
        }
    }
}
