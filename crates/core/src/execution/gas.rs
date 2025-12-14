use norn_common::types::{Transaction, Address};
use norn_common::error::{NornError, Result};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use num_bigint::BigUint;
use num_traits::{Zero, One};

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
    /// 创建新的 Gas 计算器
    pub fn new(config: GasConfig) -> Self {
        let gas_schedule = Self::create_default_gas_schedule();
        
        Self {
            config: Arc::new(RwLock::new(config)),
            gas_schedule: Arc::new(RwLock::new(gas_schedule)),
            current_gas_price: Arc::new(RwLock::new(BigUint::from(1000u64))),
            usage_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 估算交易 Gas 使用量
    pub async fn estimate_gas(&self, transaction: &Transaction) -> Result<u64> {
        debug!("Estimating gas for transaction: {:?}", transaction);
        
        let gas_schedule = self.gas_schedule.read().await;
        
        // 1. 基础交易成本
        let mut gas_used = gas_schedule.base_costs.get("transaction").copied().unwrap_or(21000);
        
        // 2. 数据传输成本
        let data_cost = self.calculate_data_cost(&transaction.data, &gas_schedule);
        gas_used += data_cost;
        
        // 3. 如果是合约创建，添加创建成本
        if transaction.to.is_none() {
            gas_used += gas_schedule.contract_costs.contract_create;
            gas_used += transaction.data.len() as u64 * gas_schedule.contract_costs.contract_code_byte;
        }
        
        // 4. 如果是合约调用，添加调用成本
        if transaction.to.is_some() {
            gas_used += gas_schedule.contract_costs.contract_call;
        }
        
        debug!("Estimated gas: {}", gas_used);
        Ok(gas_used)
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
            operations: vec![
                GasOperation {
                    operation_type: "transaction".to_string(),
                    gas_used: actual_gas_used,
                    description: "Transaction execution".to_string(),
                    parameters: HashMap::new(),
                }
            ],
        };
        
        debug!("Gas usage calculated: used={}, limit={}, fee={}", 
                actual_gas_used, gas_limit, actual_fee);
        
        Ok(usage)
    }

    /// 验证交易 Gas
    pub async fn validate_transaction_gas(&self, transaction: &Transaction) -> Result<bool> {
        debug!("Validating transaction gas: {:?}", transaction);
        
        // 1. 检查 Gas 限制
        let config = self.config.read().await;
        if transaction.gas_limit > config.max_gas_limit {
            warn!("Gas limit exceeds maximum: {} > {}", 
                   transaction.gas_limit, config.max_gas_limit);
            return Ok(false);
        }
        
        // 2. 检查 Gas 价格
        let gas_price = BigUint::from_bytes(&transaction.gas_price);
        if gas_price < config.min_gas_price {
            warn!("Gas price below minimum: {} < {}", 
                   gas_price, config.min_gas_price);
            return Ok(false);
        }
        
        if gas_price > config.max_gas_price {
            warn!("Gas price above maximum: {} > {}", 
                   gas_price, config.max_gas_price);
            return Ok(false);
        }
        
        // 3. 检查账户余额是否足够支付费用
        let max_fee = BigUint::from(transaction.gas_limit) * &gas_price;
        let total_cost = BigUint::from_bytes(&transaction.value) + &max_fee;
        
        // TODO: 检查发送者余额
        // let sender_balance = self.get_balance(&transaction.from).await?;
        // if sender_balance < total_cost {
        //     warn!("Insufficient balance for gas fees");
        //     return Ok(false);
        // }
        
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
            return Err(NornError::ValidationError("Gas price below minimum".to_string()));
        }
        
        if new_price > config.max_gas_price {
            return Err(NornError::ValidationError("Gas price above maximum".to_string()));
        }
        
        // 更新价格
        let mut current_price = self.current_gas_price.write().await;
        *current_price = new_price;
        
        info!("Gas price updated to: {}", new_price);
        Ok(())
    }

    /// 动态调整 Gas 价格
    pub async fn adjust_gas_price(&self, block_height: u64, total_gas_used: u64, gas_limit: u64) -> Result<()> {
        debug!("Adjusting gas price at block {}: used/limit = {}/{}", 
                block_height, total_gas_used, gas_limit);
        
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
            config.min_gas_price.clone()
        );
        
        self.update_gas_price(adjusted_price).await?;
        
        info!("Gas price adjusted: {} (utilization: {:.2}%)", 
               adjusted_price, utilization_rate * 100.0);
        
        Ok(())
    }

    /// 记录 Gas 使用
    pub async fn record_gas_usage(&self, block_height: u64, gas_usage: &GasUsage) -> Result<()> {
        debug!("Recording gas usage for block {}: used={}/{}", 
                block_height, gas_usage.gas_used, gas_usage.gas_limit);
        
        let record = GasUsageRecord {
            block_height,
            total_gas_used: gas_usage.gas_used,
            gas_limit: gas_usage.gas_limit,
            avg_gas_price: gas_usage.gas_price.clone(),
            transaction_count: 1, // TODO: 从实际交易数量获取
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

    /// 计算数据传输成本
    fn calculate_data_cost(&self, data: &[u8], gas_schedule: &GasSchedule) -> u64 {
        let zero_bytes = data.iter().filter(|&&b| b == 0).count() as u64;
        let non_zero_bytes = data.len() as u64 - zero_bytes;
        
        // 零字节成本较低，非零字节成本较高
        let zero_cost = zero_bytes * gas_schedule.base_costs.get("zero_byte").copied().unwrap_or(4);
        let non_zero_cost = non_zero_bytes * gas_schedule.base_costs.get("non_zero_byte").copied().unwrap_or(16);
        
        zero_cost + non_zero_cost
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
            from: Address::default(),
            to: Some(Address::default()),
            value: vec![100u8; 32],
            gas_price: vec![1u8; 32],
            gas_limit: 21000,
            nonce: 0,
            data: vec![],
            signature: vec![],
        };
        
        let estimated_gas = calculator.estimate_gas(&transaction).await.unwrap();
        assert!(estimated_gas > 0);
    }

    #[tokio::test]
    async fn test_gas_usage_calculation() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);
        
        let transaction = Transaction {
            from: Address::default(),
            to: Some(Address::default()),
            value: vec![100u8; 32],
            gas_price: vec![1u8; 32],
            gas_limit: 21000,
            nonce: 0,
            data: vec![],
            signature: vec![],
        };
        
        let usage = calculator.calculate_gas_usage(&transaction, 21000).await.unwrap();
        assert_eq!(usage.gas_limit, 21000);
        assert!(!usage.exceeded_limit);
    }

    #[tokio::test]
    async fn test_gas_price_adjustment() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);
        
        // 高使用率应该提高价格
        calculator.adjust_gas_price(100, 8000000, 10000000).await.unwrap();
        let high_price = calculator.get_current_gas_price().await;
        
        // 低使用率应该降低价格
        calculator.adjust_gas_price(101, 2000000, 10000000).await.unwrap();
        let low_price = calculator.get_current_gas_price().await;
        
        // 高使用率时的价格应该高于低使用率时
        assert!(high_price > low_price);
    }

    #[tokio::test]
    async fn test_gas_validation() {
        let config = GasConfig::default();
        let calculator = GasCalculator::new(config);
        
        let valid_transaction = Transaction {
            from: Address::default(),
            to: Some(Address::default()),
            value: vec![100u8; 32],
            gas_price: vec![1000u8; 32], // 合理的价格
            gas_limit: 21000,
            nonce: 0,
            data: vec![],
            signature: vec![],
        };
        
        let invalid_transaction = Transaction {
            from: Address::default(),
            to: Some(Address::default()),
            value: vec![100u8; 32],
            gas_price: vec![0u8; 32], // 价格太低
            gas_limit: 21000,
            nonce: 0,
            data: vec![],
            signature: vec![],
        };
        
        assert!(calculator.validate_transaction_gas(&valid_transaction).await.unwrap());
        assert!(!calculator.validate_transaction_gas(&invalid_transaction).await.unwrap());
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