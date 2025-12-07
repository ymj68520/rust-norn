pub mod proto {
    tonic::include_proto!("blockchain");
}
pub mod server;
pub mod mapper;

use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;
use norn_core::blockchain::Blockchain;
use crate::server::BlockchainRpcImpl;
use crate::proto::blockchain_service_server::BlockchainServiceServer;

pub async fn start_rpc_server(addr: SocketAddr, chain: Arc<Blockchain>) -> Result<(), tonic::transport::Error> {
    let service = BlockchainRpcImpl::new(chain);
    
    Server::builder()
        .add_service(BlockchainServiceServer::new(service))
        .serve(addr)
        .await
}
