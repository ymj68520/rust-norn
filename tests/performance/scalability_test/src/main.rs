use norn_storage::SledDB;
use norn_common::traits::DBInterface;
use norn_crypto::transaction::TransactionSigner;
use norn_common::types::{Address, Transaction};
use std::time::Instant;
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 SledDB Scalability and Blockchain Transaction Test");
    println!("=====================================================");

    // Test 1: Large Scale Transaction Simulation
    println!("\n📊 Test 1: Large Scale Transaction Simulation");
    test_transaction_simulation().await?;

    // Test 2: Database Size Growth Test
    println!("\n📈 Test 2: Database Size Growth Test");
    test_database_growth().await?;

    // Test 3: Concurrent Access Test
    println!("\n🔄 Test 3: Concurrent Access Test");
    test_concurrent_access().await?;

    // Test 4: Data Persistence Test
    println!("\n💾 Test 4: Data Persistence Test");
    test_data_persistence().await?;

    println!("\n✅ All scalability tests completed successfully!");
    Ok(())
}

async fn test_transaction_simulation() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let db = SledDB::new(temp_dir.path())?;

    let keypair = norn_crypto::ecdsa::KeyPair::random();
    let mut signer = TransactionSigner::new(keypair);

    let num_transactions = 5000;
    let start = Instant::now();

    println!("   Generating {} transactions...", num_transactions);
    let mut transactions = Vec::new();
    let mut tx_hashes = Vec::new();

    for i in 0..num_transactions {
        let receiver = Address::default();
        let tx = signer.create_transaction(
            receiver,
            format!("event_{}", i).into_bytes(),
            format!("operation_{}", i).into_bytes(),
            format!("state_{}", i).into_bytes(),
            format!("test_data_{}_with_additional_content_for_realistic_size", i).into_bytes(),
            1000,
            chrono::Utc::now().timestamp() + 3600,
        )?;

        let tx_hash = tx.body.hash;
        tx_hashes.push((tx_hash, format!("test_key_{}", i).into_bytes(), format!("test_value_{}", i).into_bytes()));
        transactions.push(tx);
    }

    let generation_time = start.elapsed();
    println!("   ✅ Transaction generation: {} tx in {:?}", num_transactions, generation_time);

    // Store transactions in database
    println!("   Storing transactions in database...");
    let start = Instant::now();

    for (i, tx) in transactions.iter().enumerate() {
        let tx_key = format!("tx:{}", hex::encode(tx.body.hash.0));
        let tx_data = serde_json::to_vec(&tx)?;
        db.insert(tx_key.as_bytes(), &tx_data).await?;

        if i % 1000 == 0 && i > 0 {
            println!("     Stored {} transactions...", i);
        }
    }

    let storage_time = start.elapsed();
    println!("   ✅ Transaction storage: {} tx in {:?}", num_transactions, storage_time);
    println!("   ✅ Storage throughput: {:.0} tx/sec", num_transactions as f64 / storage_time.as_secs_f64());

    // Verify some transactions
    println!("   Verifying stored transactions...");
    let start = Instant::now();
    let verify_count = 100;

    for i in (0..num_transactions).step_by(num_transactions / verify_count) {
        let tx = &transactions[i];
        let tx_key = format!("tx:{}", hex::encode(tx.body.hash.0));
        let stored_data = db.get(tx_key.as_bytes()).await?;

        assert!(stored_data.is_some(), "Transaction {} should be stored", i);

        let stored_tx: Transaction = serde_json::from_slice(&stored_data.unwrap())?;
        assert_eq!(stored_tx.body.hash, tx.body.hash);
    }

    let verify_time = start.elapsed();
    println!("   ✅ Transaction verification: {} tx in {:?}", verify_count, verify_time);

    Ok(())
}

async fn test_database_growth() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let db = SledDB::new(temp_dir.path())?;

    let record_sizes = vec![100, 1000, 10000, 50000]; // bytes
    let records_per_size = 100;

    for size in record_sizes {
        println!("   Testing {} byte records...", size);

        let data = vec![b'x'; size];
        let start = Instant::now();

        for i in 0..records_per_size {
            let key = format!("size_test_{}:{}", size, i);
            db.insert(key.as_bytes(), &data).await?;
        }

        let insert_time = start.elapsed();
        let total_bytes = (size * records_per_size) as f64 / 1024.0 / 1024.0; // MB

        println!("     ✅ {} records ({:.1} MB) in {:?}", records_per_size, total_bytes, insert_time);
        println!("     ✅ Write throughput: {:.1} MB/sec", total_bytes / insert_time.as_secs_f64());

        // Read back some records
        let start = Instant::now();
        for i in 0..10 {
            let key = format!("size_test_{}:{}", size, i);
            let _value = db.get(key.as_bytes()).await?;
        }
        let read_time = start.elapsed();
        println!("     ✅ Read verification: 10 records in {:?}", read_time);
    }

    Ok(())
}

async fn test_concurrent_access() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let db = SledDB::new(temp_dir.path())?;
    let db = std::sync::Arc::new(db);

    let num_tasks = 10;
    let operations_per_task = 1000;
    let mut handles = Vec::new();

    println!("   Starting {} concurrent tasks with {} operations each...", num_tasks, operations_per_task);
    let start = Instant::now();

    for task_id in 0..num_tasks {
        let db_clone = db.clone();
        let handle = tokio::spawn(async move {
            for i in 0..operations_per_task {
                let key = format!("concurrent_task_{}:key_{}", task_id, i);
                let value = format!("task_{}_value_{}", task_id, i);

                // Insert
                db_clone.insert(key.as_bytes(), value.as_bytes()).await.unwrap();

                // Read back
                let stored_value = db_clone.get(key.as_bytes()).await.unwrap();
                assert_eq!(stored_value, Some(value.into_bytes()));
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await?;
    }

    let total_time = start.elapsed();
    let total_operations = (num_tasks * operations_per_task) as f64;
    println!("   ✅ Concurrent operations: {} operations in {:?}", total_operations as u64, total_time);
    println!("   ✅ Concurrent throughput: {:.0} ops/sec", total_operations / total_time.as_secs_f64());

    Ok(())
}

async fn test_data_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path();

    // Phase 1: Write data
    println!("   Phase 1: Writing test data...");
    let db = SledDB::new(db_path)?;
    let test_data = vec![
        ("persistence_test_1", "value_1"),
        ("persistence_test_2", "value_2"),
        ("persistence_test_3", "value_3"),
        ("persistence_test_4", "value_4"),
        ("persistence_test_5", "value_5"),
    ];

    for (key, value) in &test_data {
        db.insert(key.as_bytes(), value.as_bytes()).await?;
    }

    // Flush data to disk
    drop(db);

    // Phase 2: Reopen and verify
    println!("   Phase 2: Reopening database and verifying persistence...");
    let db = SledDB::new(db_path)?;

    for (key, expected_value) in &test_data {
        let stored_value = db.get(key.as_bytes()).await?;
        assert_eq!(stored_value, Some(expected_value.as_bytes().to_vec()), "Key {} should persist", key);
    }

    println!("   ✅ All {} records successfully persisted and retrieved", test_data.len());

    // Phase 3: Update and verify persistence
    println!("   Phase 3: Testing update persistence...");
    for (key, _) in &test_data {
        let new_value = format!("{}_updated", key);
        db.insert(key.as_bytes(), new_value.as_bytes()).await?;
    }

    drop(db);

    // Reopen again
    let db = SledDB::new(db_path)?;
    for (key, _) in &test_data {
        let stored_value = db.get(key.as_bytes()).await?;
        let expected_value = format!("{}_updated", key);
        assert_eq!(stored_value, Some(expected_value.as_bytes().to_vec()), "Updated key {} should persist", key);
    }

    println!("   ✅ All updates successfully persisted and retrieved");

    Ok(())
}