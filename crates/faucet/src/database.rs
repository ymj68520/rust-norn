//! Faucet database for tracking distributions

use crate::error::{FaucetError, FaucetResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::{Db, Tree, IVec};
use std::sync::Arc;
use tracing::{debug, info};

/// Distribution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionRecord {
    /// Recipient address
    pub address: String,
    /// Amount dispensed (in wei)
    pub amount: String,
    /// Transaction hash
    pub tx_hash: String,
    /// Timestamp
    pub timestamp: i64,
    /// IP address
    pub ip_address: String,
    /// User agent
    pub user_agent: String,
}

impl DistributionRecord {
    pub fn new(
        address: String,
        amount: String,
        tx_hash: String,
        ip_address: String,
        user_agent: String,
    ) -> Self {
        Self {
            address,
            amount,
            tx_hash,
            timestamp: Utc::now().timestamp(),
            ip_address,
            user_agent,
        }
    }

    pub fn datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.timestamp, 0).unwrap_or_else(|| Utc::now())
    }
}

/// Faucet database
pub struct FaucetDatabase {
    db: Arc<Db>,
    /// Tree for distribution records
    distributions: Tree,
    /// Tree for address tracking (last request time)
    address_tracker: Tree,
    /// Tree for IP tracking (rate limiting)
    ip_tracker: Tree,
}

impl FaucetDatabase {
    /// Create or open faucet database
    pub fn new(path: &str) -> FaucetResult<Self> {
        info!("Opening faucet database at: {}", path);

        let db = sled::Config::default()
            .path(path)
            .cache_capacity(256 * 1024 * 1024) // 256MB cache
            .open()
            .map_err(FaucetError::DatabaseError)?;

        let distributions = db.open_tree("distributions").map_err(FaucetError::DatabaseError)?;
        let address_tracker = db.open_tree("address_tracker").map_err(FaucetError::DatabaseError)?;
        let ip_tracker = db.open_tree("ip_tracker").map_err(FaucetError::DatabaseError)?;

        Ok(Self {
            db: Arc::new(db),
            distributions,
            address_tracker,
            ip_tracker,
        })
    }

    /// Record a distribution
    pub fn add_distribution(&self, record: DistributionRecord) -> FaucetResult<()> {
        let key = format!("{}:{}", record.address, record.timestamp);
        let value = bincode::serialize(&record)
            .map_err(|e| FaucetError::InternalError(e.to_string()))?;

        self.distributions
            .insert(key, value)
            .map_err(FaucetError::DatabaseError)?;

        // Update address tracker
        self.address_tracker
            .insert(
                record.address.as_bytes(),
                IVec::from(record.timestamp.to_be_bytes().as_slice()),
            )
            .map_err(FaucetError::DatabaseError)?;

        debug!("Recorded distribution for address: {}", record.address);

        Ok(())
    }

    /// Get last request timestamp for an address
    pub fn get_last_request_time(&self, address: &str) -> FaucetResult<Option<i64>> {
        match self
            .address_tracker
            .get(address.as_bytes())
            .map_err(FaucetError::DatabaseError)?
        {
            Some(bytes) => {
                let timestamp = i64::from_be_bytes(
                    bytes.as_ref().try_into().map_err(|_| {
                        FaucetError::InternalError("Invalid timestamp format".to_string())
                    })?,
                );
                Ok(Some(timestamp))
            }
            None => Ok(None),
        }
    }

    /// Get total amount dispensed to an address
    pub fn get_total_amount_for_address(&self, address: &str) -> FaucetResult<u128> {
        let mut total = 0u128;

        // Iterate through all distributions for this address
        for item in self.distributions.scan_prefix(format!("{}:", address)) {
            let (_, value) = item.map_err(FaucetError::DatabaseError)?;
            let record: DistributionRecord = bincode::deserialize(&value)
                .map_err(|e| FaucetError::InternalError(e.to_string()))?;

            total += record
                .amount
                .parse::<u128>()
                .unwrap_or(0)
                .checked_add(total)
                .unwrap_or_else(|| u128::MAX);
        }

        Ok(total)
    }

    /// Get request count for IP in time window
    pub fn get_ip_request_count(&self, ip: &str, window_start: i64) -> FaucetResult<usize> {
        let mut count = 0;

        for item in self.distributions.iter() {
            let (_, value) = item.map_err(FaucetError::DatabaseError)?;
            let record: DistributionRecord = bincode::deserialize(&value)
                .map_err(|e| FaucetError::InternalError(e.to_string()))?;

            if record.ip_address == ip && record.timestamp >= window_start {
                count += 1;
            }
        }

        Ok(count)
    }

    /// Get all distributions for an address
    pub fn get_distributions_for_address(
        &self,
        address: &str,
    ) -> FaucetResult<Vec<DistributionRecord>> {
        let mut records = Vec::new();

        for item in self.distributions.scan_prefix(format!("{}:", address)) {
            let (_, value) = item.map_err(FaucetError::DatabaseError)?;
            let record: DistributionRecord = bincode::deserialize(&value)
                .map_err(|e| FaucetError::InternalError(e.to_string()))?;
            records.push(record);
        }

        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(records)
    }

    /// Get recent distributions (limit 100)
    pub fn get_recent_distributions(&self, limit: usize) -> FaucetResult<Vec<DistributionRecord>> {
        let mut records = Vec::new();

        for item in self.distributions.iter().take(limit) {
            let (_, value) = item.map_err(FaucetError::DatabaseError)?;
            let record: DistributionRecord = bincode::deserialize(&value)
                .map_err(|e| FaucetError::InternalError(e.to_string()))?;
            records.push(record);
        }

        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(records)
    }

    /// Get statistics
    pub fn get_statistics(&self) -> FaucetResult<FaucetStatistics> {
        let total_distributions = self.distributions.len();

        let mut total_amount = 0u128;
        let mut unique_addresses = std::collections::HashSet::new();

        for item in self.distributions.iter() {
            let (_, value) = item.map_err(FaucetError::DatabaseError)?;
            let record: DistributionRecord = bincode::deserialize(&value)
                .map_err(|e| FaucetError::InternalError(e.to_string()))?;

            total_amount += record.amount.parse::<u128>().unwrap_or(0);
            unique_addresses.insert(record.address);
        }

        Ok(FaucetStatistics {
            total_distributions,
            total_amount: total_amount.to_string(),
            unique_addresses: unique_addresses.len() as u64,
        })
    }

    /// Clean old records (older than specified days)
    pub fn cleanup_old_records(&self, days: i64) -> FaucetResult<usize> {
        let cutoff = Utc::now().timestamp() - (days * 86400);
        let mut removed = 0;

        let mut keys_to_remove = Vec::new();

        for item in self.distributions.iter() {
            let (key, value) = item.map_err(FaucetError::DatabaseError)?;
            let record: DistributionRecord = bincode::deserialize(&value)
                .map_err(|e| FaucetError::InternalError(e.to_string()))?;

            if record.timestamp < cutoff {
                keys_to_remove.push(key.to_vec());
            }
        }

        for key in keys_to_remove {
            self.distributions
                .remove(key)
                .map_err(FaucetError::DatabaseError)?;
            removed += 1;
        }

        info!("Cleaned up {} old records (older than {} days)", removed, days);
        Ok(removed)
    }
}

/// Faucet statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaucetStatistics {
    pub total_distributions: usize,
    pub total_amount: String,
    pub unique_addresses: u64,
}
