use anyhow::{anyhow, Result};
use k256::ecdsa::signature::Signer;
use k256::ecdsa::SigningKey;
use norn_common::chain_context::{
    ChainContext, PeerRole, MAX_BLOCK_MESSAGE_BYTES, MAX_TRANSACTION_MESSAGE_BYTES,
};
use norn_common::consensus_types::{
    CommitCertificate, ConsensusEnvelope, ConsensusMessage, MAX_CONSENSUS_ENVELOPE_BYTES,
};
use norn_common::genesis::ProtocolResourceLimits;
use norn_common::types::ValidatorId;
use norn_core::blockchain::Blockchain;
use norn_core::consensus::povf::PoVFEngine;
use norn_core::consensus::producer::{BlockProducer, BlockProducerConfig};
use norn_core::consensus::safety_store::{ConsensusSigner, PersistentSafetyStore};
use norn_core::consensus::types::{ConsensusConfig, ProposalSigner};
use norn_core::evm::{EVMConfig, EVMExecutor};
use norn_core::execution::overlay::ExecutionOverlay;
use norn_core::finality::FinalityStore;
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
use std::path::Path;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, warn};

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

        let consensus = Arc::new(PoVFEngine::new_with_parent_randomness(
            consensus_config,
            genesis_snapshot,
            genesis_config.initial_randomness,
            persistent_safety_store,
            local_validator_id,
        ));
        info!("Initialized disk-backed BFT consensus engine");

        let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
        let evm_config = EVMConfig::default();
        let evm_executor = Arc::new(EVMExecutor::new(state_manager.clone(), evm_config));

        if let Some((finalized, state_writes)) =
            finality_store.recover_finalized_tip_with_state().await?
        {
            ExecutionOverlay::apply_persisted_writes(
                &state_writes,
                &state_manager,
                evm_executor.code_storage().as_ref(),
            )
            .await
            .map_err(|error| anyhow!("failed to replay finalized state writes: {error}"))?;
            let next_snapshot = {
                let sm = consensus.state_machine.read().await;
                let next_height = finalized
                    .consensus_state
                    .height
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("finalized height overflow during recovery"))?;
                let next_epoch = sm.config.epoch_for_height(next_height)?;
                sm.snapshot.for_epoch(next_epoch)?
            };
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
            tokio::spawn(async move {
                let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
                loop {
                    timer.tick().await;
                    if producer.should_produce().await {
                        if let Ok((proposal, block)) = producer.produce_proposal().await {
                            let envelope = ConsensusEnvelope {
                                wire_version: chain_context.wire_version,
                                protocol_version: proposal.protocol_version,
                                chain_id: proposal.chain_id,
                                genesis_hash: chain_context.genesis_hash,
                                payload: ConsensusMessage::Proposal {
                                    proposal: proposal.clone(),
                                    block: block.clone(),
                                },
                            };

                            if let Ok(msg_bytes) = bincode::serialize(&envelope) {
                                let _ = network_ref
                                    .command_tx
                                    .send(NetworkCommand::BroadcastConsensus(msg_bytes))
                                    .await;
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

        if let Some(rx) = self.network_rx.take() {
            self.run_loop(rx).await;
        }

        Ok(())
    }

    async fn finalize_commit(&self, commit: CommitCertificate) -> Result<()> {
        let durable_v2 = self
            .finality_store
            .recover_finalized_v2_with_state(commit.height)
            .await?;
        let is_v2 = self
            .consensus
            .candidate_blocks_v2
            .read()
            .await
            .contains_key(&(commit.height, commit.block_id))
            || durable_v2.is_some();
        if is_v2 {
            let (finalized, state_write_values, commit_status) =
                if let Some((persisted, state_writes)) = durable_v2 {
                    if persisted.block.header.height < 0
                        || persisted.block.header.height as u64 != commit.height
                        || persisted.commit.block_id != commit.block_id
                        || persisted.commit.certificate_hash() != commit.certificate_hash()
                    {
                        return Err(anyhow!(
                            "durable finalized payload conflicts with received certificate"
                        ));
                    }
                    let status = self
                        .finality_store
                        .commit_finalized_transaction_with_state(&persisted, &state_writes)
                        .await?;
                    (persisted, state_writes, status)
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
                    let status = self
                        .finality_store
                        .commit_finalized_transaction_with_state(&finalized, &state_writes)
                        .await?;
                    (finalized, state_writes, status)
                };
            ExecutionOverlay::apply_persisted_writes(
                &state_write_values,
                &self.state_manager,
                self.evm_executor.code_storage().as_ref(),
            )
            .await
            .map_err(|error| anyhow!("failed to apply finalized state writes: {error}"))?;
            let next_snapshot = {
                let sm = self.consensus.state_machine.read().await;
                let next_height = finalized
                    .consensus_state
                    .height
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("finalized height overflow"))?;
                let next_epoch = sm.config.epoch_for_height(next_height)?;
                sm.snapshot.for_epoch(next_epoch)?
            };
            self.consensus
                .advance_after_finalized_v2(&finalized, next_snapshot)
                .await?;
            info!(
                "Finalized V2 block {:?} at height {}; durable finality status {:?}",
                finalized.block.header.block_hash, finalized.block.header.height, commit_status
            );
            Ok(())
        } else {
            let finalized = self.consensus.finalize_block(commit).await?;
            info!(
                "Finalized block {:?} at height {}",
                finalized.block.header.block_hash, finalized.block.header.height
            );
            self.blockchain.commit_block(&finalized.block).await?;
            Ok(())
        }
    }

    pub async fn run_loop(
        &mut self,
        mut network_events: tokio::sync::mpsc::Receiver<norn_network::service::NetworkEvent>,
    ) {
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
                                            ConsensusMessage::Proposal { proposal, block } => {
                                                let Some(signer) = self.signer.clone() else {
                                                    // FullNode validation is implemented in the later
                                                    // proposal-validation phase. Until then it must not
                                                    // manufacture a signer or imply that the payload was
                                                    // fully validated.
                                                    warn!("FullNode received Proposal; no local vote will be cast");
                                                    continue;
                                                };
                                                if let Ok(Some(vote)) = self.consensus.handle_proposal(proposal, block, signer.as_ref()).await {
                                                    let resp_env = ConsensusEnvelope {
                                                        wire_version: self.chain_context.wire_version,
                                                        protocol_version: vote.protocol_version,
                                                        chain_id: vote.chain_id,
                                                        genesis_hash: self.chain_context.genesis_hash,
                                                        payload: ConsensusMessage::Vote(vote),
                                                    };
                                                    if let Ok(vote_msg) = bincode::serialize(&resp_env) {
                                                        if let Err(err) = self
                                                            .network
                                                            .command_tx
                                                            .send(NetworkCommand::BroadcastConsensus(vote_msg))
                                                            .await
                                                        {
                                                            warn!("Failed to enqueue signed proposal vote: {}", err);
                                                        }
                                                    } else {
                                                        warn!("Failed to encode signed proposal vote");
                                                    }
                                                }
                                            }
                                            ConsensusMessage::ProposalV2 { proposal, block } => {
                                                if let Err(err) = block.validate_structure(
                                                    &self.chain_context,
                                                    &self.resource_limits,
                                                ) {
                                                    warn!("Rejected V2 proposal resource/commitment validation: {}", err);
                                                    continue;
                                                }
                                                let latest = self.blockchain.latest_block.read().await;
                                                let parent_matches = latest.header.block_hash
                                                    == block.header.prev_block_hash
                                                    && latest.header.height.checked_add(1)
                                                        == Some(block.header.height);
                                                drop(latest);
                                                if !parent_matches {
                                                    warn!("Rejected V2 proposal with non-canonical parent or height");
                                                    continue;
                                                }
                                                let Some(signer) = self.signer.clone() else {
                                                    warn!("FullNode received V2 Proposal; no local vote will be cast");
                                                    continue;
                                                };
                                                if let Ok(Some(vote)) = self
                                                    .consensus
                                                    .handle_proposal_v2(
                                                        proposal,
                                                        block,
                                                        signer.as_ref(),
                                                        &self.state_manager,
                                                        &self.resource_limits,
                                                        &self.chain_context,
                                                        self.evm_executor.code_storage(),
                                                    )
                                                    .await
                                                {
                                                    let resp_env = ConsensusEnvelope {
                                                        wire_version: self.chain_context.wire_version,
                                                        protocol_version: vote.protocol_version,
                                                        chain_id: vote.chain_id,
                                                        genesis_hash: self.chain_context.genesis_hash,
                                                        payload: ConsensusMessage::Vote(vote),
                                                    };
                                                    if let Ok(vote_msg) = bincode::serialize(&resp_env) {
                                                        if let Err(err) = self
                                                            .network
                                                            .command_tx
                                                            .send(NetworkCommand::BroadcastConsensus(vote_msg))
                                                            .await
                                                        {
                                                            warn!("Failed to enqueue signed V2 proposal vote: {}", err);
                                                        }
                                                    } else {
                                                        warn!("Failed to encode signed V2 proposal vote");
                                                    }
                                                }
                                            }
                                            ConsensusMessage::Vote(vote) => {
                                                let Some(signer) = self.signer.clone() else {
                                                    warn!("FullNode received Vote; no local vote action will be produced");
                                                    continue;
                                                };
                                                if let Ok((vote_opt, cert_opt)) = self.consensus.handle_vote(vote, signer.as_ref()).await {
                                                    if let Some(precommit_vote) = vote_opt {
                                                        let resp_env = ConsensusEnvelope {
                                                            wire_version: self.chain_context.wire_version,
                                                            protocol_version: precommit_vote.protocol_version,
                                                            chain_id: precommit_vote.chain_id,
                                                            genesis_hash: self.chain_context.genesis_hash,
                                                            payload: ConsensusMessage::Vote(precommit_vote),
                                                        };
                                                        if let Ok(vote_msg) = bincode::serialize(&resp_env) {
                                                            if let Err(err) = self
                                                                .network
                                                                .command_tx
                                                                .send(NetworkCommand::BroadcastConsensus(vote_msg))
                                                                .await
                                                            {
                                                                warn!("Failed to enqueue signed precommit vote: {}", err);
                                                            }
                                                        } else {
                                                            warn!("Failed to encode signed precommit vote");
                                                        }
                                                     }
                                                     if let Some(commit_cert) = cert_opt {
                                                         match self.finalize_commit(commit_cert.clone()).await {
                                                             Ok(()) => {
                                                                 let commit_env = ConsensusEnvelope {
                                                                     wire_version: self.chain_context.wire_version,
                                                                     protocol_version: commit_cert.protocol_version,
                                                                     chain_id: commit_cert.chain_id,
                                                                     genesis_hash: self.chain_context.genesis_hash,
                                                                     payload: ConsensusMessage::Commit(commit_cert),
                                                                 };
                                                                 match bincode::serialize(&commit_env) {
                                                                     Ok(commit_msg) => {
                                                                         if let Err(err) = self
                                                                             .network
                                                                             .command_tx
                                                                             .send(NetworkCommand::BroadcastConsensus(commit_msg))
                                                                             .await
                                                                         {
                                                                             warn!("Failed to enqueue durable Commit broadcast: {}", err);
                                                                         }
                                                                     }
                                                                     Err(err) => warn!("Failed to encode durable Commit broadcast: {}", err),
                                                                 }
                                                             }
                                                             Err(err) => error!(
                                                                 "Finalized commit was not durably accepted; Commit will not be broadcast: {}",
                                                                 err
                                                             ),
                                                         }
                                                     }
                                                 }
                                             }
                                             ConsensusMessage::Commit(commit_cert) => {
                                                 if let Err(err) = self.finalize_commit(commit_cert).await {
                                                     error!("Failed to apply incoming Commit certificate: {}", err);
                                                 }
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
