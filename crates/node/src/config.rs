use k256::ecdsa::VerifyingKey;
use norn_common::consensus_types::StakeSnapshot;
use norn_common::genesis::GenesisConfig;
use norn_common::types::ValidatorId;
use norn_core::config::CoreConfig;
use norn_network::config::NetworkConfig;
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    Production,
    Devnet,
    Test,
}

impl Default for NetworkMode {
    fn default() -> Self {
        Self::Devnet
    }
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Validator,
    FullNode,
}

impl Default for NodeRole {
    fn default() -> Self {
        // A node without an explicitly selected role must not accidentally
        // generate validator keys or participate in consensus.
        Self::FullNode
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub core: CoreConfig,
    pub network: NetworkConfig,
    pub rpc_address: SocketAddr,
    pub data_dir: String,

    #[serde(default)]
    pub network_mode: NetworkMode,

    #[serde(default)]
    pub node_role: NodeRole,

    /// Explicit Genesis JSON path. Production mode never falls back to the
    /// historical built-in Genesis block.
    #[serde(default)]
    pub genesis_path: Option<String>,

    /// Dev/test-only grace period before the first consensus round. This lets
    /// a fresh validator set form its authenticated mesh (and test fixtures
    /// finish deterministic pre-funding) before it can sign a vote.
    #[serde(default)]
    pub consensus_start_delay_ms: u64,

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

impl NodeConfig {
    pub fn load_genesis_config(&self) -> anyhow::Result<GenesisConfig> {
        match self.genesis_path.as_deref() {
            Some(path) => GenesisConfig::load_from_path(path),
            None if self.network_mode == NetworkMode::Production => Err(anyhow::anyhow!(
                "Production mode requires an explicit genesis_path"
            )),
            None => Ok(GenesisConfig::from_fixed_genesis()),
        }
    }

    pub fn validate_genesis_for_role(
        &self,
        genesis: &GenesisConfig,
    ) -> anyhow::Result<StakeSnapshot> {
        let allow_empty_non_production_genesis = self.genesis_path.is_none()
            && matches!(self.network_mode, NetworkMode::Devnet | NetworkMode::Test)
            && self.node_role == NodeRole::FullNode;

        if allow_empty_non_production_genesis {
            genesis
                .validate_allow_empty_validators()
                .map_err(|e| anyhow::anyhow!("invalid test Genesis: {}", e))?;
            return Ok(StakeSnapshot::default());
        }

        genesis
            .validate()
            .map_err(|e| anyhow::anyhow!("invalid Genesis: {}", e))?;
        let snapshot = genesis
            .stake_snapshot()
            .map_err(|e| anyhow::anyhow!("invalid Genesis validator snapshot: {}", e))?;

        for (map_id, record) in &snapshot.validators {
            if *map_id != record.validator_id {
                return Err(anyhow::anyhow!(
                    "Genesis validator map key does not match ValidatorId {}",
                    record.validator_id
                ));
            }
            VerifyingKey::from_sec1_bytes(&record.consensus_public_key.0).map_err(|_| {
                anyhow::anyhow!(
                    "Genesis validator {} has an invalid consensus public key",
                    record.validator_id
                )
            })?;
            norn_crypto::vrf::VRFKeyPair::validate_public_key_bytes(&record.vrf_public_key.0)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Genesis validator {} has an invalid VRF public key: {}",
                        record.validator_id,
                        e
                    )
                })?;
        }

        Ok(snapshot)
    }
}

pub fn validate_validator_key_match(
    snapshot: &StakeSnapshot,
    consensus_public_key: [u8; 33],
    vrf_public_key: [u8; 32],
) -> anyhow::Result<ValidatorId> {
    // ValidatorId is an independent, stable identity. Match both rotating
    // key records and return the ID declared by Genesis rather than deriving
    // identity from one of the keys.
    snapshot
        .validators
        .values()
        .find(|record| {
            record.consensus_public_key.0 == consensus_public_key
                && record.vrf_public_key.0 == vrf_public_key
        })
        .map(|record| record.validator_id)
        .ok_or_else(|| anyhow::anyhow!("validator public keys do not match any Genesis record"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::SigningKey;
    use norn_common::consensus_types::ValidatorRecord;
    use norn_common::genesis::GenesisConfig;
    use norn_common::types::{ConsensusPublicKey, VrfPublicKey};
    use norn_core::consensus::types::ElectionMath;
    use norn_crypto::vrf::VRFKeyPair;
    use tempfile::tempdir;

    fn test_core_config() -> CoreConfig {
        CoreConfig {
            consensus: norn_core::config::ConsensusConfig {
                pub_key: "".into(),
                prv_key: "".into(),
            },
        }
    }

    #[test]
    fn production_requires_explicit_genesis() {
        let config = NodeConfig {
            core: test_core_config(),
            network: NetworkConfig::default(),
            rpc_address: "127.0.0.1:0".parse().unwrap(),
            data_dir: tempdir().unwrap().path().to_string_lossy().to_string(),
            network_mode: NetworkMode::Production,
            node_role: NodeRole::FullNode,
            genesis_path: None,
            ..NodeConfig::test_defaults()
        };

        assert!(config.load_genesis_config().is_err());
    }

    #[test]
    fn txpool_default_matches_deserialization_defaults() {
        let config = TxPoolConfig::default();

        assert!(config.enabled);
        assert!(config.enhanced);
        assert_eq!(config.max_size, 50_000);
        assert_eq!(config.v2_max_txs_per_block, 8192);
        assert_eq!(config.expiration_seconds, 3_600);
    }

    #[test]
    fn logging_default_keeps_stdout_enabled() {
        let config = LoggingConfig::default();

        assert_eq!(config.level, "info");
        assert_eq!(config.format, "json");
        assert_eq!(config.outputs, vec!["stdout"]);
        assert_eq!(config.max_file_size, 100);
        assert_eq!(config.max_files, 10);
        assert!(config.compress);
    }

    #[test]
    fn validator_keys_must_match_genesis_record() {
        let consensus_key = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let vrf_key = VRFKeyPair::from_seed(b"stage-one-validator");
        let consensus_bytes: [u8; 33] = consensus_key
            .verifying_key()
            .to_sec1_bytes()
            .as_ref()
            .try_into()
            .unwrap();
        let vrf_bytes = vrf_key.public_key_bytes();
        let validator_id = ValidatorId([0xA5; 32]);

        let snapshot = StakeSnapshot::from_genesis(
            1,
            vec![ValidatorRecord {
                validator_id,
                consensus_public_key: ConsensusPublicKey(consensus_bytes),
                vrf_public_key: VrfPublicKey(vrf_bytes),
                voting_power: 1,
                jailed_until_epoch: None,
                slashed: false,
            }],
        )
        .unwrap();

        assert_eq!(
            validate_validator_key_match(&snapshot, consensus_bytes, vrf_bytes).unwrap(),
            validator_id
        );
        assert!(validate_validator_key_match(&snapshot, [2u8; 33], vrf_bytes).is_err());
    }

    #[test]
    fn four_configs_load_identical_snapshot_and_proposer_sequence() {
        let mut genesis = GenesisConfig::from_fixed_genesis();
        let mut records = Vec::new();
        for index in 1u8..=4 {
            let consensus_key = SigningKey::from_slice(&[index; 32]).unwrap();
            let vrf_key = VRFKeyPair::from_seed(&[index; 8]);
            let consensus_bytes: [u8; 33] = consensus_key
                .verifying_key()
                .to_sec1_bytes()
                .as_ref()
                .try_into()
                .unwrap();
            let vrf_bytes = vrf_key.public_key_bytes();
            records.push(ValidatorRecord {
                // ValidatorId is deliberately independent from the VRF key.
                validator_id: ValidatorId([index + 10; 32]),
                consensus_public_key: ConsensusPublicKey(consensus_bytes),
                vrf_public_key: VrfPublicKey(vrf_bytes),
                voting_power: u64::from(index),
                jailed_until_epoch: None,
                slashed: false,
            });
        }

        let snapshot = StakeSnapshot::from_genesis(1, records.clone()).unwrap();
        genesis.validators = records;
        genesis.genesis_block.header.stake_snapshot_hash = snapshot.snapshot_hash;

        let genesis_path = tempdir().unwrap();
        let path = genesis_path.path().join("genesis.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&genesis).unwrap()).unwrap();

        let mut expected = None;
        for index in 0..4 {
            let mut config = NodeConfig::test_defaults();
            config.genesis_path = Some(path.to_string_lossy().to_string());
            config.node_role = NodeRole::FullNode;
            config.network_mode = NetworkMode::Test;
            config.data_dir = format!("node-{index}");

            let loaded = config.load_genesis_config().unwrap();
            let loaded_snapshot = config.validate_genesis_for_role(&loaded).unwrap();
            let sequence: Vec<_> = (1..=8)
                .map(|height| {
                    ElectionMath::select_deterministic_proposer(
                        &loaded.context().chain_id,
                        loaded.epoch,
                        height,
                        0,
                        &loaded.initial_randomness,
                        &loaded_snapshot,
                    )
                })
                .collect();

            if let Some((expected_hash, expected_snapshot, expected_sequence)) = &expected {
                assert_eq!(loaded.context().genesis_hash, *expected_hash);
                assert_eq!(&loaded_snapshot, expected_snapshot);
                assert_eq!(&sequence, expected_sequence);
            } else {
                expected = Some((loaded.context().genesis_hash, loaded_snapshot, sequence));
            }
        }
    }

    impl NodeConfig {
        fn test_defaults() -> Self {
            Self {
                core: test_core_config(),
                network: NetworkConfig::default(),
                rpc_address: "127.0.0.1:0".parse().unwrap(),
                data_dir: String::new(),
                network_mode: NetworkMode::Devnet,
                node_role: NodeRole::FullNode,
                genesis_path: None,
                consensus_start_delay_ms: 0,
                txpool: TxPoolConfig::default(),
                sync: SyncConfig::default(),
                monitoring: MonitoringConfig::default(),
                logging: LoggingConfig::default(),
            }
        }
    }
}

/// Transaction pool configuration
#[derive(Debug, Deserialize, Clone)]
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

    /// Local V2 proposal execution cap. This is intentionally separate from
    /// the protocol maximum and from the verification work queue so ARM
    /// validators can apply bounded backpressure without rejecting a short
    /// burst at the RPC boundary.
    #[serde(default = "default_txpool_v2_max_txs_per_block")]
    pub v2_max_txs_per_block: usize,

    /// Transaction expiration time in seconds
    #[serde(default = "default_txpool_expiration")]
    pub expiration_seconds: i64,
}

impl Default for TxPoolConfig {
    fn default() -> Self {
        Self {
            enabled: default_txpool_enabled(),
            enhanced: default_txpool_enhanced(),
            max_size: default_txpool_max_size(),
            v2_max_txs_per_block: default_txpool_v2_max_txs_per_block(),
            expiration_seconds: default_txpool_expiration(),
        }
    }
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
#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    #[serde(default = "default_logging_level")]
    pub level: String,

    /// Log format: "json" or "pretty"
    #[serde(default = "default_logging_format")]
    pub format: String,

    /// Log outputs: "stdout", "file", or both
    #[serde(default = "default_logging_outputs")]
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

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_logging_level(),
            format: default_logging_format(),
            outputs: default_logging_outputs(),
            file_path: None,
            max_file_size: default_logging_max_file_size(),
            max_files: default_logging_max_files(),
            compress: default_logging_compress(),
        }
    }
}

// Convert from config::LoggingConfig to logging::LoggingConfig
impl From<crate::config::LoggingConfig> for crate::logging::LoggingConfig {
    fn from(config: crate::config::LoggingConfig) -> Self {
        use crate::logging::{LogFormat, LogOutput};

        let format = match config.format.as_str() {
            "json" => LogFormat::Json,
            "pretty" | _ => LogFormat::Pretty,
        };

        let outputs = config
            .outputs
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

fn default_txpool_enabled() -> bool {
    true
}
fn default_txpool_enhanced() -> bool {
    true
}
fn default_txpool_max_size() -> usize {
    50000
}
fn default_txpool_v2_max_txs_per_block() -> usize {
    // Keep the local cap aligned with the default Genesis transaction limit.
    // Byte and gas ceilings still bound the actual proposal size.
    8192
}
fn default_txpool_expiration() -> i64 {
    3600
}

fn default_sync_mode() -> String {
    "fast".to_string()
}
fn default_sync_header_batch() -> usize {
    500
}
fn default_sync_body_batch() -> usize {
    100
}
fn default_sync_checkpoint() -> u64 {
    1000
}

fn default_monitoring_prometheus() -> bool {
    true
}
fn default_monitoring_prometheus_addr() -> String {
    "0.0.0.0:9090".to_string()
}
fn default_monitoring_health() -> bool {
    true
}
fn default_monitoring_health_addr() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_logging_level() -> String {
    "info".to_string()
}
fn default_logging_format() -> String {
    "json".to_string()
}
fn default_logging_outputs() -> Vec<String> {
    vec!["stdout".to_string()]
}
fn default_logging_max_file_size() -> u64 {
    100
}
fn default_logging_max_files() -> usize {
    10
}
fn default_logging_compress() -> bool {
    true
}
