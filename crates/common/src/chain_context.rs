use crate::types::{ChainId, Hash, ProtocolVersion};
use serde::{Deserialize, Serialize};

/// Network identity shared by every consensus and synchronization component.
///
/// This is deliberately derived from the canonical Genesis document rather
/// than from locally generated keys or process configuration.  Later wire
/// protocol work will use this value to validate envelopes and topics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainContext {
    pub wire_version: u16,
    pub genesis_schema_version: u16,
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub genesis_hash: Hash,
}

impl ChainContext {
    pub const CURRENT_WIRE_VERSION: u16 = 1;

    pub fn new(
        genesis_schema_version: u16,
        protocol_version: ProtocolVersion,
        chain_id: ChainId,
        genesis_hash: Hash,
    ) -> Self {
        Self {
            wire_version: Self::CURRENT_WIRE_VERSION,
            genesis_schema_version,
            protocol_version,
            chain_id,
            genesis_hash,
        }
    }
}
