use norn_common::types::{Transaction, TransactionBody, Address, Hash, PublicKey};
use norn_common::types::PUBLIC_KEY_LENGTH;
use crate::ecdsa::{KeyPair, verify};
use sha2::{Sha256, Digest};
use anyhow::Result;
use thiserror::Error;
use p256::ecdsa::VerifyingKey;

#[derive(Error, Debug)]
pub enum TxError {
    #[error("Invalid transaction format")]
    InvalidFormat,
    #[error("Signature verification failed")]
    VerificationFailed,
    #[error("Invalid nonce")]
    InvalidNonce,
    #[error("Insufficient gas")]
    InsufficientGas,
}

pub struct TransactionSigner {
    keypair: KeyPair,
    address: Address,
    nonce: u64,
}

impl TransactionSigner {
    pub fn new(keypair: KeyPair) -> Self {
        let public_key = keypair.public_key();
        let address = public_key_to_address(&public_key);

        Self {
            keypair,
            address,
            nonce: 0,
        }
    }

    pub fn from_private_key(private_key_hex: &str) -> Result<Self> {
        let keypair = KeyPair::from_private_key_hex(private_key_hex)?;
        Ok(Self::new(keypair))
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn next_nonce(&mut self) -> u64 {
        let nonce = self.nonce;
        self.nonce += 1;
        nonce
    }

    pub fn create_transaction(
        &mut self,
        receiver: Address,
        event: Vec<u8>,
        opt: Vec<u8>,
        state: Vec<u8>,
        data: Vec<u8>,
        gas: i64,
        expire: i64,
    ) -> Result<Transaction> {
        let nonce = self.next_nonce() as i64;
        let timestamp = chrono::Utc::now().timestamp();

        // Create unsigned transaction body
        let mut unsigned_body = TransactionBody {
            hash: Hash::default(),
            address: self.address,
            receiver,
            gas,
            nonce,
            event: event.clone(),
            opt: opt.clone(),
            state: state.clone(),
            data: data.clone(),
            expire,
            height: 0,
            index: 0,
            block_hash: Hash::default(),
            timestamp,
            public: PublicKey::default(),
            signature: Vec::new(),
        };

        // Calculate hash of unsigned transaction
        unsigned_body.hash = hash_transaction_body(&unsigned_body);

        // Set the public key
        let encoded_point = self.keypair.public_key().to_encoded_point(true);
        let public_key_bytes = encoded_point.as_bytes();
        let mut public_key = PublicKey::default();
        if public_key_bytes.len() == PUBLIC_KEY_LENGTH {
            public_key.0.copy_from_slice(public_key_bytes);
        }
        unsigned_body.public = public_key;

        // Create message to sign
        let message = create_signing_message(&unsigned_body);

        // Sign the transaction
        let signature = self.keypair.sign(&message);
        unsigned_body.signature = signature;

        // Recalculate final hash with signature
        unsigned_body.hash = hash_transaction_body(&unsigned_body);

        Ok(Transaction {
            body: unsigned_body,
        })
    }
}

pub fn verify_transaction(tx: &Transaction) -> Result<(), TxError> {
    // 1. Verify transaction hash
    let calculated_hash = hash_transaction_body(&tx.body);
    if calculated_hash != tx.body.hash {
        return Err(TxError::InvalidFormat);
    }

    // 2. Verify signature
    let message = create_signing_message(&tx.body);
    let public_key_bytes = tx.body.public.0.to_vec();

    if !verify(&public_key_bytes, &message, &tx.body.signature)
        .map_err(|_| TxError::VerificationFailed)?
    {
        return Err(TxError::VerificationFailed);
    }

    // 3. Basic validation
    if tx.body.gas <= 0 {
        return Err(TxError::InsufficientGas);
    }

    if tx.body.nonce < 0 {
        return Err(TxError::InvalidNonce);
    }

    Ok(())
}

fn hash_transaction_body(body: &TransactionBody) -> Hash {
    let mut hasher = Sha256::new();

    // Include all fields except hash and signature
    hasher.update(body.address.0);
    hasher.update(body.receiver.0);
    hasher.update(body.gas.to_le_bytes());
    hasher.update(body.nonce.to_le_bytes());
    hasher.update(&body.event);
    hasher.update(&body.opt);
    hasher.update(&body.state);
    hasher.update(&body.data);
    hasher.update(body.expire.to_le_bytes());
    hasher.update(body.height.to_le_bytes());
    hasher.update(body.index.to_le_bytes());
    hasher.update(body.block_hash.0);
    hasher.update(body.timestamp.to_le_bytes());
    hasher.update(body.public.0);

    let result = hasher.finalize();
    let mut hash = Hash::default();
    hash.0.copy_from_slice(&result);
    hash
}

fn create_signing_message(body: &TransactionBody) -> Vec<u8> {
    let mut hasher = Sha256::new();

    // Create message by hashing key transaction fields
    hasher.update("NORN_TRANSACTION".as_bytes());
    hasher.update(body.address.0);
    hasher.update(body.receiver.0);
    hasher.update(body.gas.to_le_bytes());
    hasher.update(body.nonce.to_le_bytes());
    hasher.update(&body.event);
    hasher.update(&body.opt);
    hasher.update(&body.state);
    hasher.update(&body.data);
    hasher.update(body.expire.to_le_bytes());
    hasher.update(body.timestamp.to_le_bytes());

    hasher.finalize().to_vec()
}

fn public_key_to_address(public_key: &VerifyingKey) -> Address {
    let mut hasher = Sha256::new();
    hasher.update(public_key.to_encoded_point(true).as_bytes());
    let hash = hasher.finalize();

    // Take first 20 bytes as address
    let mut address_bytes = [0u8; 20];
    address_bytes.copy_from_slice(&hash[..20]);
    Address(address_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_creation_and_verification() {
        let keypair = KeyPair::random();
        let mut signer = TransactionSigner::new(keypair);

        let receiver = Address::default();
        let tx = signer.create_transaction(
            receiver,
            b"test_event".to_vec(),
            b"test_opt".to_vec(),
            b"test_state".to_vec(),
            b"test_data".to_vec(),
            1000,
            chrono::Utc::now().timestamp() + 3600,
        ).unwrap();

        assert!(verify_transaction(&tx).is_ok());
    }

    #[test]
    fn test_invalid_transaction_verification() {
        let keypair = KeyPair::random();
        let mut signer = TransactionSigner::new(keypair);

        let receiver = Address::default();
        let mut tx = signer.create_transaction(
            receiver,
            b"test_event".to_vec(),
            b"test_opt".to_vec(),
            b"test_state".to_vec(),
            b"test_data".to_vec(),
            1000,
            chrono::Utc::now().timestamp() + 3600,
        ).unwrap();

        // Modify the signature to make it invalid
        tx.body.signature[0] ^= 0xFF;

        assert!(verify_transaction(&tx).is_err());
    }
}