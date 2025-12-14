// 交易执行器
// 
// 负责执行交易并更新区块链状态

use crate::execution::gas::{GasCalculator, GasUsage, GasSchedule};
use crate::state::{StateDB, AccountStateManager, AccountState, StateChange};
use crate::types::{Transaction, TransactionResult, Address, H256, U256, Wei};
use crate::crypto::hash;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};

/// 交易执行上下文
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// 区块号
    pub block_number: u64,
    /// 区块时间戳
    pub block_timestamp: u64,
    /// 区块提议者
    pub block_proposer: Address,
    /// Gas 价格
    pub gas_price: Wei,
    /// 区块 Gas 限制
    pub block_gas_limit: U256,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            block_number: 0,
            block_timestamp: chrono::Utc::now().timestamp() as u64,
            block_proposer: Address::default(),
            gas_price: Wei::from(1_000_000_000u64), // 1 Gwei
            block_gas_limit: U256::from(30_000_000u64), // 30 million
        }
    }
}

/// 交易执行结果
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// 交易哈希
    pub tx_hash: H256,
    /// 是否成功
    pub success: bool,
    /// 错误信息
    pub error: Option<String>,
    /// 使用的 Gas
    pub gas_used: U256,
    /// Gas 费用
    pub gas_fee: Wei,
    /// 状态变更
    pub state_changes: Vec<StateChange>,
    /// 日志
    pub logs: Vec<TransactionLog>,
    /// 返回数据
    pub return_data: Vec<u8>,
}

/// 交易日志
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionLog {
    /// 地址
    pub address: Address,
    /// 主题
    pub topics: Vec<H256>,
    /// 数据
    pub data: Vec<u8>,
}

/// 交易验证器
pub struct TransactionValidator {
    gas_calculator: Arc<GasCalculator>,
}

impl TransactionValidator {
    pub fn new(gas_calculator: Arc<GasCalculator>) -> Self {
        Self { gas_calculator }
    }

    /// 验证交易
    pub async fn validate_transaction(
        &self,
        tx: &Transaction,
        ctx: &ExecutionContext,
        sender_state: &AccountState,
    ) -> Result<()> {
        // 验证基本字段
        if tx.to.is_none() && tx.data.is_empty() {
            return Err(anyhow!("合约创建交易必须包含代码"));
        }

        // 验证 Nonce
        if tx.nonce != sender_state.nonce {
            return Err(anyhow!(
                "无效的 nonce: 期望 {}, 得到 {}",
                sender_state.nonce,
                tx.nonce
            ));
        }

        // 验证余额
        let required_balance = tx.value + Wei::from(tx.gas_limit * tx.gas_price);
        if sender_state.balance < required_balance {
            return Err(anyhow!(
                "余额不足: 需要 {}, 可用 {}",
                required_balance,
                sender_state.balance
            ));
        }

        // 验证 Gas 限制
        let min_gas_limit = self.gas_calculator.calculate_min_gas_limit(tx).await?;
        if tx.gas_limit < min_gas_limit {
            return Err(anyhow!(
                "Gas 限制过低: 最小 {}, 指定 {}",
                min_gas_limit,
                tx.gas_limit
            ));
        }

        // 验证 Gas 价格
        if tx.gas_price < ctx.gas_price {
            return Err(anyhow!(
                "Gas 价格过低: 最小 {}, 指定 {}",
                ctx.gas_price,
                tx.gas_price
            ));
        }

        // 验证签名
        if !self.verify_signature(tx) {
            return Err(anyhow!("无效的交易签名"));
        }

        Ok(())
    }

    /// 验证交易签名
    fn verify_signature(&self, tx: &Transaction) -> bool {
        // 这里应该实现实际的签名验证逻辑
        // 暂时返回 true 作为占位符
        true
    }
}

/// 交易执行器
pub struct TransactionExecutor {
    state_db: Arc<RwLock<StateDB>>,
    account_manager: Arc<RwLock<AccountStateManager>>,
    gas_calculator: Arc<GasCalculator>,
    validator: TransactionValidator,
}

impl TransactionExecutor {
    pub fn new(
        state_db: Arc<RwLock<StateDB>>,
        account_manager: Arc<RwLock<AccountStateManager>>,
        gas_calculator: Arc<GasCalculator>,
    ) -> Self {
        let validator = TransactionValidator::new(gas_calculator.clone());
        Self {
            state_db,
            account_manager,
            gas_calculator,
            validator,
        }
    }

    /// 执行单个交易
    pub async fn execute_transaction(
        &self,
        tx: &Transaction,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult> {
        let tx_hash = hash::hash_transaction(tx);
        debug!("开始执行交易: {:?}", tx_hash);

        // 获取发送者状态
        let sender_state = self
            .account_manager
            .read()
            .await
            .get_account_state(&tx.from)
            .await?
            .unwrap_or_default();

        // 验证交易
        if let Err(e) = self.validator.validate_transaction(tx, ctx, &sender_state).await {
            warn!("交易验证失败: {} - {:?}", tx_hash, e);
            return Ok(ExecutionResult {
                tx_hash,
                success: false,
                error: Some(e.to_string()),
                gas_used: U256::zero(),
                gas_fee: Wei::zero(),
                state_changes: vec![],
                logs: vec![],
                return_data: vec![],
            });
        }

        // 执行交易
        let execution_result = self.execute_valid_transaction(tx, ctx, &sender_state).await;

        match execution_result {
            Ok(mut result) => {
                result.tx_hash = tx_hash;
                info!("交易执行成功: {:?}, Gas 使用: {}", tx_hash, result.gas_used);
                Ok(result)
            }
            Err(e) => {
                error!("交易执行失败: {:?} - {:?}", tx_hash, e);
                Ok(ExecutionResult {
                    tx_hash,
                    success: false,
                    error: Some(e.to_string()),
                    gas_used: U256::zero(),
                    gas_fee: Wei::zero(),
                    state_changes: vec![],
                    logs: vec![],
                    return_data: vec![],
                })
            }
        }
    }

    /// 执行已验证的交易
    async fn execute_valid_transaction(
        &self,
        tx: &Transaction,
        ctx: &ExecutionContext,
        sender_state: &AccountState,
    ) -> Result<ExecutionResult> {
        let mut state_changes = Vec::new();
        let mut logs = Vec::new();

        // 1. 扣除发送者的余额和 Gas 费用
        let gas_cost = Wei::from(tx.gas_limit * tx.gas_price);
        let total_cost = tx.value + gas_cost;

        if sender_state.balance < total_cost {
            return Err(anyhow!("余额不足"));
        }

        // 更新发送者状态
        let mut new_sender_state = sender_state.clone();
        new_sender_state.balance -= total_cost;
        new_sender_state.nonce += 1;

        state_changes.push(StateChange::Account {
            address: tx.from,
            old_state: sender_state.clone(),
            new_state: new_sender_state.clone(),
        });

        // 2. 执行交易逻辑
        let (gas_used, return_data) = match tx.to {
            Some(to) => {
                // 转账到现有账户
                self.execute_transfer(tx, &mut state_changes, &mut logs).await?
            }
            None => {
                // 创建合约
                self.execute_contract_creation(tx, ctx, &mut state_changes, &mut logs)
                    .await?
            }
        };

        // 3. 计算实际 Gas 费用
        let actual_gas_cost = Wei::from(gas_used * tx.gas_price);
        let gas_refund = gas_cost - actual_gas_cost;

        // 退还未使用的 Gas
        if gas_refund > Wei::zero() {
            new_sender_state.balance += gas_refund;
            // 更新状态变更
            if let Some(last_change) = state_changes.last_mut() {
                if let StateChange::Account { new_state, .. } = last_change {
                    new_state.balance = new_sender_state.balance;
                }
            }
        }

        // 4. 应用状态变更
        self.apply_state_changes(&state_changes).await?;

        Ok(ExecutionResult {
            tx_hash: H256::default(), // 将在调用方设置
            success: true,
            error: None,
            gas_used,
            gas_fee: actual_gas_cost,
            state_changes,
            logs,
            return_data,
        })
    }

    /// 执行转账
    async fn execute_transfer(
        &self,
        tx: &Transaction,
        state_changes: &mut Vec<StateChange>,
        logs: &mut Vec<TransactionLog>,
    ) -> Result<(U256, Vec<u8>)> {
        let to = tx.to.unwrap();
        
        // 获取接收者状态
        let receiver_state = self
            .account_manager
            .read()
            .await
            .get_account_state(&to)
            .await?
            .unwrap_or_default();

        // 更新接收者状态
        let mut new_receiver_state = receiver_state.clone();
        new_receiver_state.balance += tx.value;

        state_changes.push(StateChange::Account {
            address: to,
            old_state: receiver_state,
            new_state: new_receiver_state,
        });

        // 创建转账日志
        let log = TransactionLog {
            address: tx.from,
            topics: vec![
                H256::from_slice(&[0u8; 32]), // Transfer 事件签名
                H256::from_slice(&tx.from.as_bytes()),
                H256::from_slice(&to.as_bytes()),
            ],
            data: tx.value.to_be_bytes().to_vec(),
        };
        logs.push(log);

        // 转账的 Gas 使用量
        let gas_used = self.gas_calculator.calculate_transfer_gas().await?;

        Ok((gas_used, vec![]))
    }

    /// 执行合约创建
    async fn execute_contract_creation(
        &self,
        tx: &Transaction,
        ctx: &ExecutionContext,
        state_changes: &mut Vec<StateChange>,
        logs: &mut Vec<TransactionLog>,
    ) -> Result<(U256, Vec<u8>)> {
        // 生成合约地址
        let contract_address = self.generate_contract_address(&tx.from, tx.nonce);

        // 检查地址是否已存在
        let existing_state = self
            .account_manager
            .read()
            .await
            .get_account_state(&contract_address)
            .await?;

        if existing_state.is_some() {
            return Err(anyhow!("合约地址已存在"));
        }

        // 创建合约账户状态
        let contract_state = AccountState {
            balance: tx.value,
            nonce: 1,
            code_hash: hash::hash_bytes(&tx.data),
            code: Some(tx.data.clone()),
            storage_root: H256::default(),
        };

        state_changes.push(StateChange::Account {
            address: contract_address,
            old_state: AccountState::default(),
            new_state: contract_state,
        });

        // 创建合约创建日志
        let log = TransactionLog {
            address: contract_address,
            topics: vec![
                H256::from_slice(&[1u8; 32]), // ContractCreated 事件签名
            ],
            data: contract_address.as_bytes().to_vec(),
        };
        logs.push(log);

        // 合约创建的 Gas 使用量
        let gas_used = self.gas_calculator.calculate_contract_creation_gas(&tx.data).await?;

        Ok((gas_used, contract_address.as_bytes().to_vec()))
    }

    /// 生成合约地址
    fn generate_contract_address(&sender: &Address, nonce: u64) -> Address {
        use crate::crypto::hash::hash_bytes;
        
        let mut data = Vec::new();
        data.extend_from_slice(sender.as_bytes());
        data.extend_from_slice(&nonce.to_be_bytes());
        
        let hash = hash_bytes(&data);
        Address::from_slice(&hash[12..32])
    }

    /// 应用状态变更
    async fn apply_state_changes(&self, changes: &[StateChange]) -> Result<()> {
        let mut state_db = self.state_db.write().await;
        let mut account_manager = self.account_manager.write().await;

        for change in changes {
            match change {
                StateChange::Account { address, new_state, .. } => {
                    account_manager
                        .update_account_state(*address, new_state.clone())
                        .await?;
                    state_db.update_account(*address, new_state).await?;
                }
                StateChange::Storage { address, key, value } => {
                    state_db.update_storage(*address, *key, *value).await?;
                }
                StateChange::Code { address, code } => {
                    state_db.update_code(*address, code.clone()).await?;
                }
            }
        }

        Ok(())
    }

    /// 批量执行交易
    pub async fn execute_transactions(
        &self,
        transactions: &[Transaction],
        ctx: &ExecutionContext,
    ) -> Result<Vec<ExecutionResult>> {
        let mut results = Vec::new();
        let mut total_gas_used = U256::zero();

        for tx in transactions {
            // 检查区块 Gas 限制
            if total_gas_used + tx.gas_limit > ctx.block_gas_limit {
                warn!("达到区块 Gas 限制，停止执行交易");
                break;
            }

            let result = self.execute_transaction(tx, ctx).await?;
            total_gas_used += result.gas_used;
            results.push(result);
        }

        Ok(results)
    }

    /// 估算交易 Gas
    pub async fn estimate_gas(
        &self,
        tx: &Transaction,
        ctx: &ExecutionContext,
    ) -> Result<U256> {
        // 获取发送者状态
        let sender_state = self
            .account_manager
            .read()
            .await
            .get_account_state(&tx.from)
            .await?
            .unwrap_or_default();

        // 验证基本条件
        self.validator.validate_transaction(tx, ctx, &sender_state).await?;

        // 估算 Gas 使用量
        let gas_used = match tx.to {
            Some(_) => self.gas_calculator.calculate_transfer_gas().await?,
            None => self.gas_calculator.calculate_contract_creation_gas(&tx.data).await?,
        };

        // 添加一些缓冲
        Ok(gas_used + U256::from(21000u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Transaction;
    use std::str::FromStr;

    async fn create_test_executor() -> TransactionExecutor {
        let state_db = Arc::new(RwLock::new(StateDB::new(":memory:").await.unwrap()));
        let account_manager = Arc::new(RwLock::new(AccountStateManager::new(state_db.clone())));
        let gas_calculator = Arc::new(GasCalculator::new(GasSchedule::default()));
        
        TransactionExecutor::new(state_db, account_manager, gas_calculator)
    }

    #[tokio::test]
    async fn test_transfer_execution() {
        let executor = create_test_executor().await;
        let ctx = ExecutionContext::default();

        // 创建测试交易
        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let to = Address::from_str("0x9876543210987654321098765432109876543210").unwrap();

        let tx = Transaction {
            from,
            to: Some(to),
            value: Wei::from(1000u64),
            gas_limit: 21000,
            gas_price: Wei::from(1_000_000_000u64),
            nonce: 0,
            data: vec![],
            signature: vec![],
        };

        // 设置发送者余额
        let mut account_manager = executor.account_manager.write().await;
        account_manager
            .update_account_state(from, AccountState {
                balance: Wei::from(1_000_000u64),
                nonce: 0,
                code_hash: H256::default(),
                code: None,
                storage_root: H256::default(),
            })
            .await
            .unwrap();
        drop(account_manager);

        // 执行交易
        let result = executor.execute_transaction(&tx, &ctx).await.unwrap();

        assert!(result.success);
        assert!(result.error.is_none());
        assert!(result.gas_used > U256::zero());
        assert_eq!(result.logs.len(), 1);
    }

    #[tokio::test]
    async fn test_contract_creation() {
        let executor = create_test_executor().await;
        let ctx = ExecutionContext::default();

        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let contract_code = vec![0x60, 0x61, 0x02]; // 简单的合约代码

        let tx = Transaction {
            from,
            to: None, // 合约创建
            value: Wei::zero(),
            gas_limit: 100000,
            gas_price: Wei::from(1_000_000_000u64),
            nonce: 0,
            data: contract_code.clone(),
            signature: vec![],
        };

        // 设置发送者余额
        let mut account_manager = executor.account_manager.write().await;
        account_manager
            .update_account_state(from, AccountState {
                balance: Wei::from(1_000_000u64),
                nonce: 0,
                code_hash: H256::default(),
                code: None,
                storage_root: H256::default(),
            })
            .await
            .unwrap();
        drop(account_manager);

        // 执行交易
        let result = executor.execute_transaction(&tx, &ctx).await.unwrap();

        assert!(result.success);
        assert!(result.error.is_none());
        assert!(result.gas_used > U256::zero());
        assert_eq!(result.logs.len(), 1);
        assert!(!result.return_data.is_empty()); // 合约地址
    }

    #[tokio::test]
    async fn test_insufficient_balance() {
        let executor = create_test_executor().await;
        let ctx = ExecutionContext::default();

        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let to = Address::from_str("0x9876543210987654321098765432109876543210").unwrap();

        let tx = Transaction {
            from,
            to: Some(to),
            value: Wei::from(1_000_000u64), // 大于余额
            gas_limit: 21000,
            gas_price: Wei::from(1_000_000_000u64),
            nonce: 0,
            data: vec![],
            signature: vec![],
        };

        // 设置不足的余额
        let mut account_manager = executor.account_manager.write().await;
        account_manager
            .update_account_state(from, AccountState {
                balance: Wei::from(1000u64), // 不足的余额
                nonce: 0,
                code_hash: H256::default(),
                code: None,
                storage_root: H256::default(),
            })
            .await
            .unwrap();
        drop(account_manager);

        // 执行交易
        let result = executor.execute_transaction(&tx, &ctx).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(result.gas_used, U256::zero());
    }

    #[tokio::test]
    async fn test_batch_execution() {
        let executor = create_test_executor().await;
        let mut ctx = ExecutionContext::default();
        ctx.block_gas_limit = U256::from(100_000u64); // 较小的限制

        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let to = Address::from_str("0x9876543210987654321098765432109876543210").unwrap();

        // 创建多个交易
        let transactions = vec![
            Transaction {
                from,
                to: Some(to),
                value: Wei::from(100u64),
                gas_limit: 21000,
                gas_price: Wei::from(1_000_000_000u64),
                nonce: 0,
                data: vec![],
                signature: vec![],
            },
            Transaction {
                from,
                to: Some(to),
                value: Wei::from(200u64),
                gas_limit: 21000,
                gas_price: Wei::from(1_000_000_000u64),
                nonce: 1,
                data: vec![],
                signature: vec![],
            },
            Transaction {
                from,
                to: Some(to),
                value: Wei::from(300u64),
                gas_limit: 21000,
                gas_price: Wei::from(1_000_000_000u64),
                nonce: 2,
                data: vec![],
                signature: vec![],
            },
        ];

        // 设置足够的余额
        let mut account_manager = executor.account_manager.write().await;
        account_manager
            .update_account_state(from, AccountState {
                balance: Wei::from(10_000_000u64),
                nonce: 0,
                code_hash: H256::default(),
                code: None,
                storage_root: H256::default(),
            })
            .await
            .unwrap();
        drop(account_manager);

        // 批量执行
        let results = executor.execute_transactions(&transactions, &ctx).await.unwrap();

        // 由于 Gas 限制，只能执行部分交易
        assert!(results.len() < 3);
        for result in &results {
            assert!(result.success);
        }
    }

    #[tokio::test]
    async fn test_gas_estimation() {
        let executor = create_test_executor().await;
        let ctx = ExecutionContext::default();

        let from = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let to = Address::from_str("0x9876543210987654321098765432109876543210").unwrap();

        let tx = Transaction {
            from,
            to: Some(to),
            value: Wei::from(1000u64),
            gas_limit: 21000,
            gas_price: Wei::from(1_000_000_000u64),
            nonce: 0,
            data: vec![],
            signature: vec![],
        };

        // 设置发送者状态
        let mut account_manager = executor.account_manager.write().await;
        account_manager
            .update_account_state(from, AccountState {
                balance: Wei::from(1_000_000u64),
                nonce: 0,
                code_hash: H256::default(),
                code: None,
                storage_root: H256::default(),
            })
            .await
            .unwrap();
        drop(account_manager);

        // 估算 Gas
        let estimated_gas = executor.estimate_gas(&tx, &ctx).await.unwrap();

        assert!(estimated_gas >= U256::from(21000u64));
    }
}