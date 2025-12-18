//! Wallet Module
//! 
//! Provides wallet and account management functionality.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use norn_common::types::{Address, Hash, Transaction, TransactionBody, PublicKey};
use norn_common::error::Result;
use norn_crypto::vrf::VRFKeyPair;

/// Wallet configuration
#[derive(Debug, Clone)]
pub struct WalletConfig {
    /// Maximum number of accounts per wallet
    pub max_accounts: usize,
    /// Enable transaction signing
    pub enable_signing: bool,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            max_accounts: 100,
            enable_signing: true,
        }
    }
}

/// Account information in wallet
#[derive(Debug, Clone)]
pub struct WalletAccount {
    /// Account address
    pub address: Address,
    /// Account name/label
    pub name: String,
    /// Account is locked
    pub is_locked: bool,
    /// Created at timestamp
    pub created_at: i64,
}

/// Simple wallet for managing accounts
pub struct Wallet {
    config: WalletConfig,
    accounts: Arc<RwLock<HashMap<Address, WalletAccount>>>,
    key_pairs: Arc<RwLock<HashMap<Address, VRFKeyPair>>>,
    default_account: Arc<RwLock<Option<Address>>>,
}

impl Wallet {
    /// Create a new wallet
    pub fn new() -> Self {
        Self::with_config(WalletConfig::default())
    }

    /// Create wallet with custom config
    pub fn with_config(config: WalletConfig) -> Self {
        Self {
            config,
            accounts: Arc::new(RwLock::new(HashMap::new())),
            key_pairs: Arc::new(RwLock::new(HashMap::new())),
            default_account: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new account
    pub async fn create_account(&self, name: &str) -> Result<Address> {
        let accounts = self.accounts.read().await;
        if accounts.len() >= self.config.max_accounts {
            return Err(norn_common::error::NornError::Internal(
                "Maximum accounts reached".to_string()
            ));
        }
        drop(accounts);

        // Generate new key pair
        let key_pair = VRFKeyPair::generate();
        let pub_key_bytes = key_pair.public_key_bytes();
        
        // Derive address from public key (first 20 bytes of hash)
        let address = self.derive_address(&pub_key_bytes);
        
        // Create account
        let account = WalletAccount {
            address,
            name: name.to_string(),
            is_locked: false,
            created_at: chrono::Utc::now().timestamp(),
        };

        // Store account and key pair
        self.accounts.write().await.insert(address, account);
        self.key_pairs.write().await.insert(address, key_pair);

        // Set as default if first account
        let mut default = self.default_account.write().await;
        if default.is_none() {
            *default = Some(address);
        }

        info!("Created new account: {} ({})", name, hex::encode(&address.0));
        Ok(address)
    }

    /// Derive address from public key bytes
    fn derive_address(&self, pub_key: &[u8]) -> Address {
        use sha2::{Sha256, Digest};
        
        let mut hasher = Sha256::new();
        hasher.update(pub_key);
        let hash = hasher.finalize();
        
        let mut address = Address::default();
        address.0.copy_from_slice(&hash[..20]);
        address
    }

    /// Get account by address
    pub async fn get_account(&self, address: &Address) -> Option<WalletAccount> {
        self.accounts.read().await.get(address).cloned()
    }

    /// List all accounts
    pub async fn list_accounts(&self) -> Vec<WalletAccount> {
        self.accounts.read().await.values().cloned().collect()
    }

    /// Get account count
    pub async fn account_count(&self) -> usize {
        self.accounts.read().await.len()
    }

    /// Get default account
    pub async fn get_default_account(&self) -> Option<Address> {
        *self.default_account.read().await
    }

    /// Set default account
    pub async fn set_default_account(&self, address: Address) -> Result<()> {
        let accounts = self.accounts.read().await;
        if !accounts.contains_key(&address) {
            return Err(norn_common::error::NornError::Internal(
                "Account not found".to_string()
            ));
        }
        drop(accounts);

        *self.default_account.write().await = Some(address);
        Ok(())
    }

    /// Lock account
    pub async fn lock_account(&self, address: &Address) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        if let Some(account) = accounts.get_mut(address) {
            account.is_locked = true;
            info!("Account locked: {}", hex::encode(&address.0));
            Ok(())
        } else {
            Err(norn_common::error::NornError::Internal(
                "Account not found".to_string()
            ))
        }
    }

    /// Unlock account
    pub async fn unlock_account(&self, address: &Address) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        if let Some(account) = accounts.get_mut(address) {
            account.is_locked = false;
            info!("Account unlocked: {}", hex::encode(&address.0));
            Ok(())
        } else {
            Err(norn_common::error::NornError::Internal(
                "Account not found".to_string()
            ))
        }
    }

    /// Sign transaction with account's key
    pub async fn sign_transaction(&self, from: &Address, tx_body: &TransactionBody) -> Result<Vec<u8>> {
        if !self.config.enable_signing {
            return Err(norn_common::error::NornError::Internal(
                "Signing disabled".to_string()
            ));
        }

        let accounts = self.accounts.read().await;
        let account = accounts.get(from).ok_or_else(|| {
            norn_common::error::NornError::Internal("Account not found".to_string())
        })?;

        if account.is_locked {
            return Err(norn_common::error::NornError::Internal(
                "Account is locked".to_string()
            ));
        }
        drop(accounts);

        // Get key pair
        let key_pairs = self.key_pairs.read().await;
        let _key_pair = key_pairs.get(from).ok_or_else(|| {
            norn_common::error::NornError::Internal("Key pair not found".to_string())
        })?;

        // Create signature (simplified - in production use proper signing)
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&tx_body.hash.0);
        let signature = hasher.finalize().to_vec();

        debug!("Signed transaction for {}", hex::encode(&from.0));
        Ok(signature)
    }

    /// Remove account
    pub async fn remove_account(&self, address: &Address) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        if accounts.remove(address).is_none() {
            return Err(norn_common::error::NornError::Internal(
                "Account not found".to_string()
            ));
        }
        drop(accounts);

        // Remove key pair
        self.key_pairs.write().await.remove(address);

        // Update default account if needed
        let mut default = self.default_account.write().await;
        if *default == Some(*address) {
            *default = None;
        }

        info!("Removed account: {}", hex::encode(&address.0));
        Ok(())
    }
}

impl Default for Wallet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_account() {
        let wallet = Wallet::new();
        
        let address = wallet.create_account("Test Account").await.unwrap();
        assert!(!address.0.iter().all(|&b| b == 0));
        
        let account = wallet.get_account(&address).await;
        assert!(account.is_some());
        assert_eq!(account.unwrap().name, "Test Account");
    }

    #[tokio::test]
    async fn test_list_accounts() {
        let wallet = Wallet::new();
        
        wallet.create_account("Account 1").await.unwrap();
        wallet.create_account("Account 2").await.unwrap();
        
        let accounts = wallet.list_accounts().await;
        assert_eq!(accounts.len(), 2);
    }

    #[tokio::test]
    async fn test_default_account() {
        let wallet = Wallet::new();
        
        let addr1 = wallet.create_account("First").await.unwrap();
        let addr2 = wallet.create_account("Second").await.unwrap();
        
        // First account should be default
        assert_eq!(wallet.get_default_account().await, Some(addr1));
        
        // Change default
        wallet.set_default_account(addr2).await.unwrap();
        assert_eq!(wallet.get_default_account().await, Some(addr2));
    }

    #[tokio::test]
    async fn test_lock_unlock() {
        let wallet = Wallet::new();
        
        let address = wallet.create_account("Test").await.unwrap();
        
        wallet.lock_account(&address).await.unwrap();
        let account = wallet.get_account(&address).await.unwrap();
        assert!(account.is_locked);
        
        wallet.unlock_account(&address).await.unwrap();
        let account = wallet.get_account(&address).await.unwrap();
        assert!(!account.is_locked);
    }
}
