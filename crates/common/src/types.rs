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


#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]

pub struct TransactionBody {

    pub hash: Hash,

    pub address: Address,

    pub receiver: Address,

    pub gas: i64,

    pub nonce: i64,

    #[serde(with = "hex_serde") ]

    pub event: Vec<u8>,

    #[serde(with = "hex_serde") ]

    pub opt: Vec<u8>,

    #[serde(with = "hex_serde") ]

    pub state: Vec<u8>,

    #[serde(with = "hex_serde") ]

    pub data: Vec<u8>,

    pub expire: i64,

    pub height: i64,

    pub index: i64,

    pub block_hash: Hash,

    pub timestamp: i64,

    pub public: PublicKey,

    #[serde(with = "hex_serde") ]

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

    #[serde(with = "hex_serde_fixed_128") ]

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
    pub timestamp: i64,
    pub prev_block_hash: Hash,
    pub block_hash: Hash,
    pub merkle_root: Hash,
    /// State root hash after executing transactions in this block
    pub state_root: Hash,
    pub height: i64,
    pub public_key: PublicKey,
    #[serde(with = "hex_serde")]
    pub params: Vec<u8>, // This might need to be parsed as GenesisParams or GeneralParams depending on logic
    pub gas_limit: i64,
    /// EIP-1559: Base fee for this block
    pub base_fee: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
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
