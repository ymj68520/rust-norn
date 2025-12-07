use anyhow::{Context, Result};
use async_trait::async_trait;
use norn_common::traits::DBInterface;
use rocksdb::{DB, Options, WriteBatch};
use std::path::Path;
use std::sync::Arc;

pub struct RocksDB {
    db: Arc<DB>,
}

impl RocksDB {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        // Enable compression as per Go project implies (LevelDB usually has Snappy)
        opts.set_compression_type(rocksdb::DBCompressionType::Snappy);

        let db = DB::open(&opts, path).context("Failed to open RocksDB")?;
        Ok(Self {
            db: Arc::new(db),
        })
    }
}

#[async_trait]
impl DBInterface for RocksDB {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // RocksDB handles concurrency internally, but the crate API is synchronous.
        // For high-throughput async context, we might want to wrap this in spawn_blocking,
        // but for simple get/put, direct call is often acceptable if fast (RAM hit).
        // However, disk I/O is blocking.
        // Let's wrap in a blocking task for correctness in async runtime.
        let db = self.db.clone();
        let key = key.to_vec();
        
        tokio::task::spawn_blocking(move || {
            db.get(&key).context("Failed to get from RocksDB")
        }).await?
    }

    async fn insert(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let db = self.db.clone();
        let key = key.to_vec();
        let value = value.to_vec();

        tokio::task::spawn_blocking(move || {
            db.put(&key, &value).context("Failed to insert into RocksDB")
        }).await?
    }

    async fn remove(&self, key: &[u8]) -> Result<()> {
        let db = self.db.clone();
        let key = key.to_vec();

        tokio::task::spawn_blocking(move || {
            db.delete(&key).context("Failed to remove from RocksDB")
        }).await?
    }

    async fn batch_insert(&self, keys: &[Vec<u8>], values: &[Vec<u8>]) -> Result<()> {
        if keys.len() != values.len() {
            anyhow::bail!("Batch insert failed: Key/Value length mismatch");
        }
        
        // Optimize: avoid cloning all data if possible, but spawn_blocking requires 'static.
        // So we must clone.
        let keys = keys.to_vec();
        let values = values.to_vec();
        let db = self.db.clone();

        tokio::task::spawn_blocking(move || {
            let mut batch = WriteBatch::default();
            for (k, v) in keys.iter().zip(values.iter()) {
                batch.put(k, v);
            }
            db.write(batch).context("Failed to execute batch insert")
        }).await?
    }

    async fn batch_delete(&self, keys: &[Vec<u8>]) -> Result<()> {
        let keys = keys.to_vec();
        let db = self.db.clone();

        tokio::task::spawn_blocking(move || {
            let mut batch = WriteBatch::default();
            for k in keys.iter() {
                batch.delete(k);
            }
            db.write(batch).context("Failed to execute batch delete")
        }).await?
    }
}
