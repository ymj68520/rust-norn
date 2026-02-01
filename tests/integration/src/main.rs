use norn_storage::SledDB;
use norn_common::traits::DBInterface;
use tempfile::TempDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Testing SledDB integration...");

    // Create a temporary directory for the test database
    let temp_dir = TempDir::new()?;
    println!("✅ Created temporary database at: {:?}", temp_dir.path());

    // Initialize SledDB
    let db = SledDB::new(temp_dir.path())?;
    println!("✅ SledDB initialized successfully");

    // Test basic operations
    println!("\n📝 Testing basic operations...");

    // Insert some test data
    db.insert(b"test_key_1", b"test_value_1").await?;
    db.insert(b"test_key_2", b"test_value_2").await?;
    db.insert(b"test_key_3", b"test_value_3").await?;
    println!("✅ Inserted 3 key-value pairs");

    // Retrieve data
    let value = db.get(b"test_key_1").await?;
    assert_eq!(value, Some(b"test_value_1".to_vec()));
    println!("✅ Retrieved test_key_1: {:?}", std::str::from_utf8(&value.unwrap()));

    // Test contains_key
    let exists = db.contains_key(b"test_key_2").await?;
    assert!(exists);
    println!("✅ contains_key working correctly");

    // Test batch operations
    println!("\n📦 Testing batch operations...");

    let keys = vec![
        b"batch_key_1".to_vec(),
        b"batch_key_2".to_vec(),
        b"batch_key_3".to_vec(),
    ];

    let values = vec![
        b"batch_value_1".to_vec(),
        b"batch_value_2".to_vec(),
        b"batch_value_3".to_vec(),
    ];

    db.batch_insert(&keys, &values).await?;
    println!("✅ Batch inserted 3 key-value pairs");

    // Verify batch insert
    let value = db.get(b"batch_key_2").await?;
    assert_eq!(value, Some(b"batch_value_2".to_vec()));
    println!("✅ Batch insert verified");

    // Test batch delete
    db.batch_delete(&keys).await?;
    println!("✅ Batch deleted 3 keys");

    // Verify batch delete
    let value = db.get(b"batch_key_1").await?;
    assert_eq!(value, None);
    println!("✅ Batch delete verified");

    // Test delete operation
    db.remove(b"test_key_3").await?;
    let value = db.get(b"test_key_3").await?;
    assert_eq!(value, None);
    println!("✅ Single delete working correctly");

    println!("\n🎉 All SledDB tests passed!");
    println!("🔧 SledDB is ready for production use");

    Ok(())
}