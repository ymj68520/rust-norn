use crate::behaviour_builder::build_behaviour;
use crate::config::NetworkConfig;
use crate::event_loop::EventLoop;
use crate::transport::build_transport;
use anyhow::{anyhow, Result};
use libp2p::identity::Keypair;
use libp2p::{Multiaddr, PeerId, SwarmBuilder};
use norn_common::chain_context::{ChainContext, PeerRole};
use tokio::sync::mpsc;
use tracing::info;

#[derive(Debug)] // Add Debug trait for easier debugging
pub enum NetworkCommand {
    BroadcastBlock(Vec<u8>),
    BroadcastTransaction(Vec<u8>),
    BroadcastConsensus(Vec<u8>),
    /// Establish an outbound connection to a configured peer. The address is
    /// still subject to libp2p transport authentication and peer identity
    /// checks; this command only controls dialing, never authorization.
    Dial(Multiaddr),
    StartListening,
}

#[derive(Debug)] // Add Debug trait for easier debugging
pub enum NetworkEvent {
    Listening(Multiaddr),
    PeerConnected(PeerId),
    DialFailed { address: Multiaddr, reason: String },
    PeerAuthenticated { peer_id: PeerId, role: PeerRole },
    PeerDisconnected(PeerId),
    BlockReceived(Vec<u8>),
    TransactionReceived(Vec<u8>),
    ConsensusMessageReceived(Vec<u8>),
}

pub struct NetworkService {
    pub command_tx: mpsc::Sender<NetworkCommand>,
    pub event_rx: mpsc::Receiver<NetworkEvent>,
    pub local_peer_id: PeerId,
}

impl NetworkService {
    /// Fail-closed compatibility shim for callers that have not migrated to
    /// the context-bound network entry point.
    #[deprecated(note = "use start_with_context with ChainContext and PeerRole")]
    pub async fn start(config: NetworkConfig, keypair: Keypair) -> Result<Self> {
        let _ = (config, keypair);
        Err(anyhow!(
            "network startup requires ChainContext and PeerRole; use start_with_context"
        ))
    }

    pub async fn start_with_context(
        config: NetworkConfig,
        keypair: Keypair,
        context: ChainContext,
        peer_role: PeerRole,
    ) -> Result<Self> {
        Self::start_internal(config, keypair, context, peer_role).await
    }

    async fn start_internal(
        config: NetworkConfig,
        keypair: Keypair,
        context: ChainContext,
        peer_role: PeerRole,
    ) -> Result<Self> {
        let local_peer_id = PeerId::from(keypair.public());
        info!("Local peer id: {:?}", local_peer_id);

        let bootstrap_peers = config
            .bootstrap_peers
            .iter()
            .map(|address| {
                let parsed = address
                    .parse::<Multiaddr>()
                    .map_err(|error| anyhow!("invalid bootstrap multiaddr {address:?}: {error}"))?;
                if !parsed
                    .iter()
                    .any(|protocol| matches!(protocol, libp2p::multiaddr::Protocol::P2p(_)))
                {
                    return Err(anyhow!(
                        "bootstrap multiaddr {address:?} must include /p2p/<PeerId>"
                    ));
                }
                Ok(parsed)
            })
            .collect::<Result<Vec<_>>>()?;

        let transport = build_transport(&keypair)?;
        let behaviour = build_behaviour(&keypair, &local_peer_id, &config)?;

        let mut swarm = SwarmBuilder::with_existing_identity(keypair.clone())
            .with_tokio()
            .with_other_transport(|_| transport)
            .expect("Failed to build transport")
            .with_behaviour(|_| behaviour)
            .expect("Failed to build behaviour")
            .build();

        swarm.listen_on(config.listen_address.parse()?)?;

        let (command_tx, command_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);

        let event_loop = EventLoop::new_with_context(
            swarm,
            command_rx,
            event_tx,
            context,
            peer_role,
            local_peer_id,
            bootstrap_peers,
        );

        tokio::spawn(event_loop.run());

        Ok(Self {
            command_tx,
            event_rx,
            local_peer_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkConfig;
    use norn_common::consensus_types::{ConsensusEnvelope, ConsensusMessage, SignedVote, VoteStep};
    use norn_common::types::{
        BlockId, ChainId, Hash, ProtocolVersion, StakeSnapshotHash, ValidatorId,
    };
    use tokio::time::{timeout, Duration};

    fn context(genesis_byte: u8) -> ChainContext {
        ChainContext::new(
            2,
            ProtocolVersion(2),
            ChainId(Hash([7u8; 32])),
            Hash([genesis_byte; 32]),
        )
    }

    fn network_config() -> NetworkConfig {
        NetworkConfig {
            listen_address: "/ip4/127.0.0.1/tcp/0".to_string(),
            bootstrap_peers: Vec::new(),
            mdns: false,
        }
    }

    fn dial_address(address: Multiaddr, peer_id: PeerId) -> Multiaddr {
        address.with(libp2p::multiaddr::Protocol::P2p(peer_id))
    }

    async fn next_listen_address(events: &mut mpsc::Receiver<NetworkEvent>) -> Multiaddr {
        timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await {
                    Some(NetworkEvent::Listening(address)) => return address,
                    Some(_) => continue,
                    None => panic!("network event stream closed before listening"),
                }
            }
        })
        .await
        .expect("network did not report a listen address")
    }

    async fn wait_for_peer_network_state(
        events: &mut mpsc::Receiver<NetworkEvent>,
        expected_peers: &[PeerId],
    ) -> std::result::Result<Vec<(PeerId, PeerRole)>, Vec<String>> {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut connected = Vec::new();
        let mut authenticated = Vec::new();
        let mut observed = Vec::new();
        loop {
            if expected_peers
                .iter()
                .all(|peer_id| connected.contains(peer_id))
                && expected_peers
                    .iter()
                    .all(|peer_id| authenticated.iter().any(|(id, _)| id == peer_id))
            {
                return Ok(authenticated);
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(observed);
            }
            match timeout(remaining, events.recv()).await {
                Ok(Some(event)) => {
                    observed.push(format!("{event:?}"));
                    match event {
                        NetworkEvent::PeerConnected(peer_id) => {
                            if !connected.contains(&peer_id) {
                                connected.push(peer_id);
                            }
                        }
                        NetworkEvent::PeerAuthenticated { peer_id, role } => {
                            if !authenticated.iter().any(|(id, _)| *id == peer_id) {
                                authenticated.push((peer_id, role));
                            }
                        }
                        _ => {}
                    }
                }
                Ok(None) | Err(_) => return Err(observed),
            }
        }
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
            validator: ValidatorId([validator_byte; 32]),
            signature: [3u8; 64],
        };
        bincode::serialize(&ConsensusEnvelope {
            wire_version: context.wire_version,
            protocol_version: context.protocol_version,
            chain_id: context.chain_id,
            genesis_hash: context.genesis_hash,
            payload: ConsensusMessage::Vote(vote),
        })
        .expect("consensus envelope serializes")
    }

    #[tokio::test]
    async fn stage7_authenticates_valid_peers_and_rejects_wrong_role_and_context() {
        let valid_context = context(1);
        let mut validator = NetworkService::start_with_context(
            network_config(),
            Keypair::generate_ed25519(),
            valid_context,
            PeerRole::Validator,
        )
        .await
        .expect("validator network starts");
        let validator_address = next_listen_address(&mut validator.event_rx).await;

        let mut peer = NetworkService::start_with_context(
            network_config(),
            Keypair::generate_ed25519(),
            valid_context,
            PeerRole::Validator,
        )
        .await
        .expect("validator peer network starts");
        let _ = next_listen_address(&mut peer.event_rx).await;
        peer.command_tx
            .send(NetworkCommand::Dial(dial_address(
                validator_address.clone(),
                validator.local_peer_id,
            )))
            .await
            .expect("dial command accepted");

        let mut full_node = NetworkService::start_with_context(
            network_config(),
            Keypair::generate_ed25519(),
            valid_context,
            PeerRole::FullNode,
        )
        .await
        .expect("full node network starts");
        let _ = next_listen_address(&mut full_node.event_rx).await;
        full_node
            .command_tx
            .send(NetworkCommand::Dial(dial_address(
                validator_address.clone(),
                validator.local_peer_id,
            )))
            .await
            .expect("full node dial command accepted");

        let authenticated = wait_for_peer_network_state(
            &mut validator.event_rx,
            &[peer.local_peer_id, full_node.local_peer_id],
        )
        .await
        .unwrap_or_else(|observed| panic!("peers did not authenticate; observed {observed:?}"));
        assert!(authenticated
            .iter()
            .any(|(peer_id, role)| *peer_id == peer.local_peer_id && *role == PeerRole::Validator));
        assert!(authenticated.iter().any(|(peer_id, role)| {
            *peer_id == full_node.local_peer_id && *role == PeerRole::FullNode
        }));

        peer.command_tx
            .send(NetworkCommand::BroadcastConsensus(consensus_message(
                valid_context,
                1,
            )))
            .await
            .expect("validator broadcast command accepted");
        let received = timeout(Duration::from_secs(5), async {
            loop {
                match validator.event_rx.recv().await {
                    Some(NetworkEvent::ConsensusMessageReceived(data)) => return data,
                    Some(_) => continue,
                    None => panic!("validator event stream closed"),
                }
            }
        })
        .await
        .expect("validator consensus message was not delivered");
        assert_eq!(received, consensus_message(valid_context, 1));

        full_node
            .command_tx
            .send(NetworkCommand::BroadcastConsensus(consensus_message(
                valid_context,
                3,
            )))
            .await
            .expect("full node broadcast command accepted");
        let wrong_role_delivery = timeout(Duration::from_secs(3), async {
            loop {
                match validator.event_rx.recv().await {
                    Some(NetworkEvent::ConsensusMessageReceived(data)) => return Some(data),
                    Some(_) => continue,
                    None => return None,
                }
            }
        })
        .await;
        assert!(
            wrong_role_delivery.is_err(),
            "FullNode consensus reached validator"
        );

        let mut wrong_context_peer = NetworkService::start_with_context(
            network_config(),
            Keypair::generate_ed25519(),
            context(2),
            PeerRole::Validator,
        )
        .await
        .expect("wrong-context network starts");
        let wrong_address = next_listen_address(&mut wrong_context_peer.event_rx).await;
        wrong_context_peer
            .command_tx
            .send(NetworkCommand::Dial(dial_address(
                validator_address,
                validator.local_peer_id,
            )))
            .await
            .expect("wrong-context dial command accepted");
        let wrong_context_auth = timeout(Duration::from_secs(3), async {
            loop {
                match validator.event_rx.recv().await {
                    Some(NetworkEvent::PeerAuthenticated { peer_id, .. })
                        if peer_id == wrong_context_peer.local_peer_id =>
                    {
                        return Some(peer_id)
                    }
                    Some(_) => continue,
                    None => return None,
                }
            }
        })
        .await;
        let _ = wrong_address;
        assert!(
            wrong_context_auth.is_err(),
            "wrong Genesis peer was authenticated"
        );
    }
}
