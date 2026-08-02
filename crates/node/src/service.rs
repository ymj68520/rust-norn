use anyhow::Result;
use norn_core::blockchain::Blockchain;
use norn_core::txpool::TxPool;
use norn_core::txpool_enhanced::EnhancedTxPool;
use norn_core::consensus::povf::PoVFEngine;
use norn_core::consensus::types::ConsensusConfig;
use norn_core::consensus::safety_store::MemorySafetyStore;
use norn_core::consensus::producer::{BlockProducer, BlockProducerConfig};
use norn_core::state::{AccountStateManager, AccountStateConfig};
use norn_core::evm::{EVMExecutor, EVMConfig};
use norn_network::NetworkService;
use norn_storage::SledDB;
use norn_crypto::vrf::VRFKeyPair;
use norn_common::consensus_types::StakeSnapshot;

use libp2p::identity::Keypair;
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

pub struct NornNode {
    config: NodeConfig,
    blockchain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    #[allow(dead_code)]
    network: Arc<NetworkService>,

    /// Consensus engine for PoVF consensus
    #[allow(dead_code)]
    consensus: Arc<PoVFEngine>,

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

        let tx_pool = if config.txpool.enhanced {
            info!("Initializing enhanced transaction pool (BinaryHeap prioritization, EIP-1559)");
            Arc::new(TxPool::new())
        } else {
            info!("Initializing standard transaction pool");
            Arc::new(TxPool::new())
        };
        
        let vrf_key_pair = VRFKeyPair::generate();
        info!("Generated VRF key pair");
        
        let consensus_config = ConsensusConfig::default();
        let stake_snapshot = StakeSnapshot::default();
        let safety_store = Arc::new(MemorySafetyStore::new());

        let consensus = Arc::new(PoVFEngine::new(
            consensus_config,
            stake_snapshot,
            safety_store,
            None,
        ));
        info!("Initialized BFT consensus engine");

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
                                norn_network::service::NetworkEvent::ConsensusMessageReceived(_data) => {
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
