use anyhow::{anyhow, Result};
use k256::ecdsa::signature::Signer;
use k256::ecdsa::SigningKey;
use norn_common::chain_context::{
    ChainContext, PeerRole, MAX_BLOCK_MESSAGE_BYTES, MAX_TRANSACTION_MESSAGE_BYTES,
};
use norn_common::consensus_types::{
    CanonicalFinalizedTip, CommitCertificate, ConsensusEnvelope, ConsensusMessage, SignedVote,
    MAX_CONSENSUS_ENVELOPE_BYTES,
};
use norn_common::genesis::ProtocolResourceLimits;
use norn_common::types::ValidatorId;
use norn_core::blockchain::Blockchain;
use norn_core::consensus::driver::{ConsensusDriver, ConsensusDriverEvent};
use norn_core::consensus::povf::PoVFEngine;
use norn_core::consensus::producer::{BlockProducer, BlockProducerConfig};
use norn_core::consensus::safety_store::{ConsensusSigner, PersistentSafetyStore};
use norn_core::consensus::types::{ConsensusConfig, ProposalSigner};
use norn_core::evm::{EVMConfig, EVMExecutor};
use norn_core::finality::FinalityStore;
use norn_core::state::merkle::StateRootCalculator;
use norn_core::state::{AccountStateConfig, AccountStateManager};
use norn_core::txpool::TxPool;
use norn_core::txpool_v2::TransactionV2Pool;
use norn_network::{NetworkCommand, NetworkService};
use norn_storage::SledDB;

use crate::config::{validate_validator_key_match, NetworkMode, NodeConfig, NodeRole};
use crate::keystore::NodeKeyStore;
use crate::manager::PeerManager;
use crate::syncer::BlockSyncer;
use crate::tx_handler::TxHandler;
use libp2p::identity::Keypair;
use norn_rpc::{create_ethereum_rpc, start_ethereum_rpc_server, start_rpc_server};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::signal;
use tracing::{debug, error, info, warn};

use crate::metrics::MetricsCollector;
use crate::monitoring::MonitoringServer;

pub struct EcdsaConsensusSigner {
    signing_key: SigningKey,
    validator_id: ValidatorId,
}

impl EcdsaConsensusSigner {
    pub fn new(signing_key: SigningKey, validator_id: ValidatorId) -> Self {
        Self {
            signing_key,
            validator_id,
        }
    }
}

impl ConsensusSigner for EcdsaConsensusSigner {
    fn sign_canonical_bytes(&self, bytes: &[u8]) -> Result<[u8; 64]> {
        let sig: k256::ecdsa::Signature = self
            .signing_key
            .try_sign(bytes)
            .map_err(|e| anyhow!("ECDSA signing failed: {:?}", e))?;
        let sig_canonical = sig.normalize_s().unwrap_or(sig);
        let bytes_ref = sig_canonical.to_bytes();
        let arr: [u8; 64] = bytes_ref
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("Invalid signature length"))?;
        Ok(arr)
    }
}

impl ProposalSigner for EcdsaConsensusSigner {
    fn validator_id(&self) -> ValidatorId {
        self.validator_id
    }

    fn sign_proposal(&self, sign_bytes: &[u8]) -> Result<[u8; 64]> {
        self.sign_canonical_bytes(sign_bytes)
    }
}

pub struct NornNode {
    config: NodeConfig,
    blockchain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    #[allow(dead_code)]
    tx_pool_v2: Arc<TransactionV2Pool>,
    #[allow(dead_code)]
    network: Arc<NetworkService>,

    /// Consensus engine for PoVF BFT consensus
    consensus: Arc<PoVFEngine>,
    consensus_driver: ConsensusDriver,
    finality_store: Arc<FinalityStore>,
    signer: Option<Arc<EcdsaConsensusSigner>>,

    /// Block producer
    block_producer: Option<Arc<BlockProducer>>,

    chain_context: ChainContext,
    resource_limits: ProtocolResourceLimits,

    peer_manager: Arc<PeerManager>,
    syncer: Arc<BlockSyncer>,
    tx_handler: Arc<TxHandler>,

    /// State manager for EVM
    state_manager: Arc<AccountStateManager>,

    /// EVM executor
    evm_executor: Arc<EVMExecutor>,

    // Temp holder for startup
    network_rx: Option<tokio::sync::mpsc::Receiver<norn_network::service::NetworkEvent>>,

    #[allow(dead_code)]
    metrics_collector: Option<Arc<MetricsCollector>>,
    #[allow(dead_code)]
    _monitoring_server: Option<MonitoringServer>,
    #[allow(dead_code)]
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

impl NornNode {
    pub async fn new(config: NodeConfig, keypair: Keypair) -> Result<Self> {
        use crate::logging::LoggingConfig;
        let log_config: LoggingConfig = config.logging.clone().into();
        let log_guard = log_config.init()?;
        info!(
            "Logging initialized: format={:?}, level={}",
            log_config.format, log_config.level
        );

        let metrics_collector = if config.monitoring.prometheus_enabled {
            info!(
                "Initializing metrics collector on {}",
                config.monitoring.prometheus_address
            );
            Some(Arc::new(MetricsCollector::new()))
        } else {
            info!("Metrics collection disabled");
            None
        };

        if config.monitoring.health_check_enabled {
            if let Some(ref collector) = metrics_collector {
                let server = MonitoringServer::new(collector.clone());
                let address = config.monitoring.health_check_address.clone();
                info!("Monitoring server starting on {}", address);
                let address_log = address.clone();
                tokio::spawn(async move {
                    if let Err(e) = server.start(&address).await {
                        error!("Monitoring server failed: {}", e);
                    }
                });
                info!("Monitoring server started on {}", address_log);
            } else {
                warn!("Health check enabled but metrics collector is disabled");
            }
        } else {
            info!("Health check endpoint disabled");
        }

        let genesis_config = config.load_genesis_config()?;
        let genesis_snapshot = config.validate_genesis_for_role(&genesis_config)?;
        let genesis_snapshot_hash = genesis_snapshot.snapshot_hash;
        let chain_context = genesis_config.context();

        let db = Arc::new(SledDB::new(&config.data_dir)?);
        let blockchain = Blockchain::try_new_with_genesis(
            db.clone(),
            genesis_config.genesis_block.clone(),
            chain_context.genesis_hash,
        )
        .await?;

        let tx_pool = Arc::new(TxPool::new());
        let tx_pool_v2 = Arc::new(TransactionV2Pool::new_with_capacity(
            genesis_config.resource_limits.max_verification_queue as usize,
        ));

        let (local_validator_id, signer, vrf_key_pair) = if config.node_role == NodeRole::Validator
        {
            let keystore_dir = Path::new(&config.data_dir).join("keystore");
            let keystore = match config.network_mode {
                NetworkMode::Production => NodeKeyStore::open_existing(&keystore_dir)?,
                NetworkMode::Devnet | NetworkMode::Test => {
                    NodeKeyStore::open_or_create(&keystore_dir)?
                }
            };
            info!(
                "Loaded persistent validator keystore from {:?}",
                keystore_dir
            );

            let consensus_pubkey_bytes: [u8; 33] = keystore
                .consensus_key()
                .verifying_key()
                .to_sec1_bytes()
                .as_ref()
                .try_into()
                .map_err(|_| anyhow!("Invalid SEC1 public key length"))?;
            let vrf_pubkey_bytes = keystore.vrf_key().public_key_bytes();
            let local_validator_id = validate_validator_key_match(
                &genesis_snapshot,
                consensus_pubkey_bytes,
                vrf_pubkey_bytes,
            )?;

            let signer = Arc::new(EcdsaConsensusSigner::new(
                keystore.consensus_key().clone(),
                local_validator_id,
            ));
            (
                Some(local_validator_id),
                Some(signer),
                Some(keystore.vrf_key().clone()),
            )
        } else {
            info!("Starting as FullNode; validator private keys are not loaded");
            (None, None, None)
        };

        let defaults = ConsensusConfig::default();
        let consensus_config = ConsensusConfig {
            protocol_version: chain_context.protocol_version,
            chain_id: chain_context.chain_id,
            epoch: genesis_config.epoch,
            epoch_length: genesis_config.epoch_length,
            validator_update_delay: genesis_config.validator_update_delay,
            unbonding_delay: genesis_config.unbonding_delay,
            key_rotation_delay: genesis_config.key_rotation_delay,
            slashing_activation_delay: genesis_config.slashing_activation_delay,
            timeout_propose_ms: defaults.timeout_propose_ms,
            timeout_prevote_ms: defaults.timeout_prevote_ms,
            timeout_precommit_ms: defaults.timeout_precommit_ms,
            target_numerator: defaults.target_numerator,
            target_denominator: defaults.target_denominator,
            max_certificate_members: genesis_config.resource_limits.max_certificate_members,
            max_future_height: genesis_config.resource_limits.max_future_height,
            max_future_round: genesis_config.resource_limits.max_future_round,
        };

        let safety_path = Path::new(&config.data_dir).join("safety_store.log");
        let persistent_safety_store = Arc::new(PersistentSafetyStore::open(safety_path)?);
        let finality_store = Arc::new(FinalityStore::new(db.clone()));
        let initialized_tip = finality_store
            .initialize_genesis_tip(
                &genesis_config.genesis_block,
                genesis_snapshot_hash,
                genesis_config.initial_randomness,
            )
            .await?;

        let consensus = Arc::new(PoVFEngine::new_with_parent_randomness(
            consensus_config,
            genesis_snapshot,
            genesis_config.initial_randomness,
            persistent_safety_store,
            local_validator_id,
        ));
        {
            let sm = consensus.state_machine.read().await;
            info!(
                "Consensus V2 initial height={} round={} local_validator={:?} proposer={:?}",
                sm.height,
                sm.round,
                sm.local_validator_id,
                sm.get_current_proposer()
            );
        }
        let consensus_driver = ConsensusDriver::start(
            genesis_config.resource_limits.max_verification_tasks.max(1) as usize,
        )?;
        info!("Initialized disk-backed BFT consensus engine");

        let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
        let evm_config = EVMConfig::default();
        let evm_executor = Arc::new(EVMExecutor::new(state_manager.clone(), evm_config));

        if let Some((finalized, state_writes, checkpoint)) = finality_store
            .recover_finalized_tip_with_state_and_checkpoint()
            .await?
        {
            let checkpoint = checkpoint.ok_or_else(|| {
                anyhow!(
                    "durable finalized V2 state has no canonical state checkpoint; refusing startup"
                )
            })?;
            if checkpoint.state_root != finalized.block.header.state_root {
                return Err(anyhow!(
                    "durable canonical state root does not match finalized block"
                ));
            }
            state_manager
                .restore_canonical_state(
                    &checkpoint.accounts,
                    &checkpoint.storage,
                    checkpoint.state_root,
                )
                .await
                .map_err(|error| anyhow!("failed to restore canonical state: {error}"))?;
            evm_executor
                .code_storage()
                .restore_checkpoint(&checkpoint.code)
                .await
                .map_err(|error| anyhow!("failed to restore canonical code: {error}"))?;
            let recomputed_root = StateRootCalculator::new(false)
                .calculate_from_manager(&state_manager)
                .await?;
            if recomputed_root != checkpoint.state_root {
                return Err(anyhow!(
                    "recovered canonical state root recomputation mismatch"
                ));
            }
            let _ = state_writes;
            let next_snapshot = {
                let sm = consensus.state_machine.read().await;
                let next_height = finalized
                    .consensus_state
                    .height
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("finalized height overflow during recovery"))?;
                let next_epoch = sm.config.epoch_for_height(next_height)?;
                if let Some(snapshot) = finality_store.recover_snapshot(next_epoch).await? {
                    if snapshot.epoch != next_epoch {
                        return Err(anyhow!(
                            "durable next validator snapshot has an unexpected epoch"
                        ));
                    }
                    snapshot
                } else {
                    sm.snapshot_for_height(next_height)?
                }
            };
            let expected_tip = CanonicalFinalizedTip::from_finalized_with_next_snapshot(
                &finalized,
                Some(&next_snapshot),
            )
            .map_err(|error| anyhow!("failed to derive canonical recovery tip: {error}"))?;
            if initialized_tip != expected_tip {
                return Err(anyhow!(
                    "durable canonical tip conflicts with finalized block, state root, randomness, or next validator snapshot"
                ));
            }
            {
                let mut sm = consensus.state_machine.write().await;
                sm.restore_after_finalized(&finalized.consensus_state, next_snapshot)
                    .map_err(|error| {
                        anyhow!("failed to restore finalized consensus state: {error}")
                    })?;
                *consensus.current_height.write().await = sm.height;
            }
            info!(
                "Recovered finalized V2 state at height {}; consensus resumes at {}",
                finalized.consensus_state.height,
                finalized.consensus_state.height.saturating_add(1)
            );
        }

        let block_producer = match (config.node_role, vrf_key_pair, signer.clone()) {
            (NodeRole::Validator, Some(vrf_key_pair), Some(signer)) => {
                let producer_config = BlockProducerConfig {
                    is_validator: true,
                    block_interval: 1,
                    max_txs_per_block: genesis_config.resource_limits.max_transactions_per_block
                        as usize,
                    max_gas_per_block: genesis_config.resource_limits.max_block_gas as i64,
                    max_block_bytes: genesis_config.resource_limits.max_block_bytes as usize,
                    max_transaction_bytes: genesis_config.resource_limits.max_transaction_bytes
                        as usize,
                    ..Default::default()
                };
                let mut producer = BlockProducer::new(
                    producer_config,
                    blockchain.clone(),
                    tx_pool.clone(),
                    vrf_key_pair,
                    state_manager.clone(),
                    Some(consensus.clone()),
                    Some(signer),
                );
                producer.attach_v2_pool(tx_pool_v2.clone());
                producer.attach_v2_code_storage(evm_executor.code_storage().clone());
                producer.attach_finality_store(finality_store.clone());
                Some(Arc::new(producer))
            }
            (NodeRole::FullNode, None, None) => None,
            _ => return Err(anyhow!("invalid node role/key initialization state")),
        };

        let peer_role = match config.node_role {
            NodeRole::Validator => PeerRole::Validator,
            NodeRole::FullNode => PeerRole::FullNode,
        };
        let mut network_svc = NetworkService::start_with_context(
            config.network.clone(),
            keypair,
            chain_context,
            peer_role,
        )
        .await?;
        let rx = std::mem::replace(&mut network_svc.event_rx, tokio::sync::mpsc::channel(1).1);
        let network = Arc::new(network_svc);

        let peer_manager = Arc::new(PeerManager::new(
            blockchain.clone(),
            tx_pool_v2.clone(),
            network.clone(),
            chain_context,
            genesis_config.resource_limits.max_transaction_bytes as usize,
            genesis_config.resource_limits.max_verification_tasks as usize,
        ));
        let syncer = Arc::new(BlockSyncer::new(blockchain.clone(), network.clone()));
        let tx_handler = Arc::new(TxHandler::new(
            tx_pool_v2.clone(),
            chain_context,
            genesis_config.resource_limits.max_transaction_bytes as usize,
            genesis_config.resource_limits.max_verification_tasks as usize,
        ));

        Ok(Self {
            config,
            blockchain,
            tx_pool,
            tx_pool_v2,
            network,
            consensus,
            consensus_driver,
            finality_store,
            signer,
            block_producer,
            chain_context,
            resource_limits: genesis_config.resource_limits,
            peer_manager,
            syncer,
            tx_handler,
            state_manager,
            evm_executor,
            network_rx: Some(rx),
            metrics_collector,
            _monitoring_server: None,
            _log_guard: log_guard,
        })
    }

    pub async fn start(mut self) -> Result<()> {
        info!("Starting Norn Node...");

        let rpc_addr = self.config.rpc_address;
        let eth_rpc_addr = {
            let port = rpc_addr.port() + 1000;
            format!("{}:{}", rpc_addr.ip(), port).parse::<std::net::SocketAddr>()?
        };

        let chain_ref = self.blockchain.clone();
        let tx_pool_ref = self.tx_pool.clone();
        let rpc_addr_clone = rpc_addr;
        tokio::spawn(async move {
            info!("gRPC Server listening on {}", rpc_addr_clone);
            if let Err(e) = start_rpc_server(rpc_addr_clone, chain_ref, tx_pool_ref).await {
                error!("gRPC Server failed: {:?}", e);
            }
        });
        info!("gRPC Server started on {}", rpc_addr);

        let eth_rpc = create_ethereum_rpc(
            self.blockchain.clone(),
            self.state_manager.clone(),
            self.evm_executor.clone(),
            self.tx_pool.clone(),
            31337,
        );
        tokio::spawn(async move {
            info!("Ethereum JSON-RPC server listening on {}", eth_rpc_addr);
            if let Err(e) = start_ethereum_rpc_server(eth_rpc_addr, eth_rpc).await {
                error!("Ethereum JSON-RPC server failed: {:?}", e);
            }
        });
        info!("Ethereum JSON-RPC server started on {}", eth_rpc_addr);

        let syncer = self.syncer.clone();
        tokio::spawn(async move {
            syncer.start().await;
        });

        if let Some(producer) = self.block_producer.clone() {
            let network_ref = self.network.clone();
            let chain_context = self.chain_context;
            let resource_limits = self.resource_limits.clone();
            let consensus = self.consensus.clone();
            let consensus_driver = self.consensus_driver.clone();
            let signer = self.signer.clone();
            let state_manager = self.state_manager.clone();
            let code_storage = self.evm_executor.code_storage().clone();
            let finality_store = self.finality_store.clone();
            tokio::spawn(async move {
                let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
                let mut produced_slot: Option<(u64, u32)> = None;
                loop {
                    timer.tick().await;
                    let slot = {
                        let state_machine = consensus.state_machine.read().await;
                        (state_machine.height, state_machine.round)
                    };
                    if produced_slot == Some(slot) {
                        continue;
                    }
                    if producer.should_produce().await {
                        let (proposal, block, recovered_pending) = match finality_store
                            .recover_pending_proposal(slot.0, slot.1)
                            .await
                        {
                            Ok(Some((proposal, block))) => {
                                info!(
                                    "Recovering pending V2 proposal at height {} round {}",
                                    proposal.height, proposal.round
                                );
                                (proposal, block, true)
                            }
                            Ok(None) => {
                                let produced = match producer
                                    .produce_v2_proposal(&chain_context, &resource_limits)
                                    .await
                                {
                                    Ok(produced) => produced,
                                    Err(error) => {
                                        warn!("V2 proposal production failed: {}", error);
                                        continue;
                                    }
                                };
                                if let Err(error) = finality_store
                                    .persist_pending_proposal(&produced.0, &produced.1)
                                    .await
                                {
                                    warn!(
                                        "V2 proposal was not durably recorded before voting: {}",
                                        error
                                    );
                                    continue;
                                }
                                (produced.0, produced.1, false)
                            }
                            Err(error) => {
                                warn!("Failed to recover pending V2 proposal: {}", error);
                                continue;
                            }
                        };
                        {
                            produced_slot = Some(slot);
                            let _ = consensus_driver
                                .dispatch(ConsensusDriverEvent::LocalProposalReady {
                                    height: block.header.height as u64,
                                    block_id: proposal.block_id,
                                })
                                .await;
                            consensus.candidate_blocks_v2.write().await.insert(
                                (block.header.height as u64, proposal.block_id),
                                block.clone(),
                            );
                            let local_vote = if recovered_pending {
                                // Re-admit the recovered proposal through the same
                                // state-machine path as a live proposal. The
                                // SafetyStore makes this idempotent and returns the
                                // exact durable prevote instead of signing a new
                                // value for the slot. Keeping the vote in the local
                                // pool is required for the proposer to observe a
                                // post-restart Polka and produce its precommit.
                                if let Some(signer) = signer.as_ref() {
                                    match consensus
                                        .handle_proposal_v2(
                                            proposal.clone(),
                                            block.clone(),
                                            signer.as_ref(),
                                            &state_manager,
                                            &resource_limits,
                                            &chain_context,
                                            &code_storage,
                                        )
                                        .await
                                    {
                                        Ok(vote) => vote,
                                        Err(error) => {
                                            warn!(
                                                "Recovered pending V2 proposal was not re-admitted to the voting state machine: {}",
                                                error
                                            );
                                            continue;
                                        }
                                    }
                                } else {
                                    match consensus
                                        .verify_proposal_v2(
                                            proposal.clone(),
                                            block.clone(),
                                            &state_manager,
                                            &resource_limits,
                                            &chain_context,
                                            &code_storage,
                                        )
                                        .await
                                    {
                                        Ok(validated) => {
                                            consensus
                                                .remember_validated_candidate(&validated)
                                                .await;
                                        }
                                        Err(error) => {
                                            warn!(
                                                "Recovered pending V2 proposal failed verification: {}",
                                                error
                                            );
                                            continue;
                                        }
                                    }
                                    None
                                }
                            } else if let Some(signer) = signer.as_ref() {
                                match consensus
                                    .handle_proposal_v2(
                                        proposal.clone(),
                                        block.clone(),
                                        signer.as_ref(),
                                        &state_manager,
                                        &resource_limits,
                                        &chain_context,
                                        &code_storage,
                                    )
                                    .await
                                {
                                    Ok(vote) => vote,
                                    Err(error) => {
                                        warn!(
                                                "Local V2 proposal was not accepted by the voting state machine: {}",
                                                error
                                            );
                                        None
                                    }
                                }
                            } else {
                                None
                            };
                            let local_precommit = match (local_vote.as_ref(), signer.as_ref()) {
                                (Some(vote), Some(signer)) => {
                                    match consensus.handle_vote(vote.clone(), signer.as_ref()).await
                                    {
                                        Ok((precommit, _)) => {
                                            if let Some(precommit) = precommit.as_ref() {
                                                if let Err(error) = consensus
                                                    .handle_vote(precommit.clone(), signer.as_ref())
                                                    .await
                                                {
                                                    warn!(
                                                        "Local V2 precommit was not admitted to the vote pool: {}",
                                                        error
                                                    );
                                                }
                                            }
                                            precommit
                                        }
                                        Err(error) => {
                                            warn!(
                                                "Local V2 prevote was not admitted to the vote pool: {}",
                                                error
                                            );
                                            None
                                        }
                                    }
                                }
                                _ => None,
                            };
                            let envelope = ConsensusEnvelope {
                                wire_version: chain_context.wire_version,
                                protocol_version: proposal.protocol_version,
                                chain_id: proposal.chain_id,
                                genesis_hash: chain_context.genesis_hash,
                                payload: ConsensusMessage::ProposalV2 {
                                    proposal: proposal.clone(),
                                    block: block.clone(),
                                },
                            };

                            match bincode::serialize(&envelope) {
                                Ok(msg_bytes) => {
                                    debug!(
                                        "Enqueueing V2 proposal broadcast at height {}",
                                        proposal.height
                                    );
                                    if let Err(error) = network_ref
                                        .command_tx
                                        .send(NetworkCommand::BroadcastConsensus(msg_bytes))
                                        .await
                                    {
                                        warn!("Failed to enqueue local V2 proposal: {}", error);
                                    }
                                }
                                Err(error) => {
                                    warn!("Failed to encode local V2 proposal: {}", error)
                                }
                            }
                            if let Some(vote) = local_vote {
                                let vote_envelope = ConsensusEnvelope {
                                    wire_version: chain_context.wire_version,
                                    protocol_version: vote.protocol_version,
                                    chain_id: vote.chain_id,
                                    genesis_hash: chain_context.genesis_hash,
                                    payload: ConsensusMessage::Vote(vote),
                                };
                                match bincode::serialize(&vote_envelope) {
                                    Ok(vote_bytes) => {
                                        if let Err(error) = network_ref
                                            .command_tx
                                            .send(NetworkCommand::BroadcastConsensus(vote_bytes))
                                            .await
                                        {
                                            warn!(
                                                "Failed to enqueue local V2 proposal vote: {}",
                                                error
                                            );
                                        }
                                    }
                                    Err(error) => {
                                        warn!("Failed to encode local V2 proposal vote: {}", error)
                                    }
                                }
                            }
                            if let Some(precommit) = local_precommit {
                                let precommit_envelope = ConsensusEnvelope {
                                    wire_version: chain_context.wire_version,
                                    protocol_version: precommit.protocol_version,
                                    chain_id: precommit.chain_id,
                                    genesis_hash: chain_context.genesis_hash,
                                    payload: ConsensusMessage::Vote(precommit),
                                };
                                match bincode::serialize(&precommit_envelope) {
                                    Ok(precommit_bytes) => {
                                        if let Err(error) = network_ref
                                            .command_tx
                                            .send(NetworkCommand::BroadcastConsensus(
                                                precommit_bytes,
                                            ))
                                            .await
                                        {
                                            warn!(
                                                "Failed to enqueue local V2 precommit: {}",
                                                error
                                            );
                                        }
                                    }
                                    Err(error) => {
                                        warn!("Failed to encode local V2 precommit: {}", error)
                                    }
                                }
                            }
                        }
                    }
                }
            });
            info!("Block Producer started");
        } else {
            info!("Block Producer disabled for FullNode");
        }

        // A vote whose WAL completion was durable before a crash is still a
        // valid vote. Re-broadcast the exact persisted signature on startup;
        // never manufacture a replacement vote for the same signing slot.
        let recovered_votes = {
            let sm = self.consensus.state_machine.read().await;
            sm.safety_store.recover_signed_votes()
        };
        for vote in recovered_votes {
            if vote.protocol_version != self.chain_context.protocol_version
                || vote.chain_id != self.chain_context.chain_id
            {
                warn!(
                    "Discarding recovered vote from a different protocol/chain context: height={}, round={}, step={:?}",
                    vote.height, vote.round, vote.step
                );
                continue;
            }
            if let Some(signer) = self.signer.as_ref() {
                match self
                    .consensus
                    .handle_vote(vote.clone(), signer.as_ref())
                    .await
                {
                    Ok((Some(follow_up), _)) => {
                        if let Err(error) = self.broadcast_v2_vote(follow_up).await {
                            warn!("Failed to enqueue recovered follow-up vote: {}", error);
                        }
                    }
                    Ok((None, _)) => {}
                    Err(error) => {
                        warn!(
                            "Recovered vote was not admitted to the local vote pool: {}",
                            error
                        );
                    }
                }
            }
            let envelope = ConsensusEnvelope {
                wire_version: self.chain_context.wire_version,
                protocol_version: vote.protocol_version,
                chain_id: vote.chain_id,
                genesis_hash: self.chain_context.genesis_hash,
                payload: ConsensusMessage::Vote(vote),
            };
            match bincode::serialize(&envelope) {
                Ok(bytes) => {
                    if let Err(err) = self
                        .network
                        .command_tx
                        .send(NetworkCommand::BroadcastConsensus(bytes))
                        .await
                    {
                        warn!("Failed to enqueue recovered consensus vote: {}", err);
                    }
                }
                Err(err) => warn!("Failed to encode recovered consensus vote: {}", err),
            }
        }

        // Any node may have been offline when validators broadcast several
        // Commit certificates. Start an ordered finalized-record sync from
        // the next canonical height; each response requests the following
        // height after it has been verified and durably applied. Validators
        // use the same verify-and-recover path after a crash/partition; they
        // never treat a missing gossip replay as an implicit new proposal.
        if let Some(tip) = self.finality_store.recover_canonical_tip().await? {
            self.request_v2_finality(tip.next_height()?).await;
        }

        if let Some(rx) = self.network_rx.take() {
            self.run_loop(rx).await;
        }

        Ok(())
    }

    async fn finalize_commit(&self, commit: CommitCertificate) -> Result<()> {
        if commit.protocol_version != self.chain_context.protocol_version
            || commit.chain_id != self.chain_context.chain_id
        {
            return Err(anyhow!(
                "UnsupportedProtocolVersion: finalized certificate is not for the active V2 chain"
            ));
        }
        let canonical_tip = self
            .finality_store
            .recover_canonical_tip()
            .await?
            .ok_or_else(|| anyhow!("canonical finalized tip is unavailable"))?;
        // A delayed/replayed certificate for an already superseded height is
        // verified against its immutable durable record, then ignored. It
        // must never restore an older checkpoint into the live state or move
        // the consensus state backwards.
        if canonical_tip.height > commit.height {
            let persisted = self
                .finality_store
                .recover_finalized_v2(commit.height)
                .await?
                .ok_or_else(|| anyhow!("replayed finalized certificate has no durable record"))?;
            self.verify_replayed_or_equivalent_certificate(&persisted, &commit)
                .await?;
            return Ok(());
        }
        let next_height = commit
            .height
            .checked_add(1)
            .ok_or_else(|| anyhow!("finalized height overflow"))?;
        let next_snapshot = self
            .consensus
            .state_machine
            .read()
            .await
            .snapshot_for_height(next_height)?;
        if canonical_tip.height > 0
            || canonical_tip.state_root != norn_common::types::Hash::default()
        {
            let current_root = StateRootCalculator::new(false)
                .calculate_from_manager(&self.state_manager)
                .await?;
            if current_root != canonical_tip.state_root {
                return Err(anyhow!(
                    "execution parent state root does not match canonical finalized tip"
                ));
            }
        }
        let durable_v2 = self
            .finality_store
            .recover_finalized_v2_with_state_and_checkpoint(commit.height)
            .await?;
        let (finalized, state_write_values, checkpoint, commit_status) = if let Some((
            persisted,
            state_writes,
            checkpoint,
        )) = durable_v2
        {
            if persisted.block.header.height < 0
                || persisted.block.header.height as u64 != commit.height
                || persisted.commit.block_id != commit.block_id
            {
                warn!(
                        "Durable V2 record mismatch: persisted_height={}, received_height={}, persisted_block={:?}, received_block={:?}",
                        persisted.block.header.height,
                        commit.height,
                        persisted.commit.block_id,
                        commit.block_id
                    );
                return Err(anyhow!(
                    "durable finalized payload conflicts with received certificate"
                ));
            }
            if persisted.commit != commit {
                // Multiple valid quorum certificates can exist for the
                // same block when weighted voting allows different
                // minimal quorum member sets. The first durable record is
                // canonical; a later equivalent certificate is verified
                // and treated as an idempotent replay, never as a new
                // state transition.
                self.verify_replayed_or_equivalent_certificate(&persisted, &commit)
                    .await?;
                return Ok(());
            }
            let checkpoint = checkpoint.ok_or_else(|| {
                anyhow!("durable V2 finality is missing canonical state checkpoint")
            })?;
            let status = self
                .finality_store
                .commit_finalized_transaction_with_state_and_checkpoint_and_snapshot(
                    &persisted,
                    &state_writes,
                    Some(&checkpoint),
                    Some(&next_snapshot),
                )
                .await?;
            (persisted, state_writes, checkpoint, status)
        } else {
            let finalized = self.consensus.finalize_block_v2(commit).await?;
            let execution = self
                .consensus
                .execute_v2_block_for_finality(
                    &finalized.block,
                    &self.state_manager,
                    &self.resource_limits,
                    &self.chain_context,
                    self.evm_executor.code_storage(),
                )
                .await?;
            let state_writes = execution.overlay.canonical_persistence_values()?;
            let checkpoint = execution
                .overlay
                .canonical_state_checkpoint(&self.state_manager, self.evm_executor.code_storage())
                .await?;
            let status = self
                .finality_store
                .commit_finalized_transaction_with_state_and_checkpoint_and_snapshot(
                    &finalized,
                    &state_writes,
                    Some(&checkpoint),
                    Some(&next_snapshot),
                )
                .await?;
            (finalized, state_writes, checkpoint, status)
        };
        self.state_manager
            .restore_canonical_state(
                &checkpoint.accounts,
                &checkpoint.storage,
                checkpoint.state_root,
            )
            .await
            .map_err(|error| anyhow!("failed to apply finalized canonical state: {error}"))?;
        self.evm_executor
            .code_storage()
            .restore_checkpoint(&checkpoint.code)
            .await
            .map_err(|error| anyhow!("failed to apply finalized canonical code: {error}"))?;
        let recomputed_root = StateRootCalculator::new(false)
            .calculate_from_manager(&self.state_manager)
            .await?;
        if recomputed_root != finalized.block.header.state_root {
            return Err(anyhow!(
                "finalized canonical state root does not match block state_root"
            ));
        }
        let _ = state_write_values;
        self.consensus
            .record_finalized_v2_after_durable(&finalized)
            .await?;
        self.consensus
            .advance_after_finalized_v2(&finalized, next_snapshot)
            .await?;
        if let Err(error) = self
            .finality_store
            .clear_pending_proposal(finalized.commit.height, finalized.commit.round)
            .await
        {
            warn!(
                "Failed to clear finalized V2 pending proposal record: {}",
                error
            );
        }
        info!(
            "Finalized V2 block {:?} at height {}; durable finality status {:?}",
            finalized.block.header.block_hash, finalized.block.header.height, commit_status
        );
        Ok(())
    }

    async fn verify_replayed_or_equivalent_certificate(
        &self,
        persisted: &norn_common::consensus_types::FinalizedBlockV2,
        received: &CommitCertificate,
    ) -> Result<()> {
        if persisted.commit.block_id != received.block_id
            || persisted.commit.height != received.height
            || persisted.commit.protocol_version != received.protocol_version
            || persisted.commit.chain_id != received.chain_id
            || persisted.commit.epoch != received.epoch
            || persisted.commit.round != received.round
            || persisted.commit.stake_snapshot_hash != received.stake_snapshot_hash
        {
            return Err(anyhow!(
                "replayed finalized certificate conflicts with durable history"
            ));
        }
        if persisted.commit == *received {
            return Ok(());
        }
        let snapshot = self
            .finality_store
            .recover_snapshot(received.epoch)
            .await?
            .ok_or_else(|| anyhow!("snapshot for replayed finalized certificate is missing"))?;
        self.consensus
            .verify_commit_certificate_v2(&persisted.block, received, &snapshot)
            .map_err(|error| {
                anyhow!("equivalent finalized certificate failed verification: {error}")
            })
    }

    async fn validate_and_remember_v2_candidate(
        &self,
        proposal: norn_common::consensus_types::Proposal,
        block: norn_common::types::BlockV2,
    ) -> Result<()> {
        block.validate_structure(&self.chain_context, &self.resource_limits)?;
        let tip = self
            .finality_store
            .recover_canonical_tip()
            .await?
            .ok_or_else(|| anyhow!("canonical finalized tip is unavailable"))?;
        let parent_matches = block.header.height >= 0
            && tip.next_height()? == block.header.height as u64
            && tip.block_id.0 == block.header.prev_block_hash
            && tip.next_randomness == block.header.parent_randomness;
        if !parent_matches {
            return Err(anyhow!(
                "V2 proposal has a non-canonical parent, height, or randomness"
            ));
        }
        let validated = self
            .consensus
            .verify_proposal_v2(
                proposal,
                block,
                &self.state_manager,
                &self.resource_limits,
                &self.chain_context,
                self.evm_executor.code_storage(),
            )
            .await?;
        self.consensus
            .remember_validated_candidate(&validated)
            .await;
        Ok(())
    }

    async fn request_v2_block(&self, height: u64, block_id: norn_common::types::BlockId) {
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::BlockRequest { height, block_id },
        };
        match bincode::serialize(&envelope) {
            Ok(bytes) => {
                if let Err(error) = self
                    .network
                    .command_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                {
                    warn!("Failed to request missing V2 block: {}", error);
                }
            }
            Err(error) => warn!("Failed to encode V2 block request: {}", error),
        }
    }

    async fn request_v2_finality(&self, height: u64) {
        if height == 0 {
            return;
        }
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::FinalityRequest { height },
        };
        match bincode::serialize(&envelope) {
            Ok(bytes) => {
                if let Err(error) = self
                    .network
                    .command_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                {
                    warn!(
                        "Failed to request missing V2 finality at height {}: {}",
                        height, error
                    );
                }
            }
            Err(error) => warn!("Failed to encode V2 finality request: {}", error),
        }
    }

    async fn respond_with_v2_block(
        &self,
        proposal: norn_common::consensus_types::Proposal,
        block: norn_common::types::BlockV2,
    ) {
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::BlockResponse { proposal, block },
        };
        match bincode::serialize(&envelope) {
            Ok(bytes) => {
                if let Err(error) = self
                    .network
                    .command_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                {
                    warn!("Failed to enqueue V2 block response: {}", error);
                }
            }
            Err(error) => warn!("Failed to encode V2 block response: {}", error),
        }
    }

    async fn respond_with_v2_finality(
        &self,
        finalized: norn_common::consensus_types::FinalizedBlockV2,
    ) {
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::FinalityResponse { finalized },
        };
        match bincode::serialize(&envelope) {
            Ok(bytes) => {
                if let Err(error) = self
                    .network
                    .command_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                {
                    warn!("Failed to enqueue V2 finality response: {}", error);
                }
            }
            Err(error) => warn!("Failed to encode V2 finality response: {}", error),
        }
    }

    async fn respond_to_v2_block_request(
        &self,
        height: u64,
        block_id: norn_common::types::BlockId,
    ) {
        if let Some((proposal, block)) = self
            .consensus
            .get_validated_candidate(height, block_id)
            .await
        {
            self.respond_with_v2_block(proposal, block).await;
            return;
        }

        // Candidate caches are intentionally pruned after durable finality.
        // The finality record remains an authoritative source for a missed
        // proposal, including its VRF proof and proposal context.
        match self.finality_store.recover_finalized_v2(height).await {
            Ok(Some(finalized)) if finalized.commit.block_id == block_id => {
                self.respond_with_v2_block(finalized.proposal, finalized.block)
                    .await;
            }
            Ok(Some(_)) => warn!(
                "Refused V2 block request with a block ID different from durable height {}",
                height
            ),
            Ok(None) => warn!(
                "No durable or in-memory V2 candidate available for requested height {} block {:?}",
                height, block_id
            ),
            Err(error) => warn!(
                "Failed to recover durable V2 block for requested height {}: {}",
                height, error
            ),
        }
    }

    async fn respond_to_v2_finality_request(&self, height: u64) {
        match self.finality_store.recover_finalized_v2(height).await {
            Ok(Some(finalized)) => self.respond_with_v2_finality(finalized).await,
            Ok(None) => debug!(
                "No durable V2 finality record exists at requested height {}",
                height
            ),
            Err(error) => warn!(
                "Failed to recover durable V2 finality at requested height {}: {}",
                height, error
            ),
        }
    }

    async fn broadcast_v2_vote(&self, vote: SignedVote) -> Result<()> {
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: vote.protocol_version,
            chain_id: vote.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::Vote(vote),
        };
        let bytes = bincode::serialize(&envelope)
            .map_err(|error| anyhow!("failed to encode V2 vote: {error}"))?;
        self.network
            .command_tx
            .send(NetworkCommand::BroadcastConsensus(bytes))
            .await
            .map_err(|error| anyhow!("failed to enqueue V2 vote: {error}"))
    }

    async fn finalize_and_broadcast_commit(&self, commit: CommitCertificate) -> Result<()> {
        self.finalize_commit(commit.clone()).await?;
        let _ = self
            .consensus_driver
            .dispatch(ConsensusDriverEvent::FinalityDurable(commit.clone()))
            .await;
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: commit.protocol_version,
            chain_id: commit.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::Commit(commit),
        };
        let bytes = bincode::serialize(&envelope)
            .map_err(|error| anyhow!("failed to encode durable Commit broadcast: {error}"))?;
        self.network
            .command_tx
            .send(NetworkCommand::BroadcastConsensus(bytes))
            .await
            .map_err(|error| anyhow!("failed to enqueue durable Commit broadcast: {error}"))
    }

    pub async fn run_loop(
        &mut self,
        mut network_events: tokio::sync::mpsc::Receiver<norn_network::service::NetworkEvent>,
    ) {
        let mut pending_commits: HashMap<(u64, norn_common::types::BlockId), CommitCertificate> =
            HashMap::new();
        loop {
            tokio::select! {
                event = network_events.recv() => {
                    match event {
                        Some(e) => {
                            match e {
                                norn_network::service::NetworkEvent::Listening(address) => {
                                    info!("Network listening at {:?}", address);
                                }
                                norn_network::service::NetworkEvent::PeerConnected(peer_id) => {
                                    info!("Network peer connected: {:?}", peer_id);
                                }
                                norn_network::service::NetworkEvent::DialFailed { address, reason } => {
                                    warn!("Network dial to {:?} failed: {}", address, reason);
                                }
                                norn_network::service::NetworkEvent::PeerAuthenticated { peer_id, role } => {
                                    info!("Authenticated network peer {:?} as {:?}", peer_id, role);
                                    if let Ok(Some(tip)) = self.finality_store.recover_canonical_tip().await {
                                        if let Ok(next_height) = tip.next_height() {
                                            self.request_v2_finality(next_height).await;
                                        }
                                    }
                                }
                                norn_network::service::NetworkEvent::PeerDisconnected(peer_id) => {
                                    info!("Network peer disconnected: {:?}", peer_id);
                                }
                                norn_network::service::NetworkEvent::BlockReceived(data) => {
                                    if data.len() > MAX_BLOCK_MESSAGE_BYTES {
                                        warn!("Rejected oversized block network message");
                                        continue;
                                    }
                                    self.peer_manager.handle_network_event(norn_network::service::NetworkEvent::BlockReceived(data)).await;
                                }
                                norn_network::service::NetworkEvent::TransactionReceived(data) => {
                                    if data.len() > MAX_TRANSACTION_MESSAGE_BYTES {
                                        warn!("Rejected oversized transaction network message");
                                        continue;
                                    }
                                    self.tx_handler.handle_tx_data(data).await;
                                }
                                norn_network::service::NetworkEvent::ConsensusMessageReceived(data) => {
                                    debug!(
                                        "Node received consensus event ({} bytes)",
                                        data.len()
                                    );
                                    if data.len() > MAX_CONSENSUS_ENVELOPE_BYTES {
                                        warn!("Rejected oversized consensus network message");
                                        continue;
                                    }

                                    let envelope = match ConsensusEnvelope::decode_and_validate(
                                        &data,
                                        &self.chain_context,
                                    ) {
                                        Ok(envelope) => envelope,
                                        Err(e) => {
                                            warn!("Rejected consensus envelope: {}", e);
                                            continue;
                                        }
                                    };
                                    match envelope.payload {
                                            ConsensusMessage::ProposalV2 { proposal, block } => {
                                                if let Err(err) = self
                                                    .consensus_driver
                                                    .dispatch(ConsensusDriverEvent::NetworkProposal {
                                                        proposal: proposal.clone(),
                                                        block: block.clone(),
                                                    })
                                                    .await
                                                {
                                                    warn!("Consensus driver rejected proposal event: {}", err);
                                                    continue;
                                                }
                                                if let Err(err) = self
                                                    .validate_and_remember_v2_candidate(
                                                        proposal.clone(),
                                                        block.clone(),
                                                    )
                                                    .await
                                                {
                                                    warn!("Rejected V2 proposal verification: {}", err);
                                                    continue;
                                                }
                                                if let Some(commit) = pending_commits
                                                    .remove(&(proposal.height, proposal.block_id))
                                                {
                                                    if let Err(err) = self.finalize_commit(commit).await {
                                                        warn!("Validated V2 proposal could not satisfy pending Commit: {}", err);
                                                    }
                                                }
                                                let Some(signer) = self.signer.clone() else {
                                                    info!(
                                                        "FullNode verified V2 proposal at height {}; no vote will be cast",
                                                        proposal.height
                                                    );
                                                    continue;
                                                };
                                                match self
                                                    .consensus
                                                    .handle_proposal_v2(
                                                        proposal.clone(),
                                                        block.clone(),
                                                        signer.as_ref(),
                                                        &self.state_manager,
                                                        &self.resource_limits,
                                                        &self.chain_context,
                                                        self.evm_executor.code_storage(),
                                                    )
                                                    .await
                                                {
                                                    Ok(Some(vote)) => {
                                                        let precommit = match self
                                                            .consensus
                                                            .handle_vote(vote.clone(), signer.as_ref())
                                                            .await
                                                        {
                                                        Ok((precommit, _)) => {
                                                            if let Some(precommit) = precommit.as_ref() {
                                                                if let Err(error) = self
                                                                    .consensus
                                                                    .handle_vote(
                                                                        precommit.clone(),
                                                                        signer.as_ref(),
                                                                    )
                                                                    .await
                                                                {
                                                                    warn!(
                                                                        "Signed V2 precommit was not admitted locally: {}",
                                                                        error
                                                                    );
                                                                }
                                                            }
                                                            precommit
                                                        }
                                                        Err(error) => {
                                                            warn!(
                                                                "Signed V2 proposal vote was not admitted locally: {}",
                                                                error
                                                            );
                                                            None
                                                        }
                                                        };
                                                        if let Err(error) = self.broadcast_v2_vote(vote).await {
                                                            warn!("Failed to enqueue signed V2 proposal vote: {}", error);
                                                        }
                                                        if let Some(precommit) = precommit {
                                                            if let Err(error) =
                                                                self.broadcast_v2_vote(precommit).await
                                                            {
                                                                warn!("Failed to enqueue signed V2 precommit: {}", error);
                                                            }
                                                        }
                                                    }
                                                    Ok(None) => {}
                                                    Err(error) => {
                                                        warn!(
                                                            "Rejected V2 proposal during voting state transition at height {} round {} block {:?}: {}",
                                                            proposal.height,
                                                            proposal.round,
                                                            proposal.block_id,
                                                            error
                                                        );
                                                    }
                                                }
                                             }
                                             ConsensusMessage::BlockRequest { height, block_id } => {
                                                 self.respond_to_v2_block_request(height, block_id).await;
                                             }
                                             ConsensusMessage::BlockResponse { proposal, block } => {
                                                 if let Err(err) = self
                                                     .consensus_driver
                                                     .dispatch(ConsensusDriverEvent::NetworkProposal {
                                                         proposal: proposal.clone(),
                                                         block: block.clone(),
                                                     })
                                                     .await
                                                 {
                                                     warn!("Consensus driver rejected V2 block response: {}", err);
                                                     continue;
                                                 }
                                                 if let Err(err) = self
                                                     .validate_and_remember_v2_candidate(
                                                         proposal.clone(),
                                                         block,
                                                     )
                                                     .await
                                                 {
                                                     warn!("Rejected V2 block response: {}", err);
                                                     continue;
                                                 }
                                                 if let Some(commit) = pending_commits
                                                     .remove(&(proposal.height, proposal.block_id))
                                                 {
                                                     if let Err(err) = self.finalize_commit(commit).await {
                                                         warn!("V2 block response could not satisfy pending Commit: {}", err);
                                                     }
                                                 }
                                             }
                                             ConsensusMessage::FinalityRequest { height } => {
                                                 self.respond_to_v2_finality_request(height).await;
                                             }
                                             ConsensusMessage::FinalityResponse { finalized } => {
                                                 let height = finalized.commit.height;
                                                 if let Err(err) = self
                                                     .validate_and_remember_v2_candidate(
                                                         finalized.proposal.clone(),
                                                         finalized.block.clone(),
                                                     )
                                                     .await
                                                 {
                                                     warn!(
                                                         "Rejected V2 finalized-record response at height {}: {}",
                                                         height, err
                                                     );
                                                     continue;
                                                 }
                                                 if let Err(err) = self.finalize_commit(finalized.commit.clone()).await {
                                                     warn!(
                                                         "V2 finalized-record response could not be applied at height {}: {}",
                                                         height, err
                                                     );
                                                     continue;
                                                 }
                                                if let Some(next_height) = height.checked_add(1) {
                                                    self.request_v2_finality(next_height).await;
                                                }
                                             }
                                             ConsensusMessage::Vote(vote) => {
                                                if let Err(err) = self
                                                    .consensus_driver
                                                    .dispatch(ConsensusDriverEvent::NetworkVote(vote.clone()))
                                                    .await
                                                {
                                                    warn!("Consensus driver rejected vote event: {}", err);
                                                    continue;
                                                }
                                                let Some(signer) = self.signer.clone() else {
                                                    if let Err(err) = self.consensus.verify_vote(&vote).await {
                                                        warn!("FullNode rejected invalid V2 vote: {}", err);
                                                    } else {
                                                        info!("FullNode verified V2 vote; no local vote action will be produced");
                                                    }
                                                    continue;
                                                };
                                                 match self.consensus.handle_vote(vote.clone(), signer.as_ref()).await {
                                                     Ok((vote_opt, cert_opt)) => {
                                                     if let Some(precommit_vote) = vote_opt {
                                                         info!(
                                                             "Generated local V2 precommit for height {} round {} block {:?}",
                                                             precommit_vote.height,
                                                             precommit_vote.round,
                                                             precommit_vote.block_id
                                                         );
                                                         let local_cert = match self
                                                             .consensus
                                                             .handle_vote(
                                                                precommit_vote.clone(),
                                                                signer.as_ref(),
                                                            )
                                                            .await
                                                        {
                                                            Ok((_, cert)) => cert,
                                                            Err(error) => {
                                                                warn!(
                                                                    "Signed V2 precommit was not admitted locally: {}",
                                                                    error
                                                                );
                                                                None
                                                            }
                                                        };
                                                        if let Err(error) =
                                                            self.broadcast_v2_vote(precommit_vote).await
                                                        {
                                                            warn!(
                                                                "Failed to enqueue signed precommit vote: {}",
                                                                error
                                                            );
                                                        }
                                                        if let Some(commit_cert) = local_cert {
                                                            if let Err(error) = self
                                                                .finalize_and_broadcast_commit(commit_cert)
                                                                .await
                                                            {
                                                                error!(
                                                                    "Finalized commit was not durably accepted: {}",
                                                                    error
                                                                );
                                                            }
                                                        }
                                                     }
                                                         if let Some(commit_cert) = cert_opt {
                                                         if let Err(err) = self
                                                             .finalize_and_broadcast_commit(commit_cert)
                                                             .await
                                                         {
                                                             error!(
                                                                 "Finalized commit was not durably accepted; Commit will not be broadcast: {}",
                                                                 err
                                                             );
                                                             }
                                                         }
                                                     }
                                                     Err(error) => {
                                                         warn!(
                                                             "Rejected incoming V2 vote at height {} round {} step {:?} block {:?}: {}",
                                                             vote.height,
                                                             vote.round,
                                                             vote.step,
                                                             vote.block_id,
                                                             error
                                                         );
                                                     }
                                                 }
                                              }
                                             ConsensusMessage::Commit(commit_cert) => {
                                                 if let Err(err) = self
                                                     .consensus_driver
                                                     .dispatch(ConsensusDriverEvent::NetworkCommit(commit_cert.clone()))
                                                     .await
                                                 {
                                                     error!("Consensus driver rejected Commit event: {}", err);
                                                     continue;
                                                 }
                                                 let candidate_available = self
                                                     .consensus
                                                     .get_validated_candidate(
                                                         commit_cert.height,
                                                         commit_cert.block_id,
                                                     )
                                                     .await
                                                     .is_some();
                                                 let already_durable = self
                                                     .finality_store
                                                     .recover_finalized_v2(commit_cert.height)
                                                     .await
                                                     .ok()
                                                     .flatten()
                                                     .is_some();
                                                 if !candidate_available && !already_durable {
                                                     pending_commits.insert(
                                                         (commit_cert.height, commit_cert.block_id),
                                                         commit_cert.clone(),
                                                     );
                                                     self.request_v2_block(
                                                         commit_cert.height,
                                                         commit_cert.block_id,
                                                     )
                                                     .await;
                                                     info!(
                                                         "Queued Commit for missing V2 candidate at height {}; requested Proposal/Block",
                                                         commit_cert.height
                                                     );
                                                     continue;
                                                 }
                                                 if let Err(err) = self.finalize_commit(commit_cert).await {
                                                     error!("Failed to apply incoming Commit certificate: {}", err);
                                                 }
                                             }
                                             _ => {
                                                 warn!("UnsupportedProtocolVersion: rejected legacy consensus payload");
                                             }
                                    }
                                }
                            }
                        }
                        None => break,
                    }
                }
                _ = signal::ctrl_c() => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }
    }
}
