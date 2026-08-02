use anyhow::{anyhow, Result};
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use norn_common::chain_context::{ChainContext, PeerRole};
use norn_common::consensus_types::{ConsensusEnvelope, ConsensusMessage, SignedVote, VoteStep};
use norn_common::types::{BlockId, ChainId, Hash, ProtocolVersion, StakeSnapshotHash};
use norn_network::{NetworkCommand, NetworkConfig, NetworkEvent, NetworkService};
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
    let bootstrap_peers = args.collect::<Vec<_>>();

    let keypair = Keypair::generate_ed25519();
    let local_peer_id = libp2p::PeerId::from(keypair.public());
    let chain_context = context(genesis_byte);
    let network = NetworkService::start_with_context(
        NetworkConfig {
            listen_address: "/ip4/127.0.0.1/tcp/0".to_string(),
            bootstrap_peers,
            mdns: false,
        },
        keypair,
        chain_context,
        role,
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
