use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub const HASH_LENGTH: usize = 32;
pub const ADDRESS_LENGTH: usize = 20;
pub const PUBLIC_KEY_LENGTH: usize = 33;
pub const GENESIS_ORDER_LENGTH: usize = 128;

// --- NewTypes ---

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Hash(pub [u8; HASH_LENGTH]);

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", hex::encode(self.0))
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Strip 0x prefix if present
        let s = s.strip_prefix("0x").unwrap_or(&s);
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        if bytes.len() != HASH_LENGTH {
            return Err(serde::de::Error::custom("Invalid hash length"));
        }
        let mut arr = [0u8; HASH_LENGTH];
        arr.copy_from_slice(&bytes);
        Ok(Hash(arr))
    }
}

impl Hash {
    pub fn from_slice(bytes: &[u8]) -> Self {
        let mut arr = [0u8; HASH_LENGTH];
        let len = bytes.len().min(HASH_LENGTH);
        arr[..len].copy_from_slice(&bytes[..len]);
        Hash(arr)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Address(pub [u8; ADDRESS_LENGTH]);

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", hex::encode(self.0))
    }
}

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Strip 0x prefix if present
        let s = s.strip_prefix("0x").unwrap_or(&s);
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        if bytes.len() != ADDRESS_LENGTH {
            return Err(serde::de::Error::custom("Invalid address length"));
        }
        let mut arr = [0u8; ADDRESS_LENGTH];
        arr.copy_from_slice(&bytes);
        Ok(Address(arr))
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]

pub struct PublicKey(pub [u8; PUBLIC_KEY_LENGTH]);

impl Default for PublicKey {
    fn default() -> Self {
        Self([0u8; PUBLIC_KEY_LENGTH])
    }
}

// --- Consensus Strong Types ---

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct ValidatorId(pub [u8; 32]);

impl fmt::Debug for ValidatorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValidatorId({})", hex::encode(self.0))
    }
}

impl fmt::Display for ValidatorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl Serialize for ValidatorId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for ValidatorId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.strip_prefix("0x").unwrap_or(&s);
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("Invalid ValidatorId length"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(ValidatorId(arr))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConsensusPublicKey(pub [u8; 33]);

impl Default for ConsensusPublicKey {
    fn default() -> Self {
        Self([0u8; 33])
    }
}

impl fmt::Debug for ConsensusPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ConsensusPublicKey({})", hex::encode(self.0))
    }
}

impl Serialize for ConsensusPublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for ConsensusPublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 33 {
            return Err(serde::de::Error::custom(
                "Invalid ConsensusPublicKey length",
            ));
        }
        let mut arr = [0u8; 33];
        arr.copy_from_slice(&bytes);
        Ok(ConsensusPublicKey(arr))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VrfPublicKey(pub [u8; 32]);

impl fmt::Debug for VrfPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VrfPublicKey({})", hex::encode(self.0))
    }
}

impl Serialize for VrfPublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for VrfPublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("Invalid VrfPublicKey length"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(VrfPublicKey(arr))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StakeSnapshotHash(pub [u8; 32]);

impl fmt::Display for StakeSnapshotHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl Serialize for StakeSnapshotHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for StakeSnapshotHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.strip_prefix("0x").unwrap_or(&s);
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("Invalid StakeSnapshotHash length"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(StakeSnapshotHash(arr))
    }
}

pub mod hex_serde_fixed_64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.strip_prefix("0x").unwrap_or(&s);
        let decoded = hex::decode(s).map_err(serde::de::Error::custom)?;
        if decoded.len() != 64 {
            return Err(serde::de::Error::custom("Invalid length for [u8; 64]"));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&decoded);
        Ok(arr)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct ProtocolVersion(pub u16);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct ChainId(pub Hash);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct BlockId(pub Hash);

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", hex::encode(self.0))
    }
}

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;

        if bytes.len() != PUBLIC_KEY_LENGTH {
            return Err(serde::de::Error::custom("Invalid public key length"));
        }

        let mut arr = [0u8; PUBLIC_KEY_LENGTH];

        arr.copy_from_slice(&bytes);

        Ok(PublicKey(arr))
    }
}

// --- Domain Structs ---

/// Transaction type enum for distinguishing between native and EVM transactions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy, Default)]
pub enum TransactionType {
    /// Native norn blockchain transaction
    #[default]
    Native,
    /// Ethereum-compatible EVM transaction
    EVM,
}

/// Stable identifier of a signed TransactionV2.
///
/// The identifier is deliberately not a field of the signed preimage. It is
/// derived after the signature is attached, which prevents a self-referential
/// hash while keeping the signature committed by the transaction ID.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct TransactionId(pub Hash);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransactionV2Error {
    #[error("transaction protocol version must be non-zero")]
    InvalidProtocolVersion,
    #[error("transaction chain ID must be non-zero")]
    InvalidChainId,
    #[error("transaction sender must be non-zero")]
    InvalidSender,
    #[error("transaction gas limit must be non-zero")]
    InvalidGasLimit,
    #[error("transaction fee bounds are invalid")]
    InvalidFeeBounds,
    #[error("transaction signature must be a non-zero 64-byte value")]
    InvalidSignature,
    #[error("transaction public key must be non-zero")]
    InvalidPublicKey,
    #[error("transaction ID does not match its canonical bytes")]
    InvalidTransactionId,
    #[error("transaction field is too large for canonical encoding")]
    FieldTooLarge,
}

/// Protocol-v2 transaction object.
///
/// This object is the only transaction shape permitted by the new protocol
/// path. Inclusion metadata (`height`, `index`, and `block_hash`) is not part
/// of it; those values are derived from the containing block when needed for
/// receipts and indexing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionV2 {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub nonce: u64,
    pub sender: Address,
    pub receiver: Option<Address>,
    pub value: u128,
    pub gas_limit: u64,
    pub max_fee_per_gas: u64,
    pub max_priority_fee_per_gas: u64,
    #[serde(with = "hex_serde")]
    pub data: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub event: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub opt: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub state: Vec<u8>,
    pub expire: Option<u64>,
    pub timestamp: u64,
    pub tx_type: TransactionType,
    pub access_list: Vec<AccessListItem>,
    pub public_key: PublicKey,
    #[serde(with = "hex_serde_fixed_64")]
    pub signature: [u8; 64],
    pub transaction_id: TransactionId,
}

impl Default for TransactionV2 {
    fn default() -> Self {
        Self {
            protocol_version: ProtocolVersion::default(),
            chain_id: ChainId::default(),
            nonce: 0,
            sender: Address::default(),
            receiver: None,
            value: 0,
            gas_limit: 0,
            max_fee_per_gas: 0,
            max_priority_fee_per_gas: 0,
            data: Vec::new(),
            event: Vec::new(),
            opt: Vec::new(),
            state: Vec::new(),
            expire: None,
            timestamp: 0,
            tx_type: TransactionType::default(),
            access_list: Vec::new(),
            public_key: PublicKey::default(),
            signature: [0u8; 64],
            transaction_id: TransactionId::default(),
        }
    }
}

impl TransactionV2 {
    pub const DOMAIN: &'static [u8] = b"NORN_TRANSACTION_V2";
    pub const MAX_FIELD_BYTES: usize = 256 * 1024;
    /// Maximum encoded transaction size accepted by the protocol wire.
    ///
    /// This is a protocol ceiling, not a node-local tuning option. Genesis
    /// may select a lower transaction limit, but no node may raise it.
    pub const MAX_WIRE_BYTES: usize = 256 * 1024;

    /// Bytes signed by the sender. The signature and transaction ID are
    /// intentionally excluded from this preimage.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, TransactionV2Error> {
        let mut bytes = Vec::with_capacity(256);
        if self.protocol_version.0 >= 5 {
            bytes.extend_from_slice(b"NORN_TRANSACTION_V5");
        } else if self.protocol_version.0 >= 4 {
            bytes.extend_from_slice(b"NORN_TRANSACTION_V4");
        } else if self.protocol_version.0 >= 3 {
            bytes.extend_from_slice(b"NORN_TRANSACTION_V3");
        } else {
            bytes.extend_from_slice(Self::DOMAIN);
        }
        bytes.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        bytes.extend_from_slice(&self.chain_id.0 .0);
        bytes.extend_from_slice(&self.nonce.to_be_bytes());
        bytes.extend_from_slice(&self.sender.0);
        match self.receiver {
            Some(receiver) => {
                bytes.push(1);
                bytes.extend_from_slice(&receiver.0);
            }
            None => bytes.push(0),
        }
        bytes.extend_from_slice(&self.value.to_be_bytes());
        bytes.extend_from_slice(&self.gas_limit.to_be_bytes());
        bytes.extend_from_slice(&self.max_fee_per_gas.to_be_bytes());
        bytes.extend_from_slice(&self.max_priority_fee_per_gas.to_be_bytes());
        bytes.push(match self.tx_type {
            TransactionType::Native => 0,
            TransactionType::EVM => 1,
        });
        bytes.extend_from_slice(&self.timestamp.to_be_bytes());
        match self.expire {
            Some(expire) => {
                bytes.push(1);
                bytes.extend_from_slice(&expire.to_be_bytes());
            }
            None => bytes.push(0),
        }
        append_v2_bytes(&mut bytes, &self.data)?;
        append_v2_bytes(&mut bytes, &self.event)?;
        append_v2_bytes(&mut bytes, &self.opt)?;
        append_v2_bytes(&mut bytes, &self.state)?;
        let access_list_len =
            u32::try_from(self.access_list.len()).map_err(|_| TransactionV2Error::FieldTooLarge)?;
        bytes.extend_from_slice(&access_list_len.to_be_bytes());
        for item in &self.access_list {
            bytes.extend_from_slice(&item.address.0);
            let key_count = u32::try_from(item.storage_keys.len())
                .map_err(|_| TransactionV2Error::FieldTooLarge)?;
            bytes.extend_from_slice(&key_count.to_be_bytes());
            for key in &item.storage_keys {
                bytes.extend_from_slice(&key.0);
            }
        }
        bytes.extend_from_slice(&self.public_key.0);
        Ok(bytes)
    }

    /// Canonical wire bytes, including the signature and derived ID.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, TransactionV2Error> {
        let mut bytes = self.signing_bytes()?;
        bytes.extend_from_slice(&self.signature);
        bytes.extend_from_slice(&self.transaction_id.0 .0);
        Ok(bytes)
    }

    /// Derive the transaction ID from the signed transaction without any
    /// inclusion metadata from a block.
    pub fn calculate_id(&self) -> Result<TransactionId, TransactionV2Error> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        if self.protocol_version.0 >= 5 {
            hasher.update(b"NORN_TRANSACTION_ID_V5");
        } else if self.protocol_version.0 >= 4 {
            hasher.update(b"NORN_TRANSACTION_ID_V4");
        } else if self.protocol_version.0 >= 3 {
            hasher.update(b"NORN_TRANSACTION_ID_V3");
        } else {
            hasher.update(b"NORN_TRANSACTION_ID_V2");
        }
        hasher.update(self.signing_bytes()?);
        hasher.update(self.signature);
        Ok(TransactionId(Hash(hasher.finalize().into())))
    }

    pub fn id(&self) -> Hash {
        self.transaction_id.0
    }

    /// Decode a transaction received from the context-bound transaction
    /// topic. The context check happens before the object is admitted to any
    /// transaction pool, and malformed/unknown transaction shapes fail
    /// closed instead of falling back to the legacy transaction format.
    pub fn decode_and_validate(
        bytes: &[u8],
        context: &crate::chain_context::ChainContext,
    ) -> crate::error::Result<Self> {
        use crate::error::{NetworkError, NornError, ValidationError};

        if bytes.is_empty() || bytes.len() > Self::MAX_WIRE_BYTES {
            return Err(NornError::Network(NetworkError::Protocol(
                "transaction wire size is outside the protocol limit".to_owned(),
            )));
        }

        let tx = bincode::deserialize::<Self>(bytes).map_err(|e| {
            NornError::Serialization(format!("invalid TransactionV2 encoding: {e}"))
        })?;
        if tx.protocol_version != context.protocol_version {
            return Err(NornError::Network(NetworkError::Protocol(
                "transaction protocol version mismatch".to_owned(),
            )));
        }
        if tx.chain_id != context.chain_id {
            return Err(NornError::Network(NetworkError::Protocol(
                "transaction chain ID mismatch".to_owned(),
            )));
        }
        tx.validate().map_err(|e| {
            NornError::Validation(ValidationError::InvalidTransaction(e.to_string()))
        })?;

        // bincode has a fixed field order, so requiring a byte-for-byte
        // re-encoding prevents alternate encodings from entering the pool.
        let canonical = bincode::serialize(&tx).map_err(|e| {
            NornError::Serialization(format!("failed to re-encode TransactionV2: {e}"))
        })?;
        if canonical != bytes {
            return Err(NornError::Network(NetworkError::Protocol(
                "non-canonical TransactionV2 encoding".to_owned(),
            )));
        }
        Ok(tx)
    }

    pub fn validate(&self) -> Result<(), TransactionV2Error> {
        if self.protocol_version.0 == 0 {
            return Err(TransactionV2Error::InvalidProtocolVersion);
        }
        if self.chain_id.0 == Hash::default() {
            return Err(TransactionV2Error::InvalidChainId);
        }
        if self.sender == Address::default() {
            return Err(TransactionV2Error::InvalidSender);
        }
        if self.gas_limit == 0 {
            return Err(TransactionV2Error::InvalidGasLimit);
        }
        if self.max_fee_per_gas == 0 || self.max_priority_fee_per_gas > self.max_fee_per_gas {
            return Err(TransactionV2Error::InvalidFeeBounds);
        }
        if self.public_key == PublicKey::default() || self.signature == [0u8; 64] {
            return if self.public_key == PublicKey::default() {
                Err(TransactionV2Error::InvalidPublicKey)
            } else {
                Err(TransactionV2Error::InvalidSignature)
            };
        }
        if self.data.len() > Self::MAX_FIELD_BYTES
            || self.event.len() > Self::MAX_FIELD_BYTES
            || self.opt.len() > Self::MAX_FIELD_BYTES
            || self.state.len() > Self::MAX_FIELD_BYTES
        {
            return Err(TransactionV2Error::FieldTooLarge);
        }
        if self.calculate_id()? != self.transaction_id {
            return Err(TransactionV2Error::InvalidTransactionId);
        }
        Ok(())
    }
}

fn append_v2_bytes(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(), TransactionV2Error> {
    if value.len() > TransactionV2::MAX_FIELD_BYTES {
        return Err(TransactionV2Error::FieldTooLarge);
    }
    let len = u32::try_from(value.len()).map_err(|_| TransactionV2Error::FieldTooLarge)?;
    bytes.extend_from_slice(&len.to_be_bytes());
    bytes.extend_from_slice(value);
    Ok(())
}

#[cfg(test)]
mod transaction_v2_tests {
    use super::*;

    fn unsigned_transaction() -> TransactionV2 {
        TransactionV2 {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(Hash([1; 32])),
            nonce: 7,
            sender: Address([2; 20]),
            receiver: Some(Address([3; 20])),
            value: 11,
            gas_limit: 100_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 2,
            data: vec![4, 5],
            event: vec![6],
            opt: vec![7],
            state: vec![8],
            expire: Some(99),
            timestamp: 88,
            tx_type: TransactionType::Native,
            access_list: vec![],
            public_key: PublicKey([9; PUBLIC_KEY_LENGTH]),
            signature: [10; 64],
            transaction_id: TransactionId::default(),
        }
    }

    #[test]
    fn transaction_v2_id_is_self_contained_and_stable() {
        let mut tx = unsigned_transaction();
        tx.transaction_id = tx.calculate_id().unwrap();
        assert_eq!(tx.calculate_id().unwrap(), tx.transaction_id);
        tx.validate().unwrap();

        let json = serde_json::to_value(&tx).unwrap();
        let object = json.as_object().unwrap();
        assert!(!object.contains_key("block_hash"));
        assert!(!object.contains_key("height"));
        assert!(!object.contains_key("index"));
    }

    #[test]
    fn transaction_v2_changes_when_signed_content_changes() {
        let mut first = unsigned_transaction();
        first.transaction_id = first.calculate_id().unwrap();
        let mut second = first.clone();
        second.timestamp += 1;
        assert_ne!(
            first.calculate_id().unwrap(),
            second.calculate_id().unwrap()
        );
    }

    #[test]
    fn transaction_v2_wire_decode_is_context_bound_and_canonical() {
        let mut tx = unsigned_transaction();
        tx.transaction_id = tx.calculate_id().unwrap();
        let bytes = bincode::serialize(&tx).unwrap();
        let context = crate::chain_context::ChainContext::new(
            1,
            tx.protocol_version,
            tx.chain_id,
            Hash([7; HASH_LENGTH]),
        );

        let decoded = TransactionV2::decode_and_validate(&bytes, &context).unwrap();
        assert_eq!(decoded, tx);

        let wrong_context = crate::chain_context::ChainContext::new(
            1,
            ProtocolVersion(3),
            tx.chain_id,
            Hash([7; HASH_LENGTH]),
        );
        assert!(TransactionV2::decode_and_validate(&bytes, &wrong_context).is_err());
        assert!(TransactionV2::decode_and_validate(b"legacy-transaction", &context).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionBody {
    pub hash: Hash,

    pub address: Address,

    pub receiver: Address,

    pub gas: i64,

    pub nonce: i64,

    #[serde(with = "hex_serde")]
    pub event: Vec<u8>,

    #[serde(with = "hex_serde")]
    pub opt: Vec<u8>,

    #[serde(with = "hex_serde")]
    pub state: Vec<u8>,

    #[serde(with = "hex_serde")]
    pub data: Vec<u8>,

    pub expire: i64,

    pub timestamp: i64,

    pub public: PublicKey,

    #[serde(with = "hex_serde")]
    pub signature: Vec<u8>,

    /// EVM-specific: Transaction type (Native or EVM)
    #[serde(default)]
    pub tx_type: TransactionType,

    /// EVM-specific: Chain ID for EIP-155 replay protection
    #[serde(default)]
    pub chain_id: Option<u64>,

    /// EVM-specific: Transaction value in wei (for EVM transfers)
    #[serde(default)]
    pub value: Option<String>, // Use String for BigUint serialization compatibility

    /// EIP-1559: Maximum fee per gas (base fee + priority fee)
    #[serde(default)]
    pub max_fee_per_gas: Option<u64>,

    /// EIP-1559: Maximum priority fee per gas (tip to miner)
    #[serde(default)]
    pub max_priority_fee_per_gas: Option<u64>,

    /// EIP-1559: Access list for EIP-2930 (optional)
    #[serde(default)]
    pub access_list: Option<Vec<AccessListItem>>,

    /// EIP-1559: Gas price for legacy transactions
    #[serde(default)]
    pub gas_price: Option<u64>,
}

/// Access list item for EIP-2930 and EIP-1559
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AccessListItem {
    /// Address to access
    pub address: Address,
    /// Storage keys to access
    pub storage_keys: Vec<Hash>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]

pub struct Transaction {
    pub body: TransactionBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)] // Removed Default

pub struct GenesisParams {
    #[serde(with = "hex_serde_fixed_128")]
    pub order: [u8; 128],

    pub time_param: i64,

    pub seed: Hash, // Reusing Hash for [32]byte fields

    pub verify_param: Hash,
}

impl Default for GenesisParams {
    fn default() -> Self {
        Self {
            order: [0u8; GENESIS_ORDER_LENGTH],

            time_param: 0,

            seed: Hash::default(),

            verify_param: Hash::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeneralParams {
    #[serde(with = "hex_serde")]
    pub result: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub proof: Vec<u8>,
    pub random_number: PublicKey, // [33]byte, same as PublicKey
    #[serde(with = "hex_serde")]
    pub s: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub t: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BlockHeader {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub height: i64,
    pub epoch: u64,
    pub round: u32,
    pub timestamp: i64,
    pub prev_block_hash: Hash,
    pub block_hash: Hash,
    pub merkle_root: Hash,
    pub state_root: Hash,
    /// Validator that originally built this block.  This identity is part of
    /// the block ID and remains stable when the block is re-proposed in a
    /// later Tendermint round by a different proposer.
    pub block_builder: ValidatorId,
    pub stake_snapshot_hash: StakeSnapshotHash,
    pub parent_randomness: Hash,
    pub gas_limit: i64,
    pub base_fee: u64,
    pub consensus_data_hash: Hash,
}

impl BlockHeader {
    pub fn calculate_hash(&self) -> Result<Hash, crate::error::NornError> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        if self.protocol_version.0 >= 5 {
            hasher.update(b"NORN_BLOCK_HEADER_V5");
        } else if self.protocol_version.0 >= 4 {
            hasher.update(b"NORN_BLOCK_HEADER_V4");
        } else if self.protocol_version.0 >= 3 {
            hasher.update(b"NORN_BLOCK_HEADER_V3");
        } else {
            hasher.update(b"NORN_BLOCK_HEADER_V2");
        }
        hasher.update(&self.protocol_version.0.to_be_bytes());
        hasher.update(&self.chain_id.0 .0);
        hasher.update(&self.height.to_be_bytes());
        hasher.update(&self.epoch.to_be_bytes());
        hasher.update(&self.round.to_be_bytes());
        hasher.update(&self.timestamp.to_be_bytes());
        hasher.update(&self.prev_block_hash.0);
        hasher.update(&self.merkle_root.0);
        hasher.update(&self.state_root.0);
        hasher.update(&self.block_builder.0);
        hasher.update(&self.stake_snapshot_hash.0);
        hasher.update(&self.parent_randomness.0);
        hasher.update(&self.gas_limit.to_be_bytes());
        hasher.update(&self.base_fee.to_be_bytes());
        hasher.update(&self.consensus_data_hash.0);
        Ok(Hash(hasher.finalize().into()))
    }
}

/// Immutable consensus material committed by a block builder.
///
/// Proposal VRF material is intentionally absent: a later-round proposer may
/// sign the same block with a different attempt VRF, but that must not change
/// the chain randomness derived from the finalized block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockConsensusData {
    pub builder_vrf_preout: [u8; 32],
    #[serde(with = "hex_serde_fixed_64")]
    pub builder_vrf_proof: [u8; 64],
    pub builder_round: u32,
    /// Deterministic execution commitment retained separately so the header's
    /// consensus_data_hash can commit to both execution and builder VRF data.
    pub execution_data_hash: Hash,
}

impl Default for BlockConsensusData {
    fn default() -> Self {
        Self {
            builder_vrf_preout: [0u8; 32],
            builder_vrf_proof: [0u8; 64],
            builder_round: 0,
            execution_data_hash: Hash::default(),
        }
    }
}

impl BlockConsensusData {
    pub fn calculate_hash(&self, protocol_version: ProtocolVersion) -> Hash {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        if protocol_version.0 >= 5 {
            hasher.update(b"NORN_BLOCK_CONSENSUS_DATA_V5");
        } else if protocol_version.0 >= 4 {
            hasher.update(b"NORN_BLOCK_CONSENSUS_DATA_V4");
        } else {
            hasher.update(b"NORN_BLOCK_CONSENSUS_DATA_V3");
        }
        hasher.update(self.builder_vrf_preout);
        hasher.update(self.builder_vrf_proof);
        hasher.update(self.builder_round.to_be_bytes());
        hasher.update(self.execution_data_hash.0);
        Hash(hasher.finalize().into())
    }
}

/// Derive the only randomness that may become the next height's parent seed.
/// It is bound to immutable block material and deliberately excludes the
/// round-specific Proposal, proposer, and Commit certificate.
pub fn derive_chain_randomness_v4(
    parent_randomness: Hash,
    builder_vrf_randomness: Hash,
    block_id: BlockId,
    chain_id: ChainId,
    protocol_version: ProtocolVersion,
    epoch: u64,
    height: u64,
    stake_snapshot_hash: StakeSnapshotHash,
) -> Hash {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"NORN_CHAIN_RANDOMNESS_V4");
    hasher.update(parent_randomness.0);
    hasher.update(builder_vrf_randomness.0);
    hasher.update(block_id.0 .0);
    hasher.update(chain_id.0 .0);
    hasher.update(protocol_version.0.to_be_bytes());
    hasher.update(epoch.to_be_bytes());
    hasher.update(height.to_be_bytes());
    hasher.update(stake_snapshot_hash.0);
    Hash(hasher.finalize().into())
}

/// Derive the canonical V5 next-height randomness. The block ID is
/// intentionally absent: a builder must not be able to search over legal
/// transaction/timestamp encodings after seeing its one immutable VRF output
/// and select a favorable future proposer seed. This prevents block-content
/// grinding, while the single-builder selective-abort limitation remains a
/// property of the protocol and is not claimed to be unbiased randomness.
#[allow(clippy::too_many_arguments)]
pub fn derive_chain_randomness_v5(
    parent_randomness: Hash,
    builder_vrf_randomness: Hash,
    chain_id: ChainId,
    protocol_version: ProtocolVersion,
    epoch: u64,
    height: u64,
    builder_round: u32,
    block_builder: ValidatorId,
    stake_snapshot_hash: StakeSnapshotHash,
) -> Hash {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"NORN_CHAIN_RANDOMNESS_V5");
    hasher.update(parent_randomness.0);
    hasher.update(builder_vrf_randomness.0);
    hasher.update(chain_id.0 .0);
    hasher.update(protocol_version.0.to_be_bytes());
    hasher.update(epoch.to_be_bytes());
    hasher.update(height.to_be_bytes());
    hasher.update(builder_round.to_be_bytes());
    hasher.update(block_builder.0);
    hasher.update(stake_snapshot_hash.0);
    Hash(hasher.finalize().into())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

/// Protocol-v2 block payload.
///
/// `Block` remains as an explicitly separate legacy adapter.  A V2 block
/// never serializes a legacy `Transaction`, so the transaction/block hash
/// cycle cannot be reintroduced by a caller accidentally constructing the
/// old shape.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BlockV2 {
    pub header: BlockHeader,
    pub transactions: Vec<TransactionV2>,
    #[serde(default)]
    pub consensus_data: BlockConsensusData,
}

impl BlockV2 {
    pub const MERKLE_LEAF_DOMAIN: &'static [u8] = b"NORN_TX_LEAF_V2";
    pub const MERKLE_NODE_DOMAIN: &'static [u8] = b"NORN_MERKLE_NODE_V2";

    /// Build the canonical transaction Merkle root.  Transaction IDs are
    /// already self-contained signed identifiers, so no block inclusion
    /// metadata enters this commitment.
    pub fn calculate_merkle_root(transactions: &[TransactionV2]) -> crate::error::Result<Hash> {
        use sha2::{Digest, Sha256};

        if transactions.is_empty() {
            return Ok(Hash::default());
        }

        let mut level: Vec<Hash> = transactions
            .iter()
            .map(|tx| {
                let mut hasher = Sha256::new();
                hasher.update(Self::MERKLE_LEAF_DOMAIN);
                hasher.update(tx.transaction_id.0 .0);
                Hash(hasher.finalize().into())
            })
            .collect();

        while level.len() > 1 {
            let mut next = Vec::with_capacity((level.len() + 1) / 2);
            for pair in level.chunks(2) {
                let right = pair.get(1).copied().unwrap_or_default();
                let mut hasher = Sha256::new();
                hasher.update(Self::MERKLE_NODE_DOMAIN);
                hasher.update(pair[0].0);
                hasher.update(right.0);
                next.push(Hash(hasher.finalize().into()));
            }
            level = next;
        }
        Ok(level[0])
    }

    /// Recompute the header commitments after the transaction list and state
    /// execution result are finalized.
    pub fn finalize_header(&mut self) -> crate::error::Result<()> {
        self.header.merkle_root = Self::calculate_merkle_root(&self.transactions)?;
        if self.header.protocol_version.0 >= 4 {
            self.header.consensus_data_hash = self
                .consensus_data
                .calculate_hash(self.header.protocol_version);
        }
        self.header.block_hash = self.header.calculate_hash()?;
        Ok(())
    }

    /// Validate structural, context, and Genesis resource invariants before
    /// a V2 block can enter execution or consensus.
    pub fn validate_structure(
        &self,
        context: &crate::chain_context::ChainContext,
        limits: &crate::genesis::ProtocolResourceLimits,
    ) -> crate::error::Result<()> {
        limits.validate()?;
        if self.header.protocol_version != context.protocol_version
            || self.header.chain_id != context.chain_id
        {
            return Err(crate::chain_context::protocol_error(
                "V2 block chain context mismatch",
            ));
        }
        if self.header.height <= 0 {
            return Err(crate::chain_context::protocol_error(
                "V2 block height must be non-zero",
            ));
        }
        if self.header.protocol_version.0 >= 5 && self.header.timestamp < 0 {
            return Err(crate::chain_context::protocol_error(
                "V2 block timestamp must be non-negative",
            ));
        }
        if self.transactions.len() > limits.max_transactions_per_block as usize {
            return Err(crate::chain_context::protocol_error(
                "V2 block transaction count exceeds Genesis limit",
            ));
        }
        if self.header.gas_limit < 0 || self.header.gas_limit as u64 > limits.max_block_gas {
            return Err(crate::chain_context::protocol_error(
                "V2 block gas limit is outside Genesis bounds",
            ));
        }

        if self.header.protocol_version.0 >= 4 {
            if self.consensus_data.builder_vrf_preout == [0u8; 32]
                || self.consensus_data.builder_vrf_proof == [0u8; 64]
            {
                return Err(crate::chain_context::protocol_error(
                    "V4 block is missing immutable builder VRF material",
                ));
            }
            if self.consensus_data.builder_round != self.header.round {
                return Err(crate::chain_context::protocol_error(
                    "V4 builder VRF round does not match block round",
                ));
            }
            if self.header.consensus_data_hash
                != self
                    .consensus_data
                    .calculate_hash(self.header.protocol_version)
            {
                return Err(crate::chain_context::protocol_error(
                    "V4 consensus data commitment mismatch",
                ));
            }
        }

        let mut declared_gas = 0u64;
        for tx in &self.transactions {
            tx.validate().map_err(|e| {
                crate::error::NornError::Validation(
                    crate::error::ValidationError::InvalidTransaction(e.to_string()),
                )
            })?;
            let encoded = bincode::serialize(tx)
                .map_err(|e| crate::error::NornError::Serialization(e.to_string()))?;
            if encoded.len() > limits.max_transaction_bytes as usize {
                return Err(crate::chain_context::protocol_error(
                    "V2 transaction exceeds Genesis byte limit",
                ));
            }
            if tx.gas_limit > limits.max_transaction_gas {
                return Err(crate::chain_context::protocol_error(
                    "V2 transaction exceeds Genesis gas limit",
                ));
            }
            declared_gas = declared_gas
                .checked_add(tx.gas_limit)
                .ok_or_else(|| crate::chain_context::protocol_error("V2 block gas overflow"))?;
        }
        if declared_gas > limits.max_block_gas || declared_gas > self.header.gas_limit as u64 {
            return Err(crate::chain_context::protocol_error(
                "V2 block declared gas exceeds Genesis limit",
            ));
        }

        let encoded = bincode::serialize(self)
            .map_err(|e| crate::error::NornError::Serialization(e.to_string()))?;
        if encoded.len() > limits.max_block_bytes as usize {
            return Err(crate::chain_context::protocol_error(
                "V2 block exceeds Genesis byte limit",
            ));
        }
        if self.header.merkle_root != Self::calculate_merkle_root(&self.transactions)? {
            return Err(crate::chain_context::protocol_error(
                "V2 block Merkle root mismatch",
            ));
        }
        if self.header.block_hash == Hash::default()
            || self.header.block_hash != self.header.calculate_hash()?
        {
            return Err(crate::chain_context::protocol_error(
                "V2 block header hash mismatch",
            ));
        }
        Ok(())
    }

    /// Strictly decode a V2 block and reject alternate/trailing encodings.
    pub fn decode_and_validate(
        bytes: &[u8],
        context: &crate::chain_context::ChainContext,
        limits: &crate::genesis::ProtocolResourceLimits,
    ) -> crate::error::Result<Self> {
        if bytes.is_empty() || bytes.len() > limits.max_block_bytes as usize {
            return Err(crate::chain_context::protocol_error(
                "V2 block wire size is outside Genesis limits",
            ));
        }
        let block = bincode::deserialize::<Self>(bytes)
            .map_err(|e| crate::error::NornError::Serialization(e.to_string()))?;
        block.validate_structure(context, limits)?;
        let canonical = bincode::serialize(&block)
            .map_err(|e| crate::error::NornError::Serialization(e.to_string()))?;
        if canonical != bytes {
            return Err(crate::chain_context::protocol_error(
                "non-canonical V2 block encoding",
            ));
        }
        Ok(block)
    }
}

#[cfg(test)]
mod block_v2_tests {
    use super::*;
    use crate::chain_context::ChainContext;
    use crate::genesis::ProtocolResourceLimits;

    fn transaction() -> TransactionV2 {
        let mut tx = TransactionV2 {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(Hash([1; 32])),
            nonce: 0,
            sender: Address([2; 20]),
            receiver: Some(Address([3; 20])),
            value: 1,
            gas_limit: 21_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 1,
            data: Vec::new(),
            event: Vec::new(),
            opt: Vec::new(),
            state: Vec::new(),
            expire: None,
            timestamp: 1,
            tx_type: TransactionType::Native,
            access_list: Vec::new(),
            public_key: PublicKey([4; PUBLIC_KEY_LENGTH]),
            signature: [5; 64],
            transaction_id: TransactionId::default(),
        };
        tx.transaction_id = tx.calculate_id().unwrap();
        tx
    }

    fn context() -> ChainContext {
        ChainContext::new(2, ProtocolVersion(2), ChainId(Hash([1; 32])), Hash([9; 32]))
    }

    #[test]
    fn v2_block_commits_only_self_contained_transaction_ids() {
        let tx = transaction();
        let mut block = BlockV2 {
            header: BlockHeader {
                protocol_version: ProtocolVersion(2),
                chain_id: ChainId(Hash([1; 32])),
                height: 1,
                epoch: 0,
                round: 0,
                timestamp: 1,
                prev_block_hash: Hash::default(),
                block_hash: Hash::default(),
                merkle_root: Hash::default(),
                state_root: Hash([8; 32]),
                block_builder: ValidatorId([7; 32]),
                stake_snapshot_hash: StakeSnapshotHash([6; 32]),
                parent_randomness: Hash([5; 32]),
                gas_limit: 10_000_000,
                base_fee: 1,
                consensus_data_hash: Hash([4; 32]),
            },
            transactions: vec![tx.clone()],
            consensus_data: BlockConsensusData::default(),
        };
        block.finalize_header().unwrap();
        block
            .validate_structure(&context(), &ProtocolResourceLimits::default())
            .unwrap();

        let encoded = bincode::serialize(&block).unwrap();
        let decoded =
            BlockV2::decode_and_validate(&encoded, &context(), &ProtocolResourceLimits::default())
                .unwrap();
        assert_eq!(decoded, block);

        let mut changed = tx;
        changed.timestamp += 1;
        changed.transaction_id = changed.calculate_id().unwrap();
        assert_ne!(
            BlockV2::calculate_merkle_root(&[changed]).unwrap(),
            block.header.merkle_root
        );
    }

    #[test]
    fn v2_block_rejects_header_and_merkle_mutation() {
        let mut block = BlockV2 {
            header: BlockHeader {
                protocol_version: ProtocolVersion(2),
                chain_id: ChainId(Hash([1; 32])),
                height: 1,
                gas_limit: 10_000_000,
                ..BlockHeader::default()
            },
            transactions: vec![transaction()],
            consensus_data: BlockConsensusData::default(),
        };
        block.finalize_header().unwrap();
        block.header.state_root = Hash([2; 32]);
        assert!(block
            .validate_structure(&context(), &ProtocolResourceLimits::default())
            .is_err());
    }

    #[test]
    fn v5_chain_randomness_is_independent_of_block_content_identity() {
        let parent = Hash([1; 32]);
        let builder_output = Hash([2; 32]);
        let chain_id = ChainId(Hash([3; 32]));
        let protocol = ProtocolVersion(5);
        let snapshot = StakeSnapshotHash([4; 32]);
        let first = derive_chain_randomness_v5(
            parent,
            builder_output,
            chain_id,
            protocol,
            7,
            42,
            3,
            ValidatorId([5; 32]),
            snapshot,
        );
        // The V5 API has no block_id argument. Changing legal block content
        // cannot create another future seed after the immutable VRF output is
        // fixed.
        let second = derive_chain_randomness_v5(
            parent,
            builder_output,
            chain_id,
            protocol,
            7,
            42,
            3,
            ValidatorId([5; 32]),
            snapshot,
        );
        assert_eq!(first, second);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataCommand {
    #[serde(with = "hex_serde")]
    pub opt: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub key: Vec<u8>,
    #[serde(with = "hex_serde")]
    pub value: Vec<u8>,
}

// --- Helper Modules for Serde ---

mod hex_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        hex::decode(s).map_err(serde::de::Error::custom)
    }
}

mod hex_serde_fixed_128 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 128], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 128], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let decoded = hex::decode(s).map_err(serde::de::Error::custom)?;
        if decoded.len() != 128 {
            return Err(serde::de::Error::custom("Invalid length for [u8; 128]"));
        }
        let mut arr = [0u8; 128];
        arr.copy_from_slice(&decoded);
        Ok(arr)
    }
}
