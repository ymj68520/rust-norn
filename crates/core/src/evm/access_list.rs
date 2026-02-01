//! EIP-2930: Typed Transaction - Access List
//!
//! Implements EIP-2930 access list support, which allows transactions to specify
//! addresses and storage slots they will access, reducing gas costs for warm access.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use norn_common::types::{Address, Hash, AccessListItem};

/// Gas cost for accessing an address not in the access list (cold access)
pub const COLD_ACCOUNT_ACCESS_COST: u64 = 2_600;

/// Gas cost for accessing a storage slot not in the access list (cold access)
pub const COLD_SLOAD_COST: u64 = 2_100;

/// Gas cost for accessing an address in the access list (warm access)
pub const WARM_ACCOUNT_ACCESS_COST: u64 = 100;

/// Gas cost for accessing a storage slot in the access list (warm access)
pub const WARM_SLOAD_COST: u64 = 100;

/// Gas cost per access list item (additional cost for including in access list)
pub const ACCESS_LIST_ADDRESS_COST: u64 = 2_400;

/// Gas cost per storage key in access list
pub const ACCESS_LIST_STORAGE_KEY_COST: u64 = 1_900;

/// Access list tracker for EIP-2930
#[derive(Debug, Clone, Default)]
pub struct AccessListTracker {
    /// Addresses that have been accessed
    accessed_addresses: HashSet<Address>,
    /// Storage slots that have been accessed (address -> storage_keys)
    accessed_storage: HashMap<Address, HashSet<Hash>>,
    /// Pre-declared access list from transaction
    predeclared_access: HashMap<Address, HashSet<Hash>>,
}

impl AccessListTracker {
    /// Create a new access list tracker with a pre-declared access list
    pub fn new(access_list: Option<Vec<AccessListItem>>) -> Self {
        let mut predeclared_access = HashMap::new();

        if let Some(list) = access_list {
            for item in list {
                let storage_keys: HashSet<Hash> = item.storage_keys.into_iter().collect();
                predeclared_access.insert(item.address, storage_keys);
            }
        }

        Self {
            accessed_addresses: HashSet::new(),
            accessed_storage: HashMap::new(),
            predeclared_access,
        }
    }

    /// Record access to an address
    pub fn access_address(&mut self, address: &Address) -> AccessType {
        let is_warm = self.accessed_addresses.contains(address)
            || self.predeclared_access.contains_key(address);

        self.accessed_addresses.insert(*address);
        self.accessed_storage.entry(*address).or_default();

        if is_warm {
            AccessType::Warm
        } else {
            AccessType::Cold
        }
    }

    /// Record access to a storage slot
    pub fn access_storage(&mut self, address: &Address, storage_key: &Hash) -> AccessType {
        // First ensure the address is tracked
        let address_access = self.access_address(address);

        // Check if storage key was pre-declared
        let is_predeclared = self.predeclared_access
            .get(address)
            .map(|keys| keys.contains(storage_key))
            .unwrap_or(false);

        // Check if storage key was already accessed
        let is_warm = self.accessed_storage
            .get(address)
            .map(|keys| keys.contains(storage_key))
            .unwrap_or(false);

        self.accessed_storage
            .entry(*address)
            .or_default()
            .insert(*storage_key);

        if is_predeclared || is_warm {
            AccessType::Warm
        } else {
            AccessType::Cold
        }
    }

    /// Calculate gas cost for an address access
    pub fn address_access_cost(&mut self, address: &Address) -> u64 {
        match self.access_address(address) {
            AccessType::Warm => WARM_ACCOUNT_ACCESS_COST,
            AccessType::Cold => COLD_ACCOUNT_ACCESS_COST,
        }
    }

    /// Calculate gas cost for a storage access
    pub fn storage_access_cost(&mut self, address: &Address, storage_key: &Hash) -> u64 {
        match self.access_storage(address, storage_key) {
            AccessType::Warm => WARM_SLOAD_COST,
            AccessType::Cold => COLD_SLOAD_COST,
        }
    }

    /// Calculate the gas cost of including the access list in the transaction
    pub fn access_list_gas_cost(&self) -> u64 {
        let mut cost = 0u64;

        for (address, storage_keys) in &self.predeclared_access {
            // Cost for address
            cost += ACCESS_LIST_ADDRESS_COST;
            // Cost for each storage key
            cost += ACCESS_LIST_STORAGE_KEY_COST * storage_keys.len() as u64;
        }

        cost
    }

    /// Validate that all accessed addresses/slots are in the access list
    pub fn validate_access(&self) -> Result<(), String> {
        // Check if any accessed address was not pre-declared
        for address in &self.accessed_addresses {
            if !self.predeclared_access.contains_key(address) {
                return Err(format!(
                    "Address {} was accessed but not in access list",
                    hex::encode(address.0)
                ));
            }
        }

        // Check if any accessed storage slot was not pre-declared
        for (address, storage_keys) in &self.accessed_storage {
            if let Some(predeclared_keys) = self.predeclared_access.get(address) {
                for key in storage_keys {
                    if !predeclared_keys.contains(key) {
                        return Err(format!(
                            "Storage key {} for address {} was accessed but not in access list",
                            hex::encode(key.0),
                            hex::encode(address.0)
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the number of unique addresses in the access list
    pub fn address_count(&self) -> usize {
        self.predeclared_access.len()
    }

    /// Get the total number of storage keys in the access list
    pub fn storage_key_count(&self) -> usize {
        self.predeclared_access.values().map(|keys| keys.len()).sum()
    }

    /// Check if an address is in the access list
    pub fn has_address(&self, address: &Address) -> bool {
        self.predeclared_access.contains_key(address)
    }

    /// Check if a storage key is in the access list
    pub fn has_storage_key(&self, address: &Address, storage_key: &Hash) -> bool {
        self.predeclared_access
            .get(address)
            .map(|keys| keys.contains(storage_key))
            .unwrap_or(false)
    }
}

/// Access type (warm or cold)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// Warm (previously accessed or in access list)
    Warm,
    /// Cold (first access)
    Cold,
}

/// EIP-2930 utilities
pub struct EIP2930Utils;

impl EIP2930Utils {
    /// Calculate the gas cost of a transaction with an access list
    pub fn calculate_transaction_gas_cost(
        intrinsic_gas: u64,
        access_list: Option<Vec<AccessListItem>>,
    ) -> u64 {
        let tracker = AccessListTracker::new(access_list);
        intrinsic_gas + tracker.access_list_gas_cost()
    }

    /// Estimate gas savings from using an access list
    pub fn estimate_gas_savings(
        cold_addresses: usize,
        cold_storage_slots: usize,
    ) -> u64 {
        // Savings = (cold cost - warm cost) * number of accesses
        let address_savings = (COLD_ACCOUNT_ACCESS_COST - WARM_ACCOUNT_ACCESS_COST) as usize * cold_addresses;
        let storage_savings = (COLD_SLOAD_COST - WARM_SLOAD_COST) as usize * cold_storage_slots;

        (address_savings + storage_savings) as u64
    }

    /// Validate access list format
    pub fn validate_access_list(access_list: &[AccessListItem]) -> Result<(), String> {
        let mut seen_addresses = HashSet::new();

        for (i, item) in access_list.iter().enumerate() {
            // Check for duplicate addresses
            if seen_addresses.contains(&item.address) {
                return Err(format!(
                    "Duplicate address at index {}: {}",
                    i,
                    hex::encode(item.address.0)
                ));
            }
            seen_addresses.insert(item.address);

            // Check for duplicate storage keys
            let mut seen_keys = HashSet::new();
            for (j, key) in item.storage_keys.iter().enumerate() {
                if seen_keys.contains(key) {
                    return Err(format!(
                        "Duplicate storage key at index {}, key {}: {}",
                        i,
                        j,
                        hex::encode(key.0)
                    ));
                }
                seen_keys.insert(key);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_list_tracker_new() {
        let address = Address([1u8; 20]);
        let storage_key = Hash([2u8; 32]);

        let access_list = vec![AccessListItem {
            address,
            storage_keys: vec![storage_key],
        }];

        let tracker = AccessListTracker::new(Some(access_list));

        assert_eq!(tracker.address_count(), 1);
        assert_eq!(tracker.storage_key_count(), 1);
        assert!(tracker.has_address(&address));
        assert!(tracker.has_storage_key(&address, &storage_key));
    }

    #[test]
    fn test_cold_address_access() {
        let mut tracker = AccessListTracker::new(None);
        let address = Address([1u8; 20]);

        // First access should be cold
        let cost = tracker.address_access_cost(&address);
        assert_eq!(cost, COLD_ACCOUNT_ACCESS_COST);

        // Second access should be warm
        let cost = tracker.address_access_cost(&address);
        assert_eq!(cost, WARM_ACCOUNT_ACCESS_COST);
    }

    #[test]
    fn test_warm_address_access() {
        let address = Address([1u8; 20]);
        let access_list = vec![AccessListItem {
            address,
            storage_keys: vec![],
        }];

        let mut tracker = AccessListTracker::new(Some(access_list));

        let access_type = tracker.access_address(&address);
        assert_eq!(access_type, AccessType::Warm);

        let cost = tracker.address_access_cost(&address);
        assert_eq!(cost, WARM_ACCOUNT_ACCESS_COST);
    }

    #[test]
    fn test_cold_storage_access() {
        let mut tracker = AccessListTracker::new(None);
        let address = Address([1u8; 20]);
        let storage_key = Hash([2u8; 32]);

        let cost = tracker.storage_access_cost(&address, &storage_key);
        assert_eq!(cost, COLD_SLOAD_COST);
    }

    #[test]
    fn test_warm_storage_access() {
        let address = Address([1u8; 20]);
        let storage_key = Hash([2u8; 32]);
        let access_list = vec![AccessListItem {
            address,
            storage_keys: vec![storage_key],
        }];

        let mut tracker = AccessListTracker::new(Some(access_list));

        let cost = tracker.storage_access_cost(&address, &storage_key);
        assert_eq!(cost, WARM_SLOAD_COST);
    }

    #[test]
    fn test_access_list_gas_cost() {
        let address1 = Address([1u8; 20]);
        let address2 = Address([2u8; 20]);
        let storage_key1 = Hash([3u8; 32]);
        let storage_key2 = Hash([4u8; 32]);

        let access_list = vec![
            AccessListItem {
                address: address1,
                storage_keys: vec![storage_key1, storage_key2],
            },
            AccessListItem {
                address: address2,
                storage_keys: vec![],
            },
        ];

        let tracker = AccessListTracker::new(Some(access_list));

        // 2 addresses + 2 storage keys
        let expected_cost = (2 * ACCESS_LIST_ADDRESS_COST) + (2 * ACCESS_LIST_STORAGE_KEY_COST);
        assert_eq!(tracker.access_list_gas_cost(), expected_cost);
    }

    #[test]
    fn test_validate_access_list_success() {
        let address = Address([1u8; 20]);
        let storage_key = Hash([2u8; 32]);

        let access_list = vec![AccessListItem {
            address,
            storage_keys: vec![storage_key],
        }];

        let result = EIP2930Utils::validate_access_list(&access_list);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_access_list_duplicate_address() {
        let address = Address([1u8; 20]);
        let storage_key = Hash([2u8; 32]);

        let access_list = vec![
            AccessListItem {
                address,
                storage_keys: vec![storage_key],
            },
            AccessListItem {
                address,
                storage_keys: vec![],
            },
        ];

        let result = EIP2930Utils::validate_access_list(&access_list);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_access_list_duplicate_storage_key() {
        let address = Address([1u8; 20]);
        let storage_key = Hash([2u8; 32]);

        let access_list = vec![AccessListItem {
            address,
            storage_keys: vec![storage_key, storage_key],
        }];

        let result = EIP2930Utils::validate_access_list(&access_list);
        assert!(result.is_err());
    }

    #[test]
    fn test_estimate_gas_savings() {
        // 2 cold addresses and 3 cold storage slots
        let savings = EIP2930Utils::estimate_gas_savings(2, 3);

        let address_savings = (COLD_ACCOUNT_ACCESS_COST - WARM_ACCOUNT_ACCESS_COST) * 2;
        let storage_savings = (COLD_SLOAD_COST - WARM_SLOAD_COST) * 3;
        let expected = address_savings + storage_savings;

        assert_eq!(savings, expected);
    }

    #[test]
    fn test_calculate_transaction_gas_cost() {
        let address = Address([1u8; 20]);
        let storage_key = Hash([2u8; 32]);

        let access_list = vec![AccessListItem {
            address,
            storage_keys: vec![storage_key],
        }];

        let intrinsic_gas = 21_000;
        let total_cost = EIP2930Utils::calculate_transaction_gas_cost(
            intrinsic_gas,
            Some(access_list),
        );

        let access_cost = ACCESS_LIST_ADDRESS_COST + ACCESS_LIST_STORAGE_KEY_COST;
        assert_eq!(total_cost, intrinsic_gas + access_cost);
    }
}
