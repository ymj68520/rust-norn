use anyhow::{Context, Result};
use norn_node::NodeConfig;
use std::path::Path;
use norn_common::utils::config::load_config;

pub fn load_node_config<P: AsRef<Path>>(path: P, data_dir_override: Option<std::path::PathBuf>) -> Result<NodeConfig> {
    let mut config: NodeConfig = load_config(path)?;
    
    if let Some(dd) = data_dir_override {
        config.data_dir = dd.to_string_lossy().to_string();
    }
    
    Ok(config)
}
