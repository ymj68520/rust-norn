//! Transaction execution module
//! 
//! Provides transaction execution and gas management.
//! 
//! TODO: Fix imports in executor.rs, gas.rs, and nonce.rs:
//! - Replace `crate::types` with `norn_common::types`
//! - Replace `crate::crypto` with `norn_crypto`
//! - Replace `crate::state` with `crate::state`
//! - Add missing type aliases (Address, H256, U256, Wei)

// TODO: These modules have import issues that need to be fixed
// pub mod gas;
// pub mod executor;
// pub mod nonce;