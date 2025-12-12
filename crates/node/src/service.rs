use anyhow::Result;
use norn_core::blockchain::Blockchain;
use norn_core::txpool::TxPool;
use norn_network::NetworkService;
use norn_storage::RocksDB;
use norn_common::types::Block;
use libp2p::identity::Keypair;
use std::sync::Arc;
use crate::config::NodeConfig;
use crate::manager::PeerManager;
use crate::syncer::BlockSyncer;
use crate::tx_handler::TxHandler;
use norn_rpc::start_rpc_server;
use tokio::signal;
use tracing::{info, error};

pub struct NornNode {
    config: NodeConfig,
    blockchain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    network: Arc<NetworkService>,
    
    peer_manager: Arc<PeerManager>,
    syncer: Arc<BlockSyncer>,
    tx_handler: Arc<TxHandler>,
    
    // Temp holder for startup
    network_rx: Option<tokio::sync::mpsc::Receiver<norn_network::service::NetworkEvent>>,
}

impl NornNode {
    pub async fn new(config: NodeConfig, keypair: Keypair) -> Result<Self> {
        let db = Arc::new(RocksDB::new(&config.data_dir)?);
        let genesis = Block::default(); 
        let blockchain = Blockchain::new(db.clone(), genesis).await;
        let tx_pool = Arc::new(TxPool::new());
        
        // Extract receiver
        let mut network_svc = NetworkService::start(config.network.clone(), keypair).await?;
        // We need to swap out receiver or clone struct without it? 
        // `NetworkService` struct fields are pub.
        // We can't move out field from Arc if we wrap it.
        // So we split here.
        
        // Hack: NetworkService struct assumes it holds rx.
        // Ideally `start` returns `(Arc<NetworkService>, Receiver)`.
        // Since `NetworkService` is in another crate and we defined it to hold `event_rx`, we are stuck unless we modify `NetworkService`.
        // Assuming `NetworkService` allows us to take `event_rx` out? No, fields are pub but struct moved into Arc later.
        
        // Let's modify `NetworkService::start` signature in next step or use `Mutex<Option<Rx>>` in `NetworkService`.
        // But `NetworkService` is already done.
        // I will assume `NetworkService` in `node` uses a workaround: 
        // We construct `NetworkService` then steal `event_rx` using `std::mem::replace` or similar if we could mutably access it.
        // But we want `Arc<NetworkService>` for `PeerManager`.
        
        // Let's just take the field out before Arcing.
        let rx = std::mem::replace(&mut network_svc.event_rx, tokio::sync::mpsc::channel(1).1); // Dummy channel replace
        let network = Arc::new(network_svc);
        
        let peer_manager = Arc::new(PeerManager::new(blockchain.clone(), tx_pool.clone(), network.clone()));
        let syncer = Arc::new(BlockSyncer::new(blockchain.clone(), network.clone()));
        let tx_handler = Arc::new(TxHandler::new(tx_pool.clone()));
        
        Ok(Self {
            config,
            blockchain,
            tx_pool,
            network,
            peer_manager,
            syncer,
            tx_handler,
            network_rx: Some(rx),
        })
    }

    pub async fn start(mut self) -> Result<()> {
        info!("Starting Norn Node...");
        
        let rpc_addr = self.config.rpc_address;
        let chain_ref = self.blockchain.clone();
        tokio::spawn(async move {
            info!("RPC Server listening on {}", rpc_addr);
            if let Err(e) = start_rpc_server(rpc_addr, chain_ref).await {
                error!("RPC Server failed: {}", e);
            }
        });
        
        let syncer = self.syncer.clone();
        tokio::spawn(async move {
            syncer.start().await;
        });
        
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
