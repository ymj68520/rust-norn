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
use norn_core::state::AccountStateManager;
use norn_core::txpool::TxPool;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;
use tracing::info;

pub async fn start_rpc_server(
    addr: SocketAddr,
    chain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
) -> Result<(), tonic::transport::Error> {
    let service = BlockchainRpcImpl::new(chain, tx_pool);

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
