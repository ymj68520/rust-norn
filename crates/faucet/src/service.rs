//! Faucet service core logic

use super::config::FaucetConfig;
use super::database::{DistributionRecord, FaucetDatabase};
use super::error::{FaucetError, FaucetResult};
use chrono::Utc;
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use k256::ecdsa::{SigningKey, signature::Signer, Signature};
use norn_common::types::Address;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// RPC client for interacting with blockchain
pub struct BlockchainRpcClient {
    rpc_url: String,
    client: reqwest::Client,
}

impl BlockchainRpcClient {
    pub fn new(rpc_url: String) -> Self {
        Self {
            rpc_url,
            client: reqwest::Client::new(),
        }
    }

    async fn call(&self, method: &str, params: serde_json::Value) -> FaucetResult<serde_json::Value> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| FaucetError::RpcError(format!("Request failed: {}", e)))?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| FaucetError::RpcError(format!("Invalid response: {}", e)))?;

        if let Some(error) = json.get("error") {
            return Err(FaucetError::RpcError(error.to_string()));
        }

        Ok(json
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    pub async fn get_balance(&self, address: &Address) -> FaucetResult<String> {
        self.call("eth_getBalance", serde_json::json!([format!("0x{}", hex::encode(address.0)), "latest"]))
            .await
            .map(|v| v.as_str().unwrap_or("0x0").to_string())
    }

    pub async fn get_transaction_count(&self, address: &Address) -> FaucetResult<u64> {
        let result = self
            .call(
                "eth_getTransactionCount",
                serde_json::json!([format!("0x{}", hex::encode(address.0)), "latest"]),
            )
            .await?;

        Ok(u64::from_str_radix(
            result.as_str().unwrap_or("0x0").trim_start_matches("0x"),
            16,
        )
        .unwrap_or(0))
    }

    pub async fn send_raw_transaction(&self, tx_data: &str) -> FaucetResult<String> {
        self.call("eth_sendRawTransaction", serde_json::json!([tx_data]))
            .await
            .map(|v| v.as_str().unwrap_or("").to_string())
    }

    pub async fn get_chain_id(&self) -> FaucetResult<u64> {
        let result = self.call("eth_chainId", serde_json::json!([])).await?;
        Ok(u64::from_str_radix(
            result.as_str().unwrap_or("0x0").trim_start_matches("0x"),
            16,
        )
        .unwrap_or(31337))
    }
}

/// Rate limiter using governor crate
type RateLimiterImpl = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Faucet service
pub struct FaucetService {
    config: FaucetConfig,
    database: Arc<FaucetDatabase>,
    rpc_client: Arc<BlockchainRpcClient>,
    signing_key: SigningKey,
    faucet_address: Address,
    rate_limiter: Arc<RateLimiterImpl>,
    ip_rate_limiters: Arc<moka::future::Cache<String, Arc<RateLimiterImpl>>>,
}

impl FaucetService {
    /// Create new faucet service
    pub fn new(config: FaucetConfig, database: FaucetDatabase) -> FaucetResult<Self> {
        // Decode private key
        let private_key_hex = config.private_key.strip_prefix("0x").unwrap_or(&config.private_key);
        let private_key_bytes =
            hex::decode(private_key_hex).map_err(|e| FaucetError::InvalidAddress(format!("Invalid private key: {}", e)))?;

        // Convert to fixed-size array
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&private_key_bytes);

        let signing_key = SigningKey::from_bytes(&key_array.into())
            .map_err(|e| FaucetError::InvalidAddress(format!("Invalid signing key: {}", e)))?;

        // Derive faucet address
        let public_key = signing_key.verifying_key();
        let pub_key_bytes = public_key.to_encoded_point(false);
        let hash = keccak_hash::keccak(&pub_key_bytes.as_bytes()[1..]); // Skip first byte
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&hash.0[12..]);
        let faucet_address = Address(addr_bytes);

        info!("Faucet address: 0x{}", hex::encode(faucet_address.0));

        // Create RPC client
        let rpc_client = Arc::new(BlockchainRpcClient::new(config.rpc_url.clone()));

        // Create global rate limiter
        let quota = Quota::per_minute(NonZeroU32::new(config.max_requests_per_window * 60 / config.rate_limit_window_secs as u32).unwrap_or(NonZeroU32::new(10).unwrap()));
        let rate_limiter = Arc::new(RateLimiter::direct(quota));

        // Create IP-specific rate limiter cache
        let ip_rate_limiters = Arc::new(moka::future::Cache::new(10000)); // Cache 10k IPs

        Ok(Self {
            config,
            database: Arc::new(database),
            rpc_client,
            signing_key,
            faucet_address,
            rate_limiter,
            ip_rate_limiters,
        })
    }

    /// Dispense tokens to an address
    pub async fn dispense(
        &self,
        address: Address,
        ip_addr: IpAddr,
        user_agent: String,
    ) -> FaucetResult<DispenseResponse> {
        info!("Dispense request for address: 0x{}, IP: {}", hex::encode(address.0), ip_addr);

        // 1. Validate address
        self.validate_address(&address)?;

        // 2. Check rate limits
        self.check_rate_limits(&address, &ip_addr).await?;

        // 3. Check faucet balance
        self.check_faucet_balance().await?;

        // 4. Check address cooldown
        self.check_address_cooldown(&address).await?;

        // 5. Check max amount per address
        self.check_max_amount_per_address(&address)?;

        // 6. Create and send transaction
        let tx_hash = self.send_transaction(&address).await?;

        // 7. Record distribution
        let record = DistributionRecord::new(
            format!("0x{}", hex::encode(address.0)),
            self.config.dispense_amount.clone(),
            tx_hash.clone(),
            ip_addr.to_string(),
            user_agent,
        );

        self.database.add_distribution(record)?;

        info!(
            "Successfully dispensed to 0x{}, tx: {}",
            hex::encode(address.0),
            tx_hash
        );

        Ok(DispenseResponse {
            tx_hash,
            amount: self.config.dispense_amount.clone(),
            address: format!("0x{}", hex::encode(address.0)),
        })
    }

    /// Validate address format
    fn validate_address(&self, address: &Address) -> FaucetResult<()> {
        if address.0 == [0u8; 20] {
            return Err(FaucetError::InvalidAddress("Zero address not allowed".to_string()));
        }
        if address.0 == self.faucet_address.0 {
            return Err(FaucetError::InvalidAddress("Cannot send to faucet address".to_string()));
        }
        Ok(())
    }

    /// Check rate limits
    async fn check_rate_limits(&self, address: &Address, ip_addr: &IpAddr) -> FaucetResult<()> {
        // Global rate limit
        self.rate_limiter
            .check()
            .map_err(|_| FaucetError::RateLimitExceeded(60))?;

        // IP-specific rate limit
        let ip_key = ip_addr.to_string();
        let ip_limiter = self
            .ip_rate_limiters
            .try_get_with(ip_key.clone(), async {
                let quota = Quota::per_hour(NonZeroU32::new(self.config.max_requests_per_window).unwrap_or(NonZeroU32::new(3).unwrap()));
                Ok::<_, FaucetError>(Arc::new(RateLimiter::direct(quota)))
            })
            .await
            .map_err(|e| FaucetError::InternalError(e.to_string()))?;

        ip_limiter.check().map_err(|_| {
            FaucetError::RateLimitExceeded(self.config.rate_limit_window_secs)
        })?;

        debug!("Rate limits passed for IP: {}", ip_addr);
        Ok(())
    }

    /// Check faucet balance
    async fn check_faucet_balance(&self) -> FaucetResult<()> {
        let balance_hex = self.rpc_client.get_balance(&self.faucet_address).await?;
        let balance = u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16).unwrap_or(0);

        let min_balance = self
            .config
            .min_balance
            .parse::<u128>()
            .unwrap_or(u128::MAX);

        if balance < min_balance {
            warn!("Faucet balance low: {} wei", balance);
            return Err(FaucetError::InsufficientFunds);
        }

        debug!("Faucet balance: {} wei", balance);
        Ok(())
    }

    /// Check address cooldown
    async fn check_address_cooldown(&self, address: &Address) -> FaucetResult<()> {
        let addr_str = format!("0x{}", hex::encode(address.0));

        if let Some(last_request) = self.database.get_last_request_time(&addr_str)? {
            let elapsed = Utc::now().timestamp() - last_request;
            let cooldown = self.config.address_cooldown_duration().as_secs() as i64;

            if elapsed < cooldown {
                let remaining = cooldown - elapsed;
                warn!(
                    "Address 0x{} requested too soon. Remaining: {}s",
                    hex::encode(address.0),
                    remaining
                );
                return Err(FaucetError::RateLimitExceeded(remaining as u64));
            }
        }

        Ok(())
    }

    /// Check max amount per address
    fn check_max_amount_per_address(&self, address: &Address) -> FaucetResult<()> {
        let addr_str = format!("0x{}", hex::encode(address.0));
        let total = self
            .database
            .get_total_amount_for_address(&addr_str)?;

        let max_amount = self
            .config
            .max_amount_per_address
            .parse::<u128>()
            .unwrap_or(u128::MAX);

        let dispense_amount = self.config.dispense_amount.parse::<u128>().unwrap_or(0);

        if total + dispense_amount > max_amount {
            warn!(
                "Address 0x{} exceeded max amount. Total: {}, Max: {}",
                hex::encode(address.0),
                total,
                max_amount
            );
            return Err(FaucetError::InvalidAmount(
                "Maximum amount per address exceeded".to_string(),
            ));
        }

        Ok(())
    }

    /// Create and send transaction
    async fn send_transaction(&self, to: &Address) -> FaucetResult<String> {
        use k256::ecdsa::Signature;
        use rlp::RlpStream;

        // Get nonce
        let nonce = self
            .rpc_client
            .get_transaction_count(&self.faucet_address)
            .await?;

        // Get chain ID
        let chain_id = self.rpc_client.get_chain_id().await?;

        // Parse amount
        let amount = self
            .config
            .dispense_amount
            .parse::<u128>()
            .map_err(|_| FaucetError::InvalidAmount("Invalid amount".to_string()))?;

        // Parse gas price
        let gas_price = self
            .config
            .gas_price
            .parse::<u128>()
            .map_err(|_| FaucetError::InvalidAmount("Invalid gas price".to_string()))?;

        // Encode legacy transaction
        let mut stream = RlpStream::new();
        stream.begin_list(9);
        stream.append(&nonce);
        stream.append(&gas_price);
        stream.append(&self.config.gas_limit);
        stream.append(&to.0.to_vec());
        stream.append(&amount.to_be_bytes().to_vec());
        stream.append(&Vec::<u8>::new()); // data

        // EIP-155: add chain ID
        stream.append(&chain_id);
        stream.append(&0u8);
        stream.append(&0u8);

        let tx_hash = keccak_hash::keccak(&stream.out());

        // Sign using Signer trait
        let signature: Signature = self.signing_key.sign(&tx_hash.0);

        // Convert r and s to byte arrays
        let r_bytes = signature.r().to_bytes();
        let s_bytes = signature.s().to_bytes();
        let r_array: [u8; 32] = r_bytes.into();
        let s_array: [u8; 32] = s_bytes.into();

        // Calculate v (recovery ID)
        // For k256, check if s is "low" (less than curve order / 2)
        // This is a simplified check - in production use proper s-value normalization
        let s_is_low = signature.s().to_bytes().last().map(|&b| b % 2 == 0).unwrap_or(false);
        let v = if chain_id > 0 {
            (chain_id * 2 + 35) as u8 + (s_is_low as u8)
        } else {
            27 + (s_is_low as u8)
        };

        // Encode signed transaction
        let mut signed_stream = RlpStream::new();
        signed_stream.begin_list(9);
        signed_stream.append(&nonce);
        signed_stream.append(&gas_price);
        signed_stream.append(&self.config.gas_limit);
        signed_stream.append(&to.0.to_vec());
        signed_stream.append(&amount.to_be_bytes().to_vec());
        signed_stream.append(&Vec::<u8>::new()); // data
        signed_stream.append(&v);
        signed_stream.append(&r_array.to_vec());
        signed_stream.append(&s_array.to_vec());

        let tx_bytes = signed_stream.out();
        let tx_hex = format!("0x{}", hex::encode(&tx_bytes));

        // Send transaction
        let tx_hash = self
            .rpc_client
            .send_raw_transaction(&tx_hex)
            .await?;

        info!("Transaction sent: {}", tx_hash);
        Ok(tx_hash)
    }

    /// Get faucet status
    pub async fn get_status(&self) -> FaucetResult<FaucetStatus> {
        let balance_hex = self
            .rpc_client
            .get_balance(&self.faucet_address)
            .await?;
        let balance = u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16).unwrap_or(0);

        let stats = self.database.get_statistics()?;

        Ok(FaucetStatus {
            address: format!("0x{}", hex::encode(self.faucet_address.0)),
            balance: balance.to_string(),
            dispense_amount: self.config.dispense_amount.clone(),
            total_distributions: stats.total_distributions,
            unique_addresses: stats.unique_addresses,
            total_dispensed: stats.total_amount,
        })
    }

    /// Cleanup old distribution records
    pub fn cleanup_old_records(&self, days: i64) -> FaucetResult<usize> {
        self.database.cleanup_old_records(days)
    }
}

/// Dispense response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispenseResponse {
    pub tx_hash: String,
    pub amount: String,
    pub address: String,
}

/// Faucet status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaucetStatus {
    pub address: String,
    pub balance: String,
    pub dispense_amount: String,
    pub total_distributions: usize,
    pub unique_addresses: u64,
    pub total_dispensed: String,
}
