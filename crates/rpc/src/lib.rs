pub mod proto {
    tonic::include_proto!("blockchain");
}
pub mod ethereum;
pub mod mapper;
pub mod rlp_tx;
pub mod server;
pub mod websocket; // WebSocket support for real-time events

use crate::ethereum::{EthereumRpcImpl, EthereumRpcServer};
use crate::proto::blockchain_service_server::BlockchainServiceServer;
use crate::server::BlockchainRpcImpl;
use jsonrpsee::server::Server as JsonRpcServer;
use norn_core::blockchain::Blockchain;
use norn_core::evm::{EVMConfig, EVMExecutor};
use norn_core::finality::FinalityStore;
use norn_core::state::AccountStateManager;
use norn_core::txpool::TxPool;
use norn_network::NetworkCommand;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tonic::transport::Server;
use tracing::info;

pub async fn start_rpc_server(
    addr: SocketAddr,
    chain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    tx_pool_v2: Option<Arc<norn_core::txpool_v2::TransactionV2Pool>>,
    finality_store: Arc<FinalityStore>,
    transaction_broadcast: Option<mpsc::Sender<NetworkCommand>>,
) -> Result<(), tonic::transport::Error> {
    let service = if let Some(v2) = tx_pool_v2 {
        BlockchainRpcImpl::new_with_v2_and_finality(
            chain,
            tx_pool,
            v2,
            finality_store,
            transaction_broadcast,
        )
    } else {
        BlockchainRpcImpl::new_with_finality(chain, tx_pool, finality_store)
    };

    Server::builder()
        .add_service(BlockchainServiceServer::new(service))
        .serve(addr)
        .await
}

/// Create Ethereum RPC service
pub fn create_ethereum_rpc(
    chain: Arc<Blockchain>,
    state_manager: Arc<AccountStateManager>,
    evm_executor: Arc<EVMExecutor>,
    tx_pool: Arc<TxPool>,
    chain_id: u64,
) -> EthereumRpcImpl {
    EthereumRpcImpl::new(chain, state_manager, evm_executor, tx_pool, chain_id)
}

// Re-export for convenience
pub use crate::ethereum::start_ethereum_rpc_server;
pub use crate::websocket::{EventBroadcaster, SubscriptionType, WebSocketConfig, WebSocketServer};
