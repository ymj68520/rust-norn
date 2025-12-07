use crate::types::Hash;

const BLOCK_PREFIX: &[u8] = b"block#";
const TX_PREFIX: &[u8] = b"tx#";
const DATA_PREFIX: &[u8] = b"data#";

pub fn block_hash_to_db_key(hash: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(BLOCK_PREFIX.len() + hash.0.len());
    key.extend_from_slice(BLOCK_PREFIX);
    key.extend_from_slice(&hash.0);
    key
}

pub fn block_height_to_db_key(height: i64) -> Vec<u8> {
    let height_str = height.to_string();
    let mut key = Vec::with_capacity(BLOCK_PREFIX.len() + height_str.len());
    key.extend_from_slice(BLOCK_PREFIX);
    key.extend_from_slice(height_str.as_bytes());
    key
}

pub fn tx_hash_to_db_key(hash: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(TX_PREFIX.len() + hash.0.len());
    key.extend_from_slice(TX_PREFIX);
    key.extend_from_slice(&hash.0);
    key
}

pub fn data_address_key_to_db_key(address: &[u8], key: &[u8]) -> Vec<u8> {
    let addr_hex = hex::encode(address);
    let key_str = String::from_utf8_lossy(key);
    // fmt.Sprintf("data#%s#%s", hex.EncodeToString(address), string(key))
    let formatted = format!("data#{}#{}", addr_hex, key_str);
    formatted.into_bytes()
}
