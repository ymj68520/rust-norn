mod cli;
mod keys;
mod config_loader;

use clap::Parser;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;
use norn_node::NornNode;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Setup Logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    // 2. Parse CLI
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

    // 3. Load Config
    info!("Loading config from {:?}", args.config);
    let config = config_loader::load_node_config(&args.config, args.data_dir)?;

    // 4. Load Keypair
    let key_path = PathBuf::from(&config.data_dir).join("node.key");
    let keypair = keys::load_or_generate_keypair(&key_path)?;

    // 5. Initialize Node
    let node = NornNode::new(config, keypair).await?;

    // 6. Start Node
    node.start().await?;

    Ok(())
}