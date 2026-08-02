use super::service::{NetworkCommand, NetworkEvent};
use crate::behaviour::NornBehaviour;
use crate::topics::Topics;
use libp2p::futures::StreamExt;
use libp2p::PeerId;
use libp2p::{gossipsub, kad, mdns, Swarm};
use norn_common::chain_context::{
    ChainContext, NetworkHandshake, PeerRole, MAX_BLOCK_MESSAGE_BYTES, MAX_HANDSHAKE_BYTES,
    MAX_TRANSACTION_MESSAGE_BYTES,
};
use norn_common::consensus_types::ConsensusEnvelope;
use norn_common::types::TransactionV2;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

pub struct EventLoop {
    swarm: Swarm<NornBehaviour>,
    command_rx: mpsc::Receiver<NetworkCommand>,
    event_tx: mpsc::Sender<NetworkEvent>,
    topics: Topics,
    context: ChainContext,
    handshake: NetworkHandshake,
    bootstrap_peers: Vec<libp2p::Multiaddr>,
    authenticated_peers: HashMap<PeerId, PeerRole>,
}

impl EventLoop {
    pub fn new_with_context(
        swarm: Swarm<NornBehaviour>,
        command_rx: mpsc::Receiver<NetworkCommand>,
        event_tx: mpsc::Sender<NetworkEvent>,
        context: ChainContext,
        peer_role: PeerRole,
        local_peer_id: PeerId,
        bootstrap_peers: Vec<libp2p::Multiaddr>,
    ) -> Self {
        Self {
            swarm,
            command_rx,
            event_tx,
            topics: Topics::for_context(&context),
            context,
            handshake: NetworkHandshake::new(context, peer_role)
                .with_peer_id(local_peer_id.to_bytes()),
            bootstrap_peers,
            authenticated_peers: HashMap::new(),
        }
    }

    pub async fn run(mut self) {
        // Subscribe to topics
        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topics.block);
        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topics.transaction);
        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topics.consensus);
        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topics.handshake);

        let mut handshake_tick = tokio::time::interval(Duration::from_secs(5));
        self.publish_handshake();
        for address in self.bootstrap_peers.clone() {
            if let Err(error) = self.swarm.dial(address.clone()) {
                error!("Bootstrap dial to {:?} failed: {:?}", address, error);
                let _ = self.event_tx.try_send(NetworkEvent::DialFailed {
                    address,
                    reason: format!("{error:?}"),
                });
            }
        }

        loop {
            tokio::select! {
                event = self.swarm.next() => {
                    self.handle_swarm_event(event).await;
                }
                command = self.command_rx.recv() => {
                    match command {
                        Some(cmd) => self.handle_command(cmd).await,
                        None => break,
                    }
                }
                _ = handshake_tick.tick() => {
                    self.publish_handshake();
                }
            }
        }
    }

    fn publish_handshake(&mut self) {
        let handshake = self.handshake.clone();
        match bincode::serialize(&handshake) {
            Ok(data) if data.len() <= MAX_HANDSHAKE_BYTES => {
                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.topics.handshake.clone(), data)
                {
                    debug!("Handshake broadcast deferred: {:?}", e);
                }
            }
            Ok(_) => error!("Local handshake exceeds the wire byte limit"),
            Err(e) => error!("Failed to encode local handshake: {}", e),
        }
    }

    async fn handle_command(&mut self, command: NetworkCommand) {
        match command {
            NetworkCommand::BroadcastBlock(data) => {
                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.topics.block.clone(), data)
                {
                    error!("Broadcast block failed: {:?}", e);
                }
            }
            NetworkCommand::BroadcastTransaction(data) => {
                if let Err(e) = TransactionV2::decode_and_validate(&data, &self.context) {
                    error!("Refused to broadcast invalid TransactionV2: {}", e);
                    return;
                }
                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.topics.transaction.clone(), data)
                {
                    error!("Broadcast transaction failed: {:?}", e);
                }
            }
            NetworkCommand::BroadcastConsensus(data) => {
                if let Err(e) = ConsensusEnvelope::decode_and_validate(&data, &self.context) {
                    error!("Refused to broadcast invalid consensus envelope: {}", e);
                    return;
                }
                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.topics.consensus.clone(), data)
                {
                    error!("Broadcast consensus failed: {:?}", e);
                }
            }
            NetworkCommand::Dial(address) => {
                if !address
                    .iter()
                    .any(|protocol| matches!(protocol, libp2p::multiaddr::Protocol::P2p(_)))
                {
                    let _ = self.event_tx.try_send(NetworkEvent::DialFailed {
                        address,
                        reason: "dial address must include /p2p/<PeerId>".to_string(),
                    });
                    return;
                }
                if let Err(error) = self.swarm.dial(address.clone()) {
                    error!("Dial to {:?} failed: {:?}", address, error);
                    let _ = self.event_tx.try_send(NetworkEvent::DialFailed {
                        address,
                        reason: error.to_string(),
                    });
                }
            }
            NetworkCommand::StartListening => {
                // Handled via external setup or if we want to start listener dynamically
            }
        }
    }

    async fn handle_swarm_event(
        &mut self,
        event: Option<libp2p::swarm::SwarmEvent<crate::behaviour::NornBehaviourEvent>>,
    ) {
        // Simplified handling
        match event {
            Some(libp2p::swarm::SwarmEvent::Behaviour(
                crate::behaviour::NornBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                    propagation_source,
                    message_id: _,
                    message,
                }),
            )) => {
                if message.topic == self.topics.handshake.hash() {
                    self.handle_handshake(propagation_source, &message.data);
                } else if message.topic == self.topics.block.hash() {
                    if message.data.len() > MAX_BLOCK_MESSAGE_BYTES {
                        debug!(
                            "Dropped oversized block message from {:?}",
                            propagation_source
                        );
                        return;
                    }
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::BlockReceived(message.data))
                        .await;
                } else if message.topic == self.topics.transaction.hash() {
                    if message.data.len() > MAX_TRANSACTION_MESSAGE_BYTES {
                        debug!(
                            "Dropped oversized transaction message from {:?}",
                            propagation_source
                        );
                        return;
                    }
                    if let Err(e) = TransactionV2::decode_and_validate(&message.data, &self.context)
                    {
                        debug!("Dropped invalid TransactionV2: {}", e);
                        return;
                    }
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::TransactionReceived(message.data))
                        .await;
                } else if message.topic == self.topics.consensus.hash() {
                    if self.authenticated_peers.get(&propagation_source)
                        != Some(&PeerRole::Validator)
                    {
                        debug!(
                            "Dropped consensus message from unauthenticated or non-validator peer {:?}",
                            propagation_source
                        );
                        return;
                    }
                    if let Err(e) =
                        ConsensusEnvelope::decode_and_validate(&message.data, &self.context)
                    {
                        debug!("Dropped invalid consensus envelope: {}", e);
                        return;
                    }
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::ConsensusMessageReceived(message.data))
                        .await;
                }
            }
            Some(libp2p::swarm::SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .add_explicit_peer(&peer_id);
                let _ = self.event_tx.try_send(NetworkEvent::PeerConnected(peer_id));
                // The first handshake may have been published before the
                // connection existed. Publish again on every reconnect so a
                // peer can re-authenticate after its session state is reset.
                self.publish_handshake();
                debug!("Connection established with {:?}", peer_id);
            }
            Some(libp2p::swarm::SwarmEvent::ConnectionClosed { peer_id, .. }) => {
                self.authenticated_peers.remove(&peer_id);
                let _ = self
                    .event_tx
                    .send(NetworkEvent::PeerDisconnected(peer_id))
                    .await;
            }
            Some(libp2p::swarm::SwarmEvent::Behaviour(
                crate::behaviour::NornBehaviourEvent::Mdns(mdns::Event::Discovered(list)),
            )) => {
                for (peer_id, addr) in list {
                    debug!("mDNS discovered peer: {:?} at {:?}", peer_id, addr);
                    // Also add to Kademlia routing table
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr);
                }
            }
            Some(libp2p::swarm::SwarmEvent::Behaviour(
                crate::behaviour::NornBehaviourEvent::Mdns(mdns::Event::Expired(list)),
            )) => {
                for (peer_id, _) in list {
                    debug!("mDNS peer expired: {:?}", peer_id);
                }
            }
            Some(libp2p::swarm::SwarmEvent::Behaviour(
                crate::behaviour::NornBehaviourEvent::Kademlia(kad::Event::RoutingUpdated {
                    peer,
                    is_new_peer: true,
                    ..
                }),
            )) => {
                debug!("Kademlia routing updated (new peer): {:?}", peer);
            }
            Some(libp2p::swarm::SwarmEvent::Behaviour(
                crate::behaviour::NornBehaviourEvent::Kademlia(kad::Event::RoutablePeer {
                    peer,
                    address,
                }),
            )) => {
                debug!("Kademlia routable peer: {:?} at {:?}", peer, address);
            }
            Some(libp2p::swarm::SwarmEvent::NewListenAddr { address, .. }) => {
                info!("Listening on {:?}", address);
                let _ = self.event_tx.send(NetworkEvent::Listening(address)).await;
            }
            _ => {}
        }
    }

    fn handle_handshake(&mut self, peer: PeerId, data: &[u8]) {
        if data.is_empty() || data.len() > MAX_HANDSHAKE_BYTES {
            debug!("Rejected oversized or empty handshake from {:?}", peer);
            return;
        }
        let handshake = match bincode::deserialize::<NetworkHandshake>(data) {
            Ok(handshake) => handshake,
            Err(e) => {
                debug!("Rejected malformed handshake from {:?}: {}", peer, e);
                return;
            }
        };
        if let Err(e) = handshake.validate_for_context(&self.context) {
            debug!("Rejected handshake from {:?}: {}", peer, e);
            return;
        }
        let advertised_peer = match PeerId::from_bytes(&handshake.peer_id) {
            Ok(peer_id) if peer_id == peer => peer_id,
            _ => {
                debug!(
                    "Rejected handshake from {:?}: advertised PeerId does not match transport source",
                    peer
                );
                return;
            }
        };
        self.authenticated_peers
            .insert(advertised_peer, handshake.peer_role);
        let _ = self.event_tx.try_send(NetworkEvent::PeerAuthenticated {
            peer_id: advertised_peer,
            role: handshake.peer_role,
        });
        info!("Authenticated {:?} as {:?}", peer, handshake.peer_role);
    }
}
