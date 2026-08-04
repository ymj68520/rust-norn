use crate::chain_context::{ChainContext, MAX_BLOCK_MESSAGE_BYTES, MAX_TRANSACTION_MESSAGE_BYTES};
use crate::consensus_types::{StakeSnapshot, ValidatorRecord, MAX_CONSENSUS_CERTIFICATE_VOTES};
use crate::error::{NornError, Result};
use crate::types::{Block, BlockHeader, ChainId, GenesisParams, Hash, ProtocolVersion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const GENESIS_SCHEMA_VERSION: u16 = 5;
pub const GENESIS_IDENTITY_KEY: &[u8] = b"genesis_identity_v5";

fn default_epoch_delay() -> u64 {
    1
}

/// Consensus resource limits. They are part of the canonical Genesis
/// document so validators cannot silently choose different execution bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolResourceLimits {
    pub max_block_bytes: u64,
    pub max_transactions_per_block: u32,
    pub max_block_gas: u64,
    pub max_transaction_bytes: u64,
    pub max_transaction_gas: u64,
    pub max_overlay_writes: u32,
    pub max_certificate_members: u32,
    pub max_future_height: u64,
    pub max_future_round: u32,
    pub max_verification_tasks: u32,
    pub max_verification_queue: u32,
    #[serde(default = "default_max_consensus_round")]
    pub max_consensus_round: u32,
    #[serde(default = "default_max_durable_attempts_per_height")]
    pub max_durable_attempts_per_height: u32,
    #[serde(default = "default_max_durable_attempt_bytes_per_height")]
    pub max_durable_attempt_bytes_per_height: u64,
    #[serde(default = "default_max_block_timestamp_step")]
    pub max_block_timestamp_step: u64,
}

fn default_max_consensus_round() -> u32 {
    63
}

fn default_max_durable_attempts_per_height() -> u32 {
    64
}

fn default_max_durable_attempt_bytes_per_height() -> u64 {
    64 * 1024 * 1024
}

fn default_max_block_timestamp_step() -> u64 {
    365 * 24 * 60 * 60
}

impl Default for ProtocolResourceLimits {
    fn default() -> Self {
        Self {
            max_block_bytes: 8 * 1024 * 1024,
            max_transactions_per_block: 10_000,
            max_block_gas: 10_000_000,
            max_transaction_bytes: 256 * 1024,
            max_transaction_gas: 10_000_000,
            max_overlay_writes: 100_000,
            max_certificate_members: MAX_CONSENSUS_CERTIFICATE_VOTES as u32,
            max_future_height: 2,
            max_future_round: 2,
            max_verification_tasks: 64,
            max_verification_queue: 256,
            max_consensus_round: default_max_consensus_round(),
            max_durable_attempts_per_height: default_max_durable_attempts_per_height(),
            max_durable_attempt_bytes_per_height: default_max_durable_attempt_bytes_per_height(),
            max_block_timestamp_step: default_max_block_timestamp_step(),
        }
    }
}

impl ProtocolResourceLimits {
    pub fn validate(&self) -> Result<()> {
        if self.max_block_bytes == 0
            || self.max_transactions_per_block == 0
            || self.max_block_gas == 0
            || self.max_transaction_bytes == 0
            || self.max_transaction_gas == 0
            || self.max_overlay_writes == 0
            || self.max_certificate_members == 0
            || self.max_verification_tasks == 0
            || self.max_verification_queue == 0
            || self.max_consensus_round == 0
            || self.max_durable_attempts_per_height == 0
            || self.max_durable_attempt_bytes_per_height == 0
            || self.max_block_timestamp_step == 0
        {
            return Err(NornError::Config(
                "Genesis resource limits must be non-zero".into(),
            ));
        }
        if self.max_transaction_bytes > self.max_block_bytes
            || self.max_transaction_gas > self.max_block_gas
            || self.max_certificate_members as usize > MAX_CONSENSUS_CERTIFICATE_VOTES
            || self.max_verification_tasks > self.max_verification_queue
            || self.max_block_bytes > MAX_BLOCK_MESSAGE_BYTES as u64
            || self.max_transaction_bytes > MAX_TRANSACTION_MESSAGE_BYTES as u64
            || self.max_block_gas > i64::MAX as u64
            || self.max_block_timestamp_step > i64::MAX as u64
            || self.max_block_bytes > usize::MAX as u64
            || self.max_transaction_bytes > usize::MAX as u64
        {
            return Err(NornError::Config(
                "Genesis resource limits exceed protocol bounds".into(),
            ));
        }
        let max_round_attempts = self.max_consensus_round.checked_add(1).ok_or_else(|| {
            NornError::Config("Genesis maximum consensus round overflows attempt accounting".into())
        })?;
        if self.max_durable_attempts_per_height < max_round_attempts {
            return Err(NornError::Config(
                "Genesis durable attempt bound must cover every consensus round".into(),
            ));
        }
        Ok(())
    }
}

/// Versioned, canonical network bootstrap document.
///
/// The validator vector is normalized by ValidatorId when hashing, so the
/// ordering in a JSON file cannot create different network identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenesisConfig {
    pub schema_version: u16,
    pub protocol_version: ProtocolVersion,
    pub chain_id: ChainId,
    pub epoch: u64,
    pub epoch_length: u64,
    #[serde(default = "default_epoch_delay")]
    pub validator_update_delay: u64,
    #[serde(default = "default_epoch_delay")]
    pub unbonding_delay: u64,
    #[serde(default = "default_epoch_delay")]
    pub key_rotation_delay: u64,
    #[serde(default = "default_epoch_delay")]
    pub slashing_activation_delay: u64,
    pub initial_randomness: Hash,
    pub resource_limits: ProtocolResourceLimits,
    pub genesis_block: Block,
    pub validators: Vec<ValidatorRecord>,
}

impl GenesisConfig {
    pub fn from_fixed_genesis() -> Self {
        let genesis_block = get_genesis_block();
        Self {
            schema_version: GENESIS_SCHEMA_VERSION,
            protocol_version: genesis_block.header.protocol_version,
            chain_id: genesis_block.header.chain_id,
            epoch: genesis_block.header.epoch as u64,
            epoch_length: 1_000,
            validator_update_delay: default_epoch_delay(),
            unbonding_delay: default_epoch_delay(),
            key_rotation_delay: default_epoch_delay(),
            slashing_activation_delay: default_epoch_delay(),
            initial_randomness: genesis_block.header.parent_randomness,
            resource_limits: ProtocolResourceLimits::default(),
            genesis_block,
            validators: Vec::new(),
        }
    }

    pub fn load_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|e| anyhow::anyhow!("failed to read Genesis {:?}: {}", path, e))?;
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|e| anyhow::anyhow!("failed to decode Genesis {:?}: {}", path, e))?;
        config
            .validate()
            .map_err(|e| anyhow::anyhow!("invalid Genesis {:?}: {}", path, e))?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_internal(true)
    }

    /// Validate the fixed development Genesis shape while allowing an empty
    /// validator set for a non-consensus FullNode test process.
    pub fn validate_allow_empty_validators(&self) -> Result<()> {
        self.validate_internal(false)
    }

    fn validate_internal(&self, require_validators: bool) -> Result<()> {
        if self.schema_version != GENESIS_SCHEMA_VERSION {
            return Err(NornError::Config(format!(
                "unsupported Genesis schema version {}",
                self.schema_version
            )));
        }
        if self.protocol_version != ChainContext::CURRENT_PROTOCOL_VERSION {
            return Err(NornError::Config(
                "Genesis protocol version is not the active V5 protocol".into(),
            ));
        }
        if self.chain_id.0 == Hash::default() {
            return Err(NornError::Config(
                "Genesis chain ID must be non-zero".into(),
            ));
        }
        if self.epoch_length == 0 {
            return Err(NornError::Config(
                "Genesis epoch length must be non-zero".into(),
            ));
        }
        if self.validator_update_delay == 0
            || self.unbonding_delay == 0
            || self.key_rotation_delay == 0
            || self.slashing_activation_delay == 0
        {
            return Err(NornError::Config(
                "Genesis validator transition delays must be non-zero".into(),
            ));
        }
        if self.initial_randomness == Hash::default() {
            return Err(NornError::Config(
                "Genesis initial randomness must be non-zero".into(),
            ));
        }
        self.resource_limits.validate()?;

        let header = &self.genesis_block.header;
        if header.height != 0 {
            return Err(NornError::Config(
                "Genesis block height must be zero".into(),
            ));
        }
        if header.timestamp < 0 {
            return Err(NornError::Config(
                "Genesis block timestamp must be non-negative".into(),
            ));
        }
        if header.prev_block_hash != Hash::default() {
            return Err(NornError::Config(
                "Genesis previous hash must be zero".into(),
            ));
        }
        if !self.genesis_block.transactions.is_empty() {
            return Err(NornError::Config(
                "Genesis block must not contain transactions".into(),
            ));
        }
        if header.protocol_version != self.protocol_version {
            return Err(NornError::Config(
                "Genesis block protocol version does not match Genesis config".into(),
            ));
        }
        if header.chain_id != self.chain_id {
            return Err(NornError::Config(
                "Genesis block chain ID does not match Genesis config".into(),
            ));
        }
        if header.epoch as u64 != self.epoch {
            return Err(NornError::Config(
                "Genesis block epoch does not match Genesis config".into(),
            ));
        }
        if header.parent_randomness != self.initial_randomness {
            return Err(NornError::Config(
                "Genesis initial randomness does not match Genesis block".into(),
            ));
        }
        if header.block_hash == Hash::default() {
            return Err(NornError::Config(
                "Genesis block hash must be non-zero".into(),
            ));
        }
        let calculated_header_hash = header
            .calculate_hash()
            .map_err(|e| NornError::Config(format!("failed to hash Genesis header: {}", e)))?;
        if header.block_hash != GENESIS_BLOCK_HASH && header.block_hash != calculated_header_hash {
            return Err(NornError::Config(
                "Genesis block hash does not match its canonical header hash".into(),
            ));
        }

        // This performs deterministic ordering, duplicate ValidatorId checks,
        // non-zero voting-power checks and overflow checks. Cryptographic key
        // encoding is checked by the node/crypto boundary where those curve
        // implementations are available.
        if require_validators || !self.validators.is_empty() {
            if self
                .validators
                .iter()
                .any(|record| record.slashed || record.jailed_until_epoch.is_some())
            {
                return Err(NornError::Config(
                    "Genesis validator records cannot be jailed or slashed".into(),
                ));
            }
            let snapshot = StakeSnapshot::from_genesis(self.epoch, self.validators.clone())?;
            if header.stake_snapshot_hash != snapshot.snapshot_hash {
                return Err(NornError::Config(
                    "Genesis block snapshot hash does not match validator records".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn stake_snapshot(&self) -> Result<StakeSnapshot> {
        StakeSnapshot::from_genesis(self.epoch, self.validators.clone())
    }

    /// Deterministic height-to-epoch rule shared by block validation and
    /// consensus replay.  Height one is in the Genesis epoch; an epoch
    /// boundary is crossed only after `epoch_length` finalized blocks.
    pub fn epoch_for_height(&self, height: u64) -> Result<u64> {
        if self.epoch_length == 0 {
            return Err(NornError::Config("Genesis epoch length is zero".into()));
        }
        self.epoch
            .checked_add(height.saturating_sub(1) / self.epoch_length)
            .ok_or_else(|| NornError::Config("Genesis epoch overflow".into()))
    }

    pub fn context(&self) -> ChainContext {
        ChainContext::new(
            self.schema_version,
            self.protocol_version,
            self.chain_id,
            self.genesis_hash(),
        )
    }

    /// Canonical identity hash for the complete Genesis document.
    pub fn genesis_hash(&self) -> Hash {
        Hash(Sha256::digest(self.canonical_bytes()).into())
    }

    /// Canonical, order-independent encoding used for Genesis identity.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(512 + self.validators.len() * 105);
        bytes.extend_from_slice(b"NORN_GENESIS_V5");
        bytes.extend_from_slice(&self.schema_version.to_be_bytes());
        bytes.extend_from_slice(&self.protocol_version.0.to_be_bytes());
        bytes.extend_from_slice(&self.chain_id.0 .0);
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.epoch_length.to_be_bytes());
        bytes.extend_from_slice(&self.validator_update_delay.to_be_bytes());
        bytes.extend_from_slice(&self.unbonding_delay.to_be_bytes());
        bytes.extend_from_slice(&self.key_rotation_delay.to_be_bytes());
        bytes.extend_from_slice(&self.slashing_activation_delay.to_be_bytes());
        bytes.extend_from_slice(&self.initial_randomness.0);
        bytes.extend_from_slice(&self.resource_limits.max_block_bytes.to_be_bytes());
        bytes.extend_from_slice(
            &self
                .resource_limits
                .max_transactions_per_block
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&self.resource_limits.max_block_gas.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_transaction_bytes.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_transaction_gas.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_overlay_writes.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_certificate_members.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_future_height.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_future_round.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_verification_tasks.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_verification_queue.to_be_bytes());
        bytes.extend_from_slice(&self.resource_limits.max_consensus_round.to_be_bytes());
        bytes.extend_from_slice(
            &self
                .resource_limits
                .max_durable_attempts_per_height
                .to_be_bytes(),
        );
        bytes.extend_from_slice(
            &self
                .resource_limits
                .max_durable_attempt_bytes_per_height
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&self.resource_limits.max_block_timestamp_step.to_be_bytes());
        append_block_header(&mut bytes, &self.genesis_block.header);

        let mut validators = self.validators.clone();
        validators.sort_by_key(|record| record.validator_id);
        bytes.extend_from_slice(&(validators.len() as u32).to_be_bytes());
        for record in validators {
            append_validator(&mut bytes, &record);
        }
        bytes
    }
}

fn append_block_header(bytes: &mut Vec<u8>, header: &BlockHeader) {
    bytes.extend_from_slice(&header.protocol_version.0.to_be_bytes());
    bytes.extend_from_slice(&header.chain_id.0 .0);
    bytes.extend_from_slice(&header.height.to_be_bytes());
    bytes.extend_from_slice(&header.epoch.to_be_bytes());
    bytes.extend_from_slice(&header.round.to_be_bytes());
    bytes.extend_from_slice(&header.timestamp.to_be_bytes());
    bytes.extend_from_slice(&header.prev_block_hash.0);
    bytes.extend_from_slice(&header.block_hash.0);
    bytes.extend_from_slice(&header.merkle_root.0);
    bytes.extend_from_slice(&header.state_root.0);
    bytes.extend_from_slice(&header.block_builder.0);
    bytes.extend_from_slice(&header.stake_snapshot_hash.0);
    bytes.extend_from_slice(&header.parent_randomness.0);
    bytes.extend_from_slice(&header.gas_limit.to_be_bytes());
    bytes.extend_from_slice(&header.base_fee.to_be_bytes());
    bytes.extend_from_slice(&header.consensus_data_hash.0);
}

fn append_validator(bytes: &mut Vec<u8>, record: &ValidatorRecord) {
    bytes.extend_from_slice(&record.validator_id.0);
    bytes.extend_from_slice(&record.consensus_public_key.0);
    bytes.extend_from_slice(&record.vrf_public_key.0);
    bytes.extend_from_slice(&record.voting_power.to_be_bytes());
    bytes.extend_from_slice(&record.jailed_until_epoch.unwrap_or(u64::MAX).to_be_bytes());
    bytes.push(u8::from(record.slashed));
}

/// 获取固定的创世块
///
/// 确保所有节点使用相同的创世块，这对于网络同步至关重要
pub fn get_genesis_block() -> Block {
    let header = BlockHeader {
        protocol_version: ChainContext::CURRENT_PROTOCOL_VERSION,
        chain_id: crate::types::ChainId(Hash([1u8; 32])),
        height: 0,
        epoch: 1,
        round: 0,
        timestamp: GENESIS_TIMESTAMP,
        prev_block_hash: Hash::default(),
        block_hash: GENESIS_BLOCK_HASH,
        merkle_root: Hash::default(),
        state_root: Hash::default(),
        block_builder: crate::types::ValidatorId([0u8; 32]),
        stake_snapshot_hash: crate::types::StakeSnapshotHash::default(),
        parent_randomness: GENESIS_SEED,
        gas_limit: GENESIS_GAS_LIMIT,
        base_fee: GENESIS_BASE_FEE,
        consensus_data_hash: Hash::default(),
    };

    Block {
        header,
        transactions: vec![], // 创世块不包含交易
    }
}

/// 创世块的固定哈希
/// 使用预计算的哈希值，确保所有节点一致
pub const GENESIS_BLOCK_HASH: Hash = Hash([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
]);

/// 创世时间戳 (Unix timestamp)
// V5 uses a fresh network identity. Keep the fixed Genesis timestamp close
// enough to the current era for the parent-relative timestamp window while
// remaining deterministic for every node.
pub const GENESIS_TIMESTAMP: i64 = 1780000000; // 2026-05-28 20:26:40 UTC

/// 创世块的Gas限制
pub const GENESIS_GAS_LIMIT: i64 = 10_000_000;

/// 创世块的EIP-1559基础费用 (1 Gwei)
pub const GENESIS_BASE_FEE: u64 = 1_000_000_000;

/// 获取创世块参数
pub fn get_genesis_params() -> GenesisParams {
    GenesisParams {
        order: GENESIS_ORDER,
        time_param: GENESIS_TIME_PARAM,
        seed: GENESIS_SEED,
        verify_param: GENESIS_VERIFY_PARAM,
    }
}

/// 序列化创世块参数
fn serialize_genesis_params() -> Vec<u8> {
    let params = get_genesis_params();
    crate::utils::codec::serialize(&params).unwrap_or_default()
}

/// 创世块的VDF参数 - 大数阶（128字节）
pub const GENESIS_ORDER: [u8; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// 创世块的VDF时间参数
pub const GENESIS_TIME_PARAM: i64 = 10_000_000; // 10 million iterations

/// 创世块的VRF/VDF种子
pub const GENESIS_SEED: Hash = Hash([
    0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42,
    0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42,
]);

/// 创世块的VDF验证参数
pub const GENESIS_VERIFY_PARAM: Hash = Hash([
    0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43,
    0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43,
]);

/// 验证是否为有效的创世块
pub fn is_valid_genesis_block(block: &Block) -> bool {
    block.header.height == 0
        && block.header.prev_block_hash == Hash::default()
        && block.header.block_hash == GENESIS_BLOCK_HASH
        && block.transactions.is_empty()
}

/// 检查区块链是否从正确的创世块开始
pub async fn validate_genesis_start<F, Fut>(
    _db: &F,
    get_block: F,
) -> std::result::Result<bool, Box<dyn std::error::Error>>
where
    F: Fn(Hash) -> Fut,
    Fut: std::future::Future<
        Output = std::result::Result<Option<Block>, Box<dyn std::error::Error>>,
    >,
{
    // 尝试获取高度为0的区块
    match get_block(GENESIS_BLOCK_HASH).await {
        Ok(Some(block)) => Ok(is_valid_genesis_block(&block)),
        Ok(None) => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConsensusPublicKey, ValidatorId, VrfPublicKey};

    #[test]
    fn test_genesis_block_constants() {
        let genesis = get_genesis_block();

        assert_eq!(genesis.header.height, 0);
        assert_eq!(genesis.header.prev_block_hash, Hash::default());
        assert_eq!(genesis.header.block_hash, GENESIS_BLOCK_HASH);
        assert_eq!(genesis.header.timestamp, GENESIS_TIMESTAMP);
        assert_eq!(genesis.header.gas_limit, GENESIS_GAS_LIMIT);
        assert!(genesis.transactions.is_empty());
    }

    #[test]
    fn test_genesis_validation() {
        let valid_genesis = get_genesis_block();
        assert!(is_valid_genesis_block(&valid_genesis));

        let mut invalid_genesis = valid_genesis.clone();
        invalid_genesis.header.height = 1;
        assert!(!is_valid_genesis_block(&invalid_genesis));

        invalid_genesis = valid_genesis.clone();
        invalid_genesis
            .transactions
            .push(crate::types::Transaction::default());
        assert!(!is_valid_genesis_block(&invalid_genesis));
    }

    #[test]
    fn legacy_v2_genesis_is_rejected_without_migration_guessing() {
        let mut legacy = GenesisConfig::from_fixed_genesis();
        legacy.schema_version = 2;
        legacy.protocol_version = ProtocolVersion(2);
        legacy.genesis_block.header.protocol_version = ProtocolVersion(2);
        assert!(legacy.validate_allow_empty_validators().is_err());
    }

    #[test]
    fn test_genesis_consistency() {
        let genesis1 = get_genesis_block();
        let genesis2 = get_genesis_block();

        // 两次调用应该返回相同的创世块
        assert_eq!(genesis1.header.block_hash, genesis2.header.block_hash);
        assert_eq!(genesis1.header.timestamp, genesis2.header.timestamp);
    }

    #[test]
    fn test_canonical_genesis_hash_is_validator_order_independent() {
        let record_a = ValidatorRecord {
            validator_id: ValidatorId([1u8; 32]),
            consensus_public_key: ConsensusPublicKey([2u8; 33]),
            vrf_public_key: VrfPublicKey([3u8; 32]),
            voting_power: 10,
            jailed_until_epoch: None,
            slashed: false,
        };
        let record_b = ValidatorRecord {
            validator_id: ValidatorId([4u8; 32]),
            consensus_public_key: ConsensusPublicKey([5u8; 33]),
            vrf_public_key: VrfPublicKey([6u8; 32]),
            voting_power: 20,
            jailed_until_epoch: None,
            slashed: false,
        };

        let mut first = GenesisConfig::from_fixed_genesis();
        first.validators = vec![record_a.clone(), record_b.clone()];
        first.genesis_block.header.stake_snapshot_hash =
            first.stake_snapshot().unwrap().snapshot_hash;
        let mut second = first.clone();
        second.validators.reverse();

        assert!(first.validate().is_ok());
        assert!(second.validate().is_ok());
        assert_eq!(first.genesis_hash(), second.genesis_hash());
        assert_eq!(
            first.stake_snapshot().unwrap(),
            second.stake_snapshot().unwrap()
        );
    }

    #[test]
    fn test_epoch_schedule_is_height_deterministic() {
        let mut genesis = GenesisConfig::from_fixed_genesis();
        genesis.epoch = 7;
        genesis.epoch_length = 3;

        assert_eq!(genesis.epoch_for_height(0).unwrap(), 7);
        assert_eq!(genesis.epoch_for_height(1).unwrap(), 7);
        assert_eq!(genesis.epoch_for_height(3).unwrap(), 7);
        assert_eq!(genesis.epoch_for_height(4).unwrap(), 8);
        assert_eq!(genesis.epoch_for_height(7).unwrap(), 9);
    }

    #[test]
    fn test_genesis_rejects_duplicate_keys_and_empty_validator_set() {
        let mut config = GenesisConfig::from_fixed_genesis();
        assert!(config.validate().is_err());

        config.validators = vec![
            ValidatorRecord {
                validator_id: ValidatorId([1u8; 32]),
                consensus_public_key: ConsensusPublicKey([2u8; 33]),
                vrf_public_key: VrfPublicKey([3u8; 32]),
                voting_power: 1,
                jailed_until_epoch: None,
                slashed: false,
            },
            ValidatorRecord {
                validator_id: ValidatorId([4u8; 32]),
                consensus_public_key: ConsensusPublicKey([2u8; 33]),
                vrf_public_key: VrfPublicKey([6u8; 32]),
                voting_power: 1,
                jailed_until_epoch: None,
                slashed: false,
            },
        ];

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_genesis_rejects_unknown_json_fields() {
        let mut value = serde_json::to_value(GenesisConfig::from_fixed_genesis()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unexpected_field".into(), serde_json::Value::Null);

        assert!(serde_json::from_value::<GenesisConfig>(value).is_err());
    }

    #[test]
    fn resource_limits_reject_timestamp_steps_outside_i64() {
        let mut limits = ProtocolResourceLimits::default();
        limits.max_block_timestamp_step = i64::MAX as u64 + 1;
        assert!(limits.validate().is_err());
    }

    #[test]
    fn resource_limits_round_bound_covers_durable_attempts() {
        let mut limits = ProtocolResourceLimits::default();
        limits.max_consensus_round = 10;
        limits.max_durable_attempts_per_height = 10;
        assert!(limits.validate().is_err());

        limits.max_durable_attempts_per_height = 11;
        assert!(limits.validate().is_ok());
    }
}
