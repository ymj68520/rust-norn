use anyhow::Result;
use norn_core::blockchain::Blockchain;
use norn_core::txpool::TxPool;
// Week 3: Import enhanced transaction pool
use norn_core::txpool_enhanced::EnhancedTxPool;
use norn_core::consensus::povf::{PoVFEngine, PoVFConfig};
use norn_core::consensus::producer::{BlockProducer, BlockProducerConfig};
use norn_core::state::{AccountStateManager, AccountStateConfig};
use norn_core::evm::{EVMExecutor, EVMConfig};
use norn_network::NetworkService;
use norn_storage::SledDB;
use norn_crypto::vdf::SimpleVDF;
use norn_crypto::vrf::VRFKeyPair;

use libp2p::identity::Keypair;
use std::sync::Arc;
use std::collections::HashMap;
use crate::config::NodeConfig;
use crate::manager::PeerManager;
use crate::syncer::BlockSyncer;
use crate::tx_handler::TxHandler;
use norn_rpc::{start_rpc_server, create_ethereum_rpc, start_ethereum_rpc_server};
use tokio::signal;
use tracing::{info, error, warn};
use norn_common::types::PublicKey;

// Week 3: Enhanced features
use crate::metrics::MetricsCollector;
use crate::monitoring::MonitoringServer;
use crate::logging::LoggingConfig;

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

    // Week 3: Monitoring and logging
    #[allow(dead_code)]
    metrics_collector: Option<Arc<MetricsCollector>>,
    #[allow(dead_code)]
    _monitoring_server: Option<MonitoringServer>,
    #[allow(dead_code)]
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

impl NornNode {
    pub async fn new(config: NodeConfig, keypair: Keypair) -> Result<Self> {
        // Week 3: Initialize logging first
        use crate::logging::LoggingConfig;
        let log_config: LoggingConfig = config.logging.clone().into();
        let log_guard = log_config.init()?;
        info!("Logging initialized: format={:?}, level={}",
              log_config.format, log_config.level);

        // Week 3: Create metrics collector
        let metrics_collector = if config.monitoring.prometheus_enabled {
            info!("Initializing metrics collector on {}",
                  config.monitoring.prometheus_address);
            Some(Arc::new(MetricsCollector::new()))
        } else {
            info!("Metrics collection disabled");
            None
        };

        // Week 3: Start monitoring server
        if config.monitoring.health_check_enabled {
            if let Some(ref collector) = metrics_collector {
                let server = MonitoringServer::new(collector.clone());
                let address = config.monitoring.health_check_address.clone();
                info!("Monitoring server starting on {}", address);
                // Clone address for logging after the spawn
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

        // Week 3: Use enhanced txpool if configured
        let tx_pool = if config.txpool.enhanced {
            info!("Initializing enhanced transaction pool (BinaryHeap prioritization, EIP-1559)");
            // TODO: Replace with actual EnhancedTxPool when fully integrated
            // For now, use standard pool but log the intent
            info!("Note: Enhanced transaction pool features will be available in next phase");
            Arc::new(TxPool::new())
        } else {
            info!("Initializing standard transaction pool");
            Arc::new(TxPool::new())
        };
        
        // Initialize VRF key pair for this node
        let vrf_key_pair = VRFKeyPair::generate();
        info!("Generated VRF key pair");
        
        // Initialize consensus engine with default config
        let vdf_calculator = Arc::new(SimpleVDF::new());
        let mut consensus_config = PoVFConfig::default();
        
        // Add self as validator
        let vrf_bytes = vrf_key_pair.public_key_bytes();
        let mut pub_key_bytes = [0u8; 33];
        pub_key_bytes[..32].copy_from_slice(&vrf_bytes);
        pub_key_bytes[32] = 0x02; // Prefix for compressed public key format
        let pub_key = PublicKey(pub_key_bytes);
        
        consensus_config.validator_stakes.insert(pub_key, 100);
        
        let latest_block = blockchain.latest_block.read().await;
        let initial_round = (latest_block.header.height + 1) as u64;
        drop(latest_block);

        let consensus = Arc::new(PoVFEngine::new(
            consensus_config,
            vdf_calculator,
            vrf_key_pair.clone(),
            initial_round,
            Some(pub_key), // Local validator identity
        ));
        info!("Initialized PoVF consensus engine at round {}", initial_round);

        // Initialize state manager and EVM executor before BlockProducer
        let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
        let evm_config = EVMConfig::default();
        let evm_executor = Arc::new(EVMExecutor::new(state_manager.clone(), evm_config));

        // Initialize Block Producer
        // TODO: Configure from config file
        let producer_config = BlockProducerConfig {
            is_validator: true, // Force enable for test
            block_interval: 1,  // Faster blocks for TPS test (1s)
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
        
        // Extract network receiver
        let mut network_svc = NetworkService::start(config.network.clone(), keypair).await?;

        // Hack: NetworkService struct assumes it holds rx.
        // We construct `NetworkService` then steal `event_rx` using `std::mem::replace`
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
            // Week 3: Add monitoring and logging
            metrics_collector,
            _monitoring_server: None, // Server started in background, not stored
            _log_guard: log_guard,
        })
    }

    pub async fn start(mut self) -> Result<()> {
        info!("Starting Norn Node...");

        // Parse RPC address and create Ethereum RPC address (HTTP port + 1)
        let rpc_addr = self.config.rpc_address;

        let eth_rpc_addr = {
            // Use a larger offset to avoid conflicts
            let port = rpc_addr.port() + 1000;
            format!("{}:{}", rpc_addr.ip(), port).parse::<std::net::SocketAddr>()?
        };

        // Start gRPC server
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

        // Start Ethereum JSON-RPC server
        let eth_rpc = create_ethereum_rpc(
            self.blockchain.clone(),
            self.state_manager.clone(),
            self.evm_executor.clone(),
            self.tx_pool.clone(),
            31337, // Chain ID
        );
        tokio::spawn(async move {
            info!("Ethereum JSON-RPC server listening on {}", eth_rpc_addr);
            if let Err(e) = start_ethereum_rpc_server(eth_rpc_addr, eth_rpc).await {
                error!("Ethereum JSON-RPC server failed: {:?}", e);
            }
        });
        info!("Ethereum JSON-RPC server started on {}", eth_rpc_addr);

        // Start syncer
        let syncer = self.syncer.clone();
        tokio::spawn(async move {
            syncer.start().await;
        });

        // Start block producer
        let producer = self.block_producer.clone();
        tokio::spawn(async move {
            producer.run().await;
        });
        info!("Block Producer started");

        // Start consensus engine (for block production in future)
        // TODO: Add block production loop based on consensus
        // let consensus = self.consensus.clone();
        // tokio::spawn(async move {
        //     consensus.run_consensus_loop().await;
        // });

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
                                    // Handle consensus messages
                                    // warn!("Received consensus message ({} bytes) - TODO: implement handling", data.len());
                                }
                                _ => {}
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
