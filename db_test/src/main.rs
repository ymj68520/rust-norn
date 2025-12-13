use norn_storage::SledDB;
use norn_common::traits::DBInterface;
use chrono::Utc;
use std::time::Instant;
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 SledDB Performance and Functionality Test");
    println!("============================================");

    // Test 1: Basic Database Operations
    println!("\n📝 Test 1: Basic Database Operations");
    test_basic_operations().await?;

    // Test 2: Batch Operations
    println!("\n📦 Test 2: Batch Operations");
    test_batch_operations().await?;

    // Test 3: Performance Test
    println!("\n⚡ Test 3: Performance Test");
    test_performance().await?;

    // Test 4: Large Data Test
    println!("\n📊 Test 4: Large Data Test");
    test_large_data().await?;

    println!("\n✅ All tests completed successfully!");
    Ok(())
}

async fn test_basic_operations() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let db = SledDB::new(temp_dir.path())?;

    let start = Instant::now();

    // Test insert
    db.insert(b"user:1", b"John Doe").await?;
    db.insert(b"user:2", b"Jane Smith").await?;
    db.insert(b"user:3", b"Bob Johnson").await?;

    // Test get
    let value = db.get(b"user:1").await?;
    assert_eq!(value, Some(b"John Doe".to_vec()));

    // Test update
    db.insert(b"user:1", b"John Doe Updated").await?;
    let value = db.get(b"user:1").await?;
    assert_eq!(value, Some(b"John Doe Updated".to_vec()));

    // Test delete
    db.remove(b"user:2").await?;
    let value = db.get(b"user:2").await?;
    assert_eq!(value, None);

    // Test contains
    let exists = db.contains_key(b"user:3").await?;
    assert!(exists);

    let elapsed = start.elapsed();
    println!("✅ Basic operations completed in {:?}", elapsed);
    println!("   - Insert: 3 records");
    println!("   - Update: 1 record");
    println!("   - Delete: 1 record");
    println!("   - Contains check: 1 check");

    Ok(())
}

async fn test_batch_operations() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let db = SledDB::new(temp_dir.path())?;

    let start = Instant::now();
    let batch_size = 1000;

    // Prepare batch data
    let mut keys = Vec::new();
    let mut values = Vec::new();

    for i in 0..batch_size {
        keys.push(format!("batch_key:{}", i).into_bytes());
        values.push(format!("batch_value:{}", i).into_bytes());
    }

    // Batch insert
    db.batch_insert(&keys, &values).await?;

    // Verify batch insert
    for i in 0..10 {
        let key = format!("batch_key:{}", i);
        let value = db.get(key.as_bytes()).await?;
        assert_eq!(value, Some(format!("batch_value:{}", i).into_bytes()));
    }

    // Batch delete
    db.batch_delete(&keys).await?;

    // Verify batch delete
    for i in 0..10 {
        let key = format!("batch_key:{}", i);
        let value = db.get(key.as_bytes()).await?;
        assert_eq!(value, None);
    }

    let elapsed = start.elapsed();
    println!("✅ Batch operations completed in {:?}", elapsed);
    println!("   - Batch insert: {} records", batch_size);
    println!("   - Batch delete: {} records", batch_size);
    println!("   - Throughput: {:.0} ops/sec", (batch_size * 2) as f64 / elapsed.as_secs_f64());

    Ok(())
}

async fn test_performance() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let db = SledDB::new(temp_dir.path())?;

    let test_size = 10000;

    // Test sequential writes
    println!("   Testing sequential writes...");
    let start = Instant::now();
    for i in 0..test_size {
        let key = format!("perf_key:{}", i);
        let value = format!("perf_value:{}", i);
        db.insert(key.as_bytes(), value.as_bytes()).await?;
    }
    let write_time = start.elapsed();

    // Test sequential reads
    println!("   Testing sequential reads...");
    let start = Instant::now();
    for i in 0..test_size {
        let key = format!("perf_key:{}", i);
        db.get(key.as_bytes()).await?;
    }
    let read_time = start.elapsed();

    println!("✅ Performance test completed:");
    println!("   - Sequential write: {} records in {:?} ({:.0} writes/sec)",
             test_size, write_time, test_size as f64 / write_time.as_secs_f64());
    println!("   - Sequential read: {} records in {:?} ({:.0} reads/sec)",
             test_size, read_time, test_size as f64 / read_time.as_secs_f64());

    Ok(())
}

async fn test_large_data() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let db = SledDB::new(temp_dir.path())?;

    // Create a large value (10KB)
    let large_value = "x".repeat(10 * 1024);
    let num_large_records = 100;

    println!("   Testing large value storage...");
    let start = Instant::now();

    for i in 0..num_large_records {
        let key = format!("large_key:{}", i);
        db.insert(key.as_bytes(), large_value.as_bytes()).await?;
    }

    let insert_time = start.elapsed();

    // Read back large values
    println!("   Testing large value retrieval...");
    let start = Instant::now();

    for i in 0..10 {
        let key = format!("large_key:{}", i);
        let value = db.get(key.as_bytes()).await?;
        assert!(value.is_some());
        assert_eq!(value.unwrap().len(), large_value.len());
    }

    let read_time = start.elapsed();

    println!("✅ Large data test completed:");
    println!("   - Large value insert: {} records (10KB each) in {:?}",
             num_large_records, insert_time);
    println!("   - Large value read: 10 records in {:?}", read_time);
    println!("   - Total data stored: {:.1} MB",
             (num_large_records * 10 * 1024) as f64 / (1024.0 * 1024.0));

    Ok(())
}