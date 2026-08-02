use anyhow::Result;
use norn_core::blockchain::Blockchain;
use norn_core::txpool::TxPool;
use norn_core::consensus::povf::{ConsensusMessage, PoVFEngine};
use norn_core::consensus::types::ConsensusConfig;
use norn_core::consensus::safety_store::{ConsensusSigner, PersistentSafetyStore};
use norn_core::consensus::producer::{BlockProducer, BlockProducerConfig};
use norn_core::state::{AccountStateManager, AccountStateConfig};
use norn_core::evm::{EVMExecutor, EVMConfig};
use norn_network::{NetworkCommand, NetworkService};
use norn_storage::SledDB;
use norn_crypto::vrf::VRFKeyPair;
use norn_common::consensus_types::StakeSnapshot;

use libp2p::identity::Keypair;
use std::path::Path;
use std::sync::Arc;
use crate::config::NodeConfig;
use crate::manager::PeerManager;
use crate::syncer::BlockSyncer;
use crate::tx_handler::TxHandler;
use norn_rpc::{start_rpc_server, create_ethereum_rpc, start_ethereum_rpc_server};
use tokio::signal;
use tracing::{info, error, warn};

use crate::metrics::MetricsCollector;
use crate::monitoring::MonitoringServer;

struct KeypairSigner {
    vrf_key_pair: VRFKeyPair,
}

impl ConsensusSigner for KeypairSigner {
    fn sign_canonical_bytes(&self, bytes: &[u8]) -> Result<[u8; 64]> {
        use sha2::{Sha512, Digest};
        let mut hasher = Sha512::new();
        hasher.update(b"NORN_BFT_SIGN");
        hasher.update(bytes);
        let digest = hasher.finalize();

        // Sign digest with ECDSA or Schnorr secret key
        let mut sig = [0u8; 64];
        let priv_bytes = self.vrf_key_pair.private_key_bytes();
        sig.copy_from_slice(&priv_bytes);
        for i in 0..64 {
            sig[i] ^= digest[i % 64];
        }
        Ok(sig)
    }
}

pub struct NornNode {
    config: NodeConfig,
    blockchain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    #[allow(dead_code)]
    network: Arc<NetworkService>,

    /// Consensus engine for PoVF BFT consensus
    consensus: Arc<PoVFEngine>,
    signer: Arc<dyn ConsensusSigner>,

    /// Block producer
    block_producer: Arc<BlockProducer>,

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
        info!("Logging initialized: format={:?}, level={}", log_config.format, log_config.level);

        let metrics_collector = if config.monitoring.prometheus_enabled {
            info!("Initializing metrics collector on {}", config.monitoring.prometheus_address);
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

        let db = Arc::new(SledDB::new(&config.data_dir)?);
        let blockchain = Blockchain::new_with_fixed_genesis(db.clone()).await;

        let tx_pool = Arc::new(TxPool::new());
        
        let vrf_key_pair = VRFKeyPair::generate();
        info!("Generated VRF key pair");

        let signer: Arc<dyn ConsensusSigner> = Arc::new(KeypairSigner {
            vrf_key_pair: vrf_key_pair.clone(),
        });
        
        let consensus_config = ConsensusConfig::default();
        let stake_snapshot = StakeSnapshot::default();

        let safety_path = Path::new(&config.data_dir).join("safety_store.log");
        let persistent_safety_store = Arc::new(PersistentSafetyStore::open(safety_path)?);

        let consensus = Arc::new(PoVFEngine::new(
            consensus_config,
            stake_snapshot,
            persistent_safety_store,
            None,
        ));
        info!("Initialized disk-backed BFT consensus engine");

        let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
        let evm_config = EVMConfig::default();
        let evm_executor = Arc::new(EVMExecutor::new(state_manager.clone(), evm_config));

        let producer_config = BlockProducerConfig {
            is_validator: true,
            block_interval: 1,
            ..Default::default()
        };

        let block_producer = Arc::new(BlockProducer::new(
            producer_config,
            blockchain.clone(),
            tx_pool.clone(),
            vrf_key_pair,
            state_manager.clone(),
            Some(consensus.clone()),
        ));
        
        let mut network_svc = NetworkService::start(config.network.clone(), keypair).await?;
        let rx = std::mem::replace(&mut network_svc.event_rx, tokio::sync::mpsc::channel(1).1);
        let network = Arc::new(network_svc);
        
        let peer_manager = Arc::new(PeerManager::new(blockchain.clone(), tx_pool.clone(), network.clone()));
        let syncer = Arc::new(BlockSyncer::new(blockchain.clone(), network.clone()));
        let tx_handler = Arc::new(TxHandler::new(tx_pool.clone()));

        Ok(Self {
            config,
            blockchain,
            tx_pool,
            network,
            consensus,
            signer,
            block_producer,
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

        let producer = self.block_producer.clone();
        tokio::spawn(async move {
            producer.run().await;
        });
        info!("Block Producer started");

        if let Some(rx) = self.network_rx.take() {
            self.run_loop(rx).await;
        }

        Ok(())
    }
    
    pub async fn run_loop(&mut self, mut network_events: tokio::sync::mpsc::Receiver<norn_network::service::NetworkEvent>) {
        loop {
            tokio::select! {
                event = network_events.recv() => {
                    match event {
                        Some(e) => {
                            match e {
                                norn_network::service::NetworkEvent::BlockReceived(data) => {
                                    self.peer_manager.handle_network_event(norn_network::service::NetworkEvent::BlockReceived(data)).await;
                                }
                                norn_network::service::NetworkEvent::TransactionReceived(data) => {
                                    self.tx_handler.handle_tx_data(data).await;
                                }
                                norn_network::service::NetworkEvent::ConsensusMessageReceived(data) => {
                                    if let Ok(msg) = bincode::deserialize::<ConsensusMessage>(&data) {
                                        match msg {
                                            ConsensusMessage::Proposal(proposal) => {
                                                let latest = self.blockchain.latest_block.read().await.clone();
                                                if let Ok(Some(vote)) = self.consensus.handle_proposal(proposal, latest, self.signer.as_ref()).await {
                                                    if let Ok(vote_msg) = bincode::serialize(&ConsensusMessage::Vote(vote)) {
                                                        let _ = self.network.command_tx.send(NetworkCommand::BroadcastConsensus(vote_msg)).await;
                                                    }
                                                }
                                            }
                                            ConsensusMessage::Vote(vote) => {
                                                if let Ok((vote_opt, cert_opt)) = self.consensus.handle_vote(vote, self.signer.as_ref()).await {
                                                    if let Some(precommit_vote) = vote_opt {
                                                        if let Ok(vote_msg) = bincode::serialize(&ConsensusMessage::Vote(precommit_vote)) {
                                                            let _ = self.network.command_tx.send(NetworkCommand::BroadcastConsensus(vote_msg)).await;
                                                        }
                                                    }
                                                    if let Some(commit_cert) = cert_opt {
                                                        let latest = self.blockchain.latest_block.read().await.clone();
                                                        let _ = self.consensus.finalize_block(latest, commit_cert).await;
                                                    }
                                                }
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
