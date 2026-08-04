use crate::error::{NornError, Result};
use crate::types::{ChainId, Hash, ProtocolVersion, ValidatorId};
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

/// Handshake messages are exchanged on the context-bound handshake topic.
/// Validator authentication is deliberately a two-step challenge/response;
/// a role claim or an unsigned Hello is never sufficient for consensus input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandshakeMessage {
    Hello,
    Challenge,
    Response,
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
    /// across different peers. Validator responses additionally sign this
    /// identity, so gossipsub forwarding cannot turn a relayed claim into a
    /// different authenticated validator.
    pub peer_id: Vec<u8>,
    /// Changes on every local connection attempt so a reconnected peer is not
    /// hidden by gossipsub's duplicate-message cache.
    #[serde(default)]
    pub session_nonce: u64,
    #[serde(default = "default_handshake_message")]
    pub message: HandshakeMessage,
    /// The peer that issued a challenge. Present on Challenge and Response.
    #[serde(default)]
    pub receiver_peer_id: Vec<u8>,
    /// Fresh receiver-generated nonce, present on Challenge and Response.
    #[serde(default)]
    pub receiver_nonce: Option<[u8; 32]>,
    /// Genesis validator identity and proof, present only on Response.
    #[serde(default)]
    pub validator_id: Option<ValidatorId>,
    #[serde(default)]
    pub consensus_public_key: Option<Vec<u8>>,
    #[serde(default)]
    pub signature: Option<Vec<u8>>,
}

fn default_handshake_message() -> HandshakeMessage {
    HandshakeMessage::Hello
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
            message: HandshakeMessage::Hello,
            receiver_peer_id: Vec::new(),
            receiver_nonce: None,
            validator_id: None,
            consensus_public_key: None,
            signature: None,
        }
    }

    pub fn challenge(
        context: ChainContext,
        challenger_peer_id: impl Into<Vec<u8>>,
        challenged_peer_id: impl Into<Vec<u8>>,
        receiver_nonce: [u8; 32],
    ) -> Self {
        Self {
            message: HandshakeMessage::Challenge,
            peer_id: challenged_peer_id.into(),
            receiver_peer_id: challenger_peer_id.into(),
            receiver_nonce: Some(receiver_nonce),
            ..Self::new(context, PeerRole::FullNode)
        }
    }

    /// Canonical bytes signed by a validator response. The transport source
    /// is included explicitly; gossipsub propagation_source is not trusted as
    /// a consensus identity.
    pub fn validator_signing_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(b"NORN_VALIDATOR_HANDSHAKE_V2");
        bytes.extend_from_slice(&self.wire_version.to_be_bytes());
        bytes.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        bytes.extend_from_slice(&self.chain_id.0 .0);
        bytes.extend_from_slice(&self.genesis_hash.0);
        if let Some(validator_id) = self.validator_id {
            bytes.extend_from_slice(&validator_id.0);
        } else {
            bytes.extend_from_slice(&[0u8; 32]);
        }
        bytes.extend_from_slice(&(self.peer_id.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&self.peer_id);
        bytes.extend_from_slice(&(self.receiver_peer_id.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&self.receiver_peer_id);
        bytes.extend_from_slice(&self.receiver_nonce.unwrap_or_default());
        bytes.push(match self.peer_role {
            PeerRole::Validator => 1,
            PeerRole::FullNode => 2,
        });
        bytes
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
    /// V4 is a new-network activation for immutable builder randomness. There
    /// is no height-based mixed mode: a node either loads the V4 Genesis
    /// identity or fails closed.
    pub const CURRENT_WIRE_VERSION: u16 = 4;
    pub const CURRENT_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(4);

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
