mod cli;
mod config_loader;
mod keys;

use clap::Parser;
use norn_node::NornNode;
use std::path::PathBuf;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI before node construction. NornNode owns the single logging
    // subscriber so configuration cannot attempt to install two globals.
    let args = cli::Cli::parse();

    match args.command {
        Some(cli::Commands::GenerateKey { out }) => {
            let path = out.unwrap_or_else(|| PathBuf::from("node.key"));
            let _ = keys::load_or_generate_keypair(&path)?;
            info!("Keypair generated at {:?}", path);
            return Ok(());
        }
        None => {}
    }

    // Load Config
    info!("Loading config from {:?}", args.config);
    let config = config_loader::load_node_config(&args.config, args.data_dir)?;

    // Load Keypair
    let key_path = PathBuf::from(&config.data_dir).join("node.key");
    let keypair = keys::load_or_generate_keypair(&key_path)?;

    // Initialize Node
    let node = NornNode::new(config, keypair).await?;

    // Start Node
    node.start().await?;

    Ok(())
}
