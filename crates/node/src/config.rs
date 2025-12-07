use serde::Deserialize;
use norn_core::config::CoreConfig;
use norn_network::config::NetworkConfig;
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub core: CoreConfig,
    pub network: NetworkConfig,
    pub rpc_address: SocketAddr,
    pub data_dir: String,
}
