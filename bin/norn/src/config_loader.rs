use anyhow::Result;
use norn_common::utils::config::load_config;
use norn_node::NodeConfig;
use std::path::Path;

pub fn load_node_config<P: AsRef<Path>>(
    path: P,
    data_dir_override: Option<std::path::PathBuf>,
) -> Result<NodeConfig> {
    let config_path = path.as_ref();
    let mut config: NodeConfig = load_config(config_path)?;

    if let Some(genesis_path) = config.genesis_path.as_mut() {
        let path = std::path::PathBuf::from(&*genesis_path);
        if path.is_relative() {
            let base = config_path.parent().unwrap_or_else(|| Path::new("."));
            *genesis_path = base.join(path).to_string_lossy().to_string();
        }
    }

    if let Some(dd) = data_dir_override {
        config.data_dir = dd.to_string_lossy().to_string();
    }

    Ok(config)
}
