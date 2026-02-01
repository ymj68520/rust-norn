use anyhow::{Context, Result};
use async_trait::async_trait;
use norn_common::traits::DBInterface;
use sled::Tree;
use std::path::Path;
use std::sync::Arc;

pub struct SledDB {
    db: Arc<Tree>,
}

impl SledDB {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path).context("Failed to open Sled database")?;

        // Use the default tree for now, could support multiple trees later
        let tree = db.open_tree("default").context("Failed to open default tree")?;

        Ok(Self {
            db: Arc::new(tree),
        })
    }

    /// Create a new SledDB instance from an existing sled::Db
    pub fn from_db(db: sled::Db) -> Result<Self> {
        let tree = db.open_tree("default").context("Failed to open default tree")?;
        Ok(Self {
            db: Arc::new(tree),
        })
    }
}

#[async_trait]
impl DBInterface for SledDB {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let db = self.db.clone();
        let key = key.to_vec();

        // Sled operations are generally fast, but we'll use spawn_blocking for consistency
        tokio::task::spawn_blocking(move || {
            match db.get(&key) {
                Ok(Some(value)) => Ok(Some(value.to_vec())),
                Ok(None) => Ok(None),
                Err(e) => Err(anyhow::anyhow!("Failed to get from SledDB: {}", e)),
            }
        }).await?
    }

    async fn insert(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let db = self.db.clone();
        let key = key.to_vec();
        let value = value.to_vec();

        tokio::task::spawn_blocking(move || {
            db.insert(key.as_slice(), value.as_slice())
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("Failed to insert into SledDB: {}", e))
        }).await?
    }

    async fn remove(&self, key: &[u8]) -> Result<()> {
        let db = self.db.clone();
        let key = key.to_vec();

        tokio::task::spawn_blocking(move || {
            db.remove(key.as_slice())
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("Failed to remove from SledDB: {}", e))
        }).await?
    }

    async fn batch_insert(&self, keys: &[Vec<u8>], values: &[Vec<u8>]) -> Result<()> {
        if keys.len() != values.len() {
            anyhow::bail!("Batch insert failed: Key/Value length mismatch");
        }

        let db = self.db.clone();
        let keys = keys.to_vec();
        let values = values.to_vec();

        tokio::task::spawn_blocking(move || {
            // Simple batch insert without transaction for simplicity
            for (key, value) in keys.iter().zip(values.iter()) {
                db.insert(key.as_slice(), value.as_slice())
                    .map_err(|e| anyhow::anyhow!("Failed to insert into SledDB: {}", e))?;
            }
            Ok(())
        }).await?
    }

    async fn batch_delete(&self, keys: &[Vec<u8>]) -> Result<()> {
        let db = self.db.clone();
        let keys = keys.to_vec();

        tokio::task::spawn_blocking(move || {
            // Simple batch delete without transaction for simplicity
            for key in keys.iter() {
                db.remove(key.as_slice())
                    .map_err(|e| anyhow::anyhow!("Failed to remove from SledDB: {}", e))?;
            }
            Ok(())
        }).await?
    }
}

// Additional utility methods specific to Sled
impl SledDB {
    /// Get the underlying sled::Db for advanced operations
    pub fn underlying_db(&self) -> &sled::Tree {
        &self.db
    }

    /// Check if a key exists
    pub async fn contains_key(&self, key: &[u8]) -> Result<bool> {
        let db = self.db.clone();
        let key = key.to_vec();

        tokio::task::spawn_blocking(move || {
            db.contains_key(&key)
                .map_err(|e| anyhow::anyhow!("Failed to check key existence in SledDB: {}", e))
        }).await?
    }

    /// Synchronous insert (for compatibility with persistent state module)
    pub fn insert_sync(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.db.insert(key, value)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into SledDB: {}", e))
    }

    /// Synchronous get (for compatibility with persistent state module)
    pub fn get_sync(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get(key)
            .map(|v| v.map(|ivec| ivec.to_vec()))
            .map_err(|e| anyhow::anyhow!("Failed to get from SledDB: {}", e))
    }

    /// Synchronous remove (for compatibility with persistent state module)
    pub fn remove_sync(&self, key: &[u8]) -> Result<()> {
        self.db.remove(key)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to remove from SledDB: {}", e))
    }

    /// Iterate over keys with a prefix
    pub fn iter_prefix(&self, prefix: &[u8]) -> impl Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> {
        self.db.scan_prefix(prefix)
            .map(|res| {
                res.map(|(k, v)| (k.to_vec(), v.to_vec()))
                    .map_err(|e| anyhow::anyhow!("DB iteration error: {}", e))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio;

    #[tokio::test]
    async fn test_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let db = SledDB::new(temp_dir.path()).unwrap();

        // Test insert and get
        db.insert(b"key1", b"value1").await.unwrap();
        let value = db.get(b"key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Test get non-existent key
        let value = db.get(b"non_existent").await.unwrap();
        assert_eq!(value, None);

        // Test remove
        db.remove(b"key1").await.unwrap();
        let value = db.get(b"key1").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_batch_operations() {
        let temp_dir = TempDir::new().unwrap();
        let db = SledDB::new(temp_dir.path()).unwrap();

        let keys = vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()];
        let values = vec![b"value1".to_vec(), b"value2".to_vec(), b"value3".to_vec()];

        // Test batch insert
        db.batch_insert(&keys, &values).await.unwrap();

        // Verify all values were inserted
        for (i, key) in keys.iter().enumerate() {
            let value = db.get(key).await.unwrap();
            assert_eq!(value, Some(values[i].clone()));
        }

        // Test batch delete
        db.batch_delete(&keys).await.unwrap();

        // Verify all values were deleted
        for key in keys.iter() {
            let value = db.get(key).await.unwrap();
            assert_eq!(value, None);
        }
    }

    #[tokio::test]
    async fn test_contains_key() {
        let temp_dir = TempDir::new().unwrap();
        let db = SledDB::new(temp_dir.path()).unwrap();

        assert!(!db.contains_key(b"test_key").await.unwrap());

        db.insert(b"test_key", b"test_value").await.unwrap();
        assert!(db.contains_key(b"test_key").await.unwrap());
    }
}