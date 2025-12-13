use anyhow::{Result, anyhow};
use p256::{ecdsa::{Signature, SigningKey, VerifyingKey}, ecdsa::signature::{Signer, Verifier, SignatureEncoding}};
use sha2::{Sha256, Digest};
use rand_core::OsRng;
use hex;

/// VRF proof and output structure
#[derive(Debug, Clone)]
pub struct VRFProof {
    /// Proof (signature)
    pub proof: Vec<u8>,
    /// VRF output (seed)
    pub output: Vec<u8>,
    /// Verification key
    pub verification_key: Vec<u8>,
}

/// Calculate VRF proof and output using ECDSA P-256
/// Returns (proof, output, verification_key)
pub fn vrf_calculate(priv_key_hex: &str, message: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    // Parse private key from hex
    let mut key_bytes = [0u8; 32];
    let hex_bytes = hex::decode(priv_key_hex)
        .map_err(|_| anyhow!("Invalid hex private key"))?;

    if hex_bytes.len() != 32 {
        return Err(anyhow!("Private key must be 32 bytes"));
    }

    key_bytes.copy_from_slice(&hex_bytes);

    // Create signing key
    let signing_key = SigningKey::from_bytes(&key_bytes.into())
        .map_err(|_| anyhow!("Invalid private key"))?;

    // Get verification key
    let verifying_key = signing_key.verifying_key();
    let vk_bytes = verifying_key.to_encoded_point(true).to_bytes().to_vec(); // Compressed format

    // Generate VRF output using HMAC-like construction
    let mut hasher = Sha256::new();
    hasher.update(message);
    hasher.update(vk_bytes.as_slice());

    // Sign the message hash
    let message_hash = hasher.finalize();
    let signature: Signature = signing_key.sign(&message_hash);

    // Convert signature to DER format for VRF proof
    let proof = signature.to_der().to_vec();

    // VRF output is SHA256 of message || signature || verification_key
    let mut vrf_hasher = Sha256::new();
    vrf_hasher.update(message);
    vrf_hasher.update(&proof);
    vrf_hasher.update(&vk_bytes);
    let output = vrf_hasher.finalize().to_vec();

    Ok((proof, output, vk_bytes))
}

/// Verify VRF proof and output
/// Returns true if verification is successful
pub fn vrf_verify(
    pub_key_bytes: &[u8],
    message: &[u8],
    proof: &[u8],     // VRF proof (signature)
    output: &[u8],    // Expected VRF output
    verification_key: &[u8], // Verification key (for consistency)
) -> Result<bool> {
    // Parse verification key
    let verifying_key = VerifyingKey::from_sec1_bytes(verification_key)
        .map_err(|_| anyhow!("Invalid verification key"))?;

    // Check that the provided public key matches the verification key
    let provided_vk = VerifyingKey::from_sec1_bytes(pub_key_bytes)
        .map_err(|_| anyhow!("Invalid public key"))?;

    if provided_vk.to_encoded_point(true) != verifying_key.to_encoded_point(true) {
        return Ok(false);
    }

    // Parse signature
    let signature = Signature::from_der(proof)
        .map_err(|_| anyhow!("Invalid signature format"))?;

    // Compute message hash (same as in vrf_calculate)
    let mut hasher = Sha256::new();
    hasher.update(message);
    hasher.update(verification_key);
    let message_hash = hasher.finalize();

    // Verify signature
    if verifying_key.verify(&message_hash, &signature).is_err() {
        return Ok(false);
    }

    // Verify VRF output
    let mut vrf_hasher = Sha256::new();
    vrf_hasher.update(message);
    vrf_hasher.update(proof);
    vrf_hasher.update(verification_key);
    let expected_output = vrf_hasher.finalize().to_vec();

    Ok(expected_output.as_slice() == output)
}

/// Generate a new VRF key pair
pub fn generate_vrf_keypair() -> Result<(String, Vec<u8>)> {
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let priv_key_hex = hex::encode(signing_key.to_bytes());
    let pub_key_bytes = verifying_key.to_encoded_point(true).to_bytes().to_vec();

    Ok((priv_key_hex, pub_key_bytes))
}

/// Check if VRF output meets a threshold (for PoVF consensus)
/// Returns true if the VRF output is below the threshold
pub fn vrf_check_threshold(vrf_output: &[u8], threshold: f64) -> Result<bool> {
    if vrf_output.len() < 8 {
        return Err(anyhow!("VRF output too short"));
    }

    // Take first 8 bytes as a 64-bit integer
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&vrf_output[..8]);
    let value = u64::from_be_bytes(bytes);

    // Normalize to [0, 1) by dividing by 2^64
    let normalized = value as f64 / (u64::MAX as f64 + 1.0);

    Ok(normalized < threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vrf_generate_keypair() {
        let (priv_key, pub_key) = generate_vrf_keypair().unwrap();
        assert_eq!(priv_key.len(), 64); // 32 bytes * 2 hex chars
        assert_eq!(pub_key.len(), 33); // Compressed format
    }

    #[test]
    fn test_vrf_calculate_and_verify() {
        let (priv_key, pub_key) = generate_vrf_keypair().unwrap();
        let message = b"Hello, VRF!";

        let (proof, output, verification_key) = vrf_calculate(&priv_key, message).unwrap();

        // Verify with the public key from keypair
        let result = vrf_verify(&pub_key, message, &proof, &output, &verification_key).unwrap();
        assert!(result, "VRF verification should succeed");

        // Verify with wrong message should fail
        let result = vrf_verify(&pub_key, b"Wrong message", &proof, &output, &verification_key).unwrap();
        assert!(!result, "VRF verification with wrong message should fail");

        // Verify with wrong public key should fail
        let (_, wrong_pub_key) = generate_vrf_keypair().unwrap();
        let result = vrf_verify(&wrong_pub_key, message, &proof, &output, &verification_key).unwrap();
        assert!(!result, "VRF verification with wrong public key should fail");
    }

    #[test]
    fn test_vrf_threshold() {
        let (priv_key, _) = generate_vrf_keypair().unwrap();
        let message = b"Test threshold";

        let (_, output, _) = vrf_calculate(&priv_key, message).unwrap();

        // Very high threshold (99.9%) should always pass
        assert!(vrf_check_threshold(&output, 0.999).unwrap());

        // Very low threshold (0.001%) might fail
        let _result = vrf_check_threshold(&output, 0.00001).unwrap();
        // We can't assert the value since it's random
    }

    #[test]
    fn test_vrf_deterministic_output() {
        let (priv_key, pub_key) = generate_vrf_keypair().unwrap();
        let message = b"Deterministic test";

        let (proof1, output1, vk1) = vrf_calculate(&priv_key, message).unwrap();
        let (proof2, output2, vk2) = vrf_calculate(&priv_key, message).unwrap();

        // Output should be deterministic with same key and message
        assert_eq!(output1, output2, "VRF output should be deterministic");
        assert_eq!(vk1, vk2, "Verification key should be the same");

        // But signatures (proofs) should be different due to randomness
        // Note: In ECDSA, signatures are deterministic if using RFC 6979
        // This test might need adjustment based on the actual implementation
        let verify1 = vrf_verify(&pub_key, message, &proof1, &output1, &vk1).unwrap();
        let verify2 = vrf_verify(&pub_key, message, &proof2, &output2, &vk2).unwrap();

        assert!(verify1 && verify2, "Both proofs should verify successfully");
    }
}
