//! Fee and Reward Distribution Module
//! 
//! Handles transaction fee calculations and block reward distribution.

use norn_common::types::{Transaction, Block, Address};
use tracing::{debug, info};

/// Fee configuration
#[derive(Debug, Clone)]
pub struct FeeConfig {
    /// Base fee per gas unit (in smallest denomination)
    pub base_fee_per_gas: u64,
    /// Minimum gas price accepted
    pub min_gas_price: u64,
    /// Maximum gas limit per transaction
    pub max_gas_per_tx: u64,
    /// Block reward (in smallest denomination)
    pub block_reward: u64,
    /// Percentage of fees burned (0-100)
    pub burn_percentage: u8,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            base_fee_per_gas: 1_000_000_000, // 1 Gwei equivalent
            min_gas_price: 1_000_000_000,
            max_gas_per_tx: 10_000_000,
            block_reward: 2_000_000_000_000_000_000, // 2 tokens
            burn_percentage: 50,
        }
    }
}

/// Fee calculator for transactions
pub struct FeeCalculator {
    config: FeeConfig,
}

impl FeeCalculator {
    /// Create new fee calculator with default config
    pub fn new() -> Self {
        Self::with_config(FeeConfig::default())
    }

    /// Create new fee calculator with custom config
    pub fn with_config(config: FeeConfig) -> Self {
        Self { config }
    }

    /// Calculate fee for a transaction
    pub fn calculate_tx_fee(&self, tx: &Transaction) -> u64 {
        let gas_used = tx.body.gas.max(0) as u64;
        let gas_price = self.config.base_fee_per_gas;
        
        gas_used.saturating_mul(gas_price)
    }

    /// Calculate total fees for a block
    pub fn calculate_block_fees(&self, block: &Block) -> u64 {
        block.transactions.iter()
            .map(|tx| self.calculate_tx_fee(tx))
            .sum()
    }

    /// Get base gas price
    pub fn get_base_gas_price(&self) -> u64 {
        self.config.base_fee_per_gas
    }

    /// Check if transaction meets minimum gas price
    pub fn is_valid_gas_price(&self, gas_price: u64) -> bool {
        gas_price >= self.config.min_gas_price
    }

    /// Estimate gas for a transaction (simplified)
    pub fn estimate_gas(&self, tx: &Transaction) -> u64 {
        // Base gas for any transaction
        let base_gas = 21_000u64;
        
        // Additional gas for data
        let data_gas = if !tx.body.data.is_empty() {
            tx.body.data.len() as u64 * 16
        } else {
            0
        };
        
        base_gas + data_gas
    }
}

impl Default for FeeCalculator {
    fn default() -> Self {
        Self::new()
    }
}

/// Reward distributor for block producers
pub struct RewardDistributor {
    config: FeeConfig,
}

impl RewardDistributor {
    /// Create new reward distributor
    pub fn new() -> Self {
        Self::with_config(FeeConfig::default())
    }

    /// Create with custom config
    pub fn with_config(config: FeeConfig) -> Self {
        Self { config }
    }

    /// Calculate reward for block producer
    pub fn calculate_producer_reward(&self, block: &Block) -> RewardBreakdown {
        let fee_calc = FeeCalculator::with_config(self.config.clone());
        let total_fees = fee_calc.calculate_block_fees(block);
        
        // Calculate fee distribution
        let burn_amount = (total_fees as u128 * self.config.burn_percentage as u128 / 100) as u64;
        let producer_fees = total_fees - burn_amount;
        
        RewardBreakdown {
            block_reward: self.config.block_reward,
            transaction_fees: producer_fees,
            burned_fees: burn_amount,
            total_reward: self.config.block_reward + producer_fees,
        }
    }

    /// Get current block reward
    pub fn get_block_reward(&self) -> u64 {
        self.config.block_reward
    }

    /// Calculate halving based on block height
    pub fn get_halved_reward(&self, block_height: i64) -> u64 {
        // Halving every 210,000 blocks (like Bitcoin)
        let halvings = (block_height / 210_000) as u32;
        
        if halvings >= 64 {
            return 0; // No more rewards after 64 halvings
        }
        
        self.config.block_reward >> halvings
    }
}

impl Default for RewardDistributor {
    fn default() -> Self {
        Self::new()
    }
}

/// Breakdown of block producer rewards
#[derive(Debug, Clone)]
pub struct RewardBreakdown {
    /// Base block reward
    pub block_reward: u64,
    /// Transaction fees going to producer
    pub transaction_fees: u64,
    /// Fees that are burned
    pub burned_fees: u64,
    /// Total reward for producer
    pub total_reward: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::Block;

    #[test]
    fn test_fee_config_default() {
        let config = FeeConfig::default();
        assert_eq!(config.base_fee_per_gas, 1_000_000_000);
        assert_eq!(config.burn_percentage, 50);
    }

    #[test]
    fn test_fee_calculator() {
        let calc = FeeCalculator::new();
        let tx = Transaction::default();
        
        let fee = calc.calculate_tx_fee(&tx);
        assert!(fee >= 0);
    }

    #[test]
    fn test_gas_estimation() {
        let calc = FeeCalculator::new();
        let tx = Transaction::default();
        
        let gas = calc.estimate_gas(&tx);
        assert_eq!(gas, 21_000); // Base gas for empty tx
    }

    #[test]
    fn test_reward_halving() {
        let distributor = RewardDistributor::new();
        
        // First halving at block 210,000
        let reward_0 = distributor.get_halved_reward(0);
        let reward_1 = distributor.get_halved_reward(210_000);
        
        assert_eq!(reward_1, reward_0 / 2);
    }

    #[test]
    fn test_reward_breakdown() {
        let distributor = RewardDistributor::new();
        let block = Block::default();
        
        let breakdown = distributor.calculate_producer_reward(&block);
        assert_eq!(breakdown.total_reward, breakdown.block_reward + breakdown.transaction_fees);
    }
}
