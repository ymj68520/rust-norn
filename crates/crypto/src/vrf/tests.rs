#[cfg(test)]
mod tests {
    use crate::vrf::{vrf_calculate, vrf_verify};
    use hex;

    #[test]
    fn test_vrf_prove_verify_flow() {
        // Since we implemented custom VRF logic matching Go, we need to test it end-to-end.
        // We need a valid private key hex.
        // P-256 private key is 32 bytes.
        let priv_key_bytes = [1u8; 32]; // Not secure, but valid scalar?
        let priv_key_hex = hex::encode(priv_key_bytes);
        
        let msg = b"test message";
        
        let res = vrf_calculate(&priv_key_hex, msg);
        assert!(res.is_ok());
        
        let (v, s, t) = res.unwrap();
        
        // Derive public key from private key manually for verify?
        // Or use p256 to get it.
        use p256::SecretKey;
        use p256::elliptic_curve::sec1::ToEncodedPoint;
        let sk = SecretKey::from_slice(&priv_key_bytes).unwrap();
        let pk = sk.public_key();
        let pk_bytes = pk.to_encoded_point(true).as_bytes().to_vec();
        
        let valid = vrf_verify(&pk_bytes, msg, &s, &t, &v);
        assert!(valid.is_ok());
        assert!(valid.unwrap());
    }
}
