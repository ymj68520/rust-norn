use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait DBInterface: Send + Sync {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    /// Read multiple keys in one backend operation where possible.  The
    /// default keeps simple test databases source-compatible; production
    /// backends should override this to avoid one async scheduling round-trip
    /// per key during large finality commits.
    async fn batch_get(&self, keys: &[Vec<u8>]) -> Result<Vec<Option<Vec<u8>>>> {
        let mut values = Vec::with_capacity(keys.len());
        for key in keys {
            values.push(self.get(key).await?);
        }
        Ok(values)
    }
    async fn insert(&self, key: &[u8], value: &[u8]) -> Result<()>;
    async fn remove(&self, key: &[u8]) -> Result<()>;
    async fn batch_insert(&self, keys: &[Vec<u8>], values: &[Vec<u8>]) -> Result<()>;
    async fn batch_delete(&self, keys: &[Vec<u8>]) -> Result<()>;

    /// Apply inserts and deletes as one database transaction when the backend
    /// supports mixed batches. The default preserves compatibility for simple
    /// test/backing stores; production single-tree implementations should
    /// override it with a true atomic batch.
    async fn batch_write(
        &self,
        insert_keys: &[Vec<u8>],
        insert_values: &[Vec<u8>],
        delete_keys: &[Vec<u8>],
    ) -> Result<()> {
        self.batch_insert(insert_keys, insert_values).await?;
        self.batch_delete(delete_keys).await
    }
}
