//! Ethereum JSON-RPC API Implementation
//!
//! Provides Ethereum-compatible JSON-RPC methods for interacting with the norn blockchain.
//! Supports standard eth_* methods like eth_getBalance, eth_call, eth_getBlockByNumber, etc.

use std::sync::Arc;
use std::net::SocketAddr;
use jsonrpsee::core::{async_trait, RpcResult};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::{error::ErrorCode, ErrorObject};
use jsonrpsee::server::ServerBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use anyhow::anyhow;
use norn_core::blockchain::Blockchain;
use norn_core::state::{AccountStateManager, AccountStateConfig};
use norn_core::evm::{EVMExecutor, EVMConfig, EVMContext};
use norn_core::TxPool;
use norn_common::types::{Address, Hash, Transaction, PublicKey};
use num_bigint::BigUint;
use keccak_hash::keccak256;

/// Ethereum JSON-RPC API
#[rpc(server)]
pub trait EthereumRpc {
    /// Get the client version (required by Remix)
    #[method(name = "web3_clientVersion")]
    async fn client_version(&self) -> RpcResult<String>;

    /// Get accounts (MetaMask requires this to return list of accounts)
    #[method(name = "eth_accounts")]
    async fn accounts(&self) -> RpcResult<Vec<Address>>;

    /// Get the balance of an address
    #[method(name = "eth_getBalance")]
    async fn get_balance(&self, address: Address, block: BlockNumber) -> RpcResult<String>;

    /// Get the number of the latest block
    #[method(name = "eth_blockNumber")]
    async fn block_number(&self) -> RpcResult<String>;

    /// Get information about a block by hash
    #[method(name = "eth_getBlockByHash")]
    async fn get_block_by_hash(&self, hash: Hash, full_transactions: bool) -> RpcResult<Option<Block>>;

    /// Get information about a block by number
    #[method(name = "eth_getBlockByNumber")]
    async fn get_block_by_number(&self, block: BlockNumber, full_transactions: bool) -> RpcResult<Option<Block>>;

    /// Get the code at a specific address
    #[method(name = "eth_getCode")]
    async fn get_code(&self, address: Address, block: BlockNumber) -> RpcResult<String>;

    /// Get the storage value at a specific position
    #[method(name = "eth_getStorageAt")]
    async fn get_storage_at(&self, address: Address, position: String, block: BlockNumber) -> RpcResult<String>;

    /// Get the number of transactions sent from an address
    #[method(name = "eth_getTransactionCount")]
    async fn get_transaction_count(&self, address: Address, block: BlockNumber) -> RpcResult<String>;

    /// Get the current gas price
    #[method(name = "eth_gasPrice")]
    async fn gas_price(&self) -> RpcResult<String>;

    /// Estimate gas for a transaction
    #[method(name = "eth_estimateGas")]
    async fn estimate_gas(&self, request: CallRequest) -> RpcResult<String>;

    /// Call a contract method without creating a transaction
    #[method(name = "eth_call")]
    async fn call(&self, request: CallRequest, block: BlockNumber) -> RpcResult<String>;

    /// Get a transaction by hash
    #[method(name = "eth_getTransactionByHash")]
    async fn get_transaction_by_hash(&self, hash: Hash) -> RpcResult<Option<Transaction>>;

    /// Get transaction receipt by hash
    #[method(name = "eth_getTransactionReceipt")]
    async fn get_transaction_receipt(&self, hash: Hash) -> RpcResult<Option<TransactionReceipt>>;

    /// Get the chain ID
    #[method(name = "eth_chainId")]
    async fn chain_id(&self) -> RpcResult<String>;

    /// Get the latest block's hash
    #[method(name = "eth_getLatestBlock")]
    async fn get_latest_block(&self) -> RpcResult<Option<Block>>;

    /// Get logs matching a filter
    #[method(name = "eth_getLogs")]
    async fn get_logs(&self, filter: LogFilter) -> RpcResult<Vec<Log>>;

    /// Send a raw transaction (signed and RLP-encoded)
    #[method(name = "eth_sendRawTransaction")]
    async fn send_raw_transaction(&self, data: String) -> RpcResult<Hash>;

    /// Send a transaction (for wallet integration)
    #[method(name = "eth_sendTransaction")]
    async fn send_transaction(&self, request: TransactionRequest) -> RpcResult<Hash>;

    /// Get uncle count by block hash (always 0 for PoVF consensus)
    #[method(name = "eth_getUncleCountByBlockHash")]
    async fn get_uncle_count_by_block_hash(&self, hash: Hash) -> RpcResult<String>;

    /// Get uncle count by block number (always 0 for PoVF consensus)
    #[method(name = "eth_getUncleCountByBlockNumber")]
    async fn get_uncle_count_by_block_number(&self, block: BlockNumber) -> RpcResult<String>;

    /// Get uncle by block hash and index (always null for PoVF consensus)
    #[method(name = "eth_getUncleByBlockHashAndIndex")]
    async fn get_uncle_by_block_hash_and_index(&self, hash: Hash, index: String) -> RpcResult<Option<Block>>;

    /// Get uncle by block number and index (always null for PoVF consensus)
    #[method(name = "eth_getUncleByBlockNumberAndIndex")]
    async fn get_uncle_by_block_number_and_index(&self, block: BlockNumber, index: String) -> RpcResult<Option<Block>>;

    /// Get available compilers (returns empty list)
    #[method(name = "eth_getCompilers")]
    async fn get_compilers(&self) -> RpcResult<Vec<String>>;

    /// Get network hashrate (returns "0x0" for PoVF consensus)
    #[method(name = "eth_hashrate")]
    async fn hashrate(&self) -> RpcResult<String>;

    /// Get mining status (returns false for PoVF consensus)
    #[method(name = "eth_mining")]
    async fn mining(&self) -> RpcResult<bool>;

    /// Get sync status (returns false when synced)
    #[method(name = "eth_syncing")]
    async fn syncing(&self) -> RpcResult<bool>;

    /// Get transaction count by block hash
    #[method(name = "eth_getBlockTransactionCountByHash")]
    async fn get_block_transaction_count_by_hash(&self, hash: Hash) -> RpcResult<String>;

    /// Get transaction count by block number
    #[method(name = "eth_getBlockTransactionCountByNumber")]
    async fn get_block_transaction_count_by_number(&self, block: BlockNumber) -> RpcResult<String>;

    /// Get base fee and reward percentiles for a range of blocks
    #[method(name = "eth_feeHistory")]
    async fn fee_history(&self, block_count: String, newest_block: BlockNumber, reward_percentiles: Option<Vec<f64>>) -> RpcResult<FeeHistory>;

    // ========== Development Only Methods ==========

    /// Development only: Mint ETH to an address (faucet)
    #[method(name = "dev_faucet")]
    async fn dev_faucet(&self, address: Address, amount: String) -> RpcResult<bool>;
}

/// Block identifier for RPC calls
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum BlockNumber {
    #[serde(rename = "earliest")]
    Earliest,
    #[serde(rename = "latest")]
    Latest,
    #[serde(rename = "pending")]
    Pending,
    Number(u64),
}

impl<'de> Deserialize<'de> for BlockNumber {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        // Deserialize as a string first to handle hex format
        let s = String::deserialize(deserializer)?;

        match s.as_str() {
            "earliest" => Ok(BlockNumber::Earliest),
            "latest" => Ok(BlockNumber::Latest),
            "pending" => Ok(BlockNumber::Pending),
            hex_str => {
                // Try to parse as hex string (with or without 0x prefix)
                let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                u64::from_str_radix(hex_str, 16)
                    .map(BlockNumber::Number)
                    .map_err(|_| Error::custom(format!("Invalid block number: {}", s)))
            }
        }
    }
}

impl Default for BlockNumber {
    fn default() -> Self {
        BlockNumber::Latest
    }
}

/// Call request for eth_call and eth_estimateGas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRequest {
    /// The address of the contract (None for contract creation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Address>,
    /// The address sending the transaction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<Address>,
    /// The value sent with the transaction (in wei)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Gas limit (optional for estimateGas)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas: Option<String>,
    /// Gas price (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_price: Option<String>,
    /// Transaction data (function selector + arguments)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// Transaction request for eth_sendTransaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRequest {
    /// The address sending the transaction
    pub from: Address,
    /// The address of the contract (or recipient for transfers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Address>,
    /// The value sent with the transaction (in wei)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Gas limit
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas: Option<String>,
    /// Gas price for legacy transactions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_price: Option<String>,
    /// Max fee per gas (EIP-1559)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fee_per_gas: Option<String>,
    /// Max priority fee per gas (EIP-1559)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_priority_fee_per_gas: Option<String>,
    /// Transaction nonce
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Transaction data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Chain ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
}

/// Transaction receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    /// Transaction hash
    pub transaction_hash: Hash,
    /// Transaction index
    pub transaction_index: String,
    /// Block hash
    pub block_hash: Hash,
    /// Block number
    pub block_number: String,
    /// From address
    pub from: Address,
    /// To address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Address>,
    /// Gas used
    pub gas_used: String,
    /// Cumulative gas used
    pub cumulative_gas_used: String,
    /// Contract address (for contract creation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_address: Option<Address>,
    /// Logs
    pub logs: Vec<Log>,
    /// Logs bloom filter
    pub logs_bloom: String,
    /// Status (1 for success, 0 for failure)
    pub status: String,
}

/// Log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    /// Log index
    pub log_index: String,
    /// Transaction index
    pub transaction_index: String,
    /// Transaction hash
    pub transaction_hash: Hash,
    /// Block hash
    pub block_hash: Hash,
    /// Block number
    pub block_number: String,
    /// Address that emitted the log
    pub address: Address,
    /// Topics
    pub topics: Vec<Hash>,
    /// Log data
    pub data: String,
}

/// Filter for getting logs
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogFilter {
    /// From block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_block: Option<BlockNumber>,
    /// To block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_block: Option<BlockNumber>,
    /// Contract address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    /// Topics to filter by
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<Option<Hash>>>,
}

/// Fee history information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeHistory {
    /// Base fee per gas for each block
    pub base_fee_per_gas: Vec<String>,
    /// Gas used ratio for each block
    pub gas_used_ratio: Vec<f64>,
    /// Oldest block number in the response
    pub oldest_block: String,
    /// Reward percentiles requested
    pub reward: Vec<Vec<String>>,
}

/// Block information (RPC format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Block hash
    pub hash: String,
    /// Parent hash
    pub parent_hash: String,
    /// Block number
    pub number: String,
    /// Timestamp
    pub timestamp: String,
    /// Block hash for transactions (not used in norn)
    pub sha3_uncles: String,
    /// Miner/coinbase address
    pub miner: String,
    /// Gas limit
    pub gas_limit: String,
    /// Gas used
    pub gas_used: String,
    /// State root
    pub state_root: String,
    /// Transactions root
    pub transactions_root: String,
    /// Receipts root (not used in norn)
    pub receipts_root: String,
    /// Extra data
    pub extra_data: String,
    /// Transactions
    pub transactions: Vec<Transaction>,
}

/// Ethereum RPC implementation
pub struct EthereumRpcImpl {
    blockchain: Arc<Blockchain>,
    state_manager: Arc<AccountStateManager>,
    evm_executor: Arc<EVMExecutor>,
    tx_pool: Arc<TxPool>,
    chain_id: u64,
}

impl EthereumRpcImpl {
    /// Create a new Ethereum RPC implementation
    pub fn new(
        blockchain: Arc<Blockchain>,
        state_manager: Arc<AccountStateManager>,
        evm_executor: Arc<EVMExecutor>,
        tx_pool: Arc<TxPool>,
        chain_id: u64,
    ) -> Self {
        Self {
            blockchain,
            state_manager,
            evm_executor,
            tx_pool,
            chain_id,
        }
    }

    /// Get block number for a BlockNumber enum
    async fn resolve_block_number(&self, block: BlockNumber) -> Option<i64> {
        let latest = self.blockchain.latest_block.read().await;

        match block {
            BlockNumber::Earliest => Some(0),
            BlockNumber::Latest => Some(latest.header.height),
            BlockNumber::Pending => Some(latest.header.height),
            BlockNumber::Number(n) => {
                if n <= latest.header.height as u64 {
                    Some(n as i64)
                } else {
                    None
                }
            }
        }
    }

    /// Convert norn block to RPC block format
    fn convert_block(&self, block: &norn_common::types::Block) -> Block {
        let miner_address = block.header.public_key.to_address();
        Block {
            hash: format!("0x{}", block.header.block_hash),
            parent_hash: format!("0x{}", block.header.prev_block_hash),
            number: format!("0x{:x}", block.header.height),
            timestamp: format!("0x{:x}", block.header.timestamp),
            sha3_uncles: format!("0x{}", hex::encode(block.header.merkle_root.0)),
            miner: format!("0x{}", hex::encode(miner_address.0)),
            gas_limit: format!("0x{:x}", block.header.gas_limit),
            gas_used: format!("0x0"), // Not tracked in norn yet
            state_root: format!("0x{}", block.header.state_root),
            transactions_root: format!("0x{}", block.header.merkle_root),
            receipts_root: format!("0x{}", Hash::default()), // Not implemented
            extra_data: String::new(),
            transactions: block.transactions.clone(),
        }
    }
}

#[async_trait]
impl EthereumRpcServer for EthereumRpcImpl {
    async fn client_version(&self) -> RpcResult<String> {
        Ok("norn-rust/v0.1.0".to_string())
    }

    async fn accounts(&self) -> RpcResult<Vec<Address>> {
        // MetaMask doesn't require the node to manage accounts
        // Return empty array - accounts are managed by MetaMask itself
        Ok(vec![])
    }

    async fn get_balance(&self, address: Address, block: BlockNumber) -> RpcResult<String> {
        let _block_num = self.resolve_block_number(block).await
            .ok_or_else(|| ErrorObject::from(ErrorCode::InvalidParams))?;

        // Get balance from state manager
        let balance = self.state_manager.get_balance(&address).await
            .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;

        // Convert BigUint to hex string (in wei)
        Ok(format!("0x{:x}", balance))
    }

    async fn block_number(&self) -> RpcResult<String> {
        let latest = self.blockchain.latest_block.read().await;
        Ok(format!("0x{:x}", latest.header.height))
    }

    async fn get_block_by_hash(&self, hash: Hash, _full_transactions: bool) -> RpcResult<Option<Block>> {
        let block = self.blockchain.get_block_by_hash(&hash).await;
        Ok(block.map(|b| self.convert_block(&b)))
    }

    async fn get_block_by_number(&self, block: BlockNumber, _full_transactions: bool) -> RpcResult<Option<Block>> {
        let block_num = self.resolve_block_number(block).await
            .ok_or_else(|| ErrorObject::from(ErrorCode::InvalidParams))?;

        // For now, only latest is supported
        if block_num == 0 {
            let genesis = norn_common::genesis::get_genesis_block();
            return Ok(Some(self.convert_block(&genesis)));
        }

        let latest = self.blockchain.latest_block.read().await;
        if latest.header.height == block_num {
            Ok(Some(self.convert_block(&latest)))
        } else {
            Ok(None)
        }
    }

    async fn get_code(&self, address: Address, _block: BlockNumber) -> RpcResult<String> {
        // Get account to check code hash
        let account = self.state_manager.get_account(&address).await
            .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;

        if let Some(acc) = account {
            if acc.account_type == norn_core::state::AccountType::Contract {
                // Get code from code storage
                let code = self.evm_executor.code_storage().get_code_by_address(&address).await;
                if let Ok(Some(bytecode)) = code {
                    return Ok(format!("0x{}", hex::encode(&bytecode)));
                }
            }
        }

        Ok("0x".to_string())
    }

    async fn get_storage_at(&self, address: Address, position: String, _block: BlockNumber) -> RpcResult<String> {
        // Parse position as hex string and convert to bytes
        let pos = if position.starts_with("0x") {
            &position[2..]
        } else {
            &position
        };

        // Convert hex string to key bytes (32 bytes for storage slot)
        let mut key = [0u8; 32];
        if let Ok(pos_bytes) = hex::decode(pos) {
            let len = pos_bytes.len().min(32);
            key[..len].copy_from_slice(&pos_bytes);
        }

        // Get storage value
        let value = self.state_manager.get_storage(&address, &key).await
            .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;

        Ok(format!("0x{}", hex::encode(value.unwrap_or_default())))
    }

    async fn get_transaction_count(&self, address: Address, _block: BlockNumber) -> RpcResult<String> {
        let nonce = self.state_manager.get_nonce(&address).await
            .map_err(|_| ErrorObject::from(ErrorCode::InternalError))?;

        Ok(format!("0x{:x}", nonce))
    }

    async fn gas_price(&self) -> RpcResult<String> {
        // Return fixed gas price for now
        // In production, this should be calculated from the market
        Ok("0x3b9aca00".to_string()) // 1 Gwei in hex
    }

    async fn estimate_gas(&self, request: CallRequest) -> RpcResult<String> {
        // Create EVM context
        let latest = self.blockchain.latest_block.read().await;
        let ctx = EVMContext {
            block_number: latest.header.height as u64,
            block_timestamp: latest.header.timestamp as u64,
            block_coinbase: latest.header.public_key.to_address(),
            block_gas_limit: latest.header.gas_limit as u64,
            tx_gas_price: 1_000_000_000, // 1 Gwei
        };

        // Parse call data
        let data = request.data.and_then(|d| if d.starts_with("0x") {
            hex::decode(&d[2..]).ok()
        } else {
            hex::decode(&d).ok()
        }).unwrap_or_default();

        let from = request.from.unwrap_or(Address::default());
        let value = request.value.and_then(|v| v.parse::<u128>().ok()).unwrap_or(0);

        // Check if this is a contract creation (to is None)
        if request.to.is_none() && !data.is_empty() {
            // Contract creation - estimate gas for deployment
            // For now, return a reasonable estimate for contract creation
            // TODO: Actually execute the create to get accurate gas estimation
            Ok("0x186a0".to_string()) // 100,000 in hex
        } else {
            // Contract call
            let to = request.to.unwrap_or(Address::default());
            let _result = self.evm_executor.call_contract(
                from,
                to,
                value,
                data,
                1_000_000,
            ).await.map_err(|e| {
                tracing::error!("call_contract failed in estimate_gas: {:?}", e);
                ErrorObject::from(ErrorCode::InternalError)
            })?;

            // Return estimated gas (simplified - should be actual gas used)
            Ok("0x5208".to_string()) // 21000 in hex
        }
    }

    async fn call(&self, request: CallRequest, _block: BlockNumber) -> RpcResult<String> {
        // Parse call data
        let data = request.data.and_then(|d| if d.starts_with("0x") {
            hex::decode(&d[2..]).ok()
        } else {
            hex::decode(&d).ok()
        }).unwrap_or_default();

        let from = request.from.unwrap_or(Address::default());
        let value = request.value.and_then(|v| v.parse::<u128>().ok()).unwrap_or(0);

        // Check if this is a contract creation (to is None)
        if request.to.is_none() && !data.is_empty() {
            // Contract creation - not supported in eth_call (read-only)
            return Err(ErrorObject::from(ErrorCode::InvalidRequest));
        }

        let result = self.evm_executor.call_contract(
            from,
            request.to.unwrap_or(Address::default()),
            value,
            data,
            5_000_000,
        ).await.map_err(|e| {
            tracing::error!("call_contract failed: {:?}", e);
            ErrorObject::from(ErrorCode::InternalError)
        })?;

        Ok(format!("0x{}", hex::encode(&result.output)))
    }

    async fn get_transaction_by_hash(&self, hash: Hash) -> RpcResult<Option<Transaction>> {
        let tx = self.blockchain.get_transaction_by_hash(&hash).await;
        Ok(tx)
    }

    async fn get_transaction_receipt(&self, hash: Hash) -> RpcResult<Option<TransactionReceipt>> {
        // Try to get receipt from EVM executor's receipt database
        let receipt = self.evm_executor.receipt_db().get_receipt(&hash).await;

        match receipt {
            Ok(Some(r)) => {
                // Convert our Receipt to TransactionReceipt
                let converted = TransactionReceipt {
                    transaction_hash: r.tx_hash,
                    transaction_index: format!("0x{:x}", r.tx_index),
                    block_hash: r.block_hash,
                    block_number: format!("0x{:x}", r.block_number),
                    from: r.from,
                    to: r.to,
                    gas_used: format!("0x{:x}", r.gas_used),
                    cumulative_gas_used: format!("0x{:x}", r.cumulative_gas_used),
                    contract_address: r.contract_address,
                    logs: r.logs.iter().map(|l| Log {
                        log_index: format!("0x{:x}", l.log_index),
                        transaction_index: format!("0x{:x}", l.log_index),
                        transaction_hash: l.tx_hash,
                        block_hash: l.block_hash,
                        block_number: format!("0x{:x}", l.block_number),
                        address: l.address,
                        topics: l.topics.clone(),
                        data: format!("0x{}", hex::encode(&l.data)),
                    }).collect(),
                    logs_bloom: format!("0x{}", hex::encode(&r.logs_bloom.as_bytes())),
                    status: if r.status { "0x1".to_string() } else { "0x0".to_string() },
                };
                Ok(Some(converted))
            },
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        }
    }

    async fn send_raw_transaction(&self, data: String) -> RpcResult<Hash> {
        use crate::rlp_tx::EthereumTransaction;

        // Decode hex string
        let raw_tx = if data.starts_with("0x") {
            &data[2..]
        } else {
            &data
        };

        let tx_bytes = hex::decode(raw_tx)
            .map_err(|_| ErrorObject::from(ErrorCode::InvalidParams))?;

        // Parse RLP-encoded Ethereum transaction
        let eth_tx = match EthereumTransaction::parse(&tx_bytes) {
            Ok(tx) => tx,
            Err(_) => {
                tracing::error!("Failed to parse RLP-encoded transaction");
                return Err(ErrorObject::from(ErrorCode::InvalidParams));
            }
        };

        // Convert to norn transaction
        let norn_tx = match eth_tx.to_norn_transaction() {
            Ok(tx) => tx,
            Err(_) => {
                tracing::error!("Failed to convert Ethereum transaction to norn transaction");
                return Err(ErrorObject::from(ErrorCode::InternalError));
            }
        };

        // Validate transaction
        // 1. Check nonce
        let current_nonce = self.state_manager.get_nonce(&norn_tx.body.address).await
            .map_err(|e| {
                tracing::error!("Failed to get nonce: {:?}", e);
                ErrorObject::from(ErrorCode::InternalError)
            })?;

        if (norn_tx.body.nonce as u64) < current_nonce {
            tracing::error!("Transaction nonce {} is too old (current: {})",
                norn_tx.body.nonce, current_nonce);
            return Err(ErrorObject::from(ErrorCode::InvalidParams));
        }

        if norn_tx.body.nonce as u64 > current_nonce + 10 {
            tracing::warn!("Transaction nonce {} is too far in the future (current: {})",
                norn_tx.body.nonce, current_nonce);
            // Don't reject, but warn
        }

        // 2. Check balance
        let balance = self.state_manager.get_balance(&norn_tx.body.address).await
            .map_err(|e| {
                tracing::error!("Failed to get balance: {:?}", e);
                ErrorObject::from(ErrorCode::InternalError)
            })?;

        // balance is already BigUint, convert value to BigUint for comparison
        let value_biguint = norn_tx.body.value.clone().unwrap_or_else(|| "0".to_string())
            .parse::<num_bigint::BigUint>().unwrap_or_else(|_| num_bigint::BigUint::from(0u32));

        // Simplified gas cost calculation (gas_limit * gas_price + value)
        // In production, this should use the actual gas calculator
        let gas_cost_biguint = BigUint::from(norn_tx.body.gas as u64) * BigUint::from(1_000_000_000u64) + value_biguint;

        if balance < gas_cost_biguint {
            tracing::error!("Insufficient balance: have {}, need {}", balance, gas_cost_biguint);
            return Err(ErrorObject::from(ErrorCode::InvalidParams));
        }

        // Submit to transaction pool
        self.tx_pool.add(norn_tx.clone());

        tracing::info!(
            "Transaction submitted to pool: hash={:?}, from={:?}, to={:?}, value={}",
            norn_tx.body.hash,
            norn_tx.body.address,
            norn_tx.body.receiver,
            norn_tx.body.value.unwrap_or_else(|| "0".to_string())
        );

        Ok(norn_tx.body.hash)
    }

    async fn send_transaction(&self, request: TransactionRequest) -> RpcResult<Hash> {
        use norn_common::build_mode;

        // This is for wallet integration - RPC server holds the private key
        // WARNING: This should ONLY be enabled in development/test mode
        // In production, wallets should sign transactions locally and use eth_sendRawTransaction

        if !build_mode::IS_TEST_MODE {
            return Err(ErrorObject::from(ErrorCode::InternalError));
        }

        tracing::info!("eth_sendTransaction called (TEST MODE ONLY): from={:?}, to={:?}, value={:?}",
            request.from, request.to, request.value);

        // In test mode, create and sign a transaction using a test keypair
        // This allows easy testing without requiring wallet software

        // 1. Validate parameters
        let to = request.to.ok_or_else(|| ErrorObject::from(ErrorCode::InvalidParams))?;
        let from = request.from;

        // 2. Parse value
        let value_str = request.value.unwrap_or_else(|| "0".to_string());
        let value: num_bigint::BigUint = value_str.parse()
            .unwrap_or_else(|_| num_bigint::BigUint::from(0u32));

        // 3. Get nonce
        let nonce = match self.state_manager.get_account(&from).await {
            Ok(Some(account)) => account.nonce,
            Ok(None) => 0,
            Err(_) => return Err(ErrorObject::from(ErrorCode::InternalError)),
        };

        // 4. Create transaction (using placeholder signing for now)
        // In a full implementation, this would:
        // - Use a wallet's private key from secure storage
        // - Sign the transaction properly
        // - Return the transaction hash

        // For test mode, create a simple transaction hash
        // Build the data to hash in a buffer (keccak256 computes in place)
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&from.0);
        buffer.extend_from_slice(&to.0);
        buffer.extend_from_slice(value_str.as_bytes());
        buffer.extend_from_slice(&nonce.to_le_bytes());

        // Compute keccak256 hash (writes to first 32 bytes of buffer)
        keccak_hash::keccak256(&mut buffer);

        // Copy the hash result
        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(&buffer[..32]);

        tracing::warn!("TEST MODE: eth_sendTransaction creates unsigned transaction");
        tracing::warn!("In production, use eth_sendRawTransaction with properly signed transactions");

        Ok(Hash(hash_bytes))
    }

    async fn dev_faucet(&self, address: Address, amount: String) -> RpcResult<bool> {
        // Development only: Mint ETH to an address
        tracing::info!("dev_faucet called: address={:?}, amount={}", address, amount);

        // Parse amount to BigUint
        let amount_biguint: num_bigint::BigUint = amount.parse()
            .unwrap_or_else(|_| num_bigint::BigUint::from(0u32));

        // Update account balance in state manager
        match self.state_manager.update_balance(&address, amount_biguint).await {
            Ok(_) => {
                tracing::info!("Successfully minted {} ETH to {:?}", amount, address);
                Ok(true)
            }
            Err(e) => {
                tracing::error!("Failed to mint ETH: {:?}", e);
                Err(ErrorObject::from(ErrorCode::InternalError))
            }
        }
    }

    async fn get_uncle_count_by_block_hash(&self, _hash: Hash) -> RpcResult<String> {
        Ok("0x0".to_string())
    }

    async fn get_uncle_count_by_block_number(&self, _block: BlockNumber) -> RpcResult<String> {
        Ok("0x0".to_string())
    }

    async fn get_uncle_by_block_hash_and_index(&self, _hash: Hash, _index: String) -> RpcResult<Option<Block>> {
        Ok(None)
    }

    async fn get_uncle_by_block_number_and_index(&self, _block: BlockNumber, _index: String) -> RpcResult<Option<Block>> {
        Ok(None)
    }

    async fn get_compilers(&self) -> RpcResult<Vec<String>> {
        Ok(vec![])
    }

    async fn hashrate(&self) -> RpcResult<String> {
        Ok("0x0".to_string())
    }

    async fn mining(&self) -> RpcResult<bool> {
        Ok(false)
    }

    async fn syncing(&self) -> RpcResult<bool> {
        Ok(false)
    }

    async fn get_block_transaction_count_by_hash(&self, hash: Hash) -> RpcResult<String> {
        let block = self.blockchain.get_block_by_hash(&hash).await;
        match block {
            Some(b) => Ok(format!("0x{:x}", b.transactions.len())),
            None => Err(ErrorObject::from(ErrorCode::InvalidParams)),
        }
    }

    async fn get_block_transaction_count_by_number(&self, block: BlockNumber) -> RpcResult<String> {
        let block_num = self.resolve_block_number(block).await
            .ok_or_else(|| ErrorObject::from(ErrorCode::InvalidParams))?;

        if block_num == 0 {
            let genesis = norn_common::genesis::get_genesis_block();
            return Ok(format!("0x{:x}", genesis.transactions.len()));
        }

        let latest = self.blockchain.latest_block.read().await;
        if latest.header.height == block_num {
            Ok(format!("0x{:x}", latest.transactions.len()))
        } else {
            Ok("0x0".to_string())
        }
    }

    async fn fee_history(&self, block_count: String, newest_block: BlockNumber, _reward_percentiles: Option<Vec<f64>>) -> RpcResult<FeeHistory> {
        let block_count_num: u64 = if block_count.starts_with("0x") {
            u64::from_str_radix(&block_count[2..], 16)
                .unwrap_or(1)
        } else {
            block_count.parse().unwrap_or(1)
        };

        let newest_block_num = self.resolve_block_number(newest_block).await
            .ok_or_else(|| ErrorObject::from(ErrorCode::InvalidParams))? as u64;

        let oldest_block_num = if newest_block_num >= block_count_num {
            newest_block_num - block_count_num
        } else {
            0
        };

        let latest = self.blockchain.latest_block.read().await;
        let current_base_fee = 1_000_000_000u64;

        let mut base_fee_per_gas = Vec::new();
        let mut gas_used_ratio = Vec::new();

        for _ in oldest_block_num..=newest_block_num {
            base_fee_per_gas.push(format!("0x{:x}", current_base_fee));
            gas_used_ratio.push(0.5);
        }

        Ok(FeeHistory {
            base_fee_per_gas,
            gas_used_ratio,
            oldest_block: format!("0x{:x}", oldest_block_num),
            reward: vec![],
        })
    }

    async fn chain_id(&self) -> RpcResult<String> {
        Ok(format!("0x{:x}", self.chain_id))
    }

    async fn get_latest_block(&self) -> RpcResult<Option<Block>> {
        let latest = self.blockchain.latest_block.read().await;
        Ok(Some(self.convert_block(&latest)))
    }

    async fn get_logs(&self, filter: LogFilter) -> RpcResult<Vec<Log>> {
        // Get receipt database from evm_executor
        let receipt_db = &self.evm_executor.receipt_db();

        // Get current block height once (avoid multiple lock acquisitions)
        let current_height = self.blockchain.latest_block.read().await.header.height;

        // Convert block numbers to u64
        let from_block = match filter.from_block {
            Some(BlockNumber::Earliest) => Some(0u64),
            Some(BlockNumber::Latest) => Some(current_height as u64),
            Some(BlockNumber::Pending) => Some(current_height as u64),
            Some(BlockNumber::Number(n)) => Some(n),
            None => Some(0u64),
        };

        let to_block = match filter.to_block {
            Some(BlockNumber::Earliest) => Some(0u64),
            Some(BlockNumber::Latest) => Some(current_height as u64),
            Some(BlockNumber::Pending) => Some(current_height as u64),
            Some(BlockNumber::Number(n)) => Some(n),
            None => Some(current_height as u64),
        };

        // Convert topics
        let topics = filter.topics.unwrap_or_default();

        // Query receipts
        let receipts = receipt_db.filter_receipts(
            None, // block_hash - not used when we have range
            from_block,
            to_block,
            filter.address.as_ref(),
            &topics,
        ).await
            .map_err(|e| {
                tracing::error!("Failed to filter receipts: {:?}", e);
                ErrorObject::from(ErrorCode::InternalError)
            })?;

        // Convert receipts to logs
        let mut logs = Vec::new();
        for receipt in receipts {
            for receipt_log in receipt.logs {
                // Filter by address if specified
                if let Some(ref addr) = filter.address {
                    if receipt_log.address != *addr {
                        continue;
                    }
                }

                // Filter by topics if specified
                let mut topics_match = true;
                for (i, topic_opt) in topics.iter().enumerate() {
                    if let Some(topic) = topic_opt {
                        let log_has_topic = receipt_log.topics.get(i)
                            .map_or(false, |t| *t == *topic);
                        if !log_has_topic {
                            topics_match = false;
                            break;
                        }
                    }
                }

                if !topics_match {
                    continue;
                }

                // Convert receipt log to RPC Log format
                let log = Log {
                    log_index: format!("0x{:x}", receipt_log.log_index),
                    transaction_index: format!("0x{:x}", receipt_log.log_index), // Use log_index as tx_index
                    transaction_hash: receipt_log.tx_hash,
                    block_hash: receipt_log.block_hash,
                    block_number: format!("0x{:x}", receipt_log.block_number),
                    address: receipt_log.address,
                    topics: receipt_log.topics,
                    data: format!("0x{}", hex::encode(&receipt_log.data)),
                };
                logs.push(log);
            }
        }

        Ok(logs)
    }
}

/// Start Ethereum JSON-RPC server
pub async fn start_ethereum_rpc_server(
    addr: SocketAddr,
    ethereum_rpc: EthereumRpcImpl,
) -> Result<(), Box<dyn std::error::Error>> {
    use jsonrpsee::server::ServerBuilder;
    use jsonrpsee::server::RpcModule;
    use tracing::info;
    use std::sync::Arc;

    info!("Starting Ethereum JSON-RPC server on {}", addr);

    let server = ServerBuilder::default()
        .build(addr)
        .await?;

    let addr = server.local_addr()?;
    info!("Ethereum JSON-RPC server listening on {}", addr);

    // Wrap in Arc for sharing between method closures
    let ethereum_rpc = Arc::new(ethereum_rpc);

    // Build RPC module manually
    let mut module = RpcModule::new(ethereum_rpc.clone());

    // Register all RPC methods using async closures
    module.register_async_method("web3_clientVersion", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.client_version().await
        }
    })?;

    module.register_async_method("eth_accounts", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.accounts().await
        }
    })?;

    module.register_async_method("eth_getBalance", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (addr, block): (Address, BlockNumber) = params.parse()?;
            ethereum_rpc.get_balance(addr, block).await
        }
    })?;

    module.register_async_method("eth_blockNumber", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.block_number().await
        }
    })?;

    module.register_async_method("eth_getBlockByHash", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (hash, full): (Hash, bool) = params.parse()?;
            ethereum_rpc.get_block_by_hash(hash, full).await
        }
    })?;

    module.register_async_method("eth_getBlockByNumber", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (block, full): (BlockNumber, bool) = params.parse()?;
            ethereum_rpc.get_block_by_number(block, full).await
        }
    })?;

    module.register_async_method("eth_call", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (call, block): (CallRequest, BlockNumber) = params.parse()?;
            ethereum_rpc.call(call, block).await
        }
    })?;

    module.register_async_method("eth_sendRawTransaction", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let data: String = params.parse()?;
            ethereum_rpc.send_raw_transaction(data).await
        }
    })?;

    module.register_async_method("eth_getTransactionCount", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (addr, block): (Address, BlockNumber) = params.parse()?;
            ethereum_rpc.get_transaction_count(addr, block).await
        }
    })?;

    module.register_async_method("eth_estimateGas", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let call: CallRequest = params.parse()?;
            ethereum_rpc.estimate_gas(call).await
        }
    })?;

    module.register_async_method("eth_getCode", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (addr, block): (Address, BlockNumber) = params.parse()?;
            ethereum_rpc.get_code(addr, block).await
        }
    })?;

    module.register_async_method("eth_chainId", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.chain_id().await
        }
    })?;

    module.register_async_method("net_version", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.chain_id().await
        }
    })?;

    module.register_async_method("eth_getLogs", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let filter: LogFilter = params.parse()?;
            ethereum_rpc.get_logs(filter).await
        }
    })?;

    module.register_async_method("eth_getUncleCountByBlockHash", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let hash: Hash = params.parse()?;
            ethereum_rpc.get_uncle_count_by_block_hash(hash).await
        }
    })?;

    module.register_async_method("eth_getUncleCountByBlockNumber", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let block: BlockNumber = params.parse()?;
            ethereum_rpc.get_uncle_count_by_block_number(block).await
        }
    })?;

    module.register_async_method("eth_getUncleByBlockHashAndIndex", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (hash, index): (Hash, String) = params.parse()?;
            ethereum_rpc.get_uncle_by_block_hash_and_index(hash, index).await
        }
    })?;

    module.register_async_method("eth_getUncleByBlockNumberAndIndex", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (block, index): (BlockNumber, String) = params.parse()?;
            ethereum_rpc.get_uncle_by_block_number_and_index(block, index).await
        }
    })?;

    module.register_async_method("eth_getCompilers", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.get_compilers().await
        }
    })?;

    module.register_async_method("eth_hashrate", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.hashrate().await
        }
    })?;

    module.register_async_method("eth_mining", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.mining().await
        }
    })?;

    module.register_async_method("eth_syncing", move |_params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            ethereum_rpc.syncing().await
        }
    })?;

    module.register_async_method("eth_getBlockTransactionCountByHash", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let hash: Hash = params.parse()?;
            ethereum_rpc.get_block_transaction_count_by_hash(hash).await
        }
    })?;

    module.register_async_method("eth_getBlockTransactionCountByNumber", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let block: BlockNumber = params.parse()?;
            ethereum_rpc.get_block_transaction_count_by_number(block).await
        }
    })?;

    module.register_async_method("eth_feeHistory", move |params, ethereum_rpc| {
        let ethereum_rpc = ethereum_rpc.clone();
        async move {
            let (block_count, newest_block, reward_percentiles): (String, BlockNumber, Option<Vec<f64>>) = params.parse()?;
            ethereum_rpc.fee_history(block_count, newest_block, reward_percentiles).await
        }
    })?;

    // Start server with RPC module
    let handle = server.start(module);

    // Wait for server to finish (Ctrl+C to stop)
    handle.stopped().await;

    Ok(())
}

// Helper extension to convert public key to address
pub trait ToAddress {
    fn to_address(&self) -> Address;
}

impl ToAddress for norn_common::types::PublicKey {
    fn to_address(&self) -> Address {
        // Simple conversion: take last 20 bytes
        let bytes = &self.0;
        let mut addr = [0u8; 20];
        if bytes.len() >= 20 {
            addr.copy_from_slice(&bytes[bytes.len() - 20..]);
        } else {
            addr[..bytes.len()].copy_from_slice(bytes);
        }
        Address(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_storage::SledDB;

    #[tokio::test]
    async fn test_block_number_parsing() {
        // Test BlockNumber deserialization
        let latest: BlockNumber = serde_json::from_str("\"latest\"").unwrap();
        assert!(matches!(latest, BlockNumber::Latest));

        let earliest: BlockNumber = serde_json::from_str("\"earliest\"").unwrap();
        assert!(matches!(earliest, BlockNumber::Earliest));

        let num: BlockNumber = serde_json::from_str("\"0x10\"").unwrap();
        assert!(matches!(num, BlockNumber::Number(16)));
    }

    #[tokio::test]
    async fn test_get_balance() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());
        let blockchain = norn_core::blockchain::Blockchain::new_with_fixed_genesis(db).await;
        let state_manager = Arc::new(AccountStateManager::default());
        let evm_executor = Arc::new(EVMExecutor::new(state_manager.clone(), EVMConfig::default()));
        let tx_pool = Arc::new(norn_core::TxPool::new());

        let rpc = EthereumRpcImpl::new(blockchain, state_manager, evm_executor, tx_pool, 31337);

        let address = Address([1u8; 20]);
        let balance = rpc.get_balance(address, BlockNumber::Latest).await.unwrap();

        // Should return 0x0 for non-existent account
        assert_eq!(balance, "0x0");
    }

    #[tokio::test]
    async fn test_chain_id() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SledDB::new(temp_dir.path().to_str().unwrap()).unwrap());
        let blockchain = norn_core::blockchain::Blockchain::new_with_fixed_genesis(db).await;
        let state_manager = Arc::new(AccountStateManager::default());
        let evm_executor = Arc::new(EVMExecutor::new(state_manager.clone(), EVMConfig::default()));
        let tx_pool = Arc::new(norn_core::TxPool::new());

        let rpc = EthereumRpcImpl::new(blockchain, state_manager, evm_executor, tx_pool, 31337);

        let chain_id = rpc.chain_id().await.unwrap();
        assert_eq!(chain_id, "0x7a69"); // 31337 in hex
    }
}
