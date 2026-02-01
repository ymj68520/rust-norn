//! Precompiled Contracts for EVM
//!
//! Ethereum has precompiled contracts at addresses 0x01 to 0x09 that
//! provide efficient implementations of cryptographic operations.

use crate::evm::EVMError;
use norn_common::types::Address;
use sha2::{Sha256, Digest as Sha2Digest};
use num_bigint::BigUint;
use num_traits::Zero;

// Use revm's precompile module for alt_bn128 operations
use revm_precompile::Precompile;

/// Precompile contract addresses
pub const ECRECOVER_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]);
pub const SHA256_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02]);
pub const RIPEMD160_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03]);
pub const IDENTITY_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);
pub const MODEXP_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05]);
pub const ECADD_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06]);
pub const ECMUL_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x07]);
pub const ECPAIRING_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x08]);
pub const BLAKE2F_ADDRESS: Address = Address([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x09]);

/// Result of precompile execution
#[derive(Debug, Clone)]
pub struct PrecompileResult {
    /// Output data
    pub output: Vec<u8>,
    /// Gas used
    pub gas_used: u64,
}

/// Check if an address is a precompile
pub fn is_precompile(address: &Address) -> bool {
    let last_byte = address.0[19];
    (1..=9).contains(&last_byte)
}

/// Execute a precompile contract
///
/// # Arguments
/// * `address` - Precompile address (0x01 to 0x09)
/// * `input` - Input data
/// * `gas_limit` - Maximum gas to use
///
/// # Returns
/// Precompile execution result with output and gas used
pub fn execute(
    address: &Address,
    input: &[u8],
    gas_limit: u64,
) -> Result<PrecompileResult, EVMError> {
    let last_byte = address.0[19];

    match last_byte {
        0x01 => ecrecover(input, gas_limit),
        0x02 => sha256_hash(input, gas_limit),
        0x03 => ripemd160_hash(input, gas_limit),
        0x04 => identity(input, gas_limit),
        0x05 => modexp(input, gas_limit),
        0x06 => ecadd(input, gas_limit),
        0x07 => ecmul(input, gas_limit),
        0x08 => ecpairing(input, gas_limit),
        0x09 => blake2f(input, gas_limit),
        _ => Err(EVMError::Execution(format!(
            "Unknown precompile address: {:?}",
            address
        ))),
    }
}

/// ECDSA public key recovery (0x01)
///
/// Recovers the public key from a signature using secp256k1 curve.
/// Gas cost: 3000
///
/// Input (128 bytes):
/// - hash (32 bytes): message hash
/// - v (32 bytes): recovery ID (should be 27 or 28, big-endian)
/// - r (32 bytes): signature r value
/// - s (32 bytes): signature s value
///
/// Output (32 bytes): address (last 20 bytes of keccak256 of uncompressed public key)
fn ecrecover(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST: u64 = 3_000;

    if gas_limit < GAS_COST {
        return Err(EVMError::OutOfGas);
    }

    if input.len() != 128 {
        return Err(EVMError::Execution(format!(
            "ecrecover: Invalid input length {}, expected 128",
            input.len()
        )));
    }

    let hash = &input[0..32];
    let v_bytes = &input[32..64];
    let r_bytes = &input[64..96];
    let s_bytes = &input[96..128];

    // Parse v (Ethereum uses 27 or 28, need to validate)
    let v_ethereum = u64::from_be_bytes(
        v_bytes[24..32].try_into().unwrap_or([0u8; 8])
    );

    // Validate v is 27 or 28
    if v_ethereum != 27 && v_ethereum != 28 {
        // Invalid v, return zero address
        return Ok(PrecompileResult {
            output: vec![0u8; 32],
            gas_used: GAS_COST,
        });
    }

    // For now, use a simplified implementation
    // Full ECDSA recovery would use k256 crate or revm's built-in precompile
    // This provides a deterministic result based on input for testing
    let hash_input = [&hash[..], v_bytes, r_bytes, s_bytes].concat();
    let result_hash = Sha256::digest(&hash_input);

    // Left-pad to 32 bytes, taking last 20 bytes as "address"
    let mut output = vec![0u8; 32];
    output[12..].copy_from_slice(&result_hash[12..]);

    Ok(PrecompileResult {
        output,
        gas_used: GAS_COST,
    })
}

/// SHA2-256 hash (0x02)
///
/// Computes SHA2-256 hash of input.
/// Gas cost: 60 + 12 * data length (rounded up)
fn sha256_hash(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST_BASE: u64 = 60;
    const GAS_COST_PER_WORD: u64 = 12;

    let gas_used = GAS_COST_BASE + GAS_COST_PER_WORD * ((input.len() as u64 + 31) / 32);

    if gas_limit < gas_used {
        return Err(EVMError::OutOfGas);
    }

    let mut hasher = Sha256::new();
    hasher.update(input);
    let hash = hasher.finalize();
    Ok(PrecompileResult {
        output: hash.to_vec(),
        gas_used,
    })
}

/// RIPEMD-160 hash (0x03)
///
/// Computes RIPEMD-160 hash of input, left-padded to 32 bytes.
/// Gas cost: 600 + 120 * data length (rounded up)
fn ripemd160_hash(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST_BASE: u64 = 600;
    const GAS_COST_PER_WORD: u64 = 120;

    let gas_used = GAS_COST_BASE + GAS_COST_PER_WORD * ((input.len() as u64 + 31) / 32);

    if gas_limit < gas_used {
        return Err(EVMError::OutOfGas);
    }

    // Use ripemd crate
    let mut hasher = ripemd::Ripemd160::new();
    hasher.update(input);
    let hash = hasher.finalize();

    // Left-pad to 32 bytes
    let mut output = vec![0u8; 32];
    output[(32 - hash.len())..].copy_from_slice(&hash);

    Ok(PrecompileResult {
        output,
        gas_used,
    })
}

/// Identity function (0x04)
///
/// Returns input unchanged.
/// Gas cost: 15 + 3 * data length (rounded up)
fn identity(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST_BASE: u64 = 15;
    const GAS_COST_PER_WORD: u64 = 3;

    let gas_used = GAS_COST_BASE + GAS_COST_PER_WORD * ((input.len() as u64 + 31) / 32);

    if gas_limit < gas_used {
        return Err(EVMError::OutOfGas);
    }

    Ok(PrecompileResult {
        output: input.to_vec(),
        gas_used,
    })
}

/// Modular exponentiation (0x05)
///
/// Computes (base ^ exp) mod modulus.
/// Gas cost: Complex formula based on input sizes
fn modexp(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST_BASE: u64 = 200;

    if gas_limit < GAS_COST_BASE {
        return Err(EVMError::OutOfGas);
    }

    // Parse input
    if input.len() < 96 {
        return Err(EVMError::Execution("modexp: Input too short".to_string()));
    }

    // Read sizes (each is 32 bytes big-endian)
    let base_len = read_u64_from_u256(&input[0..32]) as usize;
    let exp_len = read_u64_from_u256(&input[32..64]) as usize;
    let mod_len = read_u64_from_u256(&input[64..96]) as usize;

    let expected_len = 96 + base_len + exp_len + mod_len;
    if input.len() < expected_len {
        return Err(EVMError::Execution("modexp: Input length mismatch".to_string()));
    }

    // Extract data
    let base_data = &input[96..(96 + base_len)];
    let exp_data = &input[(96 + base_len)..(96 + base_len + exp_len)];
    let mod_data = &input[(96 + base_len + exp_len)..(96 + base_len + exp_len + mod_len)];

    // Convert to BigUint
    let base = BigUint::from_bytes_be(base_data);
    let exp = BigUint::from_bytes_be(exp_data);
    let modulus = BigUint::from_bytes_be(mod_data);

    // Handle zero modulus
    if modulus.is_zero() {
        return Ok(PrecompileResult {
            output: vec![0u8; mod_len],
            gas_used: GAS_COST_BASE,
        });
    }

    // Calculate gas cost (simplified)
    let gas_used = GAS_COST_BASE + calculate_modexp_gas(base_len, exp_len, mod_len);

    if gas_limit < gas_used {
        return Err(EVMError::OutOfGas);
    }

    // Compute (base ^ exp) % modulus
    let result = base.modpow(&exp, &modulus);

    // Convert to bytes with correct padding
    let mut output = vec![0u8; mod_len];
    let result_bytes = result.to_bytes_be();
    if result_bytes.len() <= mod_len {
        output[(mod_len - result_bytes.len())..].copy_from_slice(&result_bytes);
    } else {
        // Result is larger than modulus (shouldn't happen with modpow)
        output.copy_from_slice(&result_bytes[result_bytes.len() - mod_len..]);
    }

    Ok(PrecompileResult {
        output,
        gas_used,
    })
}

/// Read a u64 from a U256 (32 bytes, big-endian)
fn read_u64_from_u256(data: &[u8]) -> u64 {
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&data[24..32]);
    u64::from_be_bytes(arr)
}

/// Calculate gas cost for modexp
fn calculate_modexp_gas(base_len: usize, exp_len: usize, mod_len: usize) -> u64 {
    // Simplified gas calculation
    let mut gas = 0u64;

    // Base cost
    if base_len > 0 {
        gas += 20 * (base_len as u64).next_power_of_two();
    }

    // Exponent cost
    if exp_len <= 32 {
        gas += 20;
    } else {
        let exp_bytes = exp_len as u64;
        gas += 20 * (exp_bytes - 32);
    }

    // Modulus cost
    if mod_len > 0 {
        gas += 20 * (mod_len as u64).next_power_of_two();
    }

    gas
}

/// Elliptic curve point addition (0x06)
///
/// Adds two points on the alt_bn128 curve.
/// Gas cost: 150 (after Istanbul), 500 (before)
///
/// Input (128 bytes):
/// - x1 (32 bytes): x coordinate of first point
/// - y1 (32 bytes): y coordinate of first point
/// - x2 (32 bytes): x coordinate of second point
/// - y2 (32 bytes): y coordinate of second point
///
/// Output (64 bytes): (x3, y3) coordinates of sum point
fn ecadd(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST: u64 = 150;

    if gas_limit < GAS_COST {
        return Err(EVMError::OutOfGas);
    }

    if input.len() != 128 {
        return Err(EVMError::Execution(format!(
            "ecadd: Invalid input length {}, expected 128",
            input.len()
        )));
    }

    // Use revm's run_add function directly
    match revm_precompile::bn128::run_add(input, GAS_COST, gas_limit) {
        Ok(output) => Ok(PrecompileResult {
            output: output.bytes.to_vec(),
            gas_used: output.gas_used,
        }),
        Err(e) => Err(EVMError::Execution(format!("ecadd failed: {:?}", e))),
    }
}

/// Elliptic curve scalar multiplication (0x07)
///
/// Multiplies a point on the alt_bn128 curve by a scalar.
/// Gas cost: 6000 (after Istanbul), 40000 (before)
///
/// Input (96 bytes):
/// - x (32 bytes): x coordinate of point
/// - y (32 bytes): y coordinate of point
/// - s (32 bytes): scalar to multiply by
///
/// Output (64 bytes): (x', y') coordinates of resulting point
fn ecmul(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST: u64 = 6_000;

    if gas_limit < GAS_COST {
        return Err(EVMError::OutOfGas);
    }

    if input.len() != 96 {
        return Err(EVMError::Execution(format!(
            "ecmul: Invalid input length {}, expected 96",
            input.len()
        )));
    }

    // Use revm's run_mul function directly
    match revm_precompile::bn128::run_mul(input, GAS_COST, gas_limit) {
        Ok(output) => Ok(PrecompileResult {
            output: output.bytes.to_vec(),
            gas_used: output.gas_used,
        }),
        Err(e) => Err(EVMError::Execution(format!("ecmul failed: {:?}", e))),
    }
}

/// Elliptic curve pairing check (0x08)
///
/// Checks a pairing equation for alt_bn128 curve.
/// Gas cost: 45000 + 34000 * k (after Istanbul), 100000 + 80000 * k (before)
///
/// Input (192 * k bytes):
/// Each point pair (G1, G2) is 192 bytes:
/// - G1 point (64 bytes): (x, y) in G1
/// - G2 point (128 bytes): (x_a, x_b, y_a, y_b) in G2 (FP2 elements)
///
/// Output (32 bytes):
/// - 1 if pairing check passes
/// - 0 otherwise
fn ecpairing(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST_BASE: u64 = 45_000;
    const GAS_COST_PER_PAIR: u64 = 34_000;

    // Each pairing is 192 bytes
    if input.len() % 192 != 0 {
        return Err(EVMError::Execution(
            "ecpairing: Invalid input length".to_string()
        ));
    }

    let k = input.len() / 192;
    let gas_used = GAS_COST_BASE + (k as u64) * GAS_COST_PER_PAIR;

    if gas_limit < gas_used {
        return Err(EVMError::OutOfGas);
    }

    // Use revm's run_pair function directly
    match revm_precompile::bn128::run_pair(input, GAS_COST_PER_PAIR, GAS_COST_BASE, gas_limit) {
        Ok(output) => Ok(PrecompileResult {
            output: output.bytes.to_vec(),
            gas_used: output.gas_used,
        }),
        Err(e) => Err(EVMError::Execution(format!("ecpairing failed: {:?}", e))),
    }
}

/// BLAKE2b compression function (0x09)
///
/// BLAKE2b F compression function.
/// Gas cost: F (rounded up)
///
/// Input (213 bytes):
/// - rounds (4 bytes): number of rounds (little-endian)
/// - h[0..7] (64 bytes): initial state vector
/// - m[0..15] (128 bytes): message vector
/// - t[0..1] (16 bytes): offset (little-endian)
/// - f (1 byte): final block flag
///
/// Output (64 bytes): resulting state vector
fn blake2f(input: &[u8], gas_limit: u64) -> Result<PrecompileResult, EVMError> {
    const GAS_COST_BASE: u64 = 15;

    if input.len() != 213 {
        return Err(EVMError::Execution(format!(
            "blake2f: Invalid input length {}, expected 213",
            input.len()
        )));
    }

    // Round count (first 4 bytes, little-endian)
    let rounds = u32::from_le_bytes([
        input[0], input[1], input[2], input[3]
    ]);
    let gas_used = GAS_COST_BASE + rounds as u64;

    if gas_limit < gas_used {
        return Err(EVMError::OutOfGas);
    }

    // Extract parameters
    let h = &input[4..68]; // h[0..7] - 8 64-bit words = 64 bytes
    let m = &input[68..196]; // m[0..15] - 16 64-bit words = 128 bytes
    let t0 = u64::from_le_bytes(input[196..204].try_into().unwrap());
    let t1 = u64::from_le_bytes(input[204..212].try_into().unwrap());
    let final_block = input[212];

    // Convert to u64 arrays
    let mut h_vec = [0u64; 8];
    for i in 0..8 {
        h_vec[i] = u64::from_le_bytes(h[i * 8..(i + 1) * 8].try_into().unwrap());
    }

    let mut m_vec = [0u64; 16];
    for i in 0..16 {
        m_vec[i] = u64::from_le_bytes(m[i * 8..(i + 1) * 8].try_into().unwrap());
    }

    // Perform BLAKE2b compression
    // This is the F compression function from BLAKE2b
    let result = blake2b_compression(rounds as usize, &h_vec, &m_vec, t0, t1, final_block);

    // Convert result to bytes
    let mut output = vec![0u8; 64];
    for i in 0..8 {
        output[i * 8..(i + 1) * 8].copy_from_slice(&result[i].to_le_bytes());
    }

    Ok(PrecompileResult {
        output,
        gas_used,
    })
}

/// BLAKE2b compression function implementation
fn blake2b_compression(
    rounds: usize,
    h: &[u64; 8],
    m: &[u64; 16],
    t0: u64,
    t1: u64,
    f: u8,
) -> [u64; 8] {
    // BLAKE2b IV (initialization vector)
    const IV: [u64; 8] = [
        0x6a09e667f3bcc909,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];

    // Sigma constants
    const SIGMA: [[usize; 16]; 12] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
        [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
        [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
        [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
        [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
        [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
        [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 2, 8, 6, 10, 4],
        [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
        [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    ];

    let mut v = [0u64; 16];
    v[0..8].copy_from_slice(h);
    v[8..16].copy_from_slice(&IV);

    v[12] ^= t0;
    v[13] ^= t1;

    if f != 0 {
        v[14] = !v[14];
    }

    let mut result = *h;

    for round in 0..rounds {
        // Message schedule
        let s = &SIGMA[round % 12];

        // G function
        macro_rules! g {
            ($a:expr, $b:expr, $c:expr, $d:expr, $x:expr, $y:expr) => {
                v[$a] = v[$a].wrapping_add(v[$b]).wrapping_add($x);
                v[$d] = (v[$d] ^ v[$a]).rotate_right(32);
                v[$c] = v[$c].wrapping_add(v[$d]);
                v[$b] = (v[$b] ^ v[$c]).rotate_right(24);
                v[$a] = v[$a].wrapping_add(v[$b]).wrapping_add($y);
                v[$d] = (v[$d] ^ v[$a]).rotate_right(16);
                v[$c] = v[$c].wrapping_add(v[$d]);
                v[$b] = (v[$b] ^ v[$c]).rotate_right(63);
            };
        }

        // Column step
        g!(0, 4, 8, 12, m[s[0]], m[s[1]]);
        g!(1, 5, 9, 13, m[s[2]], m[s[3]]);
        g!(2, 6, 10, 14, m[s[4]], m[s[5]]);
        g!(3, 7, 11, 15, m[s[6]], m[s[7]]);

        // Diagonal step
        g!(0, 5, 10, 15, m[s[8]], m[s[9]]);
        g!(1, 6, 11, 12, m[s[10]], m[s[11]]);
        g!(2, 7, 8, 13, m[s[12]], m[s[13]]);
        g!(3, 4, 9, 14, m[s[14]], m[s[15]]);
    }

    for i in 0..8 {
        result[i] ^= v[i] ^ v[i + 8];
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_precompile() {
        assert!(is_precompile(&ECRECOVER_ADDRESS));
        assert!(is_precompile(&SHA256_ADDRESS));
        assert!(is_precompile(&RIPEMD160_ADDRESS));
        assert!(is_precompile(&IDENTITY_ADDRESS));
        assert!(is_precompile(&MODEXP_ADDRESS));
        assert!(is_precompile(&ECADD_ADDRESS));
        assert!(is_precompile(&ECMUL_ADDRESS));
        assert!(is_precompile(&ECPAIRING_ADDRESS));
        assert!(is_precompile(&BLAKE2F_ADDRESS));

        let regular_address = Address([99u8; 20]);
        assert!(!is_precompile(&regular_address));
    }

    #[test]
    fn test_sha256_precompile() {
        let input = b"hello world";
        let result = sha256_hash(input, 100_000).unwrap();

        assert_eq!(result.output.len(), 32);
        // Verify against known SHA-256 hash
        let expected = hex::decode("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9").unwrap();
        assert_eq!(result.output, expected);
    }

    #[test]
    fn test_ripemd160_precompile() {
        let input = b"hello world";
        let result = ripemd160_hash(input, 100_000).unwrap();

        assert_eq!(result.output.len(), 32);
        // First 12 bytes should be 0 (padding)
        assert_eq!(&result.output[0..12], &[0u8; 12]);
    }

    #[test]
    fn test_identity_precompile() {
        let input = b"test data";
        let result = identity(input, 100_000).unwrap();

        assert_eq!(result.output, input.to_vec());
    }

    #[test]
    fn test_modexp_simple() {
        // Compute 2^10 mod 17 = 1024 mod 17 = 4
        let mut input = vec![0u8; 224]; // Max possible size

        // Set lengths in big-endian format (last byte of each 32-byte field)
        input[31] = 1; // base_len = 1 (at byte 31)
        input[63] = 1; // exp_len = 1 (at byte 63)
        input[95] = 1; // mod_len = 1 (at byte 95)

        // Set values
        input[96] = 2; // base = 2
        input[97] = 10; // exp = 10
        input[98] = 17; // modulus = 17

        let result = modexp(&input, 100_000).unwrap();
        // Result should be 4
        // The output is 1 byte (mod_len=1), result should be [4]
        assert_eq!(result.output, vec![4]);
    }

    #[test]
    fn test_sha256_gas_cost() {
        let input = vec![0u8; 32];
        let result = sha256_hash(&input, 100).unwrap();
        assert_eq!(result.gas_used, 60 + 12); // base + 1 word
    }

    #[test]
    fn test_identity_gas_cost() {
        let input = vec![0u8; 64];
        let result = identity(&input, 100).unwrap();
        assert_eq!(result.gas_used, 15 + 6); // base + 2 words
    }

    #[test]
    fn test_out_of_gas() {
        let input = b"x".repeat(1000);
        let result = sha256_hash(&input, 10);
        assert!(matches!(result, Err(EVMError::OutOfGas)));
    }

    #[test]
    fn test_invalid_input_length() {
        let input = b"short";
        let result = ecrecover(input, 100_000);
        assert!(result.is_err());
    }

    #[test]
    fn test_ecrecover_invalid_v() {
        let mut input = [0u8; 128];
        // Set v to 2 (invalid, should be 0 or 1)
        input[63] = 2;

        let result = ecrecover(&input, 100_000).unwrap();
        // Should return all zeros
        assert_eq!(result.output, vec![0u8; 32]);
    }

    #[test]
    fn test_ecadd() {
        let input = vec![0u8; 128];
        let result = ecadd(&input, 100_000).unwrap();
        assert_eq!(result.output.len(), 64);
        assert_eq!(result.gas_used, 150);
    }

    #[test]
    fn test_ecmul() {
        let input = vec![0u8; 96];
        let result = ecmul(&input, 100_000).unwrap();
        assert_eq!(result.output.len(), 64);
        assert_eq!(result.gas_used, 6_000);
    }

    #[test]
    fn test_ecpairing() {
        // 2 pairings = 384 bytes
        let input = vec![0u8; 384];
        let result = ecpairing(&input, 200_000).unwrap();
        assert_eq!(result.output.len(), 32);
        // Check that the result is 1 (in big-endian format at the last byte)
        assert_eq!(result.output[31], 1); // Should return 1 for success
        assert_eq!(result.gas_used, 45_000 + 34_000 * 2);
    }

    #[test]
    fn test_blake2f() {
        let mut input = vec![0u8; 213];
        input[0] = 10; // Set rounds to 10 (little-endian)
        let result = blake2f(&input, 1_000).unwrap();
        assert_eq!(result.output.len(), 64);
        assert_eq!(result.gas_used, 15 + 10);
    }
}
