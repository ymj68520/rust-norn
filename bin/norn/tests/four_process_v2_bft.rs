use libp2p::identity::Keypair;
use libp2p::PeerId;
use norn_common::consensus_types::{StakeSnapshot, ValidatorRecord};
use norn_common::genesis::GenesisConfig;
use norn_common::types::{ConsensusPublicKey, ValidatorId, VrfPublicKey};
use norn_node::NodeKeyStore;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct ProcessGuard {
    root: PathBuf,
    children: Vec<Child>,
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        if std::env::var_os("NORN_KEEP_TEST_ARTIFACTS").is_some() {
            eprintln!("preserving four-process artifacts at {:?}", self.root);
        } else {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn spawn_node(executable: &Path, config: &Path, index: usize) -> Child {
    let data_dir = config
        .parent()
        .expect("config must have a parent")
        .join(format!("node-{index}"));
    let stdout = fs::File::create(data_dir.join("process.stdout.log")).unwrap();
    let stderr = fs::File::create(data_dir.join("process.stderr.log")).unwrap();
    Command::new(executable)
        .args(["--config", config.to_str().expect("UTF-8 config path")])
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("failed to spawn norn process")
}

fn height_after_marker(line: &str, marker: &str) -> Option<u64> {
    if !line.contains(marker) {
        return None;
    }
    let suffix = line.rsplit("height ").next()?;
    let digits = suffix
        .split(|character: char| !character.is_ascii_digit())
        .find(|part| !part.is_empty())?;
    digits.parse().ok()
}

fn log_height(log_dir: &Path, marker: &str) -> u64 {
    let Ok(entries) = fs::read_dir(log_dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .flat_map(|contents| {
            contents
                .lines()
                .filter_map(|line| height_after_marker(line, marker))
                .collect::<Vec<_>>()
        })
        .max()
        .unwrap_or(0)
}

fn finalized_block_id_at_height(log_dir: &Path, height: u64) -> Option<String> {
    let Ok(entries) = fs::read_dir(log_dir) else {
        return None;
    };
    entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .flat_map(|contents| {
            contents
                .lines()
                .filter(|line| {
                    line.contains("Finalized V2 block")
                        && height_after_marker(line, "Finalized V2 block") == Some(height)
                })
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter_map(|line| {
            let start = line.find("Hash(")? + "Hash(".len();
            let end = line[start..].find(')')? + start;
            Some(line[start..end].to_owned())
        })
        .last()
}

fn make_config(
    root: &Path,
    genesis_path: &Path,
    index: usize,
    peer_ids: &[PeerId],
    validator: bool,
) -> PathBuf {
    let data_dir = root.join(format!("node-{index}"));
    let log_dir = data_dir.join("logs");
    fs::create_dir_all(&log_dir).unwrap();
    let listen_port = 41_000 + index as u16;
    let rpc_port = 42_000 + index as u16;
    // Each peer has its own listening port, so build every bootstrap address
    // with the corresponding peer port.
    // Use a deterministic star topology for clean startup. A restarted node
    // that is the star root receives a recovery bootstrap edge immediately
    // before it is relaunched below.
    let bootstrap_peers = if index == 0 {
        Vec::new()
    } else {
        vec![format!("/ip4/127.0.0.1/tcp/41000/p2p/{}", peer_ids[0])]
    };

    let config_path = root.join(format!("config-{index}.json"));
    let config = json!({
        "core": { "consensus": { "pub_key": "", "prv_key": "" } },
        "network": {
            "listen_address": format!("/ip4/127.0.0.1/tcp/{listen_port}"),
            "bootstrap_peers": bootstrap_peers,
            "mdns": false
        },
        "rpc_address": format!("127.0.0.1:{rpc_port}"),
        "data_dir": data_dir,
        "network_mode": "test",
        "node_role": if validator { "validator" } else { "fullnode" },
        "genesis_path": genesis_path,
        "logging": {
            "level": "info",
            "format": "json",
            "outputs": ["file"],
            "file_path": log_dir,
            "compress": false
        }
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    config_path
}

fn add_bootstrap_peer(config_path: &Path, address: String) {
    let bytes = fs::read(config_path).unwrap();
    let mut config: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    config["network"]["bootstrap_peers"] = json!([address]);
    fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

#[test]
fn four_process_norn_v2_bft_reaches_ten_heights_and_recovers_proposer() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rust-norn-four-process-{nonce}"));
    fs::create_dir_all(&root).unwrap();

    let mut node_keys = Vec::new();
    let mut peer_ids = Vec::new();
    for index in 0..4 {
        let data_dir = root.join(format!("node-{index}"));
        fs::create_dir_all(&data_dir).unwrap();
        let keypair = Keypair::generate_ed25519();
        fs::write(
            data_dir.join("node.key"),
            keypair.to_protobuf_encoding().unwrap(),
        )
        .unwrap();
        peer_ids.push(PeerId::from(keypair.public()));
        node_keys.push(NodeKeyStore::open_or_create(data_dir.join("keystore")).unwrap());
    }

    let validators = node_keys
        .iter()
        .take(3)
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
    // Validator 3 is reserved for the partition/Byzantine omission scenario.
    // Its power is 1 out of the total 4, strictly below one third, while the
    // two honest validators retain power 3 and can still form a quorum.
    let mut validators = validators;
    validators[0].voting_power = 2;
    let snapshot = StakeSnapshot::from_genesis(1, validators.clone()).unwrap();
    let mut genesis = GenesisConfig::from_fixed_genesis();
    genesis.validators = validators;
    genesis.genesis_block.header.stake_snapshot_hash = snapshot.snapshot_hash;
    let genesis_path = root.join("genesis.json");
    fs::write(&genesis_path, serde_json::to_vec_pretty(&genesis).unwrap()).unwrap();

    let executable = std::env::var_os("CARGO_BIN_EXE_norn")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("target")
                .join("debug")
                .join(if cfg!(windows) { "norn.exe" } else { "norn" })
        });
    assert!(
        executable.exists(),
        "norn executable not found at {executable:?}"
    );

    let configs = (0..4)
        .map(|index| make_config(&root, &genesis_path, index, &peer_ids, index < 3))
        .collect::<Vec<_>>();
    let mut guard = ProcessGuard {
        root: root.clone(),
        children: configs
            .iter()
            .enumerate()
            .map(|(index, config)| spawn_node(&executable, config, index))
            .collect(),
    };

    let started = Instant::now();
    let first_height_deadline = started + Duration::from_secs(25);
    let log_dirs = (0..4)
        .map(|index| root.join(format!("node-{index}")).join("logs"))
        .collect::<Vec<_>>();
    while Instant::now() < first_height_deadline
        && log_dirs
            .iter()
            .map(|dir| log_height(dir, "Finalized V2 block"))
            .min()
            .unwrap_or(0)
            < 1
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    let first_height = log_dirs
        .iter()
        .map(|dir| log_height(dir, "Finalized V2 block"))
        .min()
        .unwrap_or(0);
    assert!(
        first_height >= 1,
        "four processes did not finalize height one"
    );

    // Kill a validator that has demonstrably produced a proposal, then
    // restart it from the same data directory. This exercises the persistent
    // safety/finality recovery path without changing its key identity.
    let proposer_index = log_dirs
        .iter()
        .position(|dir| log_height(dir, "V2 block template produced at height") >= 1)
        .unwrap_or(0);
    guard.children[proposer_index].kill().unwrap();
    guard.children[proposer_index].wait().unwrap();
    if proposer_index == 0 {
        add_bootstrap_peer(
            &configs[proposer_index],
            format!("/ip4/127.0.0.1/tcp/41001/p2p/{}", peer_ids[1]),
        );
    }
    guard.children[proposer_index] =
        spawn_node(&executable, &configs[proposer_index], proposer_index);

    let final_deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < final_deadline
        && log_dirs
            .iter()
            .map(|dir| log_height(dir, "Finalized V2 block"))
            .min()
            .unwrap_or(0)
            < 10
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    let heights = log_dirs
        .iter()
        .map(|dir| log_height(dir, "Finalized V2 block"))
        .collect::<Vec<_>>();
    assert!(
        heights.iter().all(|height| *height >= 10),
        "four-process V2 finality did not reach height 10: {heights:?}"
    );

    // Simulate a network partition/Byzantine omission after finality has
    // converged. The isolated validator has <1/3 of voting power, so the
    // honest validator set must not produce conflicting finalized blocks.
    let partitioned_index = 2;
    let partition_height = 10;
    let honest_indices = [0usize, 1usize, 3usize];
    let honest_tip_before = honest_indices
        .iter()
        .map(|index| finalized_block_id_at_height(&log_dirs[*index], partition_height))
        .collect::<Vec<_>>();
    assert!(
        honest_tip_before.iter().all(Option::is_some),
        "honest nodes did not record a finalized height-{partition_height} tip: {honest_tip_before:?}"
    );
    assert!(
        honest_tip_before.windows(2).all(|pair| pair[0] == pair[1]),
        "honest nodes disagreed before partition: {honest_tip_before:?}"
    );
    guard.children[partitioned_index].kill().unwrap();
    guard.children[partitioned_index].wait().unwrap();
    std::thread::sleep(Duration::from_secs(3));
    let honest_tip_during = honest_indices
        .iter()
        .map(|index| finalized_block_id_at_height(&log_dirs[*index], partition_height))
        .collect::<Vec<_>>();
    assert_eq!(
        honest_tip_during, honest_tip_before,
        "honest nodes changed to conflicting height-{partition_height} tips during partition"
    );
    guard.children[partitioned_index] =
        spawn_node(&executable, &configs[partitioned_index], partitioned_index);

    let recovery_deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < recovery_deadline
        && log_dirs
            .iter()
            .map(|dir| log_height(dir, "Finalized V2 block"))
            .min()
            .unwrap_or(0)
            < 12
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    let recovered_heights = log_dirs
        .iter()
        .map(|dir| log_height(dir, "Finalized V2 block"))
        .collect::<Vec<_>>();
    assert!(
        recovered_heights.iter().all(|height| *height >= 12),
        "partitioned validator did not recover with the honest chain: {recovered_heights:?}"
    );

    // Restart the FullNode after the validators have finalized additional
    // blocks while it is offline. Its candidate cache is empty after restart;
    // Commit processing must therefore fetch the durable proposal/block pair,
    // execute it, and verify the certificate without any signer.
    let full_node_index = 3;
    guard.children[full_node_index].kill().unwrap();
    guard.children[full_node_index].wait().unwrap();
    let validator_deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < validator_deadline
        && [0usize, 1usize, 2usize]
            .iter()
            .map(|index| log_height(&log_dirs[*index], "Finalized V2 block"))
            .min()
            .unwrap_or(0)
            < 14
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    guard.children[full_node_index] =
        spawn_node(&executable, &configs[full_node_index], full_node_index);
    let full_node_deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < full_node_deadline
        && log_height(&log_dirs[full_node_index], "Finalized V2 block") < 14
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(
        log_height(&log_dirs[full_node_index], "Finalized V2 block") >= 14,
        "FullNode did not recover commit-only finality after restart"
    );
    assert!(guard
        .children
        .iter_mut()
        .all(|child| child.try_wait().unwrap().is_none()));
}
