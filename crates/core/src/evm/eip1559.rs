//! EIP-1559 Fee Market Implementation
//!
//! Implements Ethereum's EIP-1559 fee market mechanism which includes:
//! - Base fee calculation and dynamic adjustment
//! - Transaction fee structure: base fee + priority fee (tip)
//! - Fee burning mechanism
//! - Gas limits and targets

use serde::{Deserialize, Serialize};
use std::cmp;

/// EIP-1559 configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EIP1559Config {
    /// Initial base fee (in wei)
    pub initial_base_fee: u64,
    /// Target gas used per block (for base fee adjustment)
    pub gas_target: u64,
    /// Maximum gas limit per block
    pub max_gas_limit: u64,
    /// Base fee change denominator (controls adjustment speed)
    pub base_fee_change_denominator: u64,
    /// Elasticity multiplier (allows blocks to exceed target temporarily)
    pub elasticity_multiplier: u64,
    /// Minimum base fee (prevents negative or zero fees)
    pub min_base_fee: u64,
}

impl Default for EIP1559Config {
    fn default() -> Self {
        Self {
            // Ethereum mainnet-like values (scaled down for testing)
            initial_base_fee: 1_000_000_000, // 1 Gwei
            gas_target: 15_000_000,          // 15 million
            max_gas_limit: 30_000_000,        // 30 million
            base_fee_change_denominator: 8,   // 12.5% change per block
            elasticity_multiplier: 2,         // Can use up to 2x target
            min_base_fee: 1_000_000_000,      // 1 Gwei minimum
        }
    }
}

/// EIP-1559 fee calculator
pub struct EIP1559FeeCalculator {
    config: EIP1559Config,
}

impl EIP1559FeeCalculator {
    /// Create a new fee calculator with the given config
    pub fn new(config: EIP1559Config) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn default_config() -> Self {
        Self::new(EIP1559Config::default())
    }

    /// Calculate the base fee for the next block based on current block gas usage
    ///
    /// Formula:
    /// - If gas_used < target: base_fee decreases by up to 12.5%
    /// - If gas_used > target: base_fee increases by up to 12.5%
    /// - Change is proportional to the difference from target
    pub fn calculate_next_base_fee(&self, parent_base_fee: u64, gas_used: u64) -> u64 {
        let target = self.config.gas_target;

        if gas_used == target {
            // No change if exactly at target
            return parent_base_fee;
        }

        // Calculate gas delta from target
        let gas_delta = if gas_used > target {
            gas_used - target
        } else {
            target - gas_used
        };

        // Calculate fee delta (proportional change)
        let fee_delta = (parent_base_fee * gas_delta)
            .checked_div(target * self.config.base_fee_change_denominator)
            .unwrap_or(0);

        // Apply increase or decrease
        let new_base_fee = if gas_used > target {
            parent_base_fee.saturating_add(fee_delta)
        } else {
            parent_base_fee.saturating_sub(fee_delta)
        };

        // Ensure minimum base fee
        cmp::max(new_base_fee, self.config.min_base_fee)
    }

    /// Calculate the effective gas price for a transaction
    ///
    /// For EIP-1559 transactions:
    /// effective_gas_price = base_fee + min(max_priority_fee_per_gas, max_fee_per_gas - base_fee)
    ///
    /// For legacy transactions:
    /// effective_gas_price = gas_price
    pub fn calculate_effective_gas_price(
        &self,
        base_fee: u64,
        max_fee_per_gas: Option<u64>,
        max_priority_fee_per_gas: Option<u64>,
        gas_price: Option<u64>,
    ) -> u64 {
        // EIP-1559 transaction
        if let (Some(max_fee), Some(priority_fee)) = (max_fee_per_gas, max_priority_fee_per_gas) {
            if base_fee > max_fee {
                // Transaction would underpay, but validation should catch this
                // Return max_fee as the effective price (will fail validation)
                return max_fee;
            }

            // Priority fee is capped by: max_fee - base_fee
            let available_for_priority = max_fee.saturating_sub(base_fee);
            let actual_priority_fee = cmp::min(priority_fee, available_for_priority);

            base_fee.saturating_add(actual_priority_fee)
        }
        // Legacy transaction
        else if let Some(gp) = gas_price {
            gp
        } else {
            // No pricing info - use base fee
            base_fee
        }
    }

    /// Calculate the total transaction fee
    ///
    /// fee = effective_gas_price * gas_used
    pub fn calculate_transaction_fee(
        &self,
        base_fee: u64,
        gas_used: u64,
        max_fee_per_gas: Option<u64>,
        max_priority_fee_per_gas: Option<u64>,
        gas_price: Option<u64>,
    ) -> u64 {
        let effective_price = self.calculate_effective_gas_price(
            base_fee,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            gas_price,
        );

        effective_price.saturating_mul(gas_used)
    }

    /// Calculate the portion of fee that gets burned (base fee)
    pub fn calculate_burned_fee(&self, base_fee: u64, gas_used: u64) -> u64 {
        base_fee.saturating_mul(gas_used)
    }

    /// Calculate the portion that goes to the miner (priority fee)
    pub fn calculate_miner_tip(
        &self,
        base_fee: u64,
        gas_used: u64,
        max_fee_per_gas: Option<u64>,
        max_priority_fee_per_gas: Option<u64>,
        gas_price: Option<u64>,
    ) -> u64 {
        let total_fee = self.calculate_transaction_fee(
            base_fee,
            gas_used,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            gas_price,
        );

        let burned = self.calculate_burned_fee(base_fee, gas_used);

        total_fee.saturating_sub(burned)
    }

    /// Validate that a transaction's fee parameters are sufficient
    ///
    /// For EIP-1559 transactions:
    /// - max_fee_per_gas >= base_fee
    /// - max_priority_fee_per_gas <= max_fee_per_gas
    ///
    /// For legacy transactions:
    /// - gas_price >= base_fee
    pub fn validate_fee_params(
        &self,
        base_fee: u64,
        max_fee_per_gas: Option<u64>,
        max_priority_fee_per_gas: Option<u64>,
        gas_price: Option<u64>,
    ) -> Result<(), String> {
        // EIP-1559 transaction
        if let (Some(max_fee), Some(priority_fee)) = (max_fee_per_gas, max_priority_fee_per_gas) {
            if max_fee < base_fee {
                return Err(format!(
                    "max_fee_per_gas ({}) is less than base_fee ({})",
                    max_fee, base_fee
                ));
            }

            if priority_fee > max_fee {
                return Err(format!(
                    "max_priority_fee_per_gas ({}) exceeds max_fee_per_gas ({})",
                    priority_fee, max_fee
                ));
            }
        }
        // Legacy transaction
        else if let Some(gp) = gas_price {
            if gp < base_fee {
                return Err(format!(
                    "gas_price ({}) is less than base_fee ({})",
                    gp, base_fee
                ));
            }
        }

        Ok(())
    }

    /// Estimate gas price for a transaction
    ///
    /// Returns suggested max_fee_per_gas and max_priority_fee_per_gas
    pub fn estimate_gas_prices(&self, base_fee: u64) -> (u64, u64) {
        // Suggest priority fee of 1-2 Gwei
        let suggested_priority_fee = 2_000_000_000; // 2 Gwei

        // Max fee should be base fee + priority fee + some buffer
        let suggested_max_fee = base_fee.saturating_add(suggested_priority_fee * 2);

        (suggested_max_fee, suggested_priority_fee)
    }

    /// Get the gas target for blocks
    pub fn gas_target(&self) -> u64 {
        self.config.gas_target
    }

    /// Get the max gas limit for blocks
    pub fn max_gas_limit(&self) -> u64 {
        self.config.max_gas_limit
    }

    /// Get the initial base fee for genesis
    pub fn initial_base_fee(&self) -> u64 {
        self.config.initial_base_fee
    }

    /// Check if a block's gas used is within acceptable limits
    pub fn validate_block_gas(&self, gas_used: u64) -> Result<(), String> {
        if gas_used > self.config.max_gas_limit {
            return Err(format!(
                "Block gas used ({}) exceeds maximum ({})",
                gas_used, self.config.max_gas_limit
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_next_base_fee_no_change() {
        let calculator = EIP1559FeeCalculator::default_config();
        let parent_fee = 1_000_000_000; // 1 Gwei

        // Gas used exactly at target - no change
        let new_fee = calculator.calculate_next_base_fee(parent_fee, 15_000_000);
        assert_eq!(new_fee, parent_fee);
    }

    #[test]
    fn test_calculate_next_base_fee_increase() {
        let calculator = EIP1559FeeCalculator::default_config();
        let parent_fee = 1_000_000_000; // 1 Gwei

        // Gas used double the target - should increase
        let new_fee = calculator.calculate_next_base_fee(parent_fee, 30_000_000);
        assert!(new_fee > parent_fee);

        // Should increase by about 12.5%
        let expected_increase = parent_fee / 8;
        assert_eq!(new_fee, parent_fee + expected_increase);
    }

    #[test]
    fn test_calculate_next_base_fee_decrease() {
        let calculator = EIP1559FeeCalculator::default_config();
        let parent_fee = 2_000_000_000; // 2 Gwei

        // Gas used half the target - should decrease
        let new_fee = calculator.calculate_next_base_fee(parent_fee, 7_500_000);
        assert!(new_fee < parent_fee);

        // Should decrease by about 12.5%
        let expected_decrease = parent_fee / 16; // Half of delta / denominator
        assert_eq!(new_fee, parent_fee - expected_decrease);
    }

    #[test]
    fn test_calculate_effective_gas_price_eip1559() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 1_000_000_000; // 1 Gwei
        let max_fee = 3_000_000_000;   // 3 Gwei
        let priority_fee = 2_000_000_000; // 2 Gwei

        let effective = calculator.calculate_effective_gas_price(
            base_fee,
            Some(max_fee),
            Some(priority_fee),
            None,
        );

        // Should be base_fee + (max_fee - base_fee) capped by priority_fee
        // = 1 Gwei + min(2 Gwei, 2 Gwei) = 3 Gwei
        assert_eq!(effective, 3_000_000_000);
    }

    #[test]
    fn test_calculate_effective_gas_price_priority_capped() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 2_000_000_000; // 2 Gwei
        let max_fee = 3_000_000_000;   // 3 Gwei
        let priority_fee = 2_000_000_000; // 2 Gwei

        let effective = calculator.calculate_effective_gas_price(
            base_fee,
            Some(max_fee),
            Some(priority_fee),
            None,
        );

        // Available for priority = max_fee - base_fee = 1 Gwei
        // Priority fee capped to 1 Gwei
        // Effective = 2 + 1 = 3 Gwei
        assert_eq!(effective, 3_000_000_000);
    }

    #[test]
    fn test_calculate_transaction_fee() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 1_000_000_000;
        let gas_used = 21_000;
        let max_fee = 3_000_000_000;
        let priority_fee = 2_000_000_000;

        let fee = calculator.calculate_transaction_fee(
            base_fee,
            gas_used,
            Some(max_fee),
            Some(priority_fee),
            None,
        );

        // Effective price = 3 Gwei, Fee = 3 Gwei * 21,000
        assert_eq!(fee, 63_000_000_000_000);
    }

    #[test]
    fn test_validate_fee_params_valid() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 1_000_000_000;

        // Valid params
        let result = calculator.validate_fee_params(
            base_fee,
            Some(3_000_000_000),
            Some(2_000_000_000),
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_fee_params_max_fee_too_low() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 2_000_000_000;

        // max_fee < base_fee
        let result = calculator.validate_fee_params(
            base_fee,
            Some(1_000_000_000),
            Some(500_000_000),
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_fee_params_priority_exceeds_max() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 1_000_000_000;

        // priority_fee > max_fee
        let result = calculator.validate_fee_params(
            base_fee,
            Some(2_000_000_000),
            Some(3_000_000_000),
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_calculate_burned_fee() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 1_000_000_000;
        let gas_used = 21_000;

        let burned = calculator.calculate_burned_fee(base_fee, gas_used);
        assert_eq!(burned, 21_000_000_000_000);
    }

    #[test]
    fn test_calculate_miner_tip() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 1_000_000_000;
        let gas_used = 21_000;
        let max_fee = 3_000_000_000;
        let priority_fee = 2_000_000_000;

        let tip = calculator.calculate_miner_tip(
            base_fee,
            gas_used,
            Some(max_fee),
            Some(priority_fee),
            None,
        );

        // Total fee = 63_000_000_000_000, Burned = 21_000_000_000_000
        // Tip = 42_000_000_000_000
        assert_eq!(tip, 42_000_000_000_000);
    }

    #[test]
    fn test_estimate_gas_prices() {
        let calculator = EIP1559FeeCalculator::default_config();
        let base_fee = 1_500_000_000;

        let (max_fee, priority_fee) = calculator.estimate_gas_prices(base_fee);

        assert!(priority_fee == 2_000_000_000);
        assert!(max_fee > base_fee);
    }

    #[test]
    fn test_validate_block_gas_within_limit() {
        let calculator = EIP1559FeeCalculator::default_config();

        // Within limit
        let result = calculator.validate_block_gas(15_000_000);
        assert!(result.is_ok());

        // At limit
        let result = calculator.validate_block_gas(30_000_000);
        assert!(result.is_ok());

        // Over limit
        let result = calculator.validate_block_gas(31_000_000);
        assert!(result.is_err());
    }
}
