use crate::ecdsa::{verify, KeyPair};
use anyhow::Result;
use norn_common::types::{
    Address, Hash, PublicKey, Transaction, TransactionBody, TransactionType, TransactionV2,
    TransactionV2Error, PUBLIC_KEY_LENGTH,
};
use p256::ecdsa::VerifyingKey;
use p256::ecdsa::{signature::Verifier, Signature};
use sha2::{Digest, Sha256};
use thiserror::Error;

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

/// Sign the protocol-v2 transaction preimage and derive its ID.
///
/// This function intentionally signs before calculating the ID. The ID is
/// derived from the immutable transaction fields plus the final signature,
/// never from a block hash or inclusion position.
pub fn sign_transaction_v2(
    keypair: &KeyPair,
    tx: &mut TransactionV2,
) -> Result<(), TransactionV2Error> {
    let verifying_key = keypair.public_key();
    let encoded = verifying_key.to_encoded_point(true);
    if encoded.as_bytes().len() != PUBLIC_KEY_LENGTH {
        return Err(TransactionV2Error::InvalidPublicKey);
    }
    tx.public_key = PublicKey(
        encoded
            .as_bytes()
            .try_into()
            .map_err(|_| TransactionV2Error::InvalidPublicKey)?,
    );
    let signature = Signature::from_slice(&keypair.sign(&tx.signing_bytes()?))
        .map_err(|_| TransactionV2Error::InvalidSignature)?;
    let normalized = signature.normalize_s().unwrap_or(signature);
    tx.signature = normalized.to_bytes().into();
    tx.transaction_id = tx.calculate_id()?;
    Ok(())
}

/// Validate a signed protocol-v2 transaction, including its ID, key binding,
/// and canonical low-S ECDSA signature.
pub fn verify_transaction_v2(tx: &TransactionV2) -> Result<(), TxError> {
    tx.validate().map_err(|_| TxError::InvalidFormat)?;
    let verifying_key =
        VerifyingKey::from_sec1_bytes(&tx.public_key.0).map_err(|_| TxError::InvalidFormat)?;
    let derived_sender = public_key_to_address(&verifying_key);
    if derived_sender != tx.sender {
        return Err(TxError::VerificationFailed);
    }
    let signature =
        Signature::from_slice(&tx.signature).map_err(|_| TxError::VerificationFailed)?;
    if signature.normalize_s().is_some() {
        return Err(TxError::VerificationFailed);
    }
    let valid = verifying_key
        .verify(
            &tx.signing_bytes().map_err(|_| TxError::InvalidFormat)?,
            &signature,
        )
        .is_ok();
    if valid {
        Ok(())
    } else {
        Err(TxError::VerificationFailed)
    }
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
            timestamp,
            public: PublicKey::default(),
            signature: Vec::new(),
            tx_type: TransactionType::default(), // Native by default
            chain_id: None,
            value: None,
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
            access_list: None,
            gas_price: None,
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

    // Legacy adapter hash with protocol-v2 semantics: inclusion metadata is
    // intentionally excluded. Height, index, and block hash are derived by
    // the containing block and must never affect the transaction ID.
    hasher.update(b"NORN_TRANSACTION_V2_ADAPTER");
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
    hasher.update(body.public.0);
    hasher.update([match body.tx_type {
        TransactionType::Native => 0,
        TransactionType::EVM => 1,
    }]);
    hasher.update(body.chain_id.unwrap_or_default().to_be_bytes());
    hasher.update(body.value.as_deref().unwrap_or("0").as_bytes());
    hasher.update(body.max_fee_per_gas.unwrap_or_default().to_be_bytes());
    hasher.update(
        body.max_priority_fee_per_gas
            .unwrap_or_default()
            .to_be_bytes(),
    );
    hasher.update(body.gas_price.unwrap_or_default().to_be_bytes());
    if let Some(access_list) = &body.access_list {
        hasher.update((access_list.len() as u64).to_be_bytes());
        for item in access_list {
            hasher.update(item.address.0);
            hasher.update((item.storage_keys.len() as u64).to_be_bytes());
            for key in &item.storage_keys {
                hasher.update(key.0);
            }
        }
    } else {
        hasher.update(0u64.to_be_bytes());
    }

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
        let tx = signer
            .create_transaction(
                receiver,
                b"test_event".to_vec(),
                b"test_opt".to_vec(),
                b"test_state".to_vec(),
                b"test_data".to_vec(),
                1000,
                chrono::Utc::now().timestamp() + 3600,
            )
            .unwrap();

        assert!(verify_transaction(&tx).is_ok());
    }

    #[test]
    fn test_invalid_transaction_verification() {
        let keypair = KeyPair::random();
        let mut signer = TransactionSigner::new(keypair);

        let receiver = Address::default();
        let mut tx = signer
            .create_transaction(
                receiver,
                b"test_event".to_vec(),
                b"test_opt".to_vec(),
                b"test_state".to_vec(),
                b"test_data".to_vec(),
                1000,
                chrono::Utc::now().timestamp() + 3600,
            )
            .unwrap();

        // Modify the signature to make it invalid
        tx.body.signature[0] ^= 0xFF;

        assert!(verify_transaction(&tx).is_err());
    }

    #[test]
    fn test_transaction_v2_signs_and_verifies_without_inclusion_metadata() {
        let keypair = KeyPair::random();
        let mut tx = TransactionV2 {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId(Hash([1; 32])),
            nonce: 0,
            sender: public_key_to_address(&keypair.public_key()),
            receiver: Some(Address([3; 20])),
            value: 7,
            gas_limit: 21_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 1,
            data: vec![1, 2, 3],
            event: vec![],
            opt: vec![],
            state: vec![],
            expire: None,
            timestamp: 10,
            tx_type: norn_common::types::TransactionType::Native,
            access_list: vec![],
            public_key: PublicKey::default(),
            signature: [0; 64],
            transaction_id: norn_common::types::TransactionId::default(),
        };

        sign_transaction_v2(&keypair, &mut tx).unwrap();
        verify_transaction_v2(&tx).unwrap();
        assert_ne!(tx.id(), Hash::default());
    }
}
