use anyhow::{anyhow, Result};
use k256::ecdsa::{signature::Signer, SigningKey};
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use norn_common::chain_context::{ChainContext, PeerRole};
use norn_common::consensus_types::{ConsensusEnvelope, ConsensusMessage, SignedVote, VoteStep};
use norn_common::types::{BlockId, ChainId, Hash, ProtocolVersion, StakeSnapshotHash, ValidatorId};
use norn_network::{
    NetworkAuthConfig, NetworkCommand, NetworkConfig, NetworkEvent, NetworkService,
    ValidatorHandshakeIdentity,
};
use std::collections::HashMap;
use std::env;
use std::io::Write;
use tokio::io::{AsyncBufReadExt, BufReader};

fn context(genesis_byte: u8) -> ChainContext {
    ChainContext::new(
        2,
        ProtocolVersion(2),
        ChainId(Hash([7u8; 32])),
        Hash([genesis_byte; 32]),
    )
}

fn consensus_message(context: ChainContext, validator_byte: u8) -> Vec<u8> {
    let vote = SignedVote {
        protocol_version: context.protocol_version,
        chain_id: context.chain_id,
        epoch: 1,
        height: 1,
        round: 0,
        step: VoteStep::Prevote,
        block_id: Some(BlockId(Hash([9u8; 32]))),
        stake_snapshot_hash: StakeSnapshotHash([8u8; 32]),
        validator: norn_common::types::ValidatorId([validator_byte; 32]),
        signature: [3u8; 64],
    };
    bincode::serialize(&ConsensusEnvelope {
        wire_version: context.wire_version,
        protocol_version: context.protocol_version,
        chain_id: context.chain_id,
        genesis_hash: context.genesis_hash,
        payload: ConsensusMessage::Vote(vote),
    })
    .expect("worker consensus envelope serializes")
}

fn emit(line: impl AsRef<str>) {
    println!("{}", line.as_ref());
    let _ = std::io::stdout().flush();
}

fn validator_key(byte: u8) -> SigningKey {
    SigningKey::from_bytes((&[byte; 32]).into()).expect("fixed stage7 key is valid")
}

fn validator_auth(validator_byte: u8) -> NetworkAuthConfig {
    let validator_keys = (1u8..=3)
        .map(|byte| {
            let key = validator_key(byte);
            let public_key: [u8; 33] = key
                .verifying_key()
                .to_encoded_point(true)
                .as_bytes()
                .try_into()
                .expect("compressed stage7 public key has the expected length");
            (ValidatorId([byte; 32]), public_key)
        })
        .collect::<HashMap<_, _>>();
    let signing_key = validator_key(validator_byte);
    let consensus_public_key = *validator_keys
        .get(&ValidatorId([validator_byte; 32]))
        .expect("local stage7 validator key is in the Genesis key table");
    let signer = signing_key.clone();
    NetworkAuthConfig {
        local_validator: Some(ValidatorHandshakeIdentity {
            validator_id: ValidatorId([validator_byte; 32]),
            consensus_public_key,
            sign: std::sync::Arc::new(move |bytes| {
                let signature: k256::ecdsa::Signature = signer.sign(bytes);
                let signature = signature.normalize_s().unwrap_or(signature);
                Ok(signature.to_bytes().into())
            }),
        }),
        validator_public_keys: validator_keys,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let role = match args.next().as_deref() {
        Some("validator") => PeerRole::Validator,
        Some("fullnode") => PeerRole::FullNode,
        Some(other) => return Err(anyhow!("unknown worker role {other:?}")),
        None => return Err(anyhow!("worker role is required")),
    };
    let genesis_byte = args
        .next()
        .ok_or_else(|| anyhow!("genesis byte is required"))?
        .parse::<u8>()?;
    let validator_byte = if role == PeerRole::Validator {
        Some(
            args.next()
                .ok_or_else(|| anyhow!("validator byte is required"))?
                .parse::<u8>()?,
        )
    } else {
        None
    };
    let bootstrap_peers = args.collect::<Vec<_>>();

    let keypair = Keypair::generate_ed25519();
    let local_peer_id = libp2p::PeerId::from(keypair.public());
    let chain_context = context(genesis_byte);
    let network = NetworkService::start_with_context_and_auth(
        NetworkConfig {
            listen_address: "/ip4/127.0.0.1/tcp/0".to_string(),
            bootstrap_peers,
            mdns: false,
        },
        keypair,
        chain_context,
        role,
        validator_byte
            .map(validator_auth)
            .unwrap_or_else(NetworkAuthConfig::default),
    )
    .await?;
    let command_tx = network.command_tx.clone();
    let mut event_rx = network.event_rx;
    let mut stdin = BufReader::new(tokio::io::stdin()).lines();

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(NetworkEvent::Listening(address)) => {
                        let address = address.with(Protocol::P2p(local_peer_id));
                        emit(format!("LISTEN {address}"));
                    }
                    Some(NetworkEvent::PeerAuthenticated { peer_id, role }) => {
                        emit(format!("AUTH {peer_id} {role:?}"));
                    }
                    Some(NetworkEvent::ConsensusMessageReceived(_)) => emit("CONSENSUS"),
                    Some(NetworkEvent::DialFailed { address, reason }) => {
                        emit(format!("DIAL_FAILED {address} {reason}"));
                    }
                    Some(NetworkEvent::PeerConnected(peer_id)) => {
                        emit(format!("CONNECTED {peer_id}"));
                    }
                    Some(NetworkEvent::PeerDisconnected(peer_id)) => {
                        emit(format!("DISCONNECTED {peer_id}"));
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            line = stdin.next_line() => {
                match line? {
                    Some(command) if command == "EXIT" => break,
                    Some(command) if command.starts_with("BROADCAST ") => {
                        let validator_byte = command[10..].trim().parse::<u8>()?;
                        command_tx
                            .send(NetworkCommand::BroadcastConsensus(
                                consensus_message(chain_context, validator_byte),
                            ))
                            .await
                            .map_err(|_| anyhow!("network command channel closed"))?;
                    }
                    Some(_) => emit("IGNORED"),
                    None => break,
                }
            }
        }
    }

    Ok(())
}
