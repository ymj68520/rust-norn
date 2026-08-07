use norn_common::consensus_types::{StakeSnapshot, ValidatorRecord};
use norn_common::genesis::GenesisConfig;
use norn_common::types::{ConsensusPublicKey, ValidatorId, VrfPublicKey};
use norn_node::NodeKeyStore;
use std::fs;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let out_dir = PathBuf::from("cluster_setup");
    fs::create_dir_all(&out_dir)?;

    let mut node_keys = Vec::new();
    let ips = ["192.168.31.227", "192.168.31.6", "192.168.31.169"];

    for index in 0..3 {
        let data_dir = out_dir.join(format!("node{}", index + 1));
        let keystore_dir = data_dir.join("keystore");
        fs::create_dir_all(&keystore_dir)?;
        let key_store = NodeKeyStore::open_or_create(&keystore_dir)?;
        node_keys.push(key_store);
    }

    let validators = node_keys
        .iter()
        .enumerate()
        .map(|(index, key_store)| {
            let consensus_key: [u8; 33] = key_store
                .consensus_key()
                .verifying_key()
                .to_sec1_bytes()
                .as_ref()
                .try_into()
                .unwrap();
            ValidatorRecord {
                validator_id: ValidatorId([(index + 1) as u8; 32]),
                consensus_public_key: ConsensusPublicKey(consensus_key),
                vrf_public_key: VrfPublicKey(key_store.vrf_key().public_key_bytes()),
                voting_power: 1,
                jailed_until_epoch: None,
                slashed: false,
            }
        })
        .collect::<Vec<_>>();

    let snapshot = StakeSnapshot::from_genesis(1, validators.clone()).unwrap();
    let mut genesis = GenesisConfig::from_fixed_genesis();
    genesis.validators = validators;
    genesis.genesis_block.header.stake_snapshot_hash = snapshot.snapshot_hash;

    let genesis_path = out_dir.join("genesis.json");
    fs::write(&genesis_path, serde_json::to_vec_pretty(&genesis)?)?;
    println!("Genesis generated at {:?}", genesis_path);

    for index in 0..3 {
        let node_id = index + 1;
        let data_dir = format!("node{}_data", node_id);
        let bootstrap = if index == 0 {
            vec![]
        } else {
            vec![format!("/ip4/192.168.31.227/tcp/4001")]
        };

        let config = serde_json::json!({
            "core": { "consensus": { "pub_key": "", "prv_key": "" } },
            "network": {
                "listen_address": "/ip4/0.0.0.0/tcp/4001",
                "bootstrap_peers": bootstrap,
                "mdns": false
            },
            "rpc_address": "0.0.0.0:45555",
            "data_dir": data_dir,
            "network_mode": "devnet",
            "node_role": "validator",
            "genesis_path": genesis_path.to_str().unwrap(),
            "logging": {
                "level": "info",
                "format": "pretty",
                "outputs": ["stdout"],
                "compress": false
            }
        });
        let config_file = out_dir.join(format!("node{}_config.json", node_id));
        fs::write(&config_file, serde_json::to_vec_pretty(&config)?)?;
        println!("Config for node {} generated at {:?}", node_id, config_file);
    }

    Ok(())
}
