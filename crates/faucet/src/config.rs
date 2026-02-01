//! Faucet configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Faucet service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaucetConfig {
    /// Server address
    pub server_addr: String,

    /// RPC endpoint for blockchain
    pub rpc_url: String,

    /// Faucet account private key
    pub private_key: String,

    /// Amount to dispense per request (in wei)
    pub dispense_amount: String,

    /// Minimum balance required (in wei)
    pub min_balance: String,

    /// Rate limit: maximum requests per time window
    pub max_requests_per_window: u32,

    /// Rate limit: time window in seconds
    pub rate_limit_window_secs: u64,

    /// Cooldown period between requests for same address (seconds)
    pub address_cooldown_secs: u64,

    /// Maximum amount per address total (in wei)
    pub max_amount_per_address: String,

    /// Enable captcha verification
    pub captcha_enabled: bool,

    /// Captcha secret key
    pub captcha_secret: Option<String>,

    /// Database path
    pub db_path: String,

    /// Enable metrics
    pub metrics_enabled: bool,

    /// Metrics port
    pub metrics_port: u16,

    /// Enable CORS
    pub cors_enabled: bool,

    /// Allowed origins
    pub allowed_origins: Vec<String>,

    /// Enable auto-refill
    pub auto_refill_enabled: bool,

    /// Auto-refill threshold (in wei)
    pub auto_refill_threshold: String,

    /// Auto-refill amount (in wei)
    pub auto_refill_amount: String,

    /// Gas price to use (in wei)
    pub gas_price: String,

    /// Gas limit for transactions
    pub gas_limit: u64,
}

impl Default for FaucetConfig {
    fn default() -> Self {
        Self {
            server_addr: "0.0.0.0:3000".to_string(),
            rpc_url: "http://localhost:8545".to_string(),
            private_key: std::env::var("FAUCET_PRIVATE_KEY")
                .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000000000000000000000000001".to_string()),
            dispense_amount: "1000000000000000000000".to_string(), // 1000 ETH
            min_balance: "100000000000000000000".to_string(), // 100 ETH
            max_requests_per_window: 3,
            rate_limit_window_secs: 3600, // 1 hour
            address_cooldown_secs: 86400, // 24 hours
            max_amount_per_address: "5000000000000000000000".to_string(), // 5000 ETH
            captcha_enabled: false,
            captcha_secret: None,
            db_path: "./faucet_data".to_string(),
            metrics_enabled: true,
            metrics_port: 9091,
            cors_enabled: true,
            allowed_origins: vec!["*".to_string()],
            auto_refill_enabled: false,
            auto_refill_threshold: "50000000000000000000".to_string(), // 50 ETH
            auto_refill_amount: "1000000000000000000000".to_string(), // 1000 ETH
            gas_price: "1000000000".to_string(), // 1 Gwei
            gas_limit: 21000,
        }
    }
}

impl FaucetConfig {
    /// Load from environment variables with defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(addr) = std::env::var("FAUCET_SERVER_ADDR") {
            config.server_addr = addr;
        }

        if let Ok(rpc_url) = std::env::var("FAUCET_RPC_URL") {
            config.rpc_url = rpc_url;
        }

        if let Ok(key) = std::env::var("FAUCET_PRIVATE_KEY") {
            config.private_key = key;
        }

        if let Ok(amount) = std::env::var("FAUCET_DISPENSE_AMOUNT") {
            config.dispense_amount = amount;
        }

        if let Ok(min_bal) = std::env::var("FAUCET_MIN_BALANCE") {
            config.min_balance = min_bal;
        }

        if let Ok(max_req) = std::env::var("FAUCET_MAX_REQUESTS") {
            config.max_requests_per_window = max_req.parse().unwrap_or(config.max_requests_per_window);
        }

        if let Ok(window) = std::env::var("FAUCET_RATE_LIMIT_WINDOW") {
            config.rate_limit_window_secs = window.parse().unwrap_or(config.rate_limit_window_secs);
        }

        if let Ok(cooldown) = std::env::var("FAUCET_ADDRESS_COOLDOWN") {
            config.address_cooldown_secs = cooldown.parse().unwrap_or(config.address_cooldown_secs);
        }

        if let Ok(max_amount) = std::env::var("FAUCET_MAX_AMOUNT_PER_ADDRESS") {
            config.max_amount_per_address = max_amount;
        }

        if let Ok(enabled) = std::env::var("FAUCET_CAPTCHA_ENABLED") {
            config.captcha_enabled = enabled.to_lowercase() == "true";
        }

        if let Ok(secret) = std::env::var("FAUCET_CAPTCHA_SECRET") {
            config.captcha_secret = Some(secret);
        }

        if let Ok(db_path) = std::env::var("FAUCET_DB_PATH") {
            config.db_path = db_path;
        }

        if let Ok(metrics_port) = std::env::var("FAUCET_METRICS_PORT") {
            config.metrics_port = metrics_port.parse().unwrap_or(config.metrics_port);
        }

        config
    }

    /// Get rate limit duration
    pub fn rate_limit_duration(&self) -> Duration {
        Duration::from_secs(self.rate_limit_window_secs)
    }

    /// Get address cooldown duration
    pub fn address_cooldown_duration(&self) -> Duration {
        Duration::from_secs(self.address_cooldown_secs)
    }
}
