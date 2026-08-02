//! Node KeyStore for Consensus ECDSA and Ristretto VRF Keys

use anyhow::{anyhow, Result};
use k256::ecdsa::SigningKey;
use norn_crypto::vrf::VRFKeyPair;
use rand::thread_rng;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;
use tracing::info;

pub struct NodeKeyStore {
    key_dir: PathBuf,
    consensus_key: SigningKey,
    vrf_key: VRFKeyPair,
}

impl NodeKeyStore {
    pub fn open_or_create(dir: impl AsRef<Path>) -> Result<Self> {
        let key_dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&key_dir)?;

        let consensus_path = key_dir.join("consensus.key");
        let vrf_path = key_dir.join("vrf.key");

        let consensus_key = if consensus_path.exists() {
            let bytes = Zeroizing::new(Self::read_key_file(&consensus_path)?);
            if bytes.len() != 32 {
                return Err(anyhow!("Corrupted consensus key file at {:?}", consensus_path));
            }
            SigningKey::from_slice(&bytes)
                .map_err(|_| anyhow!("Invalid consensus secret key bytes"))?
        } else {
            let key = SigningKey::random(&mut thread_rng());
            let bytes = Zeroizing::new(key.to_bytes().to_vec());
            Self::write_key_file_atomic(&consensus_path, &bytes)?;
            info!("Generated and saved new consensus signing key to {:?}", consensus_path);
            key
        };

        let vrf_key = if vrf_path.exists() {
            let bytes = Zeroizing::new(Self::read_key_file(&vrf_path)?);
            if bytes.len() != 64 {
                return Err(anyhow!("Corrupted VRF key file at {:?}", vrf_path));
            }
            let mut secret_arr = [0u8; 64];
            secret_arr.copy_from_slice(&bytes);
            VRFKeyPair::from_secret_key_bytes(&secret_arr)?
        } else {
            let key = VRFKeyPair::generate();
            let secret_bytes = Zeroizing::new(key.private_key_bytes().to_vec());
            Self::write_key_file_atomic(&vrf_path, &secret_bytes)?;
            info!("Generated and saved new VRF key pair to {:?}", vrf_path);
            key
        };

        Ok(Self {
            key_dir,
            consensus_key,
            vrf_key,
        })
    }

    pub fn consensus_key(&self) -> &SigningKey {
        &self.consensus_key
    }

    pub fn vrf_key(&self) -> &VRFKeyPair {
        &self.vrf_key
    }

    fn read_key_file(path: &Path) -> Result<Vec<u8>> {
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn write_key_file_atomic(path: &Path, content: &[u8]) -> Result<()> {
        let parent = path.parent().ok_or_else(|| anyhow!("Invalid key path"))?;
        let tmp_path = parent.join(format!(".tmp_{}", path.file_name().unwrap().to_string_lossy()));

        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = file.metadata()?.permissions();
                perms.set_mode(0o600);
                file.set_permissions(perms)?;
            }

            file.write_all(content)?;
            file.flush()?;
            file.sync_all()?;
        }

        std::fs::rename(&tmp_path, path)?;

        #[cfg(unix)]
        {
            let dir_file = File::open(parent)?;
            let _ = dir_file.sync_all();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_keystore_persistence_and_fail_closed() {
        let temp_dir = TempDir::new().unwrap();
        let store1 = NodeKeyStore::open_or_create(temp_dir.path()).unwrap();
        let pub1 = store1.consensus_key().verifying_key().to_sec1_bytes();

        let store2 = NodeKeyStore::open_or_create(temp_dir.path()).unwrap();
        let pub2 = store2.consensus_key().verifying_key().to_sec1_bytes();

        assert_eq!(pub1, pub2);

        // Corrupt key file and ensure fail-closed
        let consensus_path = temp_dir.path().join("consensus.key");
        std::fs::write(&consensus_path, b"corrupted").unwrap();
        assert!(NodeKeyStore::open_or_create(temp_dir.path()).is_err());
    }
}
