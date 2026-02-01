//! Contract code storage module
//!
//! Manages storage and retrieval of smart contract bytecode.

use crate::evm::{EVMError, EVMResult};
use norn_common::types::{Address, Hash};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use sha2::{Sha256, Digest};

/// Contract code storage
pub struct CodeStorage {
    /// Code database: code_hash -> bytecode
    codes: Arc<RwLock<HashMap<Hash, Vec<u8>>>>,

    /// Address to code hash mapping
    address_to_code: Arc<RwLock<HashMap<Address, Hash>>>,

    /// Code hash to addresses mapping (one code can be deployed to multiple addresses)
    code_to_addresses: Arc<RwLock<HashMap<Hash, Vec<Address>>>>,
}

impl CodeStorage {
    /// Create new code storage
    pub fn new() -> Self {
        Self {
            codes: Arc::new(RwLock::new(HashMap::new())),
            address_to_code: Arc::new(RwLock::new(HashMap::new())),
            code_to_addresses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store contract code
    pub async fn store_code(&self, code_hash: Hash, code: Vec<u8>) -> EVMResult<()> {
        let mut codes = self.codes.write().await;
        codes.insert(code_hash, code);
        debug!("Stored code: hash={:?}, size={} bytes", code_hash, codes.get(&code_hash).map(|c| c.len()).unwrap_or(0));
        Ok(())
    }

    /// Get contract code by hash
    pub async fn get_code(&self, code_hash: &Hash) -> EVMResult<Option<Vec<u8>>> {
        let codes = self.codes.read().await;
        Ok(codes.get(code_hash).cloned())
    }

    /// Bind code to address
    pub async fn bind_code_to_address(&self, address: Address, code_hash: Hash) -> EVMResult<()> {
        {
            let mut addr_to_code = self.address_to_code.write().await;
            addr_to_code.insert(address, code_hash);
        }

        {
            let mut code_to_addrs = self.code_to_addresses.write().await;
            code_to_addrs.entry(code_hash).or_insert_with(Vec::new).push(address);
        }

        info!("Bound code to address: address={:?}, code_hash={:?}", address, code_hash);
        Ok(())
    }

    /// Get code hash for an address
    pub async fn get_code_hash(&self, address: &Address) -> EVMResult<Option<Hash>> {
        let addr_to_code = self.address_to_code.read().await;
        Ok(addr_to_code.get(address).copied())
    }

    /// Get contract code by address
    pub async fn get_code_by_address(&self, address: &Address) -> EVMResult<Option<Vec<u8>>> {
        if let Some(code_hash) = self.get_code_hash(address).await? {
            self.get_code(&code_hash).await
        } else {
            Ok(None)
        }
    }

    /// Check if address is a contract
    pub async fn is_contract(&self, address: &Address) -> bool {
        self.get_code_hash(address).await.unwrap_or(None).is_some()
    }

    /// Calculate contract creation address (CREATE rule)
    ///
    /// Address = keccak256(rlp.encode([sender, nonce]))
    pub fn calculate_create_address(sender: Address, nonce: u64) -> Address {
        use rlp::RlpStream;

        // Encode [address, nonce] using RLP
        let address_bytes = &sender.0;
        let mut stream = RlpStream::new();
        stream.begin_list(2);
        stream.append(&address_bytes.to_vec()); // Convert to Vec<u8> which is Encodable
        stream.append(&nonce);
        let encoded = stream.out();

        let hash = Sha256::digest(&encoded);

        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[12..32]);
        Address(addr)
    }

    /// Calculate contract creation address with salt (CREATE2 rule)
    ///
    /// Address = keccak256(0xff ++ sender ++ salt ++ keccak256(init_code))[12..]
    pub fn calculate_create2_address(
        sender: Address,
        salt: [u8; 32],
        init_code_hash: Hash,
    ) -> Address {
        use sha2::{Sha256, Digest as _};

        let mut hasher = Sha256::new();
        hasher.update([0xff]);
        hasher.update(&sender.0);
        hasher.update(&salt);
        hasher.update(&init_code_hash.0);
        let hash = hasher.finalize();

        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[12..32]);
        Address(addr)
    }

    /// Get all addresses with a specific code
    pub async fn get_addresses_with_code(&self, code_hash: &Hash) -> EVMResult<Vec<Address>> {
        let code_to_addrs = self.code_to_addresses.read().await;
        Ok(code_to_addrs.get(code_hash).cloned().unwrap_or_default())
    }
}

impl Default for CodeStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_code_storage() {
        let storage = CodeStorage::new();
        let code_hash = Hash([42u8; 32]);
        let code = vec![0x60, 0x61, 0x60]; // PUSH1 PUSH1 PUSH1

        // Store code
        storage.store_code(code_hash, code.clone()).await.unwrap();

        // Retrieve code
        let retrieved = storage.get_code(&code_hash).await.unwrap();
        assert_eq!(retrieved, Some(code));
    }

    #[tokio::test]
    async fn test_address_binding() {
        let storage = CodeStorage::new();
        let address = Address([1u8; 20]);
        let code_hash = Hash([42u8; 32]);
        let code = vec![0x60, 0x61];

        // Store and bind
        storage.store_code(code_hash, code.clone()).await.unwrap();
        storage.bind_code_to_address(address, code_hash).await.unwrap();

        // Check binding
        assert!(storage.is_contract(&address).await);
        let retrieved_hash = storage.get_code_hash(&address).await.unwrap();
        assert_eq!(retrieved_hash, Some(code_hash));

        let retrieved_code = storage.get_code_by_address(&address).await.unwrap();
        assert_eq!(retrieved_code, Some(code));
    }

    #[test]
    fn test_calculate_create_address() {
        let sender = Address([1u8; 20]);

        // Same nonce should produce same address
        let addr1 = CodeStorage::calculate_create_address(sender, 0);
        let addr2 = CodeStorage::calculate_create_address(sender, 0);
        assert_eq!(addr1, addr2);

        // Different nonce should produce different address
        let addr3 = CodeStorage::calculate_create_address(sender, 1);
        assert_ne!(addr1, addr3);
    }

    #[test]
    fn test_calculate_create2_address() {
        let sender = Address([1u8; 20]);
        let salt = [2u8; 32];
        let init_code_hash = Hash([3u8; 32]);

        let addr = CodeStorage::calculate_create2_address(sender, salt, init_code_hash);

        // CREATE2 address should be deterministic
        let addr2 = CodeStorage::calculate_create2_address(sender, salt, init_code_hash);
        assert_eq!(addr, addr2);

        // Different salt should produce different address
        let salt2 = [3u8; 32];
        let addr3 = CodeStorage::calculate_create2_address(sender, salt2, init_code_hash);
        assert_ne!(addr, addr3);
    }

    #[tokio::test]
    async fn test_multiple_addresses_same_code() {
        let storage = CodeStorage::new();
        let code_hash = Hash([42u8; 32]);
        let code = vec![0x60, 0x61];
        let addr1 = Address([1u8; 20]);
        let addr2 = Address([2u8; 20]);

        storage.store_code(code_hash, code).await.unwrap();
        storage.bind_code_to_address(addr1, code_hash).await.unwrap();
        storage.bind_code_to_address(addr2, code_hash).await.unwrap();

        let addresses = storage.get_addresses_with_code(&code_hash).await.unwrap();
        assert_eq!(addresses.len(), 2);
        assert!(addresses.contains(&addr1));
        assert!(addresses.contains(&addr2));
    }
}
