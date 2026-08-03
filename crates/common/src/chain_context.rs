use crate::error::{NornError, Result};
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

/// Role advertised during the network identity handshake. This is a wire
/// role, intentionally independent from the node crate's configuration type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerRole {
    Validator,
    FullNode,
}

/// The first message exchanged on a context-bound handshake topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkHandshake {
    pub wire_version: u16,
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub genesis_hash: Hash,
    pub peer_role: PeerRole,
    /// Canonical libp2p PeerId bytes. Binding this into the payload prevents
    /// identical role/context handshakes from being gossipsub-deduplicated
    /// across different peers and lets receivers detect identity claims that
    /// do not match the authenticated transport source.
    pub peer_id: Vec<u8>,
    /// Changes on every local connection attempt so a reconnected peer is not
    /// hidden by gossipsub's duplicate-message cache.
    #[serde(default)]
    pub session_nonce: u64,
}

pub const MAX_HANDSHAKE_BYTES: usize = 1024;
/// Protocol hard ceilings used before payload decoding. Genesis-level limits
/// may be stricter later, but node-local configuration may never increase
/// these bounds.
pub const MAX_BLOCK_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_TRANSACTION_MESSAGE_BYTES: usize = 256 * 1024;

impl NetworkHandshake {
    pub fn new(context: ChainContext, peer_role: PeerRole) -> Self {
        Self {
            wire_version: context.wire_version,
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            genesis_hash: context.genesis_hash,
            peer_role,
            peer_id: Vec::new(),
            session_nonce: 0,
        }
    }

    pub fn with_peer_id(mut self, peer_id: impl Into<Vec<u8>>) -> Self {
        self.peer_id = peer_id.into();
        self
    }

    pub fn with_session_nonce(mut self, session_nonce: u64) -> Self {
        self.session_nonce = session_nonce;
        self
    }

    pub fn validate_for_context(&self, context: &ChainContext) -> Result<()> {
        if self.wire_version != context.wire_version {
            return Err(protocol_error("handshake wire version mismatch"));
        }
        if self.protocol_version != context.protocol_version {
            return Err(protocol_error("handshake protocol version mismatch"));
        }
        if self.chain_id != context.chain_id {
            return Err(protocol_error("handshake chain ID mismatch"));
        }
        if self.genesis_hash != context.genesis_hash {
            return Err(protocol_error("handshake Genesis hash mismatch"));
        }
        Ok(())
    }
}

pub(crate) fn protocol_error(message: impl Into<String>) -> NornError {
    NornError::Network(crate::error::NetworkError::Protocol(message.into()))
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
