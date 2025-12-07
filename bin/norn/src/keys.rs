use libp2p::identity::Keypair;
use std::fs;
use std::path::Path;
use anyhow::Result;
use tracing::info;

pub fn load_or_generate_keypair<P: AsRef<Path>>(path: P) -> Result<Keypair> {
    if path.as_ref().exists() {
        info!("Loading keypair from {:?}", path.as_ref());
        let bytes = fs::read(path)?;
        let keypair = Keypair::from_protobuf_encoding(&bytes)?;
        Ok(keypair)
    } else {
        info!("Generating new keypair");
        let keypair = Keypair::generate_ed25519();
        let bytes = keypair.to_protobuf_encoding()?;
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
        Ok(keypair)
    }
}
