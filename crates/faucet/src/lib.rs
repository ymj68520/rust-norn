//! Production-grade Faucet Service for Norn Blockchain
//!
//! This service provides secure token distribution with:
//! - Rate limiting (IP and address-based)
//! - Cooldown periods
//! - Database tracking
//! - Monitoring and metrics
//! - Web interface

pub mod config;
pub mod database;
pub mod error;
pub mod service;
pub mod api;

pub use config::FaucetConfig;
pub use database::{DistributionRecord, FaucetDatabase, FaucetStatistics};
pub use error::{FaucetError, FaucetResult};
pub use service::{BlockchainRpcClient, DispenseResponse, FaucetService, FaucetStatus};
