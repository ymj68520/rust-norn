pub mod sync;
pub mod compression;

// Re-exports
pub use compression::{CompressedMessage};

// Placeholders for message structs from Go's Karmem definitions if needed
// Currently we use raw Vec<u8> in NetworkEvent, but specific structs can go here.

#[derive(Debug, Clone)]
pub struct SyncStatusMsg {
    // Define fields based on p2p_message.km
    pub current_height: i64,
    pub last_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TimeSyncMsg {
    pub request_id: i64,
    pub timestamp: i64,
}
