use p256::ecdsa::{SigningKey, VerifyingKey, Signature, signature::Signer};
use p256::ecdsa::signature::Verifier;
use rand_core::OsRng;
use thiserror::Error;
use std::str::FromStr;

#[derive(Error, Debug)]
pub enum EcdsaError {
    #[error("Invalid Hex String")]
    HexError(#[from] hex::FromHexError),
    #[error("Invalid Key")]
    KeyError,
    #[error("Signature Verification Failed")]
    VerificationFailed,
}

pub struct KeyPair {
    signing_key: SigningKey,
}

impl KeyPair {
    pub fn random() -> Self {
        let signing_key = SigningKey::random(&mut OsRng);
        Self { signing_key }
    }

    pub fn from_private_key_hex(hex_str: &str) -> Result<Self, EcdsaError> {
        let bytes = hex::decode(hex_str)?;
        let signing_key = SigningKey::from_slice(&bytes).map_err(|_| EcdsaError::KeyError)?;
        Ok(Self { signing_key })
    }

    pub fn public_key(&self) -> VerifyingKey {
        *self.signing_key.verifying_key()
    }
    
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key().to_encoded_point(true).as_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let signature: Signature = self.signing_key.sign(msg);
        signature.to_vec()
    }
}

pub fn verify(public_key_bytes: &[u8], msg: &[u8], signature_bytes: &[u8]) -> Result<bool, EcdsaError> {
    let public_key = VerifyingKey::from_sec1_bytes(public_key_bytes).map_err(|_| EcdsaError::KeyError)?;
    let signature = Signature::from_der(signature_bytes).or_else(|_| Signature::from_slice(signature_bytes)).map_err(|_| EcdsaError::KeyError)?;
    
    Ok(public_key.verify(msg, &signature).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_verify() {
        let pair = KeyPair::random();
        let msg = b"hello world";
        let sig = pair.sign(msg);
        
        let pk_bytes = pair.public_key().to_encoded_point(true).as_bytes().to_vec();
        assert!(verify(&pk_bytes, msg, &sig).unwrap());
    }
}
