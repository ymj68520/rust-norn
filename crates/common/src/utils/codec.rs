use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Serialize any serde-compatible type to JSON bytes.
/// Note: In production blockchain, consider using a binary format like bincode or Protobuf.
pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| anyhow::anyhow!("Serialization failed: {}", e))
}

/// Deserialize JSON bytes to a type.
pub fn deserialize<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| anyhow::anyhow!("Deserialization failed: {}", e))
}
