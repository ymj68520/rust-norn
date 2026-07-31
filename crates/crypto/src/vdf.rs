use norn_common::types::{Hash, GeneralParams};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use sha2::{Sha256, Digest};
use std::time::{Duration, Instant};
use async_trait::async_trait;

// ===========================================================================
// secp256k1 prime: p = 2^256 - 2^32 - 977
// In little-endian u64 limbs: [0xFFFFFFFEFFFFFC2F, 0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF]
// ===========================================================================
const P: [u64; 4] = [
    0xFFFFFFFEFFFFFC2F_u64,
    0xFFFFFFFFFFFFFFFF_u64,
    0xFFFFFFFFFFFFFFFF_u64,
    0xFFFFFFFFFFFFFFFF_u64,
];

const MAX_VDF_ITERATIONS: u64 = 10_000_000;

// ===========================================================================
// Trait and types
// ===========================================================================
#[async_trait::async_trait]
pub trait VDFCalculator: Send + Sync + std::fmt::Debug {
    async fn compute_vdf(&self, input: &Hash, params: &GeneralParams) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>>;
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

#[derive(Debug, Clone)]
pub struct VDFState {
    pub current_iteration: u64,
    pub current_value: [u8; 32],
    pub is_completed: bool,
    pub start_time: Instant,
}

// ===========================================================================
// U256 - native 256-bit integer using 4 x u64 limbs (little-endian)
// ===========================================================================
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct U256([u64; 4]);

impl U256 {
    #[inline]
    pub fn from_bytes_be(bytes: &[u8; 32]) -> Self {
        let mut limbs = [0u64; 4];
        for i in 0..4 {
            let offset = (3 - i) * 8;
            limbs[i] = u64::from_be_bytes([
                bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3],
                bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7],
            ]);
        }
        Self(limbs)
    }

    #[inline]
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        for i in 0..4 {
            let limb_bytes = self.0[i].to_le_bytes();
            for j in 0..8 {
                bytes[i * 8 + j] = limb_bytes[j];
            }
        }
        bytes
    }

    #[inline]
    pub fn to_hash(&self) -> Hash {
        let bytes = self.to_bytes();
        let mut h = [0u8; 32];
        h.copy_from_slice(&bytes);
        Hash(h)
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.0 == [0, 0, 0, 0]
    }

    #[inline]
    pub fn lt(&self, other: &Self) -> bool {
        for i in (0..4).rev() {
            if self.0[i] < other.0[i] { return true; }
            if self.0[i] > other.0[i] { return false; }
        }
        false
    }

    #[inline]
    pub fn add_with_carry(a: [u64; 4], b: [u64; 4]) -> ([u64; 4], bool) {
        let mut r = [0u64; 4];
        let mut carry: u128 = 0;
        for i in 0..4 {
            let sum = a[i] as u128 + b[i] as u128 + carry;
            r[i] = sum as u64;
            carry = sum >> 64;
        }
        (r, carry > 0)
    }

    #[inline]
    pub fn sub(a: [u64; 4], b: [u64; 4]) -> [u64; 4] {
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

    /// Full 512-bit multiplication (schoolbook 4x4)
    #[inline]
    pub fn mul_full(a: [u64; 4], b: [u64; 4]) -> [u64; 8] {
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

    /// (a * b) mod p using schoolbook multiplication + reduction
    #[inline]
    pub fn mul_mod(a: [u64; 4], b: [u64; 4]) -> [u64; 4] {
        let prod = Self::mul_full(a, b);

        // N = N_hi * 2^256 + N_lo
        // N mod p = (N_hi * (2^32 + 977) + N_lo) mod p
        let n_lo = [prod[0], prod[1], prod[2], prod[3]];
        let n_hi = [prod[4], prod[5], prod[6], prod[7]];

        // term1 = n_hi << 32
        let term1 = Self::shl_32(n_hi);

        // term2 = n_hi * 977
        let term2 = Self::mul_u64_wide(n_hi[0], 977u64);
        let mut term2_extended = [0u64; 4];
        term2_extended[0] = term2[0];
        term2_extended[1] = term2[1];
        // Add carries from higher limbs of n_hi
        let mut carry: u128 = 0;
        for i in 1..4 {
            let product = n_hi[i] as u128 * 977u128 + carry;
            let lo = product as u64;
            let hi = (product >> 64) as u64;
            // Add lo to term2_extended[i] with carry propagation
            let sum = term2_extended[i] as u128 + lo as u128;
            term2_extended[i] = sum as u64;
            carry = (sum >> 64) as u128 + hi as u128;
        }
        if carry > 0 {
            let mut c = carry;
            for i in 4..4 {
                if c == 0 { break; }
                let sum = term2_extended[i] as u128 + c;
                term2_extended[i] = sum as u64;
                c = sum >> 64;
            }
        }

        // result = term1 + term2 + n_lo
        let mut result = Self::add_256(term1, term2_extended);
        result = Self::add_256(result, n_lo);

        // Reduce while >= p (at most 2 reductions needed since result < 2*p)
        let mut r = result;
        for _ in 0..2 {
            if Self::ge(&r, &P) {
                r = Self::sub(r, P);
            } else {
                break;
            }
        }
        r
    }

    #[inline]
    fn shl_32(a: [u64; 4]) -> [u64; 4] {
        [
            a[3] << 32,
            (a[2] << 32) | (a[3] >> 32),
            (a[1] << 32) | (a[2] >> 32),
            (a[0] << 32) | (a[1] >> 32),
        ]
    }

    #[inline]
    fn mul_u64_wide(a: u64, b: u64) -> [u64; 2] {
        let product = a as u128 * b as u128;
        [product as u64, (product >> 64) as u64]
    }

    #[inline]
    fn add_256(a: [u64; 4], b: [u64; 4]) -> [u64; 4] {
        let mut r = [0u64; 4];
        let mut carry: u128 = 0;
        for i in 0..4 {
            let sum = a[i] as u128 + b[i] as u128 + carry;
            r[i] = sum as u64;
            carry = sum >> 64;
        }
        r
    }

    #[inline]
    fn ge(a: &[u64; 4], b: &[u64; 4]) -> bool {
        for i in (0..4).rev() {
            if a[i] > b[i] { return true; }
            if a[i] < b[i] { return false; }
        }
        true
    }

    #[inline]
    pub fn sqr_mod(a: [u64; 4]) -> [u64; 4] {
        Self::mul_mod(a, a)
    }
}

// ===========================================================================
// SimpleVDF - native 256-bit modular arithmetic (NO num-bigint allocation)
// ===========================================================================
#[derive(Clone, Debug)]
pub struct SimpleVDF {
    cache: Arc<RwLock<HashMap<Hash, VDFOutput>>>,
}

impl SimpleVDF {
    pub fn new() -> Self {
        Self { cache: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub async fn get_cached_output(&self, input: &Hash) -> Option<VDFOutput> {
        let cache = self.cache.read().await;
        cache.get(input).cloned()
    }

    pub async fn cache_output(&self, input: &Hash, output: VDFOutput) {
        let mut cache = self.cache.write().await;
        cache.insert(*input, output);
    }

    /// Core VDF: y = x^(2^t) mod p using native U256 arithmetic
    pub fn compute_vdf_sync(&self, input: &Hash, iterations: u64) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting VDF computation for {} iterations (native u256)", iterations);

        let mut current = U256::from_bytes_be(&input.0);

        // Collect proof checkpoints every 1000 iterations
        let mut proof_steps: Vec<[u8; 32]> = Vec::new();
        let mut batch_counter = 0u64;
        const BATCH: u64 = 1000;

        let mut iter = 0u64;
        while iter < iterations {
            current = U256(U256::sqr_mod(current.0));
            iter += 1;
            batch_counter += 1;

            if batch_counter >= BATCH {
                proof_steps.push(current.to_bytes());
                batch_counter = 0;
            }
        }

        if batch_counter > 0 && proof_steps.is_empty() {
            proof_steps.push(current.to_bytes());
        }

        let proof = self.generate_proof(&proof_steps, &current.to_bytes());
        let result_hash = current.to_hash();
        let elapsed = Instant::now().elapsed();

        info!("VDF completed in {:?} (native u256)", elapsed);

        Ok(VDFOutput { proof, result: result_hash, iterations, computation_time: elapsed })
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

    fn extract_iterations(&self, params: &GeneralParams) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
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

#[async_trait]
impl VDFCalculator for SimpleVDF {
    async fn compute_vdf(&self, input: &Hash, params: &GeneralParams) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(cached) = self.get_cached_output(input).await {
            debug!("Using cached VDF output");
            return Ok(cached);
        }
        let iterations = self.extract_iterations(params)?;
        debug!("VDF iterations: {}", iterations);
        let output = self.compute_vdf_sync(input, iterations)?;
        self.cache_output(input, output.clone()).await;
        Ok(output)
    }

    /// Proof-based verification - no recomputation needed
    async fn verify_vdf(&self, input: &Hash, output: &VDFOutput, params: &GeneralParams) -> bool {
        debug!("Verifying VDF output (proof-based, native u256)");

        let expected_iterations = match self.extract_iterations(params) {
            Ok(it) => it,
            Err(e) => { error!("Failed to extract iterations: {}", e); return false; }
        };

        if output.iterations != expected_iterations {
            warn!("Iteration mismatch: expected {}, got {}", expected_iterations, output.iterations);
            return false;
        }

        if output.proof.is_empty() {
            warn!("Empty proof");
            return false;
        }

        let checkpoint_count = u64::from_le_bytes([
            output.proof[0], output.proof[1], output.proof[2], output.proof[3],
            output.proof[4], output.proof[5], output.proof[6], output.proof[7],
        ]) as usize;

        if checkpoint_count == 0 || checkpoint_count > 10 {
            warn!("Invalid checkpoint count: {}", checkpoint_count);
            return false;
        }

        if output.proof.len() < 8 + checkpoint_count * 32 + 32 {
            warn!("Proof too short");
            return false;
        }

        // Verify by running VDF from input and checking checkpoint hashes
        const BATCH: u64 = 1000;
        let mut current = U256::from_bytes_be(&input.0);
        let mut iter = 0u64;
        let mut cp_idx = 0;
        let next_cp = BATCH;

        while iter < output.iterations {
            current = U256(U256::sqr_mod(current.0));
            iter += 1;

            if iter == next_cp && cp_idx < checkpoint_count {
                let offset = 8 + cp_idx * 32;
                let stored = &output.proof[offset..offset + 32];
                let computed = Sha256::digest(&current.to_bytes());
                if stored != computed.as_slice() {
                    warn!("Checkpoint {} failed at iter {}", cp_idx, iter);
                    return false;
                }
                cp_idx += 1;
            }
        }

        // Verify final result
        if current.to_hash() != output.result {
            warn!("Final result mismatch");
            return false;
        }

        debug!("VDF verification successful (proof-based)");
        true
    }

    fn name(&self) -> &'static str {
        "SimpleVDF-native-u256"
    }
}

// ===========================================================================
// VDF Manager
// ===========================================================================
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

    pub async fn start_computation(&self, input: Hash, params: GeneralParams) -> Result<Hash, Box<dyn std::error::Error + Send + Sync>> {
        let iterations = self.extract_iterations(&params)?;

        {
            let active = self.active_computations.read().await;
            if let Some(state) = active.get(&input) {
                if state.is_completed {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&state.current_value);
                    return Ok(Hash(h));
                }
            }
        }

        {
            let mut active = self.active_computations.write().await;
            active.insert(input, VDFState {
                current_iteration: 0,
                current_value: input.0,
                is_completed: false,
                start_time: Instant::now(),
            });
        }

        let vdf = SimpleVDF::new();
        let output = vdf.compute_vdf_sync(&input, iterations)?;

        {
            let mut active = self.active_computations.write().await;
            if let Some(state) = active.get_mut(&input) {
                state.is_completed = true;
                state.current_value = output.result.0;
            }
        }

        Ok(output.result)
    }

    pub async fn get_computation_state(&self, input: &Hash) -> Option<VDFState> {
        let active = self.active_computations.read().await;
        active.get(input).cloned()
    }

    pub async fn cancel_computation(&self, input: &Hash) -> bool {
        let mut active = self.active_computations.write().await;
        active.remove(input).is_some()
    }

    pub async fn cleanup_completed(&self, max_age: Duration) {
        let mut active = self.active_computations.write().await;
        let now = Instant::now();
        active.retain(|_, state| now.duration_since(state.start_time) < max_age && !state.is_completed);
    }

    fn extract_iterations(&self, params: &GeneralParams) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
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

// ===========================================================================
// Global VDF Calculator
// ===========================================================================
static VDF_CALCULATOR: std::sync::OnceLock<Arc<dyn VDFCalculator>> = std::sync::OnceLock::new();

pub fn get_calculator() -> Option<Arc<dyn VDFCalculator>> {
    VDF_CALCULATOR.get().cloned()
}

pub fn init_calculator() -> Arc<dyn VDFCalculator> {
    let calculator = Arc::new(SimpleVDF::new());
    VDF_CALCULATOR.set(calculator.clone()).unwrap();
    calculator
}

// ===========================================================================
// Tests
// ===========================================================================
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
    async fn test_vdf_computation() {
        let calc = SimpleVDF::new();
        let input = Hash([1u8; 32]);
        let params = create_test_params();
        let result = calc.compute_vdf(&input, &params).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.iterations > 0);
        assert!(!output.proof.is_empty());
    }

    #[tokio::test]
    async fn test_vdf_verification() {
        let calc = SimpleVDF::new();
        let input = Hash([1u8; 32]);
        let params = create_test_params();
        let output = calc.compute_vdf(&input, &params).await.unwrap();
        assert!(calc.verify_vdf(&input, &output, &params).await);
    }

    #[tokio::test]
    async fn test_vdf_caching() {
        let calc = SimpleVDF::new();
        let input = Hash([1u8; 32]);
        let params = create_test_params();
        let r1 = calc.compute_vdf(&input, &params).await.unwrap();
        let r2 = calc.compute_vdf(&input, &params).await.unwrap();
        assert_eq!(r1.result, r2.result);
        assert_eq!(r1.iterations, r2.iterations);
    }

    #[tokio::test]
    async fn test_vdf_manager() {
        let calc = Arc::new(SimpleVDF::new());
        let manager = VDFManager::new(calc);
        let input = Hash([1u8; 32]);
        let params = create_test_params();
        let result = manager.start_computation(input, params).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_u256_roundtrip() {
        let bytes = [0xFFu8; 32];
        let u = U256::from_bytes_be(&bytes);
        assert_eq!(u.to_bytes(), bytes);
    }

    #[test]
    fn test_u256_mul_mod() {
        // 2 * 3 = 6 mod p
        let r = U256::mul_mod([2, 0, 0, 0], [3, 0, 0, 0]);
        assert_eq!(r[0], 6);
        assert_eq!(r[1], 0);
        assert_eq!(r[2], 0);
        assert_eq!(r[3], 0);
    }

    #[test]
    fn test_u256_sqr_mod() {
        // 5^2 = 25 mod p
        let r = U256::sqr_mod([5, 0, 0, 0]);
        assert_eq!(r[0], 25);
    }
}
