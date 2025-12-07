use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait DBInterface: Send + Sync {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    async fn insert(&self, key: &[u8], value: &[u8]) -> Result<()>;
    async fn remove(&self, key: &[u8]) -> Result<()>;
    async fn batch_insert(&self, keys: &[Vec<u8>], values: &[Vec<u8>]) -> Result<()>;
    async fn batch_delete(&self, keys: &[Vec<u8>]) -> Result<()>;
}
