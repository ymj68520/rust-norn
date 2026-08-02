//! VRF (Verifiable Random Function) Module using schnorrkel (Ristretto255 + Merlin Transcript)
//!
//! Implements VRF signing, verification, and domain-separated score/randomness derivation
//! according to the Norn security and consensus specification.

use anyhow::{anyhow, Result};
use merlin::Transcript;
use norn_common::types::{
    ChainId, Hash, ProtocolVersion, StakeSnapshotHash, ValidatorId, VrfPublicKey,
};
use schnorrkel::vrf::{VRFInOut, VRFPreOut, VRFProof};
use schnorrkel::{Keypair, PublicKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Struct containing context fields for VRF domain binding
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrfContext {
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub height: u64,
    pub round: u32,
    pub parent_block_hash: Hash,
    pub stake_snapshot_hash: StakeSnapshotHash,
    pub validator_id: ValidatorId,
}

impl VrfContext {
    pub fn build_transcript(&self) -> Transcript {
        let mut t = Transcript::new(b"NORN_VRF_V2");
        t.append_message(b"protocol_version", &self.protocol_version.0.to_be_bytes());
        t.append_message(b"chain_id", &self.chain_id.0 .0);
        t.append_message(b"epoch", &self.epoch.to_be_bytes());
        t.append_message(b"height", &self.height.to_be_bytes());
        t.append_message(b"round", &self.round.to_be_bytes());
        t.append_message(b"parent_block_hash", &self.parent_block_hash.0);
        t.append_message(b"stake_snapshot_hash", &self.stake_snapshot_hash.0);
        t.append_message(b"validator_id", &self.validator_id.0);
        t
    }
}

/// Helper function to build a generic transcript for ad-hoc messages
pub fn build_message_transcript(message: &[u8]) -> Transcript {
    let mut t = Transcript::new(b"NORN_VRF_V2_GENERIC");
    t.append_message(b"message", message);
    t
}

/// VRF Key Pair wrapping Schnorrkel Keypair
#[derive(Clone)]
pub struct VRFKeyPair {
    keypair: Keypair,
}

impl fmt::Debug for VRFKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "VRFKeyPair(public: {:?})",
            hex::encode(self.public_key_bytes())
        )
    }
}

impl VRFKeyPair {
    /// Validate a serialized Schnorrkel public key without requiring the
    /// corresponding private key. Genesis loading uses this to fail closed on
    /// malformed validator records.
    pub fn validate_public_key_bytes(bytes: &[u8; 32]) -> Result<()> {
        PublicKey::from_bytes(bytes)
            .map(|_| ())
            .map_err(|e| anyhow!("Invalid Schnorrkel VRF public key: {:?}", e))
    }

    /// Generate a new random VRF KeyPair
    pub fn generate() -> Self {
        let keypair = Keypair::generate_with(&mut rand::thread_rng());
        Self { keypair }
    }

    /// Generate VRF KeyPair from seed bytes
    pub fn from_seed(seed: &[u8]) -> Self {
        use sha2::{Digest, Sha512};
        let mut hasher = Sha512::new();
        hasher.update(b"NORN_VRF_SEED");
        hasher.update(seed);
        let hash = hasher.finalize();
        let mini_secret = schnorrkel::MiniSecretKey::from_bytes(&hash[..32])
            .expect("32 bytes is valid MiniSecretKey");
        let secret = mini_secret.expand(schnorrkel::ExpansionMode::Ed25519);
        let keypair = secret.to_keypair();
        Self { keypair }
    }

    /// Generate VRF KeyPair from 32-byte secret key bytes
    pub fn from_secret_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let mini_secret = schnorrkel::MiniSecretKey::from_bytes(bytes)
            .map_err(|e| anyhow!("Invalid MiniSecretKey bytes: {:?}", e))?;
        let secret = mini_secret.expand(schnorrkel::ExpansionMode::Ed25519);
        let keypair = secret.to_keypair();
        Ok(Self { keypair })
    }

    /// Generate VRF KeyPair from 64-byte secret key bytes
    pub fn from_secret_key_bytes(bytes: &[u8; 64]) -> Result<Self> {
        let secret = schnorrkel::SecretKey::from_bytes(bytes)
            .map_err(|e| anyhow!("Invalid SecretKey bytes: {:?}", e))?;
        let keypair = secret.to_keypair();
        Ok(Self { keypair })
    }

    /// Get 32-byte public key
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.keypair.public.to_bytes()
    }

    /// Get VrfPublicKey wrapper
    pub fn vrf_public_key(&self) -> VrfPublicKey {
        VrfPublicKey(self.public_key_bytes())
    }

    /// Get 64-byte secret key representation
    pub fn private_key_bytes(&self) -> [u8; 64] {
        self.keypair.secret.to_bytes()
    }

    /// Sign transcript to produce VRF pre-out (32 bytes) and VRF proof (64 bytes)
    pub fn vrf_sign(&self, transcript: Transcript) -> (VRFPreOutBytes, VRFProofBytes) {
        let (inout, proof, _) = self.keypair.vrf_sign(transcript);
        let preout_bytes = inout.output.to_bytes();
        let proof_bytes = proof.to_bytes();
        (VRFPreOutBytes(preout_bytes), VRFProofBytes(proof_bytes))
    }
}

/// 32-byte VRF PreOut bytes wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VRFPreOutBytes(pub [u8; 32]);

impl Serialize for VRFPreOutBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for VRFPreOutBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("Invalid VRFPreOutBytes length"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(VRFPreOutBytes(arr))
    }
}

/// 64-byte VRF Proof bytes wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VRFProofBytes(pub [u8; 64]);

impl Default for VRFProofBytes {
    fn default() -> Self {
        Self([0u8; 64])
    }
}

impl Serialize for VRFProofBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for VRFProofBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::custom("Invalid VRFProofBytes length"));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&bytes);
        Ok(VRFProofBytes(arr))
    }
}

/// Serde-compatible complete VRF Output struct
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VRFOutputData {
    pub preout: VRFPreOutBytes,
    pub proof: VRFProofBytes,
    pub output_bytes: [u8; 32],
}

/// Verify a VRF signature and derive VRFInOut
pub fn verify_vrf(
    pub_key_bytes: &[u8; 32],
    transcript: Transcript,
    preout_bytes: &VRFPreOutBytes,
    proof_bytes: &VRFProofBytes,
) -> Result<VRFInOut> {
    let public_key = PublicKey::from_bytes(pub_key_bytes)
        .map_err(|e| anyhow!("Invalid public key bytes: {:?}", e))?;

    let preout = VRFPreOut::from_bytes(&preout_bytes.0)
        .map_err(|e| anyhow!("Invalid VRF preout bytes: {:?}", e))?;
    let proof = VRFProof::from_bytes(&proof_bytes.0)
        .map_err(|e| anyhow!("Invalid VRF proof bytes: {:?}", e))?;

    // Note: vrf_verify uses non-malleable public key binding by default (NOT Malleable)
    let (vrf_inout, _) = public_key
        .vrf_verify(transcript, &preout, &proof)
        .map_err(|e| anyhow!("VRF verification failed: {:?}", e))?;

    Ok(vrf_inout)
}

/// Derive 256-bit score bytes for proposer selection using VRFInOut::make_bytes
pub fn derive_vrf_score_bytes(vrf_inout: &VRFInOut) -> [u8; 32] {
    vrf_inout.make_bytes::<[u8; 32]>(b"NORN_VRF_SCORE_V2")
}

/// Derive 256-bit randomness seed bytes using VRFInOut::make_bytes
pub fn derive_vrf_randomness_bytes(vrf_inout: &VRFInOut) -> [u8; 32] {
    vrf_inout.make_bytes::<[u8; 32]>(b"NORN_VRF_RANDOMNESS_V2")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedVrfOutput {
    pub score: [u8; 32],
    pub randomness: [u8; 32],
}

pub fn verify_and_derive(
    pub_key_bytes: &[u8; 32],
    context: &VrfContext,
    preout_bytes: &VRFPreOutBytes,
    proof_bytes: &VRFProofBytes,
) -> Result<VerifiedVrfOutput> {
    let transcript = context.build_transcript();
    let vrf_inout = verify_vrf(pub_key_bytes, transcript, preout_bytes, proof_bytes)?;
    let score = derive_vrf_score_bytes(&vrf_inout);
    let randomness = derive_vrf_randomness_bytes(&vrf_inout);
    Ok(VerifiedVrfOutput { score, randomness })
}

/// High-level VRF Calculator
pub struct VRFCalculator;

impl VRFCalculator {
    pub fn calculate(key_pair: &VRFKeyPair, message: &[u8]) -> Result<VRFOutputData> {
        let transcript = build_message_transcript(message);
        let (preout, proof) = key_pair.vrf_sign(transcript.clone());
        let vrf_inout = verify_vrf(&key_pair.public_key_bytes(), transcript, &preout, &proof)?;
        let score_bytes = derive_vrf_score_bytes(&vrf_inout);

        Ok(VRFOutputData {
            preout,
            proof,
            output_bytes: score_bytes,
        })
    }

    pub fn calculate_with_context(
        key_pair: &VRFKeyPair,
        context: &VrfContext,
    ) -> Result<VRFOutputData> {
        let transcript = context.build_transcript();
        let (preout, proof) = key_pair.vrf_sign(transcript.clone());
        let vrf_inout = verify_vrf(&key_pair.public_key_bytes(), transcript, &preout, &proof)?;
        let score_bytes = derive_vrf_score_bytes(&vrf_inout);

        Ok(VRFOutputData {
            preout,
            proof,
            output_bytes: score_bytes,
        })
    }

    pub fn verify(
        pub_key_bytes: &[u8; 32],
        message: &[u8],
        output: &VRFOutputData,
    ) -> Result<bool> {
        let transcript = build_message_transcript(message);
        match verify_vrf(pub_key_bytes, transcript, &output.preout, &output.proof) {
            Ok(vrf_inout) => {
                let score_bytes = derive_vrf_score_bytes(&vrf_inout);
                Ok(score_bytes == output.output_bytes)
            }
            Err(_) => Ok(false),
        }
    }

    pub fn verify_with_context(
        pub_key_bytes: &[u8; 32],
        context: &VrfContext,
        output: &VRFOutputData,
    ) -> Result<bool> {
        let transcript = context.build_transcript();
        match verify_vrf(pub_key_bytes, transcript, &output.preout, &output.proof) {
            Ok(vrf_inout) => {
                let score_bytes = derive_vrf_score_bytes(&vrf_inout);
                Ok(score_bytes == output.output_bytes)
            }
            Err(_) => Ok(false),
        }
    }
}

/// Lightweight selector registry storing ONLY VrfPublicKey and voting power
pub struct VRFSelector {
    validators: std::collections::BTreeMap<ValidatorId, (VrfPublicKey, u64)>,
}

impl VRFSelector {
    pub fn new() -> Self {
        Self {
            validators: std::collections::BTreeMap::new(),
        }
    }

    pub fn add_validator(&mut self, id: ValidatorId, vrf_pub_key: VrfPublicKey, voting_power: u64) {
        self.validators.insert(id, (vrf_pub_key, voting_power));
    }

    pub fn get_validator(&self, id: &ValidatorId) -> Option<&(VrfPublicKey, u64)> {
        self.validators.get(id)
    }

    pub fn total_voting_power(&self) -> u128 {
        self.validators.values().fold(0u128, |acc, (_, weight)| {
            acc.saturating_add(*weight as u128)
        })
    }

    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }
}

impl Default for VRFSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vrf_keypair_generation() {
        let keypair = VRFKeyPair::generate();
        assert_eq!(keypair.public_key_bytes().len(), 32);
        assert_eq!(keypair.private_key_bytes().len(), 64);
    }

    #[test]
    fn test_vrf_keypair_from_seed() {
        let seed = b"test_seed_for_vrf_schnorrkel";
        let key1 = VRFKeyPair::from_seed(seed);
        let key2 = VRFKeyPair::from_seed(seed);
        assert_eq!(key1.public_key_bytes(), key2.public_key_bytes());
        assert_eq!(key1.private_key_bytes(), key2.private_key_bytes());
    }

    #[test]
    fn test_vrf_calculate_and_verify() {
        let keypair = VRFKeyPair::generate();
        let message = b"Hello VRF Schnorrkel!";

        let output = VRFCalculator::calculate(&keypair, message).unwrap();
        let valid = VRFCalculator::verify(&keypair.public_key_bytes(), message, &output).unwrap();
        assert!(valid);

        let wrong_message = b"Wrong message";
        let invalid =
            VRFCalculator::verify(&keypair.public_key_bytes(), wrong_message, &output).unwrap();
        assert!(!invalid);
    }

    #[test]
    fn test_vrf_domain_separation_make_bytes() {
        let keypair = VRFKeyPair::generate();
        let transcript = build_message_transcript(b"test_domain");
        let (preout, proof) = keypair.vrf_sign(transcript.clone());

        let vrf_inout =
            verify_vrf(&keypair.public_key_bytes(), transcript, &preout, &proof).unwrap();

        let score = derive_vrf_score_bytes(&vrf_inout);
        let randomness = derive_vrf_randomness_bytes(&vrf_inout);

        // NORN_VRF_SCORE_V2 and NORN_VRF_RANDOMNESS_V2 must yield different outputs
        assert_ne!(score, randomness);
    }

    #[test]
    fn test_verify_and_derive_reconstructs_both_outputs() {
        let keypair = VRFKeyPair::from_seed(b"stage-four-vrf");
        let context = VrfContext {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(Hash([1; 32])),
            epoch: 1,
            height: 1,
            round: 0,
            parent_block_hash: Hash([2; 32]),
            stake_snapshot_hash: StakeSnapshotHash([3; 32]),
            validator_id: ValidatorId([4; 32]),
        };
        let output = VRFCalculator::calculate_with_context(&keypair, &context).unwrap();
        let derived = verify_and_derive(
            &keypair.public_key_bytes(),
            &context,
            &output.preout,
            &output.proof,
        )
        .unwrap();
        assert_eq!(derived.score, output.output_bytes);
        assert_ne!(derived.score, derived.randomness);
    }

    #[test]
    fn test_vrf_tampered_proof_fails() {
        let keypair = VRFKeyPair::generate();
        let message = b"Tamper proof test";
        let mut output = VRFCalculator::calculate(&keypair, message).unwrap();

        // Flip a byte in the proof
        output.proof.0[0] ^= 0xFF;
        let valid = VRFCalculator::verify(&keypair.public_key_bytes(), message, &output).unwrap();
        assert!(!valid);
    }
}
