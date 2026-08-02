//! VDF & Delay Benchmark Module
//! 
//! [SECURITY NOTICE]: The `SequentialDelayBenchmark` in this module is based on sequential squaring
//! modulo a known-order prime (secp256k1 prime). IT IS NOT A CRYPTOGRAPHICALLY SECURE VDF
//! AND MUST NOT BE USED FOR CONSENSUS FINALITY OR RANDOMNESS UNPREDICTABILITY IN PRODUCTION NETWORKS.
//! It is provided solely as a performance benchmark and sequential delay demonstrator.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use norn_common::types::{GeneralParams, Hash};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

pub const MIN_VDF_ITERATIONS: u64 = 1_000;
pub const MAX_VDF_ITERATIONS: u64 = 10_000_000;

pub fn validate_iterations(iterations: u64) -> Result<()> {
    if iterations == 0 {
        return Err(anyhow!("VDF iterations cannot be zero"));
    }
    if iterations < MIN_VDF_ITERATIONS || iterations > MAX_VDF_ITERATIONS {
        return Err(anyhow!(
            "VDF iterations {} out of valid range [{}, {}]",
            iterations,
            MIN_VDF_ITERATIONS,
            MAX_VDF_ITERATIONS
        ));
    }
    Ok(())
}

#[async_trait]
pub trait VDFCalculator: Send + Sync + std::fmt::Debug {
    async fn compute_vdf(
        &self,
        input: &Hash,
        params: &GeneralParams,
    ) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>>;
    async fn verify_vdf(&self, input: &Hash, output: &VDFOutput, params: &GeneralParams) -> bool;
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VDFOutput {
    pub proof: Vec<u8>,
    pub result: Hash,
    pub iterations: u64,
    pub computation_time: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VdfCacheKey {
    pub algorithm_version: u16,
    pub modulus_id: Hash,
    pub input_hash: Hash,
    pub iterations: u64,
}

#[derive(Debug, Clone)]
pub struct VDFState {
    pub current_iteration: u64,
    pub current_value: [u8; 32],
    pub is_completed: bool,
    pub start_time: Instant,
}

// Simple U256 for non-cryptographic delay benchmark
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct U256([u64; 4]);

impl U256 {
    pub fn from_bytes_be(bytes: &[u8; 32]) -> Self {
        let mut limbs = [0u64; 4];
        for i in 0..4 {
            let offset = (3 - i) * 8;
            limbs[i] = u64::from_be_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
                bytes[offset + 4],
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
            ]);
        }
        Self(limbs)
    }

    pub fn to_bytes_be(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        for i in 0..4 {
            let offset = (3 - i) * 8;
            let limb_bytes = self.0[i].to_be_bytes();
            bytes[offset..offset + 8].copy_from_slice(&limb_bytes);
        }
        bytes
    }

    pub fn to_hash(&self) -> Hash {
        Hash(self.to_bytes_be())
    }

    pub fn sqr_mod_secp256k1(a: [u64; 4]) -> [u64; 4] {
        // Simple schoolbook 4x4 multiplication & reduction modulo p = 2^256 - 2^32 - 977
        let prod = Self::mul_full(a, a);
        Self::reduce_secp256k1(prod)
    }

    fn mul_full(a: [u64; 4], b: [u64; 4]) -> [u64; 8] {
        let mut r = [0u64; 8];
        for i in 0..4 {
            let mut carry: u128 = 0;
            for j in 0..4 {
                let idx = i + j;
                let product = a[i] as u128 * b[j] as u128 + r[idx] as u128 + carry;
                r[idx] = product as u64;
                carry = product >> 64;
            }
            r[i + 4] = carry as u64;
        }
        r
    }

    fn reduce_secp256k1(prod: [u64; 8]) -> [u64; 4] {
        // secp256k1 prime P: 2^256 - 0x1000003D1
        let n_lo = [prod[0], prod[1], prod[2], prod[3]];
        let n_hi = [prod[4], prod[5], prod[6], prod[7]];

        let term1 = [
            n_hi[3] << 32,
            (n_hi[2] << 32) | (n_hi[3] >> 32),
            (n_hi[1] << 32) | (n_hi[2] >> 32),
            (n_hi[0] << 32) | (n_hi[1] >> 32),
        ];

        let mut term2 = [0u64; 4];
        let mut carry: u128 = 0;
        for i in 0..4 {
            let p = n_hi[i] as u128 * 977u128 + carry;
            term2[i] = p as u64;
            carry = p >> 64;
        }

        let mut res = Self::add(term1, term2);
        res = Self::add(res, n_lo);

        const P: [u64; 4] = [
            0xFFFFFFFEFFFFFC2F_u64,
            0xFFFFFFFFFFFFFFFF_u64,
            0xFFFFFFFFFFFFFFFF_u64,
            0xFFFFFFFFFFFFFFFF_u64,
        ];

        for _ in 0..4 {
            if Self::ge(&res, &P) {
                res = Self::sub(res, P);
            } else {
                break;
            }
        }
        res
    }

    fn add(a: [u64; 4], b: [u64; 4]) -> [u64; 4] {
        let mut r = [0u64; 4];
        let mut carry: u128 = 0;
        for i in 0..4 {
            let sum = a[i] as u128 + b[i] as u128 + carry;
            r[i] = sum as u64;
            carry = sum >> 64;
        }
        r
    }

    fn sub(a: [u64; 4], b: [u64; 4]) -> [u64; 4] {
        let mut r = [0u64; 4];
        let mut borrow: i128 = 0;
        for i in 0..4 {
            let diff = a[i] as i128 - b[i] as i128 - borrow;
            if diff < 0 {
                r[i] = (diff + (1i128 << 64)) as u64;
                borrow = 1;
            } else {
                r[i] = diff as u64;
                borrow = 0;
            }
        }
        r
    }

    fn ge(a: &[u64; 4], b: &[u64; 4]) -> bool {
        for i in (0..4).rev() {
            if a[i] > b[i] {
                return true;
            }
            if a[i] < b[i] {
                return false;
            }
        }
        true
    }
}

/// SequentialDelayBenchmark (Demoted SimpleVDF for performance benchmarking only)
#[derive(Clone, Debug)]
pub struct SequentialDelayBenchmark {
    cache: Arc<RwLock<HashMap<VdfCacheKey, VDFOutput>>>,
}

impl SequentialDelayBenchmark {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_cached_output(&self, key: &VdfCacheKey) -> Option<VDFOutput> {
        let cache = self.cache.read().await;
        cache.get(key).cloned()
    }

    pub async fn cache_output(&self, key: VdfCacheKey, output: VDFOutput) {
        let mut cache = self.cache.write().await;
        cache.insert(key, output);
    }

    pub fn compute_sync(
        &self,
        input: &Hash,
        iterations: u64,
    ) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>> {
        validate_iterations(iterations)?;

        let mut current = U256::from_bytes_be(&input.0);
        let mut proof_steps: Vec<[u8; 32]> = Vec::new();
        let mut batch_counter = 0u64;
        const BATCH: u64 = 1000;

        let start = Instant::now();
        let mut iter = 0u64;
        while iter < iterations {
            current = U256(U256::sqr_mod_secp256k1(current.0));
            iter += 1;
            batch_counter += 1;

            if batch_counter >= BATCH {
                proof_steps.push(current.to_bytes_be());
                batch_counter = 0;
            }
        }

        if batch_counter > 0 && proof_steps.is_empty() {
            proof_steps.push(current.to_bytes_be());
        }

        let proof = self.generate_proof(&proof_steps, &current.to_bytes_be());
        let result_hash = current.to_hash();
        let elapsed = start.elapsed();

        Ok(VDFOutput {
            proof,
            result: result_hash,
            iterations,
            computation_time: elapsed,
        })
    }

    fn generate_proof(&self, steps: &[[u8; 32]], final_value: &[u8; 32]) -> Vec<u8> {
        let mut proof = Vec::with_capacity(8 + steps.len().min(10) * 32 + 32);
        proof.extend_from_slice(&(steps.len().min(10) as u64).to_le_bytes());
        for step in steps.iter().take(10) {
            proof.extend_from_slice(&Sha256::digest(step));
        }
        proof.extend_from_slice(&Sha256::digest(final_value));
        proof
    }

    fn extract_iterations(
        &self,
        params: &GeneralParams,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let iterations = if params.t.len() >= 8 {
            let bytes: [u8; 8] = params.t[..8].try_into().map_err(|_| "Invalid t parameter")?;
            u64::from_le_bytes(bytes)
        } else if !params.t.is_empty() {
            let mut bytes = [0u8; 8];
            bytes[..params.t.len()].copy_from_slice(&params.t);
            u64::from_le_bytes(bytes)
        } else {
            return Err("Invalid time parameter".into());
        };

        validate_iterations(iterations)?;
        Ok(iterations)
    }
}

impl Default for SequentialDelayBenchmark {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VDFCalculator for SequentialDelayBenchmark {
    async fn compute_vdf(
        &self,
        input: &Hash,
        params: &GeneralParams,
    ) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>> {
        let iterations = self.extract_iterations(params)?;
        let cache_key = VdfCacheKey {
            algorithm_version: 1,
            modulus_id: Hash::default(),
            input_hash: *input,
            iterations,
        };

        if let Some(cached) = self.get_cached_output(&cache_key).await {
            return Ok(cached);
        }

        let output = self.compute_sync(input, iterations)?;
        self.cache_output(cache_key, output.clone()).await;
        Ok(output)
    }

    async fn verify_vdf(&self, input: &Hash, output: &VDFOutput, params: &GeneralParams) -> bool {
        let expected_iterations = match self.extract_iterations(params) {
            Ok(it) => it,
            Err(_) => return false,
        };

        if output.iterations != expected_iterations {
            return false;
        }

        // Strict proof length check: must be at least 8 bytes
        if output.proof.len() < 8 {
            return false;
        }

        let checkpoint_count = u64::from_le_bytes([
            output.proof[0],
            output.proof[1],
            output.proof[2],
            output.proof[3],
            output.proof[4],
            output.proof[5],
            output.proof[6],
            output.proof[7],
        ]) as usize;

        if checkpoint_count == 0 || checkpoint_count > 10 {
            return false;
        }

        let expected_len = match checkpoint_count.checked_mul(32) {
            Some(mul) => match 8usize.checked_add(mul).and_then(|v| v.checked_add(32)) {
                Some(l) => l,
                None => return false,
            },
            None => return false,
        };

        // Strict exact length check
        if output.proof.len() != expected_len {
            return false;
        }

        const BATCH: u64 = 1000;
        let mut current = U256::from_bytes_be(&input.0);
        let mut iter = 0u64;
        let mut cp_idx = 0;
        let mut next_cp = BATCH;

        while iter < output.iterations {
            current = U256(U256::sqr_mod_secp256k1(current.0));
            iter += 1;

            if iter == next_cp && cp_idx < checkpoint_count {
                let offset = 8 + cp_idx * 32;
                let stored = &output.proof[offset..offset + 32];
                let computed = Sha256::digest(&current.to_bytes_be());
                if stored != computed.as_slice() {
                    return false;
                }
                cp_idx += 1;
                // Increment next_cp step
                next_cp = match next_cp.checked_add(BATCH) {
                    Some(n) => n,
                    None => break,
                };
            }
        }

        current.to_hash() == output.result
    }

    fn name(&self) -> &'static str {
        "SequentialDelayBenchmark-NonCryptographic"
    }
}

// Backward compatibility alias for VDFManager
pub struct VDFManager {
    calculator: Arc<dyn VDFCalculator>,
    active_computations: Arc<RwLock<HashMap<Hash, VDFState>>>,
}

impl VDFManager {
    pub fn new(calculator: Arc<dyn VDFCalculator>) -> Self {
        Self {
            calculator,
            active_computations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_computation(
        &self,
        input: Hash,
        params: GeneralParams,
    ) -> Result<Hash, Box<dyn std::error::Error + Send + Sync>> {
        let iterations = self.extract_iterations(&params)?;
        validate_iterations(iterations)?;

        let benchmark = SequentialDelayBenchmark::new();
        let output = benchmark.compute_sync(&input, iterations)?;
        Ok(output.result)
    }

    fn extract_iterations(
        &self,
        params: &GeneralParams,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        if params.t.len() >= 8 {
            let bytes: [u8; 8] = params.t[..8].try_into().map_err(|_| "Invalid t parameter")?;
            Ok(u64::from_le_bytes(bytes))
        } else if !params.t.is_empty() {
            let mut bytes = [0u8; 8];
            bytes[..params.t.len()].copy_from_slice(&params.t);
            Ok(u64::from_le_bytes(bytes))
        } else {
            Err("Invalid time parameter".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_params() -> GeneralParams {
        GeneralParams {
            result: vec![],
            random_number: norn_common::types::PublicKey::default(),
            s: vec![],
            t: vec![0xE8, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            proof: vec![],
        }
    }

    #[tokio::test]
    async fn test_delay_benchmark_computation() {
        let calc = SequentialDelayBenchmark::new();
        let input = Hash([1u8; 32]);
        let params = create_test_params();
        let result = calc.compute_vdf(&input, &params).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.iterations >= MIN_VDF_ITERATIONS);
        assert!(!output.proof.is_empty());
    }

    #[tokio::test]
    async fn test_short_proof_does_not_panic() {
        let calc = SequentialDelayBenchmark::new();
        let input = Hash([1u8; 32]);
        let params = create_test_params();
        let mut output = calc.compute_vdf(&input, &params).await.unwrap();

        // Truncate proof to 4 bytes
        output.proof = vec![0x01, 0x02, 0x03, 0x04];
        let valid = calc.verify_vdf(&input, &output, &params).await;
        assert!(!valid);
    }

    #[test]
    fn test_u256_roundtrip_be() {
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = i as u8;
        }
        let u = U256::from_bytes_be(&bytes);
        assert_eq!(u.to_bytes_be(), bytes);
    }
}
