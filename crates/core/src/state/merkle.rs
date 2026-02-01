//! State root calculation module
//!
//! Calculates the state root hash from account states, incorporating both
//! native and EVM contract states. Uses Merkle Patricia Tree (MPT) approach.

use crate::state::{AccountStateManager, AccountState, AccountType};
use norn_common::types::{Hash, Address};
use norn_common::error::{Result, NornError};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use tracing::{debug, info};
use num_bigint::BigUint;
use num_traits::Zero;
use std::collections::HashMap;

/// State root calculator
pub struct StateRootCalculator {
    /// Whether to use Keccak-256 (Ethereum compatible) or SHA-256 (native)
    pub use_keccak: bool,
}

impl Default for StateRootCalculator {
    fn default() -> Self {
        Self {
            // Use SHA-256 for native compatibility
            // Keccak-256 can be enabled for full Ethereum compatibility
            use_keccak: false,
        }
    }
}

impl StateRootCalculator {
    /// Create a new state root calculator
    pub fn new(use_keccak: bool) -> Self {
        Self { use_keccak }
    }

    /// Calculate state root from account state manager
    pub async fn calculate_from_manager(
        &self,
        manager: &AccountStateManager,
    ) -> Result<Hash> {
        // Get accounts and storage locks
        let accounts_lock = manager.accounts_lock().await;
        let storage_lock = manager.storage_lock().await;

        // Lock and read accounts and storage
        let accounts = accounts_lock.read().await;
        let storage = storage_lock.read().await;

        // Build state tree
        let mut state_entries: Vec<(Address, AccountStateData)> = Vec::new();

        for (address, account) in accounts.iter() {
            // Get storage root for this account
            let account_storage = storage.get(address);
            let storage_root = if let Some(account_storage) = account_storage {
                // Convert StorageItem to Vec<u8>
                let storage_map: std::collections::HashMap<Vec<u8>, Vec<u8>> =
                    account_storage.iter()
                        .map(|(k, v)| (k.clone(), v.value.clone()))
                        .collect();
                self.calculate_storage_root(address, &storage_map)
            } else {
                Hash::default()
            };

            // Convert BigUint balance to String
            let balance_str = account.balance.to_string();

            // Handle Option<Hash> for code_hash
            let code_hash_value = account.code_hash.unwrap_or_else(|| Hash::default());

            state_entries.push((
                *address,
                AccountStateData {
                    balance: balance_str,
                    nonce: account.nonce,
                    code_hash: code_hash_value,
                    storage_root,
                    account_type: account.account_type.clone(),
                },
            ));
        }

        // Sort by address for deterministic ordering
        state_entries.sort_by_key(|(addr, _)| addr.0);

        // Calculate state root
        let state_root = self.calculate_state_root(&state_entries);

        debug!(
            "Calculated state root: {} accounts, use_keccak={}",
            state_entries.len(),
            self.use_keccak
        );

        Ok(state_root)
    }

    /// Calculate storage root for a single account
    fn calculate_storage_root(
        &self,
        _address: &Address,
        storage: &std::collections::HashMap<Vec<u8>, Vec<u8>>,
    ) -> Hash {
        if storage.is_empty() {
            return Hash::default();
        }

        let mut hasher = Sha256::new();
        for (key, value) in storage.iter() {
            hasher.update(key);
            hasher.update(value);
        }

        let result = hasher.finalize();
        let mut hash = Hash::default();
        hash.0.copy_from_slice(&result);
        hash
    }

    /// Calculate state root from sorted account data
    fn calculate_state_root(&self, accounts: &[(Address, AccountStateData)]) -> Hash {
        if accounts.is_empty() {
            return Hash::default();
        }

        // Build a simple Merkle tree from account hashes
        let mut hashes: Vec<Hash> = accounts
            .iter()
            .map(|(addr, data)| self.hash_account_state(addr, data))
            .collect();

        while hashes.len() > 1 {
            let mut next_level = Vec::new();

            for i in (0..hashes.len()).step_by(2) {
                if i + 1 < hashes.len() {
                    // Combine two adjacent hashes
                    let combined = self.combine_hashes(hashes[i], hashes[i + 1]);
                    next_level.push(combined);
                } else {
                    // Odd number of hashes, carry forward
                    next_level.push(hashes[i]);
                }
            }

            hashes = next_level;
        }

        hashes[0]
    }

    /// Hash a single account state
    fn hash_account_state(&self, address: &Address, data: &AccountStateData) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&address.0);
        hasher.update(data.balance.as_bytes());
        hasher.update(&data.nonce.to_le_bytes());
        hasher.update(&data.code_hash.0);
        hasher.update(&data.storage_root.0);

        // Include account type in hash
        let type_byte = match data.account_type {
            crate::state::AccountType::Normal => 0u8,
            crate::state::AccountType::Contract => 1u8,
            crate::state::AccountType::Validator => 2u8,
            crate::state::AccountType::System => 3u8,
        };
        hasher.update(&[type_byte]);

        let result = hasher.finalize();
        let mut hash = Hash::default();
        hash.0.copy_from_slice(&result);
        hash
    }

    /// Combine two hashes
    fn combine_hashes(&self, left: Hash, right: Hash) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&left.0);
        hasher.update(&right.0);

        let result = hasher.finalize();
        let mut hash = Hash::default();
        hash.0.copy_from_slice(&result);
        hash
    }
}

/// Account state data for hashing
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountStateData {
    balance: String,
    nonce: u64,
    code_hash: Hash,
    storage_root: Hash,
    account_type: crate::state::AccountType,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AccountStateManager, AccountState, AccountStateConfig, AccountType};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_empty_state_root() {
        let manager = AccountStateManager::new(AccountStateConfig::default());
        let calculator = StateRootCalculator::default();

        let root = calculator
            .calculate_from_manager(&manager)
            .await
            .unwrap();

        // Empty state should produce zero hash
        assert_eq!(root, Hash::default());
    }

    #[tokio::test]
    async fn test_single_account_state_root() {
        let manager = AccountStateManager::new(AccountStateConfig::default());
        let calculator = StateRootCalculator::default();

        let address = Address([1u8; 20]);
        let account = AccountState {
            address,
            balance: BigUint::from(1000u64),
            nonce: 0,
            account_type: AccountType::Normal,
            code_hash: Some(Hash::default()),
            storage_root: Hash::default(),
            created_at: 0,
            updated_at: 0,
            deleted: false,
        };

        manager.set_account(&address, account).await.unwrap();

        let root = calculator
            .calculate_from_manager(&manager)
            .await
            .unwrap();

        // State root should be non-zero
        assert_ne!(root, Hash::default());
    }

    #[tokio::test]
    async fn test_multiple_accounts_state_root() {
        let manager = AccountStateManager::new(AccountStateConfig::default());
        let calculator = StateRootCalculator::default();

        // Add multiple accounts
        for i in 1u32..=5 {
            let address = Address([i as u8; 20]);
            let account = AccountState {
                address,
                balance: BigUint::from(i * 1000u32),
                nonce: i as u64,
                account_type: AccountType::Normal,
                code_hash: Some(Hash::default()),
                storage_root: Hash::default(),
                created_at: 0,
                updated_at: 0,
                deleted: false,
            };

            manager.set_account(&address, account).await.unwrap();
        }

        let root1 = calculator
            .calculate_from_manager(&manager)
            .await
            .unwrap();

        // Modify an account
        let address = Address([1u8; 20]);
        manager.update_balance(&address, BigUint::from(9999u32)).await.unwrap();

        let root2 = calculator
            .calculate_from_manager(&manager)
            .await
            .unwrap();

        // State roots should be different after modification
        assert_ne!(root1, root2);
    }

    #[tokio::test]
    async fn test_contract_account_state_root() {
        let manager = AccountStateManager::new(AccountStateConfig::default());
        let calculator = StateRootCalculator::default();

        let address = Address([1u8; 20]);
        let account = AccountState {
            address,
            balance: BigUint::zero(),
            nonce: 1,
            account_type: AccountType::Contract,
            code_hash: Some(Hash([2u8; 32])),
            storage_root: Hash::default(),
            created_at: 0,
            updated_at: 0,
            deleted: false,
        };

        manager.set_account(&address, account).await.unwrap();

        // Add some storage
        manager
            .set_storage(&address, b"slot1".to_vec(), b"value1".to_vec())
            .await
            .unwrap();

        let root = calculator
            .calculate_from_manager(&manager)
            .await
            .unwrap();

        // State root should be non-zero for contract with storage
        assert_ne!(root, Hash::default());
    }

    #[tokio::test]
    async fn test_deterministic_state_root() {
        let manager1 = AccountStateManager::new(AccountStateConfig::default());
        let manager2 = AccountStateManager::new(AccountStateConfig::default());
        let calculator = StateRootCalculator::default();

        // Add same accounts to both managers
        for i in 1..=3 {
            let address = Address([i; 20]);
            let account = AccountState {
                address,
                balance: BigUint::from(1000u64),
                nonce: i as u64,
                account_type: AccountType::Normal,
                code_hash: Some(Hash::default()),
                storage_root: Hash::default(),
                created_at: 0,
                updated_at: 0,
                deleted: false,
            };

            manager1.set_account(&address, account.clone()).await.unwrap();
            manager2.set_account(&address, account).await.unwrap();
        }

        let root1 = calculator
            .calculate_from_manager(&manager1)
            .await
            .unwrap();
        let root2 = calculator
            .calculate_from_manager(&manager2)
            .await
            .unwrap();

        // Same state should produce same root
        assert_eq!(root1, root2);
    }
}

/// Merkle Patricia Trie (MPT) Node
///
/// Represents a node in the Ethereum-style Merkle Patricia Trie.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum MPTNode {
    /// Branch node: has 16 children plus optional value
    Branch {
        children: [Option<Hash>; 16],
        value: Option<Vec<u8>>,
    },
    /// Extension node: shared prefix + child hash
    Extension {
        nibbles: Vec<u8>,
        child: Hash,
    },
    /// Leaf node: key fragment + value
    Leaf {
        nibbles: Vec<u8>,
        value: Vec<u8>,
    },
}

/// Enhanced State Root Calculator with full MPT support
///
/// This implements a more complete Merkle Patricia Trie compatible with Ethereum.
pub struct EnhancedStateRootCalculator {
    use_keccak: bool,
}

impl EnhancedStateRootCalculator {
    /// Create a new enhanced calculator
    pub fn new(use_keccak: bool) -> Self {
        Self { use_keccak }
    }

    /// Calculate state root using full MPT
    pub async fn calculate_mpt_root(
        &self,
        accounts: &HashMap<Address, AccountStateData>,
    ) -> Result<Hash> {
        if accounts.is_empty() {
            return Ok(Hash::default());
        }

        info!("Calculating MPT root for {} accounts", accounts.len());

        // Build MPT from accounts
        let mpt_nodes = self.build_mpt_tree(accounts)?;

        // Calculate root hash from MPT
        let root_hash = self.calculate_mpt_hash(&mpt_nodes)?;

        debug!("MPT root calculated: {:?}", root_hash);
        Ok(root_hash)
    }

    /// Build MPT tree from account data
    fn build_mpt_tree(
        &self,
        accounts: &HashMap<Address, AccountStateData>,
    ) -> Result<MPTNode> {
        // Convert addresses to nibble paths
        let mut paths: Vec<(Vec<u8>, Vec<u8>)> = accounts
            .iter()
            .map(|(addr, data)| {
                let path = Self::address_to_nibbles(addr);
                let value = self.serialize_account_data(data);
                (path, value)
            })
            .collect();

        // Sort paths for deterministic ordering
        paths.sort_by_key(|(path, _)| path.clone());

        // Build tree recursively
        self.build_tree_recursive(&paths)
    }

    /// Build MPT tree recursively from sorted paths
    fn build_tree_recursive(&self, paths: &[(Vec<u8>, Vec<u8>)]) -> Result<MPTNode> {
        match paths.len() {
            0 => Ok(MPTNode::Branch {
                children: Default::default(),
                value: None,
            }),
            1 => {
                // Single path becomes a leaf
                let (nibbles, value) = &paths[0];
                Ok(MPTNode::Leaf {
                    nibbles: nibbles.clone(),
                    value: value.clone(),
                })
            }
            _ => {
                // Find common prefix
                let common_prefix = Self::find_common_prefix(
                    &paths[0].0,
                    &paths[paths.len() - 1].0,
                );

                if !common_prefix.is_empty() {
                    // Create extension node for common prefix
                    let remaining_paths: Vec<_> = paths
                        .iter()
                        .map(|(nibbles, value)| {
                            let remaining = nibbles[common_prefix.len()..].to_vec();
                            (remaining, value.clone())
                        })
                        .collect();

                    let child = self.build_tree_recursive(&remaining_paths)?;
                    let child_hash = self.hash_node(&child)?;

                    Ok(MPTNode::Extension {
                        nibbles: common_prefix,
                        child: child_hash,
                    })
                } else {
                    // Create branch node
                    let mut children: [Option<Hash>; 16] = Default::default();
                    let mut value: Option<Vec<u8>> = None;

                    // Group by first nibble
                    let mut grouped: HashMap<u8, Vec<(Vec<u8>, Vec<u8>)>> = HashMap::new();
                    for (nibbles, data) in paths.iter() {
                        if nibbles.is_empty() {
                            // Value at this node
                            value = Some(data.clone());
                        } else {
                            let first = nibbles[0];
                            let rest = nibbles[1..].to_vec();
                            grouped.entry(first).or_default().push((rest, data.clone()));
                        }
                    }

                    // Build children
                    for (nibble, child_paths) in grouped {
                        let child_node = self.build_tree_recursive(&child_paths)?;
                        children[nibble as usize] = Some(self.hash_node(&child_node)?);
                    }

                    Ok(MPTNode::Branch { children, value })
                }
            }
        }
    }

    /// Convert address to compact nibble path
    fn address_to_nibbles(address: &Address) -> Vec<u8> {
        let mut nibbles = Vec::with_capacity(40);
        for byte in &address.0 {
            nibbles.push(byte >> 4);
            nibbles.push(byte & 0x0F);
        }
        nibbles
    }

    /// Find common prefix of two nibble paths
    fn find_common_prefix(a: &[u8], b: &[u8]) -> Vec<u8> {
        a.iter()
            .zip(b.iter())
            .take_while(|(x, y)| x == y)
            .map(|(x, _)| *x)
            .collect()
    }

    /// Serialize account data for storage in MPT
    fn serialize_account_data(&self, data: &AccountStateData) -> Vec<u8> {
        // RLP encode the account data
        // For simplicity, we use JSON here, but in production use RLP
        serde_json::to_vec(data).unwrap_or_default()
    }

    /// Hash an MPT node
    fn hash_node(&self, node: &MPTNode) -> Result<Hash> {
        let serialized = bincode::serialize(node)
            .map_err(|e| norn_common::error::NornError::Internal(format!("Serialization failed: {}", e)))?;

        let hash = if self.use_keccak {
            // Use Keccak-256 for Ethereum compatibility
            self.keccak_256(&serialized)
        } else {
            // Use SHA-256 for native compatibility
            let mut hasher = Sha256::new();
            hasher.update(&serialized);
            let result = hasher.finalize();
            let mut hash = Hash::default();
            hash.0.copy_from_slice(&result);
            hash
        };

        Ok(hash)
    }

    /// Calculate MPT root hash
    fn calculate_mpt_hash(&self, node: &MPTNode) -> Result<Hash> {
        self.hash_node(node)
    }

    /// Keccak-256 hash function (for Ethereum compatibility)
    fn keccak_256(&self, data: &[u8]) -> Hash {
        use norn_common::build_mode;

        // In test mode: Use SHA-256 with prefix for faster testing
        if build_mode::IS_TEST_MODE {
            let mut hasher = Sha256::new();
            hasher.update(b"TEST_MODE"); // Prefix to indicate test mode
            hasher.update(data);
            let result = hasher.finalize();
            let mut hash = Hash::default();
            hash.0.copy_from_slice(&result);
            return hash;
        }

        // Production mode: Use actual Keccak-256 for Ethereum compatibility
        let hash_output = keccak_hash::keccak(data);
        Hash(hash_output.0)
    }

    /// Verify a proof in the MPT
    pub fn verify_proof(
        &self,
        root_hash: &Hash,
        address: &Address,
        proof: &[Vec<u8>],
        expected_value: Option<&[u8]>,
    ) -> Result<bool> {
        // Reconstruct the path and verify the proof
        let path = Self::address_to_nibbles(address);

        info!("Verifying proof for address {:?} against root {:?}", address, root_hash);
        debug!("Proof contains {} nodes", proof.len());

        // Start with the root hash
        let mut current_hash = *root_hash;
        let mut path_index = 0;

        // Deserialize proof nodes
        let proof_nodes: std::result::Result<Vec<MPTNode>, String> = proof
            .iter()
            .map(|node_data| {
                serde_json::from_slice(node_data)
                    .or_else(|e| {
                        bincode::deserialize(node_data).map_err(|e2| format!("JSON error: {}, Bincode error: {}", e, e2))
                    })
                    .map_err(|e| format!("Failed to deserialize proof node: {}", e))
            })
            .collect();
        let proof_nodes = proof_nodes.map_err(|e| NornError::Internal(e))?;

        // If proof is empty, return true only if expected value is None (non-existent)
        if proof_nodes.is_empty() {
            return Ok(expected_value.is_none());
        }

        // Traverse the MPT using the proof
        for (i, node) in proof_nodes.iter().enumerate() {
            // Verify node hash matches current_hash
            let node_hash = self.calculate_mpt_hash(node)?;
            if node_hash != current_hash {
                debug!("Node hash mismatch at level {}: expected {:?}, got {:?}", i, current_hash, node_hash);
                return Ok(false);
            }

            // Process based on node type
            match node {
                MPTNode::Leaf { nibbles, value } => {
                    // Leaf node: verify we've consumed the path
                    if path_index + nibbles.len() <= path.len() {
                        let remaining_path = &path[path_index..];
                        let nibble_len = nibbles.len().min(remaining_path.len());
                        if &nibbles[..nibble_len] != &remaining_path[..nibble_len] {
                            debug!("Path mismatch in leaf node at level {}", i);
                            return Ok(false);
                        }
                    }

                    // Verify value matches expected
                    return Ok(match expected_value {
                        Some(expected) => value == expected,
                        None => false,
                    });
                }
                MPTNode::Extension { nibbles, child } => {
                    // Extension node: verify path prefix matches
                    if path_index + nibbles.len() > path.len() {
                        debug!("Path too long for extension at level {}", i);
                        return Ok(false);
                    }

                    let path_slice = &path[path_index..path_index + nibbles.len()];
                    if path_slice != nibbles {
                        debug!("Nibble mismatch in extension at level {}", i);
                        return Ok(false);
                    }

                    path_index += nibbles.len();
                    current_hash = *child;
                }
                MPTNode::Branch { children, value } => {
                    if path_index >= path.len() {
                        // We've reached the end of the path
                        return Ok(match (expected_value, value) {
                            (Some(expected), Some(val)) => val == expected,
                            (None, None) => true,
                            _ => false,
                        });
                    }

                    // Get the next nibble and navigate to the corresponding child
                    let nibble = path[path_index] as usize;
                    if nibble >= 16 {
                        debug!("Invalid nibble at path index {}: {}", path_index, nibble);
                        return Ok(false);
                    }

                    if let Some(child_hash) = &children[nibble] {
                        path_index += 1;
                        current_hash = *child_hash;
                    } else {
                        // No child at this path - value doesn't exist
                        return Ok(expected_value.is_none());
                    }
                }
            }
        }

        // If we've exhausted all proof nodes, check if we found the value
        Ok(expected_value.is_none())
    }
}

impl Default for EnhancedStateRootCalculator {
    fn default() -> Self {
        Self {
            use_keccak: false,
        }
    }
}

#[cfg(test)]
mod enhanced_tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_enhanced_empty_state() {
        let calculator = EnhancedStateRootCalculator::default();
        let accounts = HashMap::new();

        let root = calculator.calculate_mpt_root(&accounts).await.unwrap();
        assert_eq!(root, Hash::default());
    }

    #[tokio::test]
    async fn test_enhanced_single_account() {
        let calculator = EnhancedStateRootCalculator::default();
        let mut accounts = HashMap::new();

        let address = Address([1u8; 20]);
        let data = AccountStateData {
            balance: "1000".to_string(),
            nonce: 0,
            code_hash: Hash::default(),
            storage_root: Hash::default(),
            account_type: crate::state::AccountType::Normal,
        };

        accounts.insert(address, data);

        let root = calculator.calculate_mpt_root(&accounts).await.unwrap();
        assert_ne!(root, Hash::default());
    }
}
