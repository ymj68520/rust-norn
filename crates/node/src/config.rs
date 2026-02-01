use serde::Deserialize;
use norn_core::config::CoreConfig;
use norn_network::config::NetworkConfig;
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub core: CoreConfig,
    pub network: NetworkConfig,
    pub rpc_address: SocketAddr,
    pub data_dir: String,

    // Enhanced features configuration
    #[serde(default)]
    pub txpool: TxPoolConfig,

    #[serde(default)]
    pub sync: SyncConfig,

    #[serde(default)]
    pub monitoring: MonitoringConfig,

    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Transaction pool configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct TxPoolConfig {
    /// Enable enhanced transaction pool
    #[serde(default = "default_txpool_enabled")]
    pub enabled: bool,

    /// Enable enhanced features (BinaryHeap, EIP-1559, etc.)
    #[serde(default = "default_txpool_enhanced")]
    pub enhanced: bool,

    /// Maximum pool size
    #[serde(default = "default_txpool_max_size")]
    pub max_size: usize,

    /// Transaction expiration time in seconds
    #[serde(default = "default_txpool_expiration")]
    pub expiration_seconds: i64,
}

/// Sync configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct SyncConfig {
    /// Sync mode: "fast" or "full"
    #[serde(default = "default_sync_mode")]
    pub mode: String,

    /// Number of headers to request per batch
    #[serde(default = "default_sync_header_batch")]
    pub header_batch_size: usize,

    /// Number of block bodies to request per batch
    #[serde(default = "default_sync_body_batch")]
    pub body_batch_size: usize,

    /// Verify state root every N blocks
    #[serde(default = "default_sync_checkpoint")]
    pub checkpoint_interval: u64,
}

/// Monitoring configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct MonitoringConfig {
    /// Enable Prometheus metrics
    #[serde(default = "default_monitoring_prometheus")]
    pub prometheus_enabled: bool,

    /// Prometheus metrics address
    #[serde(default = "default_monitoring_prometheus_addr")]
    pub prometheus_address: String,

    /// Enable health check endpoint
    #[serde(default = "default_monitoring_health")]
    pub health_check_enabled: bool,

    /// Health check endpoint address
    #[serde(default = "default_monitoring_health_addr")]
    pub health_check_address: String,
}

/// Logging configuration (simplified for TOML deserialization)
#[derive(Debug, Deserialize, Clone, Default)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    #[serde(default = "default_logging_level")]
    pub level: String,

    /// Log format: "json" or "pretty"
    #[serde(default = "default_logging_format")]
    pub format: String,

    /// Log outputs: "stdout", "file", or both
    #[serde(default)]
    pub outputs: Vec<String>,

    /// Log file path (if file output is enabled)
    #[serde(default)]
    pub file_path: Option<String>,

    /// Maximum log file size in MB
    #[serde(default = "default_logging_max_file_size")]
    pub max_file_size: u64,

    /// Maximum number of log files to keep
    #[serde(default = "default_logging_max_files")]
    pub max_files: usize,

    /// Compress old log files
    #[serde(default = "default_logging_compress")]
    pub compress: bool,
}

// Convert from config::LoggingConfig to logging::LoggingConfig
impl From<crate::config::LoggingConfig> for crate::logging::LoggingConfig {
    fn from(config: crate::config::LoggingConfig) -> Self {
        use crate::logging::{LogFormat, LogOutput};

        let format = match config.format.as_str() {
            "json" => LogFormat::Json,
            "pretty" | _ => LogFormat::Pretty,
        };

        let outputs = config.outputs
            .into_iter()
            .map(|s| match s.as_str() {
                "file" => LogOutput::File,
                _ => LogOutput::Stdout,
            })
            .collect();

        Self {
            level: config.level,
            format,
            outputs,
            file_path: config.file_path,
            max_file_size: config.max_file_size,
            max_files: config.max_files,
            compress: config.compress,
        }
    }
}

// Default functions

fn default_txpool_enabled() -> bool { true }
fn default_txpool_enhanced() -> bool { true }
fn default_txpool_max_size() -> usize { 10000 }
fn default_txpool_expiration() -> i64 { 3600 }

fn default_sync_mode() -> String { "fast".to_string() }
fn default_sync_header_batch() -> usize { 500 }
fn default_sync_body_batch() -> usize { 100 }
fn default_sync_checkpoint() -> u64 { 1000 }

fn default_monitoring_prometheus() -> bool { true }
fn default_monitoring_prometheus_addr() -> String { "0.0.0.0:9090".to_string() }
fn default_monitoring_health() -> bool { true }
fn default_monitoring_health_addr() -> String { "0.0.0.0:8080".to_string() }

fn default_logging_level() -> String { "info".to_string() }
fn default_logging_format() -> String { "json".to_string() }
fn default_logging_max_file_size() -> u64 { 100 }
fn default_logging_max_files() -> usize { 10 }
fn default_logging_compress() -> bool { true }
