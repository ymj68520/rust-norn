//! Test vs Release mode configuration
//!
//! This module provides compile-time flags to differentiate between testing and production builds,
//! ensuring that test-specific simplifications are only used in tests.

/// Feature flags for different build modes
///
/// Add to Cargo.toml:
/// ```toml
/// [features]
/// default = []
/// test_mode = []
/// production = []
/// ```

/// Check if we're in test mode
#[cfg(test)]
pub const IS_TEST_MODE: bool = true;

#[cfg(not(test))]
pub const IS_TEST_MODE: bool = false;

/// Check if we're in production mode
#[cfg(feature = "production")]
pub const IS_PRODUCTION_MODE: bool = true;

#[cfg(not(feature = "production"))]
pub const IS_PRODUCTION_MODE: bool = false;

/// Get VRF threshold for block producer selection
///
/// In test mode: always produce blocks (threshold = 255)
/// In production: use stake-weighted threshold
pub fn get_vrf_threshold() -> u8 {
    if IS_TEST_MODE {
        255 // Always selected in tests
    } else {
        // TODO: Calculate based on stake weight
        // For now, use 128 (50% threshold)
        128
    }
}

/// Get validation strictness level
pub enum ValidationStrictness {
    /// Skip expensive validations (testing)
    Lenient,
    /// Full validation (production)
    Strict,
    /// Extra strict (auditing/security-critical)
    Paranoid,
}

/// Get current validation strictness
pub fn get_validation_strictness() -> ValidationStrictness {
    if IS_TEST_MODE {
        ValidationStrictness::Lenient
    } else if cfg!(feature = "production") {
        ValidationStrictness::Strict
    } else {
        ValidationStrictness::Strict
    }
}

/// Whether to skip VDF verification
pub fn should_skip_vdf_verification() -> bool {
    IS_TEST_MODE
}

/// Whether to skip VRF verification
pub fn should_skip_vrf_verification() -> bool {
    IS_TEST_MODE
}

/// Whether to use placeholder implementations
pub fn use_placeholders() -> bool {
    IS_TEST_MODE
}

/// Get nonce cache size
pub fn get_nonce_cache_size() -> usize {
    if IS_TEST_MODE {
        100 // Smaller cache for tests
    } else {
        10_000 // Larger cache for production
    }
}

/// Get state cache size
pub fn get_state_cache_size() -> usize {
    if IS_TEST_MODE {
        1_000
    } else {
        100_000
    }
}

/// Get max contract size
pub fn get_max_contract_size() -> usize {
    if IS_TEST_MODE {
        24_576 // EIP-170 limit (can be overridden in tests)
    } else {
        24_576 // Always enforce EIP-170 in production
    }
}

/// Whether to enable debug logging
pub fn enable_debug_logging() -> bool {
    cfg!(debug_assertions) || IS_TEST_MODE
}

/// Get block gas limit
pub fn get_block_gas_limit() -> u64 {
    if IS_TEST_MODE {
        30_000_000 // Lower for tests
    } else {
        30_000_000 // Production limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_flags() {
        // In test mode
        assert!(IS_TEST_MODE);
        assert!(!IS_PRODUCTION_MODE);
        assert_eq!(get_vrf_threshold(), 255);
        assert_eq!(get_nonce_cache_size(), 100);
    }

    #[test]
    fn test_validation_strictness() {
        match get_validation_strictness() {
            ValidationStrictness::Lenient => println!("Lenient validation"),
            _ => panic!("Expected Lenient in test mode"),
        }
    }
}
