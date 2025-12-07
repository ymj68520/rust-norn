use anyhow::Result;
use norn_common::utils::config::load_config;
use serde::Deserialize;
use std::path::Path;
use std::sync::OnceLock;

// Singleton config instance
static CORE_CONFIG: OnceLock<CoreConfig> = OnceLock::new();

#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusConfig {
    pub pub_key: String,
    pub prv_key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoreConfig {
    pub consensus: ConsensusConfig,
    // Add other core sections here
}

pub fn init_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let config: CoreConfig = load_config(path)?;
    CORE_CONFIG.set(config).map_err(|_| anyhow::anyhow!("Config already initialized"))?;
    Ok(())
}

pub fn get_config() -> &'static CoreConfig {
    CORE_CONFIG.get().expect("Config not initialized")
}

pub fn get_consensus_pub_key() -> String {
    get_config().consensus.pub_key.clone()
}
