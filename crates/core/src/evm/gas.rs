//! Gas calculation and deduction mechanisms
//!
//! This module implements Ethereum's gas scheduling system, including:
//! - Gas cost calculation for transactions
//! - Gas deduction during execution
//! - Refund handling
//! - EIP-1559 dynamic fee calculation

use crate::evm::{EVMError, EVMResult, EIP1559Config};
use norn_common::types::{Transaction, Address};

/// Gas costs for various operations (in gas units)
pub mod costs {
    /// Base cost for a simple transaction (no data, no contract creation)
    pub const TX_BASE_COST: u64 = 21_000;

    /// Base cost for contract creation
    pub const TX_CREATE_COST: u64 = 53_000;

    /// Gas cost per zero byte in transaction data
    pub const TX_ZERO_DATA_COST: u64 = 4;

    /// Gas cost per non-zero byte in transaction data
    pub const TX_NON_ZERO_DATA_COST: u64 = 16;

    /// Gas cost for accessing a cold account (EIP-2929)
    pub const COLD_ACCOUNT_ACCESS_COST: u64 = 2_600;

    /// Gas cost for accessing a warm account (EIP-2929)
    pub const WARM_ACCOUNT_ACCESS_COST: u64 = 100;

    /// Gas cost for accessing cold storage (EIP-2929)
    pub const COLD_SLOAD_COST: u64 = 2_100;

    /// Gas cost for accessing warm storage (EIP-2929)
    pub const WARM_SLOAD_COST: u64 = 100;

    /// Access list costs (EIP-2930)
    pub const ACCESS_LIST_ADDRESS_COST: u64 = 2_400;
    pub const ACCESS_LIST_STORAGE_KEY_COST: u64 = 1_900;

    /// Gas cost for sstore (first time writing to a storage slot)
    pub const SSTORE_SET: u64 = 20_000;

    /// Gas refund for clearing storage (EIP-2200)
    pub const SSTORE_CLEAR_REFUND: u64 = 4_800;

    /// Gas cost for CALL operation
    pub const CALL_COST: u64 = 2_300;

    /// Gas cost for DELEGATECALL operation
    pub const DELEGATECALL_COST: u64 = 2_300;

    /// Gas cost for STATICCALL operation
    pub const STATICCALL_COST: u64 = 2_300;

    /// Gas cost for CREATE operation
    pub const CREATE_COST: u64 = 32_000;

    /// Gas cost for CREATE2 operation
    pub const CREATE2_COST: u64 = 32_000;

    /// Gas cost for LOG0 operation
    pub const LOG0_COST: u64 = 375;

    /// Additional cost per LOG topic
    pub const LOG_TOPIC_COST: u64 = 375;

    /// Gas cost per byte of LOG data
    pub const LOG_DATA_COST: u64 = 8;

    /// Gas cost for EXP operation
    pub const EXP_COST: u64 = 10;

    /// Additional cost per byte of EXP exponent
    pub const EXP_BYTE_COST: u64 = 50;

    /// Gas cost for memory expansion (per 32-byte word)
    pub const MEMORY_COST: u64 = 3;

    /// Gas cost for copying data to memory (per word)
    pub const COPY_COST: u64 = 3;

    /// Quota for gas refunds (max 50% of gas used can be refunded)
    pub const MAX_REFUND_QUOTIENT: u64 = 2;
}

/// Gas calculator for EVM transactions
pub struct GasCalculator {
    /// Enable EIP-2929 (net gas metering)
    eip2929_enabled: bool,

    /// Enable EIP-2200 (net gas metering for SSTORE)
    eip2200_enabled: bool,

    /// EIP-1559 configuration
    eip1559_config: EIP1559Config,
}

impl GasCalculator {
    /// Create a new gas calculator
    pub fn new(eip1559_config: EIP1559Config) -> Self {
        Self {
            eip2929_enabled: true, // Enabled by default for post-Berlin
            eip2200_enabled: true, // Enabled by default for post-Istanbul
            eip1559_config,
        }
    }

    /// Calculate intrinsic gas cost for a transaction
    ///
    /// This is the minimum gas required for a transaction before execution.
    pub fn intrinsic_gas_cost(
        &self,
        is_contract_creation: bool,
        data: &[u8],
        access_list: Option<&[(Address, Vec<[u8; 32]>)]>,
    ) -> u64 {
        // Base cost
        let mut gas = if is_contract_creation {
            costs::TX_CREATE_COST
        } else {
            costs::TX_BASE_COST
        };

        // Data cost (zero bytes vs non-zero bytes)
        for byte in data {
            if *byte == 0 {
                gas += costs::TX_ZERO_DATA_COST;
            } else {
                gas += costs::TX_NON_ZERO_DATA_COST;
            }
        }

        // Access list cost (EIP-2930)
        if let Some(list) = access_list {
            // Cost per address in access list
            gas += list.len() as u64 * costs::ACCESS_LIST_ADDRESS_COST;

            // Cost per storage key in access list
            for (_, keys) in list {
                gas += keys.len() as u64 * costs::ACCESS_LIST_STORAGE_KEY_COST;
            }
        }

        gas
    }

    /// Calculate gas cost for a contract call
    ///
    /// Includes warm/cold account access costs
    pub fn call_gas_cost(
        &self,
        callee: &Address,
        is_cold: bool,
        value_transferred: bool,
    ) -> u64 {
        let mut gas = costs::CALL_COST;

        // Cold account access cost (EIP-2929)
        if self.eip2929_enabled && is_cold {
            gas += costs::COLD_ACCOUNT_ACCESS_COST;
        }

        // Additional cost for value transfer
        if value_transferred {
            gas += 9_000; // CALL_WITH_VALUE_COST
        }

        // Memory expansion cost would be calculated by the EVM during execution
        gas
    }

    /// Calculate gas cost for CREATE/CREATE2
    pub fn create_gas_cost(&self, is_cold: bool) -> u64 {
        let mut gas = costs::CREATE_COST;

        if self.eip2929_enabled && is_cold {
            gas += costs::COLD_ACCOUNT_ACCESS_COST;
        }

        gas
    }

    /// Calculate gas refund for clearing storage
    pub fn storage_refund(&self, is_original_value_zero: bool) -> u64 {
        if self.eip2200_enabled && !is_original_value_zero {
            costs::SSTORE_CLEAR_REFUND
        } else {
            0
        }
    }

    /// Calculate maximum refund allowed
    ///
    /// EIP-3529: Refunds are capped to 50% of gas used
    pub fn max_refund(&self, gas_used: u64) -> u64 {
        gas_used / costs::MAX_REFUND_QUOTIENT
    }

    /// Calculate EIP-1559 base fee
    ///
    /// The base fee is calculated based on block gas usage vs target
    pub fn calculate_base_fee(
        &self,
        parent_base_fee: u64,
        parent_gas_used: u64,
        parent_gas_target: u64,
    ) -> u64 {
        if parent_gas_used == parent_gas_target {
            parent_base_fee
        } else if parent_gas_used < parent_gas_target {
            // Block underutilized: decrease base fee
            let delta = parent_gas_target - parent_gas_used;
            let numerator = parent_base_fee * delta;
            let denominator = parent_gas_target * self.eip1559_config.base_fee_change_denominator;
            parent_base_fee.saturating_sub((numerator / denominator).max(1))
        } else {
            // Block overutilized: increase base fee
            let delta = parent_gas_used - parent_gas_target;
            let numerator = parent_base_fee * delta;
            let denominator = parent_gas_target * self.eip1559_config.base_fee_change_denominator;
            parent_base_fee + (numerator / denominator).max(1)
        }
    }

    /// Calculate effective gas tip for EIP-1559 transaction
    ///
    /// The actual tip paid is min(max_priority_fee_per_gas, base_fee + max_fee_per_gas)
    pub fn effective_gas_tip(
        &self,
        max_priority_fee_per_gas: u64,
        max_fee_per_gas: u64,
        base_fee: u64,
    ) -> u64 {
        let tip_if_base_fee_lower = max_priority_fee_per_gas;
        let tip_if_base_fee_higher = max_fee_per_gas.saturating_sub(base_fee);

        tip_if_base_fee_lower.min(tip_if_base_fee_higher)
    }

    /// Check if a transaction's gas limit is sufficient
    pub fn is_gas_limit_sufficient(
        &self,
        gas_limit: u64,
        intrinsic_cost: u64,
    ) -> bool {
        gas_limit >= intrinsic_cost
    }

    /// Calculate final gas cost after refunds
    pub fn final_gas_cost(
        &self,
        gas_limit: u64,
        gas_used: u64,
        refund: u64,
    ) -> EVMResult<u64> {
        if gas_used > gas_limit {
            return Err(EVMError::Execution(format!(
                "Gas used {} exceeds gas limit {}",
                gas_used, gas_limit
            )));
        }

        // Cap refund to 50% of gas used
        let max_refund = self.max_refund(gas_used);
        let actual_refund = refund.min(max_refund);

        let gas_cost = gas_used - actual_refund;

        if gas_cost > gas_limit {
            return Err(EVMError::Execution(format!(
                "Final gas cost {} exceeds gas limit {}",
                gas_cost, gas_limit
            )));
        }

        Ok(gas_cost)
    }

    /// Calculate gas price for legacy transactions
    pub fn legacy_gas_price(&self, gas_price: u64) -> u64 {
        gas_price
    }

    /// Calculate gas price for EIP-1559 transactions
    pub fn eip1559_gas_price(
        &self,
        max_fee_per_gas: u64,
        max_priority_fee_per_gas: u64,
        base_fee: u64,
    ) -> EVMResult<(u64, u64)> {
        // Calculate effective gas price (what user actually pays)
        let effective_priority_fee = self.effective_gas_tip(
            max_priority_fee_per_gas,
            max_fee_per_gas,
            base_fee,
        );

        let gas_price = base_fee + effective_priority_fee;

        // Check if max_fee_per_gas is sufficient
        if gas_price > max_fee_per_gas {
            return Err(EVMError::Execution(format!(
                "Calculated gas price {} exceeds max fee per gas {}",
                gas_price, max_fee_per_gas
            )));
        }

        Ok((gas_price, effective_priority_fee))
    }
}

impl Default for GasCalculator {
    fn default() -> Self {
        Self::new(EIP1559Config::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intrinsic_gas_simple_transfer() {
        let calculator = GasCalculator::default();

        // Simple ETH transfer (no data, no contract creation)
        let gas = calculator.intrinsic_gas_cost(false, &[], None);

        assert_eq!(gas, costs::TX_BASE_COST);
    }

    #[test]
    fn test_intrinsic_gas_with_data() {
        let calculator = GasCalculator::default();

        // Transaction with 10 zero bytes and 10 non-zero bytes
        let data: Vec<u8> = vec
![0u8; 10]
            .into_iter()
            .chain(vec![1u8; 10])
            .collect();

        let gas = calculator.intrinsic_gas_cost(false, &data, None);

        let expected = costs::TX_BASE_COST
            + (10 * costs::TX_ZERO_DATA_COST)
            + (10 * costs::TX_NON_ZERO_DATA_COST);

        assert_eq!(gas, expected);
    }

    #[test]
    fn test_intrinsic_gas_contract_creation() {
        let calculator = GasCalculator::default();

        // Contract creation with 20 bytes of init code
        let init_code = vec![0x60, 0x60, 0x60]; // Simple bytecode

        let gas = calculator.intrinsic_gas_cost(true, &init_code, None);

        assert_eq!(gas, costs::TX_CREATE_COST + (3 * costs::TX_NON_ZERO_DATA_COST));
    }

    #[test]
    fn test_gas_refund_cap() {
        let calculator = GasCalculator::default();

        let gas_used = 100_000;
        let refund = 60_000; // Would be > 50% of gas used

        let max_refund = calculator.max_refund(gas_used);

        assert_eq!(max_refund, 50_000); // Capped at 50%
    }

    #[test]
    fn test_final_gas_cost() {
        let calculator = GasCalculator::default();

        let gas_limit = 100_000;
        let gas_used = 80_000;
        let refund = 10_000;

        let final_cost = calculator
            .final_gas_cost(gas_limit, gas_used, refund)
            .unwrap();

        assert_eq!(final_cost, 70_000);
    }

    #[test]
    fn test_final_gas_cost_with_refund_cap() {
        let calculator = GasCalculator::default();

        let gas_limit = 100_000;
        let gas_used = 80_000;
        let refund = 50_000; // Would be > 50% of gas used

        let final_cost = calculator
            .final_gas_cost(gas_limit, gas_used, refund)
            .unwrap();

        assert_eq!(final_cost, 40_000); // 80k used - 40k refund (capped at 50%)
    }

    #[test]
    fn test_base_fee_calculation() {
        let config = EIP1559Config::default();
        let calculator = GasCalculator::new(config);

        let parent_base_fee = 1_000_000_000; // 1 Gwei
        let parent_gas_target = 10_000_000;

        // Block at target: base fee stays the same
        let base_fee = calculator.calculate_base_fee(parent_base_fee, parent_gas_target, parent_gas_target);
        assert_eq!(base_fee, parent_base_fee);

        // Block under target: base fee decreases
        let base_fee = calculator.calculate_base_fee(parent_base_fee, 5_000_000, parent_gas_target);
        assert!(base_fee < parent_base_fee);

        // Block over target: base fee increases
        let base_fee = calculator.calculate_base_fee(parent_base_fee, 15_000_000, parent_gas_target);
        assert!(base_fee > parent_base_fee);
    }

    #[test]
    fn test_effective_gas_tip() {
        let calculator = GasCalculator::default();

        let max_priority_fee = 2_000_000_000; // 2 Gwei
        let max_fee = 5_000_000_000; // 5 Gwei
        let base_fee = 1_000_000_000; // 1 Gwei

        let tip = calculator.effective_gas_tip(max_priority_fee, max_fee, base_fee);

        assert_eq!(tip, 2_000_000_000); // priority_fee is lower
    }

    #[test]
    fn test_effective_gas_tip_base_fee_high() {
        let calculator = GasCalculator::default();

        let max_priority_fee = 2_000_000_000; // 2 Gwei
        let max_fee = 3_000_000_000; // 3 Gwei
        let base_fee = 2_500_000_000; // 2.5 Gwei

        let tip = calculator.effective_gas_tip(max_priority_fee, max_fee, base_fee);

        assert_eq!(tip, 500_000_000); // max_fee - base_fee
    }
}
