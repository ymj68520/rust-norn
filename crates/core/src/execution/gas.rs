use crate::state::AccountStateManager;
use norn_common::error::{NornError, Result};
use norn_common::types::{Address, Transaction};
use num_bigint::BigUint;
use num_traits::{FromPrimitive, One, ToPrimitive, Zero};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Gas 价格和限制配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasConfig {
    /// 基础 Gas 价格
    pub base_gas_price: BigUint,

    /// 最小 Gas 价格
    pub min_gas_price: BigUint,

    /// 最大 Gas 价格
    pub max_gas_price: BigUint,

    /// 默认 Gas 限制
    pub default_gas_limit: u64,

    /// 最大 Gas 限制
    pub max_gas_limit: u64,

    /// Gas 价格调整因子
    pub gas_price_adjustment_factor: f64,

    /// Gas 价格更新间隔（区块数）
    pub gas_price_update_interval: u64,
}

impl Default for GasConfig {
    fn default() -> Self {
        Self {
            base_gas_price: BigUint::from(1000u64), // 1000 wei
            min_gas_price: BigUint::from(1u64),
            max_gas_price: BigUint::from(1000000u64),
            default_gas_limit: 21000,
            max_gas_limit: 10000000,
            gas_price_adjustment_factor: 0.1,
            gas_price_update_interval: 100,
        }
    }
}

/// Gas 使用统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasUsage {
    /// 使用的 Gas 总量
    pub gas_used: u64,

    /// Gas 限制
    pub gas_limit: u64,

    /// Gas 价格
    pub gas_price: BigUint,

    /// 实际支付的费用
    pub actual_fee: BigUint,

    /// 最大费用
    pub max_fee: BigUint,

    /// 是否超出限制
    pub exceeded_limit: bool,

    /// 操作详情
    pub operations: Vec<GasOperation>,
}

/// Gas 操作记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasOperation {
    /// 操作类型
    pub operation_type: String,

    /// 使用的 Gas
    pub gas_used: u64,

    /// 操作描述
    pub description: String,

    /// 操作参数
    pub parameters: HashMap<String, String>,
}

/// Gas 计算器
pub struct GasCalculator {
    /// 配置
    config: Arc<RwLock<GasConfig>>,

    /// Gas 价格表
    gas_schedule: Arc<RwLock<GasSchedule>>,

    /// 当前 Gas 价格
    current_gas_price: Arc<RwLock<BigUint>>,

    /// Gas 使用历史
    usage_history: Arc<RwLock<Vec<GasUsageRecord>>>,

    /// 状态管理器（用于检查余额）
    state_manager: Option<Arc<AccountStateManager>>,
}

/// Gas 调度表
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasSchedule {
    /// 基础操作成本
    pub base_costs: HashMap<String, u64>,

    /// 存储操作成本
    pub storage_costs: StorageGasCosts,

    /// 计算操作成本
    pub computation_costs: ComputationGasCosts,

    /// 合约操作成本
    pub contract_costs: ContractGasCosts,
}

/// 存储 Gas 成本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageGasCosts {
    /// SLOAD（读取存储）
    pub sload: u64,

    /// SSTORE（写入存储）
    pub sstore: u64,

    /// SSTORE（重置存储）
    pub sstore_reset: u64,

    /// SSTORE（清除存储）
    pub sstore_clear: u64,

    /// 创建新账户
    pub create_account: u64,

    /// 删除账户
    pub delete_account: u64,
}

/// 计算 Gas 成本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputationGasCosts {
    /// ADD（加法）
    pub add: u64,

    /// MUL（乘法）
    pub mul: u64,

    /// DIV（除法）
    pub div: u64,

    /// MOD（取模）
    pub mod_op: u64,

    /// EXP（指数）
    pub exp: u64,

    /// 字节操作
    pub byte_ops: u64,

    /// 哈希操作
    pub hash_ops: u64,
}

/// 合约 Gas 成本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractGasCosts {
    /// 合约创建
    pub contract_create: u64,

    /// 合约调用
    pub contract_call: u64,

    /// 合约调用深度
    pub call_depth_cost: u64,

    /// 合约代码字节
    pub contract_code_byte: u64,

    /// 自毁操作
    pub self_destruct: u64,
}

/// Gas 使用记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasUsageRecord {
    /// 区块高度
    pub block_height: u64,

    /// 使用的 Gas 总量
    pub total_gas_used: u64,

    /// Gas 限制
    pub gas_limit: u64,

    /// 平均 Gas 价格
    pub avg_gas_price: BigUint,

    /// 交易数量
    pub transaction_count: u64,

    /// 时间戳
    pub timestamp: u64,
}

impl GasCalculator {
    /// 创建新的 Gas 计算器（不带状态管理器）
    pub fn new(config: GasConfig) -> Self {
        let gas_schedule = Self::create_default_gas_schedule();

        Self {
            config: Arc::new(RwLock::new(config)),
            gas_schedule: Arc::new(RwLock::new(gas_schedule)),
            current_gas_price: Arc::new(RwLock::new(BigUint::from(1000u64))),
            usage_history: Arc::new(RwLock::new(Vec::new())),
            state_manager: None,
        }
    }

    /// 创建新的 Gas 计算器（带状态管理器）
    pub fn with_state_manager(config: GasConfig, state_manager: Arc<AccountStateManager>) -> Self {
        let gas_schedule = Self::create_default_gas_schedule();

        Self {
            config: Arc::new(RwLock::new(config)),
            gas_schedule: Arc::new(RwLock::new(gas_schedule)),
            current_gas_price: Arc::new(RwLock::new(BigUint::from(1000u64))),
            usage_history: Arc::new(RwLock::new(Vec::new())),
            state_manager: Some(state_manager),
        }
    }

    /// 设置状态管理器
    pub fn set_state_manager(&mut self, state_manager: Arc<AccountStateManager>) {
        self.state_manager = Some(state_manager);
    }

    /// 估算交易 Gas 使用量
    pub async fn estimate_gas(&self, transaction: &Transaction) -> Result<u64> {
        debug!("Estimating gas for transaction: {:?}", transaction);

        let gas_schedule = self.gas_schedule.read().await;

        // 1. 计算内在 Gas（intrinsic gas）
        let intrinsic_gas = self
            .calculate_intrinsic_gas(transaction, &gas_schedule)
            .await?;

        // 2. 基础交易成本
        let mut gas_used = intrinsic_gas;

        // 3. 如果是合约创建，添加创建成本
        if transaction.body.receiver.0 == [0u8; 20] {
            gas_used += gas_schedule.contract_costs.contract_create;
            gas_used +=
                transaction.body.data.len() as u64 * gas_schedule.contract_costs.contract_code_byte;
        } else {
            // 4. 如果是合约调用，添加调用成本
            gas_used += gas_schedule.contract_costs.contract_call;
        }

        debug!("Estimated gas: {}", gas_used);
        Ok(gas_used)
    }

    /// 计算交易的内在 Gas（Intrinsic Gas）
    async fn calculate_intrinsic_gas(
        &self,
        transaction: &Transaction,
        gas_schedule: &GasSchedule,
    ) -> Result<u64> {
        // 1. 基础交易成本
        let mut gas = gas_schedule
            .base_costs
            .get("transaction")
            .copied()
            .unwrap_or(21000);

        // 2. 数据成本（EIP-7623: 交易数据成本）
        let (zero_count, non_zero_count) =
            transaction
                .body
                .data
                .iter()
                .fold((0, 0), |(zeros, non_zeros), &byte| {
                    if byte == 0 {
                        (zeros + 1, non_zeros)
                    } else {
                        (zeros, non_zeros + 1)
                    }
                });

        let zero_cost = zero_count * 4;
        let non_zero_cost = non_zero_count * 16;
        let data_cost = zero_cost + non_zero_cost;

        gas += data_cost;

        debug!(
            "Intrinsic gas: base={}, zero_bytes={}, non_zero_bytes={}, data_cost={}, total={}",
            gas_schedule
                .base_costs
                .get("transaction")
                .copied()
                .unwrap_or(21000),
            zero_count,
            non_zero_count,
            data_cost,
            gas
        );

        Ok(gas)
    }

    /// 计算数据传输成本（辅助方法）
    fn calculate_data_cost(&self, data: &[u8], _gas_schedule: &GasSchedule) -> u64 {
        data.iter()
            .fold(0, |acc, &byte| acc + if byte == 0 { 4 } else { 16 })
    }

    /// 计算调用深度 Gas 成本（EIP-150）
    pub fn calculate_call_depth_cost(depth: u64, gas_schedule: &GasSchedule) -> u64 {
        if depth > 0 {
            depth * gas_schedule.contract_costs.call_depth_cost
        } else {
            0
        }
    }

    /// 计算 refunds（Gas 退款）
    pub fn calculate_refund(&self, storage_clears: u64, self_destructs: u64) -> u64 {
        let storage_refund = storage_clears * 15000;
        let self_destruct_refund = self_destructs * 24000;

        let total_refund = storage_refund + self_destruct_refund;
        total_refund
    }

    /// 验证 Gas 价格（EIP-1559: Base fee）
    pub async fn validate_gas_price(
        &self,
        transaction: &Transaction,
        base_fee: Option<&BigUint>,
    ) -> Result<bool> {
        let tx_gas_price = BigUint::from(transaction.body.gas_price.unwrap_or(0));

        if let Some(base) = base_fee {
            if tx_gas_price < *base {
                warn!("Gas price below base fee: {} < {}", tx_gas_price, base);
                return Ok(false);
            }
        }

        let config = self.config.read().await;
        if tx_gas_price < config.min_gas_price {
            warn!(
                "Gas price below minimum: {} < {}",
                tx_gas_price, config.min_gas_price
            );
            return Ok(false);
        }

        Ok(true)
    }

    /// 计算 EIP-1559 费用
    ///
    /// 返回 (base_fee, priority_fee, total_fee)
    pub async fn calculate_eip1559_fees(
        &self,
        gas_limit: u64,
        max_fee_per_gas: &BigUint,
        max_priority_fee_per_gas: &BigUint,
        base_fee: &BigUint,
    ) -> Result<(BigUint, BigUint, BigUint)> {
        // Priority fee 是 min(max_priority_fee_per_gas, max_fee_per_gas - base_fee)
        let priority_fee = if max_priority_fee_per_gas < max_fee_per_gas {
            max_priority_fee_per_gas.clone()
        } else {
            let diff = max_fee_per_gas - base_fee;
            if diff < *max_priority_fee_per_gas {
                diff
            } else {
                max_priority_fee_per_gas.clone()
            }
        };

        // Total fee = (base_fee + priority_fee) * gas_limit
        let effective_gas_price = base_fee + &priority_fee;
        let total_fee = effective_gas_price * gas_limit;

        Ok((base_fee.clone(), priority_fee, total_fee))
    }

    /// 计算实际 Gas 使用量
    pub async fn calculate_gas_usage(
        &self,
        transaction: &Transaction,
        gas_limit: u64,
    ) -> Result<GasUsage> {
        debug!("Calculating gas usage for transaction: {:?}", transaction);

        let gas_price = self.get_current_gas_price().await;
        let estimated_gas = self.estimate_gas(transaction).await?;

        // 检查 Gas 限制
        let exceeded_limit = estimated_gas > gas_limit;
        let actual_gas_used = if exceeded_limit {
            gas_limit // 如果超出限制，只计算到限制的 Gas
        } else {
            estimated_gas
        };

        // 计算费用
        let actual_fee = BigUint::from(actual_gas_used) * &gas_price;
        let max_fee = BigUint::from(gas_limit) * &gas_price;

        let usage = GasUsage {
            gas_used: actual_gas_used,
            gas_limit,
            gas_price: gas_price.clone(),
            actual_fee: actual_fee.clone(),
            max_fee: max_fee.clone(),
            exceeded_limit,
            operations: vec![GasOperation {
                operation_type: "transaction".to_string(),
                gas_used: actual_gas_used,
                description: "Transaction execution".to_string(),
                parameters: HashMap::new(),
            }],
        };

        debug!(
            "Gas usage calculated: used={}, limit={}, fee={}",
            actual_gas_used, gas_limit, actual_fee
        );

        Ok(usage)
    }

    /// 验证交易 Gas
    pub async fn validate_transaction_gas(&self, transaction: &Transaction) -> Result<bool> {
        debug!("Validating transaction gas: {:?}", transaction);

        let config = self.config.read().await;
        let gas_limit = transaction.body.gas as u64;
        if gas_limit > config.max_gas_limit {
            warn!(
                "Gas limit exceeds maximum: {} > {}",
                gas_limit, config.max_gas_limit
            );
            return Ok(false);
        }

        let gas_price = BigUint::from(transaction.body.gas_price.unwrap_or(0));
        if gas_price < config.min_gas_price {
            warn!(
                "Gas price below minimum: {} < {}",
                gas_price, config.min_gas_price
            );
            return Ok(false);
        }

        if gas_price > config.max_gas_price {
            warn!(
                "Gas price above maximum: {} > {}",
                gas_price, config.max_gas_price
            );
            return Ok(false);
        }

        let max_fee = BigUint::from(gas_limit) * &gas_price;
        let val_bytes = transaction
            .body
            .value
            .as_ref()
            .map(|s| s.as_bytes())
            .unwrap_or(&[]);
        let total_cost = BigUint::from_bytes_be(val_bytes) + &max_fee;

        if let Some(state_manager) = &self.state_manager {
            match state_manager.get_account(&transaction.body.address).await {
                Ok(Some(account)) => {
                    let sender_balance = account.balance.clone();
                    if sender_balance < total_cost {
                        warn!(
                            "Insufficient balance: required={}, have={}",
                            total_cost, sender_balance
                        );
                        return Ok(false);
                    }
                    debug!(
                        "Balance check passed: required={}, have={}",
                        total_cost, sender_balance
                    );
                }
                Ok(None) => {
                    warn!(
                        "Sender account does not exist: {:?}",
                        transaction.body.address
                    );
                    return Ok(false);
                }
                Err(e) => {
                    error!("Failed to get sender balance: {:?}", e);
                    warn!("Unable to verify balance due to error, proceeding with caution");
                }
            }
        } else {
            debug!("No state manager available, skipping balance check");
        }

        debug!("Transaction gas validation passed");
        Ok(true)
    }

    /// 获取当前 Gas 价格
    pub async fn get_current_gas_price(&self) -> BigUint {
        self.current_gas_price.read().await.clone()
    }

    /// 更新 Gas 价格
    pub async fn update_gas_price(&self, new_price: BigUint) -> Result<()> {
        let config = self.config.read().await;

        // 检查价格范围
        if new_price < config.min_gas_price {
            return Err(NornError::Validation(
                norn_common::error::ValidationError::InvalidTransaction(
                    "Gas price below minimum".to_string(),
                ),
            ));
        }

        if new_price > config.max_gas_price {
            return Err(NornError::Validation(
                norn_common::error::ValidationError::InvalidTransaction(
                    "Gas price above maximum".to_string(),
                ),
            ));
        }

        // 更新价格
        let mut current_price = self.current_gas_price.write().await;
        *current_price = new_price.clone();

        info!("Gas price updated to: {}", new_price);
        Ok(())
    }

    /// 动态调整 Gas 价格
    pub async fn adjust_gas_price(
        &self,
        block_height: u64,
        total_gas_used: u64,
        gas_limit: u64,
    ) -> Result<()> {
        debug!(
            "Adjusting gas price at block {}: used/limit = {}/{}",
            block_height, total_gas_used, gas_limit
        );

        // 计算使用率
        let utilization_rate = total_gas_used as f64 / gas_limit as f64;

        // 获取当前价格
        let current_price = self.get_current_gas_price().await;
        let config = self.config.read().await;

        // 根据使用率调整价格
        let new_price = if utilization_rate > 0.8 {
            // 高使用率，提高价格
            let increase_factor = 1.0 + config.gas_price_adjustment_factor;
            BigUint::from_f64(current_price.to_f64().unwrap_or(0.0) * increase_factor)
                .unwrap_or_else(|| current_price.clone())
        } else if utilization_rate < 0.3 {
            // 低使用率，降低价格
            let decrease_factor = 1.0 - config.gas_price_adjustment_factor;
            BigUint::from_f64(current_price.to_f64().unwrap_or(0.0) * decrease_factor)
                .unwrap_or_else(|| current_price.clone())
        } else {
            // 使用率适中，保持价格
            current_price
        };

        // 应用价格限制
        let adjusted_price = std::cmp::max(
            std::cmp::min(new_price, config.max_gas_price.clone()),
            config.min_gas_price.clone(),
        );

        self.update_gas_price(adjusted_price.clone()).await?;

        info!(
            "Gas price adjusted: {} (utilization: {:.2}%)",
            adjusted_price,
            utilization_rate * 100.0
        );

        Ok(())
    }

    /// 记录 Gas 使用
    pub async fn record_gas_usage(&self, block_height: u64, gas_usage: &GasUsage) -> Result<()> {
        debug!(
            "Recording gas usage for block {}: used={}/{}",
            block_height, gas_usage.gas_used, gas_usage.gas_limit
        );

        let record = GasUsageRecord {
            block_height,
            total_gas_used: gas_usage.gas_used,
            gas_limit: gas_usage.gas_limit,
            avg_gas_price: gas_usage.gas_price.clone(),
            transaction_count: 1, // 单笔交易的 gas 使用记录
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let mut history = self.usage_history.write().await;
        history.push(record);

        // 保持历史记录在合理范围内
        if history.len() > 1000 {
            history.remove(0);
        }

        debug!("Gas usage recorded");
        Ok(())
    }

    /// 获取 Gas 使用统计
    pub async fn get_gas_stats(&self) -> GasStats {
        let history = self.usage_history.read().await;
        let current_price = self.get_current_gas_price().await;

        let mut stats = GasStats::default();

        if !history.is_empty() {
            let total_blocks = history.len() as u64;
            let total_gas_used: u64 = history.iter().map(|r| r.total_gas_used).sum();
            let total_gas_limit: u64 = history.iter().map(|r| r.gas_limit).sum();

            stats.avg_gas_used = total_gas_used / total_blocks;
            stats.avg_gas_limit = total_gas_limit / total_blocks;
            stats.avg_utilization = (total_gas_used as f64 / total_gas_limit as f64) * 100.0;
            stats.current_gas_price = current_price;
            stats.total_blocks = total_blocks;
            stats.total_gas_used = total_gas_used;
        }

        stats
    }

    /// 创建默认 Gas 调度表
    fn create_default_gas_schedule() -> GasSchedule {
        let mut base_costs = HashMap::new();
        base_costs.insert("transaction".to_string(), 21000);
        base_costs.insert("zero_byte".to_string(), 4);
        base_costs.insert("non_zero_byte".to_string(), 16);
        base_costs.insert("log".to_string(), 375);
        base_costs.insert("log_data_byte".to_string(), 8);
        base_costs.insert("log_topic".to_string(), 375);

        GasSchedule {
            base_costs,
            storage_costs: StorageGasCosts {
                sload: 200,
                sstore: 20000,
                sstore_reset: 5000,
                sstore_clear: 5000,
                create_account: 25000,
                delete_account: 0, // 返还 Gas
            },
            computation_costs: ComputationGasCosts {
                add: 3,
                mul: 5,
                div: 5,
                mod_op: 5,
                exp: 10,
                byte_ops: 3,
                hash_ops: 30,
            },
            contract_costs: ContractGasCosts {
                contract_create: 32000,
                contract_call: 700,
                call_depth_cost: 700,
                contract_code_byte: 200,
                self_destruct: 5000,
            },
        }
    }
}

/// Gas 统计信息
#[derive(Debug, Clone, Default)]
pub struct GasStats {
    pub avg_gas_used: u64,
    pub avg_gas_limit: u64,
    pub avg_utilization: f64,
    pub current_gas_price: BigUint,
    pub total_blocks: u64,
    pub total_gas_used: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::Transaction;

    #[tokio::test]
    async fn test_gas_estimation() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);

        let transaction = Transaction {
            body: norn_common::types::TransactionBody {
                address: Address::default(),
                receiver: Address::default(),
                gas: 21000,
                gas_price: Some(1),
                data: vec![],
                ..Default::default()
            },
        };

        let estimated_gas = calculator.estimate_gas(&transaction).await.unwrap();
        assert!(estimated_gas >= 21000, "Base gas should be at least 21,000");
    }

    #[tokio::test]
    async fn test_intrinsic_gas_calculation() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);

        // Test with zero data
        let tx_zero_data = Transaction {
            body: norn_common::types::TransactionBody {
                address: Address::default(),
                receiver: Address::default(),
                gas: 100000,
                gas_price: Some(1),
                data: vec![0u8; 100],
                ..Default::default()
            },
        };

        let gas_schedule = calculator.gas_schedule.read().await;
        let intrinsic = calculator
            .calculate_intrinsic_gas(&tx_zero_data, &gas_schedule)
            .await
            .unwrap();

        assert_eq!(
            intrinsic, 21400,
            "Intrinsic gas calculation incorrect for zero data"
        );
    }

    #[tokio::test]
    async fn test_intrinsic_gas_with_non_zero_data() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);

        let tx_non_zero = Transaction {
            body: norn_common::types::TransactionBody {
                address: Address::default(),
                receiver: Address::default(),
                gas: 100000,
                gas_price: Some(1),
                data: vec![0xFF; 50],
                ..Default::default()
            },
        };

        let gas_schedule = calculator.gas_schedule.read().await;
        let intrinsic = calculator
            .calculate_intrinsic_gas(&tx_non_zero, &gas_schedule)
            .await
            .unwrap();

        assert_eq!(
            intrinsic, 21800,
            "Intrinsic gas calculation incorrect for non-zero data"
        );
    }

    #[tokio::test]
    async fn test_intrinsic_gas_mixed_data() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);

        let mut data = vec![0u8; 100];
        data[0..50].copy_from_slice(&[0xFF; 50]);

        let tx_mixed = Transaction {
            body: norn_common::types::TransactionBody {
                address: Address::default(),
                receiver: Address::default(),
                gas: 100000,
                gas_price: Some(1),
                data,
                ..Default::default()
            },
        };

        let gas_schedule = calculator.gas_schedule.read().await;
        let intrinsic = calculator
            .calculate_intrinsic_gas(&tx_mixed, &gas_schedule)
            .await
            .unwrap();

        assert_eq!(
            intrinsic, 22000,
            "Intrinsic gas calculation incorrect for mixed data"
        );
    }

    #[tokio::test]
    async fn test_call_depth_cost() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);
        let gas_schedule = calculator.gas_schedule.read().await;

        let cost_depth_0 = GasCalculator::calculate_call_depth_cost(0, &gas_schedule);
        assert_eq!(cost_depth_0, 0, "Zero depth should have zero cost");

        let cost_depth_1 = GasCalculator::calculate_call_depth_cost(1, &gas_schedule);
        assert_eq!(cost_depth_1, 700, "Depth 1 should cost 700");

        let cost_depth_5 = GasCalculator::calculate_call_depth_cost(5, &gas_schedule);
        assert_eq!(cost_depth_5, 3500, "Depth 5 should cost 3500");
    }

    #[tokio::test]
    async fn test_gas_refund() {
        let calculator = GasCalculator::new(GasConfig::default());

        let refund = calculator.calculate_refund(2, 1);
        assert_eq!(refund, 54000, "Refund should be 2*15000 + 1*24000 = 54000");

        let refund_zero = calculator.calculate_refund(0, 0);
        assert_eq!(refund_zero, 0, "No operations should have zero refund");
    }

    #[tokio::test]
    async fn test_eip1559_fee_calculation() {
        let calculator = GasCalculator::new(GasConfig::default());

        let base_fee = BigUint::from(100u64);
        let max_fee = BigUint::from(200u64);
        let max_priority = BigUint::from(50u64);
        let gas_limit = 21000u64;

        let (base, priority, total) = calculator
            .calculate_eip1559_fees(gas_limit, &max_fee, &max_priority, &base_fee)
            .await
            .unwrap();

        assert_eq!(base, BigUint::from(100u64));
        assert_eq!(priority, BigUint::from(50u64));
        assert_eq!(total, BigUint::from(3150000u64));
    }

    #[tokio::test]
    async fn test_gas_price_validation() {
        let calculator = GasCalculator::new(GasConfig::default());

        let tx_valid = Transaction {
            body: norn_common::types::TransactionBody {
                address: Address::default(),
                receiver: Address::default(),
                gas: 21000,
                gas_price: Some(100),
                data: vec![],
                ..Default::default()
            },
        };

        let result = calculator
            .validate_gas_price(&tx_valid, None)
            .await
            .unwrap();
        assert!(result, "Valid gas price should pass");

        let base_fee = BigUint::from(50u64);
        let result = calculator
            .validate_gas_price(&tx_valid, Some(&base_fee))
            .await
            .unwrap();
        assert!(result, "Gas price above base fee should pass");

        let base_fee_high = BigUint::from(200u64);
        let result = calculator
            .validate_gas_price(&tx_valid, Some(&base_fee_high))
            .await
            .unwrap();
        assert!(!result, "Gas price below base fee should fail");
    }

    #[tokio::test]
    async fn test_gas_usage_calculation() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);

        let transaction = Transaction {
            body: norn_common::types::TransactionBody {
                address: Address::default(),
                receiver: Address([1u8; 20]),
                gas: 30000,
                gas_price: Some(1),
                data: vec![],
                ..Default::default()
            },
        };

        let usage = calculator
            .calculate_gas_usage(&transaction, 30000)
            .await
            .unwrap();
        assert_eq!(usage.gas_limit, 30000);
        assert!(!usage.exceeded_limit);
    }

    #[tokio::test]
    async fn test_gas_price_adjustment() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);

        calculator
            .adjust_gas_price(100, 8000000, 10000000)
            .await
            .unwrap();
        let high_price = calculator.get_current_gas_price().await;

        calculator
            .adjust_gas_price(101, 2000000, 10000000)
            .await
            .unwrap();
        let low_price = calculator.get_current_gas_price().await;

        assert!(high_price > low_price);
    }

    #[tokio::test]
    async fn test_gas_validation() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);

        let valid_transaction = Transaction {
            body: norn_common::types::TransactionBody {
                address: Address::default(),
                receiver: Address::default(),
                gas: 21000,
                gas_price: Some(1000),
                data: vec![],
                ..Default::default()
            },
        };

        let invalid_transaction = Transaction {
            body: norn_common::types::TransactionBody {
                address: Address::default(),
                receiver: Address::default(),
                gas: 21000,
                gas_price: Some(0),
                data: vec![],
                ..Default::default()
            },
        };

        assert!(calculator
            .validate_transaction_gas(&valid_transaction)
            .await
            .unwrap());
        assert!(!calculator
            .validate_transaction_gas(&invalid_transaction)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_gas_stats() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);

        // 记录一些使用数据
        let usage = GasUsage {
            gas_used: 15000,
            gas_limit: 21000,
            gas_price: BigUint::from(1000u64),
            actual_fee: BigUint::from(15000000u64),
            max_fee: BigUint::from(21000000u64),
            exceeded_limit: false,
            operations: vec![],
        };

        calculator.record_gas_usage(1, &usage).await.unwrap();
        calculator.record_gas_usage(2, &usage).await.unwrap();

        let stats = calculator.get_gas_stats().await;
        assert_eq!(stats.total_blocks, 2);
        assert_eq!(stats.total_gas_used, 30000);
        assert_eq!(stats.avg_gas_used, 15000);
    }
}
