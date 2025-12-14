use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkConfig {
    pub listen_address: String, // e.g., "/ip4/0.0.0.0/tcp/0"
    pub bootstrap_peers: Vec<String>,
    pub mdns: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_address: "/ip4/0.0.0.0/tcp/0".to_string(),
            bootstrap_peers: vec![],
            mdns: true,
        }
    }
}
