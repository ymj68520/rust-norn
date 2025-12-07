use anyhow::{Context, Result};

/// Converts a byte slice to a hex string.
pub fn to_hex<T: AsRef<[u8]>>(data: T) -> String {
    hex::encode(data)
}

/// Converts a hex string to a byte vector.
pub fn from_hex(data: &str) -> Result<Vec<u8>> {
    hex::decode(data).context("Failed to decode hex string")
}
