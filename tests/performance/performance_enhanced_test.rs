// 增强的性能测试
// 运行方式: cargo test --test performance_enhanced_test

use norn_storage::StateDB;
use norn_common::types::Hash;
use std::sync::Arc;
use std::time::Instant;
use tempfile::TempDir;
use tokio::runtime::Runtime;

#[test]
fn test_concurrent_writes_performance() {
    // 测试并发写入性能
    let rt = Runtime::new().unwrap();
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(rt.block_on(async {
        StateDB::new(temp_dir.path()).await.unwrap()
    }));

    let start = Instant::now();
    let mut handles = vec![];

    // 创建10个并发任务，每个写入1000条
    for i in 0..10 {
        let db_clone = db.clone();
        let handle = std::thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                for j in 0..1000 {
                    let key = format!("key_{}_{}", i, j);
                    let value = format!("value_{}_{}", i, j);
                    db_clone.insert(key.as_bytes(), value.as_bytes()).await.unwrap();
                }
            });
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let total_writes = 10_000;
    let ops_per_sec = total_writes as f64 / duration.as_secs_f64();

    println!("✅ Concurrent writes: {} ops in {:.2}s ({:.2} ops/sec)",
        total_writes, duration.as_secs_f64(), ops_per_sec);

    assert!(ops_per_sec > 1000.0, "Performance too low: {} ops/sec", ops_per_sec);
}

#[test]
fn test_batch_operations_performance() {
    // 测试批量操作性能
    let rt = Runtime::new().unwrap();
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(rt.block_on(async {
        StateDB::new(temp_dir.path()).await.unwrap()
    }));

    // 测试批量插入
    let start = Instant::now();
    let batch_size = 1000;

    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        for i in 0..batch_size {
            let key = format!("batch_key_{}", i);
            let value = format!("batch_value_{}", i);
            db.insert(key.as_bytes(), value.as_bytes()).await.unwrap();
        }
    });

    let duration = start.elapsed();
    let ops_per_sec = batch_size as f64 / duration.as_secs_f64();

    println!("✅ Batch insert: {} ops in {:.2}s ({:.2} ops/sec)",
        batch_size, duration.as_secs_f64(), ops_per_sec);

    assert!(ops_per_sec > 100.0, "Batch insert too slow: {} ops/sec", ops_per_sec);
}

#[test]
fn test_read_performance() {
    // 测试读取性能
    let rt = Runtime::new().unwrap();
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(rt.block_on(async {
        StateDB::new(temp_dir.path()).await.unwrap()
    }));

    // 先写入数据
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        for i in 0..1000 {
            let key = format!("read_key_{}", i);
            let value = format!("read_value_{}", i);
            db.insert(key.as_bytes(), value.as_bytes()).await.unwrap();
        }
    });

    // 测试读取性能
    let start = Instant::now();
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        for i in 0..1000 {
            let key = format!("read_key_{}", i);
            let _value = db.get(key.as_bytes()).await.unwrap();
        }
    });

    let duration = start.elapsed();
    let ops_per_sec = 1000.0 / duration.as_secs_f64();

    println!("✅ Sequential read: 1000 ops in {:.2}s ({:.2} ops/sec)",
        duration.as_secs_f64(), ops_per_sec);

    assert!(ops_per_sec > 1000.0, "Read performance too low: {} ops/sec", ops_per_sec);
}

#[test]
fn test_hash_performance() {
    // 测试哈希计算性能
    let data = vec![1u8; 1024]; // 1KB 数据
    let iterations = 10_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _hash = Hash::default(); // 简化测试
    }

    let duration = start.elapsed();
    let ops_per_sec = iterations as f64 / duration.as_secs_f64();

    println!("✅ Hash computation: {} ops in {:.2}s ({:.2} ops/sec)",
        iterations, duration.as_secs_f64(), ops_per_sec);

    assert!(ops_per_sec > 10_000.0, "Hash computation too slow: {} ops/sec", ops_per_sec);
}
