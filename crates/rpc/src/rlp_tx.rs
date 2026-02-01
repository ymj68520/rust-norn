//! RLP Transaction Parsing for Ethereum Compatibility
//!
//! This module handles parsing of RLP-encoded Ethereum transactions,
//! supporting legacy, EIP-2930, and EIP-1559 transaction types.

use norn_common::types::{Hash, Transaction, TransactionBody, TransactionType, PublicKey, AccessListItem, Address};
use rlp::{Rlp, RlpStream, DecoderError};
use anyhow::{Result, anyhow};
use k256::ecdsa::{Signature, VerifyingKey};
use std::str::FromStr;

/// Ethereum transaction type identifiers
const TX_TYPE_LEGACY: u8 = 0x00; // No type prefix for legacy
const TX_TYPE_EIP2930: u8 = 0x01;
const TX_TYPE_EIP1559: u8 = 0x02;

/// Parsed Ethereum transaction
#[derive(Debug, Clone)]
pub struct EthereumTransaction {
    /// Transaction type (None for legacy)
    pub tx_type: Option<u8>,
    /// Nonce
    pub nonce: u64,
    /// Gas price (legacy) or max priority fee per gas (EIP-1559)
    pub gas_price_or_max_priority_fee: u64,
    /// Max fee per gas (EIP-1559 only)
    pub max_fee_per_gas: Option<u64>,
    /// Gas limit
    pub gas_limit: u64,
    /// Recipient address (None for contract creation)
    pub to: Option<Address>,
    /// Value in wei
    pub value: Vec<u8>,
    /// Transaction data
    pub data: Vec<u8>,
    /// Chain ID (EIP-155)
    pub chain_id: Option<u64>,
    /// Access list (EIP-2930 and EIP-1559)
    pub access_list: Vec<AccessListItem>,
    /// Signature v
    pub v: u64,
    /// Signature r
    pub r: Vec<u8>,
    /// Signature s
    pub s: Vec<u8>,
}

impl EthereumTransaction {
    /// Parse an RLP-encoded Ethereum transaction
    pub fn parse(data: &[u8]) -> Result<Self> {
        // Check for typed transaction (EIP-2718)
        if data.len() > 0 && data[0] <= 0x7f {
            let tx_type = data[0];
            let rlp_data = &data[1..];

            return match tx_type {
                TX_TYPE_EIP2930 => Self::parse_eip2930(rlp_data),
                TX_TYPE_EIP1559 => Self::parse_eip1559(rlp_data),
                _ => Err(anyhow!("Unknown transaction type: {}", tx_type)),
            };
        }

        // Legacy transaction
        Self::parse_legacy(data)
    }

    /// Parse a legacy transaction (pre-EIP-2718)
    fn parse_legacy(data: &[u8]) -> Result<Self> {
        let rlp = Rlp::new(data);
        if !rlp.is_list() {
            return Err(anyhow!("Invalid RLP: not a list"));
        }

        let items = rlp.item_count()?;
        if items < 9 {
            return Err(anyhow!("Invalid legacy transaction: too few items (got {}, expected 9)", items));
        }

        let nonce: u64 = rlp.val_at(0)?;
        let gas_price: u64 = rlp.val_at(1)?;
        let gas_limit: u64 = rlp.val_at(2)?;

        // Parse to address (empty for contract creation)
        let to_bytes: Vec<u8> = rlp.val_at(3)?;
        let to = if to_bytes.is_empty() {
            None
        } else if to_bytes.len() == 20 {
            Some(Address(to_bytes.try_into().unwrap()))
        } else {
            return Err(anyhow!("Invalid 'to' address length: {}", to_bytes.len()));
        };

        let value: Vec<u8> = rlp.val_at(4)?;
        let data: Vec<u8> = rlp.val_at(5)?;

        // v, r, s signature components
        let v: u64 = rlp.val_at(6)?;
        let r: Vec<u8> = rlp.val_at(7)?;
        let s: Vec<u8> = rlp.val_at(8)?;

        // Extract chain ID from v (EIP-155)
        let chain_id = if v > 36 {
            let chain_id = (v - 35) / 2;
            Some(chain_id)
        } else {
            None
        };

        Ok(EthereumTransaction {
            tx_type: None,
            nonce,
            gas_price_or_max_priority_fee: gas_price,
            max_fee_per_gas: None,
            gas_limit,
            to,
            value,
            data,
            chain_id,
            access_list: vec![],
            v,
            r,
            s,
        })
    }

    /// Parse an EIP-2930 transaction (type 1)
    fn parse_eip2930(data: &[u8]) -> Result<Self> {
        let rlp = Rlp::new(data);
        if !rlp.is_list() {
            return Err(anyhow!("Invalid RLP: not a list"));
        }

        let items = rlp.item_count()?;
        if items < 11 {
            return Err(anyhow!("Invalid EIP-2930 transaction: too few items"));
        }

        let chain_id: u64 = rlp.val_at(0)?;
        let nonce: u64 = rlp.val_at(1)?;
        let gas_price: u64 = rlp.val_at(2)?;
        let gas_limit: u64 = rlp.val_at(3)?;

        let to_bytes: Vec<u8> = rlp.val_at(4)?;
        let to = if to_bytes.is_empty() {
            None
        } else if to_bytes.len() == 20 {
            Some(Address(to_bytes.try_into().unwrap()))
        } else {
            return Err(anyhow!("Invalid 'to' address length"));
        };

        let value: Vec<u8> = rlp.val_at(5)?;
        let data: Vec<u8> = rlp.val_at(6)?;

        // Parse access list
        let access_list_rlp = rlp.at(7)?;
        let access_list_count = access_list_rlp.item_count()?;
        let mut access_list = vec![];

        for i in 0..access_list_count {
            let item = access_list_rlp.at(i)?;
            let addr_bytes: Vec<u8> = item.val_at(0)?;
            let keys_rlp = item.at(1)?;
            let keys_count = keys_rlp.item_count()?;
            let mut storage_keys = vec![];

            for j in 0..keys_count {
                let key_bytes: Vec<u8> = keys_rlp.val_at(j)?;
                storage_keys.push(Hash(key_bytes.try_into().unwrap()));
            }

            access_list.push(AccessListItem {
                address: Address(addr_bytes.try_into().unwrap()),
                storage_keys,
            });
        }

        let v: u64 = rlp.val_at(8)?;
        let r: Vec<u8> = rlp.val_at(9)?;
        let s: Vec<u8> = rlp.val_at(10)?;

        Ok(EthereumTransaction {
            tx_type: Some(TX_TYPE_EIP2930),
            chain_id: Some(chain_id),
            nonce,
            gas_price_or_max_priority_fee: gas_price,
            max_fee_per_gas: None,
            gas_limit,
            to,
            value,
            data,
            access_list,
            v,
            r,
            s,
        })
    }

    /// Parse an EIP-1559 transaction (type 2)
    fn parse_eip1559(data: &[u8]) -> Result<Self> {
        let rlp = Rlp::new(data);
        if !rlp.is_list() {
            return Err(anyhow!("Invalid RLP: not a list"));
        }

        let items = rlp.item_count()?;
        if items < 12 {
            return Err(anyhow!("Invalid EIP-1559 transaction: too few items"));
        }

        let chain_id: u64 = rlp.val_at(0)?;
        let nonce: u64 = rlp.val_at(1)?;
        let max_priority_fee_per_gas: u64 = rlp.val_at(2)?;
        let max_fee_per_gas: u64 = rlp.val_at(3)?;
        let gas_limit: u64 = rlp.val_at(4)?;

        let to_bytes: Vec<u8> = rlp.val_at(5)?;
        let to = if to_bytes.is_empty() {
            None
        } else if to_bytes.len() == 20 {
            Some(Address(to_bytes.try_into().unwrap()))
        } else {
            return Err(anyhow!("Invalid 'to' address length"));
        };

        let value: Vec<u8> = rlp.val_at(6)?;
        let data: Vec<u8> = rlp.val_at(7)?;

        // Parse access list
        let access_list_rlp = rlp.at(8)?;
        let access_list_count = access_list_rlp.item_count()?;
        let mut access_list = vec![];

        for i in 0..access_list_count {
            let item = access_list_rlp.at(i)?;
            let addr_bytes: Vec<u8> = item.val_at(0)?;
            let keys_rlp = item.at(1)?;
            let keys_count = keys_rlp.item_count()?;
            let mut storage_keys = vec![];

            for j in 0..keys_count {
                let key_bytes: Vec<u8> = keys_rlp.val_at(j)?;
                storage_keys.push(Hash(key_bytes.try_into().unwrap()));
            }

            access_list.push(AccessListItem {
                address: Address(addr_bytes.try_into().unwrap()),
                storage_keys,
            });
        }

        let v: u64 = rlp.val_at(9)?;
        let r: Vec<u8> = rlp.val_at(10)?;
        let s: Vec<u8> = rlp.val_at(11)?;

        Ok(EthereumTransaction {
            tx_type: Some(TX_TYPE_EIP1559),
            chain_id: Some(chain_id),
            nonce,
            gas_price_or_max_priority_fee: max_priority_fee_per_gas,
            max_fee_per_gas: Some(max_fee_per_gas),
            gas_limit,
            to,
            value,
            data,
            access_list,
            v,
            r,
            s,
        })
    }

    /// Compute the signing hash for this transaction
    pub fn compute_signing_hash(&self) -> Result<[u8; 32]> {
        // For now, return a placeholder
        // In a real implementation, this would compute keccak256(message)
        Ok([0u8; 32])
    }

    /// Convert to Norn Transaction
    pub fn to_norn_transaction(&self) -> Result<Transaction> {
        // Placeholder conversion
        Ok(Transaction::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_legacy_transaction() {
        // Create a simple RLP-encoded legacy transaction
        let mut stream = RlpStream::new_list(9);
        stream.append(&0u64); // nonce
        stream.append(&1000u64); // gas price
        stream.append(&21000u64); // gas limit
        stream.append_empty_data(); // to (contract creation)
        stream.append(&vec![0u8; 32]); // value
        stream.append_empty_data(); // data
        stream.append(&27u64); // v
        stream.append(&vec![0u8; 32]); // r
        stream.append(&vec![0u8; 32]); // s

        let tx_bytes = stream.out();

        let result = EthereumTransaction::parse(&tx_bytes);
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());

        let tx = result.unwrap();
        assert_eq!(tx.nonce, 0);
        assert_eq!(tx.gas_limit, 21000);
        assert!(tx.to.is_none()); // Contract creation
    }

    #[test]
    fn test_parse_legacy_transaction_with_to_address() {
        // Create transaction with to address
        let mut stream = RlpStream::new_list(9);
        stream.append(&1u64); // nonce
        stream.append(&2000u64); // gas price
        stream.append(&21000u64); // gas limit
        stream.append(&vec![0xffu8; 20]); // to address
        stream.append(&vec![0u8; 32]); // value
        stream.append_empty_data(); // data
        stream.append(&37u64); // v
        stream.append(&vec![0u8; 32]); // r
        stream.append(&vec![0u8; 32]); // s

        let tx_bytes = stream.out();

        let result = EthereumTransaction::parse(&tx_bytes);
        assert!(result.is_ok());

        let tx = result.unwrap();
        assert_eq!(tx.nonce, 1);
        assert_eq!(tx.gas_limit, 21000);
        assert!(tx.to.is_some());
    }

    #[test]
    fn test_compute_signing_hash() {
        let mut stream = RlpStream::new_list(9);
        stream.append(&0u64);
        stream.append(&1000u64);
        stream.append(&21000u64);
        stream.append_empty_data();
        stream.append(&vec![0u8; 32]);
        stream.append_empty_data();
        stream.append(&27u64);
        stream.append(&vec![0u8; 32]);
        stream.append(&vec![0u8; 32]);

        let tx_bytes = stream.out();
        let tx = EthereumTransaction::parse(&tx_bytes).unwrap();
        let hash = tx.compute_signing_hash();
        assert!(hash.is_ok());
    }
}
