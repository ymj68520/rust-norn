use super::service::{NetworkAuthConfig, NetworkCommand, NetworkEvent};
use crate::behaviour::NornBehaviour;
use crate::topics::Topics;
use k256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use libp2p::futures::StreamExt;
use libp2p::PeerId;
use libp2p::{gossipsub, kad, Swarm};
use norn_common::chain_context::{
    ChainContext, HandshakeMessage, NetworkHandshake, PeerRole, MAX_BLOCK_MESSAGE_BYTES,
    MAX_HANDSHAKE_BYTES, MAX_TRANSACTION_MESSAGE_BYTES,
};
use norn_common::consensus_types::{ConsensusEnvelope, ConsensusMessage};
use norn_common::types::TransactionV2;
use rand::RngCore;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

const CONSENSUS_GOSSIP_MAGIC: &[u8] = b"NORN_CONSENSUS_GOSSIP_V2";
const CONSENSUS_GOSSIP_NONCE_BYTES: usize = std::mem::size_of::<u64>();
const MAX_REPLAYED_CONSENSUS: usize = 64;
const VALIDATOR_CHALLENGE_TTL: Duration = Duration::from_secs(10);
const MAX_PENDING_VALIDATOR_CHALLENGES: usize = 256;
/// Hard protocol ceiling for simultaneously connected peer identities. The
/// Genesis resource limits govern consensus work; this transport ceiling is
/// applied before any peer can consume handshake or verification capacity.
const MAX_CONNECTED_PEERS: usize = 128;
const SYNC_REQUEST_WINDOW: Duration = Duration::from_secs(1);
const MAX_SYNC_REQUESTS_PER_PEER: usize = 16;
const INITIAL_HANDSHAKE_DELAY: Duration = Duration::from_millis(100);
const HANDSHAKE_RETRY_DELAY: Duration = Duration::from_secs(1);
const BOOTSTRAP_DIAL_RETRY_DELAY: Duration = Duration::from_secs(3);

struct PendingChallenge {
    nonce: [u8; 32],
    expires_at: Instant,
}

fn challenge_slot_available(
    pending: &mut HashMap<PeerId, PendingChallenge>,
    peer: &PeerId,
    now: Instant,
) -> bool {
    pending.retain(|_, challenge| challenge.expires_at > now);
    pending.contains_key(peer) || pending.len() < MAX_PENDING_VALIDATOR_CHALLENGES
}

fn connection_slot_available(connected: &HashSet<PeerId>, peer: &PeerId) -> bool {
    connected.contains(peer) || connected.len() < MAX_CONNECTED_PEERS
}

fn sync_request_allowed(
    budget: &mut HashMap<PeerId, VecDeque<Instant>>,
    peer: PeerId,
    now: Instant,
) -> bool {
    let timestamps = budget.entry(peer).or_default();
    while timestamps
        .front()
        .is_some_and(|timestamp| now.saturating_duration_since(*timestamp) >= SYNC_REQUEST_WINDOW)
    {
        timestamps.pop_front();
    }
    if timestamps.len() >= MAX_SYNC_REQUESTS_PER_PEER {
        return false;
    }
    timestamps.push_back(now);
    true
}

pub struct EventLoop {
    swarm: Swarm<NornBehaviour>,
    command_rx: mpsc::Receiver<NetworkCommand>,
    event_tx: mpsc::Sender<NetworkEvent>,
    topics: Topics,
    context: ChainContext,
    handshake: NetworkHandshake,
    bootstrap_peers: Vec<libp2p::Multiaddr>,
    authenticated_peers: HashMap<PeerId, PeerRole>,
    pending_consensus: VecDeque<Vec<u8>>,
    published_consensus: VecDeque<Vec<u8>>,
    connected_peers: HashSet<PeerId>,
    dialing_bootstrap_peers: HashMap<PeerId, Instant>,
    handshake_pending: bool,
    handshake_due: Option<Instant>,
    consensus_publish_nonce: u64,
    auth: NetworkAuthConfig,
    pending_challenges: HashMap<PeerId, PendingChallenge>,
    sync_request_budget: HashMap<PeerId, VecDeque<Instant>>,
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
        Self::new_with_context_and_auth(
            swarm,
            command_rx,
            event_tx,
            context,
            peer_role,
            local_peer_id,
            bootstrap_peers,
            NetworkAuthConfig::default(),
        )
    }

    pub fn new_with_context_and_auth(
        swarm: Swarm<NornBehaviour>,
        command_rx: mpsc::Receiver<NetworkCommand>,
        event_tx: mpsc::Sender<NetworkEvent>,
        context: ChainContext,
        peer_role: PeerRole,
        local_peer_id: PeerId,
        bootstrap_peers: Vec<libp2p::Multiaddr>,
        auth: NetworkAuthConfig,
    ) -> Self {
        // Gossipsub retains message IDs across a reconnect for a bounded
        // period.  A process restart must therefore not recreate the same
        // first handshake payload, otherwise the remote peer can keep the
        // restarted validator unauthenticated and queue its consensus
        // messages indefinitely.
        let session_nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or_default();
        Self {
            swarm,
            command_rx,
            event_tx,
            topics: Topics::for_context(&context),
            context,
            handshake: NetworkHandshake::new(context, peer_role)
                .with_peer_id(local_peer_id.to_bytes())
                .with_session_nonce(session_nonce),
            bootstrap_peers,
            authenticated_peers: HashMap::new(),
            pending_consensus: VecDeque::new(),
            published_consensus: VecDeque::new(),
            connected_peers: HashSet::new(),
            dialing_bootstrap_peers: HashMap::new(),
            handshake_pending: false,
            handshake_due: None,
            consensus_publish_nonce: 0,
            auth,
            pending_challenges: HashMap::new(),
            sync_request_budget: HashMap::new(),
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

        let mut consensus_tick = tokio::time::interval(Duration::from_secs(1));
        let mut handshake_tick = tokio::time::interval(Duration::from_millis(500));
        let mut bootstrap_retry_tick = tokio::time::interval(Duration::from_secs(1));
        self.dial_bootstrap_peers();

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
                _ = consensus_tick.tick() => {
                    self.flush_pending_consensus();
                }
                _ = handshake_tick.tick() => {
                    self.flush_pending_handshake();
                }
                _ = bootstrap_retry_tick.tick() => {
                    self.dial_bootstrap_peers();
                }
            }
        }
    }

    fn dial_bootstrap_peers(&mut self) {
        let now = Instant::now();
        self.dialing_bootstrap_peers.retain(|_, started| {
            now.saturating_duration_since(*started) < BOOTSTRAP_DIAL_RETRY_DELAY
        });
        for address in self.bootstrap_peers.clone() {
            let Some(peer_id) = address.iter().find_map(|protocol| {
                if let libp2p::multiaddr::Protocol::P2p(peer_id) = protocol {
                    Some(peer_id)
                } else {
                    None
                }
            }) else {
                continue;
            };
            if self.connected_peers.contains(&peer_id)
                || self.dialing_bootstrap_peers.contains_key(&peer_id)
            {
                continue;
            }
            self.dialing_bootstrap_peers.insert(peer_id, now);
            if let Err(error) = self.swarm.dial(address.clone()) {
                self.dialing_bootstrap_peers.remove(&peer_id);
                error!("Bootstrap dial to {:?} failed: {:?}", address, error);
                let _ = self.event_tx.try_send(NetworkEvent::DialFailed {
                    address,
                    reason: format!("{error:?}"),
                });
            }
        }
    }

    fn publish_handshake(&mut self) -> bool {
        self.publish_handshake_message(self.handshake.clone())
    }

    fn publish_handshake_message(&mut self, handshake: NetworkHandshake) -> bool {
        match bincode::serialize(&handshake) {
            Ok(data) if data.len() <= MAX_HANDSHAKE_BYTES => {
                match self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.topics.handshake.clone(), data)
                {
                    Ok(_) => true,
                    Err(e) => {
                        debug!("Handshake broadcast deferred: {:?}", e);
                        false
                    }
                }
            }
            Ok(_) => {
                error!("Local handshake exceeds the wire byte limit");
                false
            }
            Err(e) => {
                error!("Failed to encode local handshake: {}", e);
                false
            }
        }
    }

    fn flush_pending_handshake(&mut self) {
        if !self.handshake_pending || self.connected_peers.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.handshake_due.is_some_and(|due| due > now) {
            return;
        }
        debug!(
            "Attempting handshake broadcast to {} connected peers",
            self.connected_peers.len()
        );
        // A successful local gossipsub publish only means that the message
        // entered this node's router.  A peer can still miss it while its
        // subscription is converging, and authentication is directional: a
        // peer proving us does not prove that it also received our identity.
        // Keep refreshing Hello while the transport is connected so either
        // side can repair a one-way handshake.  Authenticated peers ignore
        // repeated Hello messages, so this remains bounded control traffic.
        self.handshake.session_nonce = self.handshake.session_nonce.wrapping_add(1);
        if self.publish_handshake() {
            debug!("Handshake broadcast accepted by gossipsub");
            self.handshake_due = Some(now + HANDSHAKE_RETRY_DELAY);
        } else {
            self.handshake_due = Some(now + HANDSHAKE_RETRY_DELAY);
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
                let envelope = match ConsensusEnvelope::decode_and_validate(&data, &self.context) {
                    Ok(envelope) => envelope,
                    Err(e) => {
                        error!("Refused to broadcast invalid consensus envelope: {}", e);
                        return;
                    }
                };
                if !local_consensus_broadcast_allowed(self.handshake.peer_role, &envelope.payload) {
                    error!("Refused FullNode consensus broadcast that is not a V2 sync request");
                    return;
                }
                self.publish_or_queue_consensus(data);
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
                    self.handle_handshake(propagation_source, &message.data)
                        .await;
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
                    let Some(consensus_data) = Self::decode_consensus_gossip(&message.data) else {
                        debug!("Dropped malformed or unsupported consensus gossip frame");
                        return;
                    };
                    if let Err(e) =
                        ConsensusEnvelope::decode_and_validate(consensus_data, &self.context)
                    {
                        debug!("Dropped invalid consensus envelope: {}", e);
                        return;
                    }
                    debug!(
                        "Received validated consensus message from {:?} ({} bytes)",
                        propagation_source,
                        consensus_data.len()
                    );
                    // `propagation_source` is only the direct gossipsub relay
                    // peer. It is not the original publisher and therefore
                    // must not be used as the consensus identity. Deliver
                    // context-valid V2 messages to the node, where the
                    // proposal/vote/certificate signatures and snapshot
                    // membership are verified. This also keeps a valid
                    // Validator message working when it is relayed through a
                    // FullNode.
                    let decoded_envelope = match bincode::deserialize::<ConsensusEnvelope>(
                        consensus_data,
                    ) {
                        Ok(envelope) => envelope,
                        Err(error) => {
                            debug!(
                                "Dropped consensus envelope that could not be decoded after validation: {}",
                                error
                            );
                            return;
                        }
                    };
                    if matches!(
                        decoded_envelope.payload,
                        norn_common::consensus_types::ConsensusMessage::Proposal { .. }
                    ) {
                        debug!(
                            "UnsupportedProtocolVersion: dropped legacy consensus Proposal at V2 network ingress"
                        );
                        return;
                    }
                    if matches!(
                        decoded_envelope.payload,
                        ConsensusMessage::BlockRequest { .. }
                            | ConsensusMessage::FinalityRequest { .. }
                    ) && !sync_request_allowed(
                        &mut self.sync_request_budget,
                        propagation_source,
                        Instant::now(),
                    ) {
                        debug!(
                            "Dropped sync request from {:?}: per-peer request budget exhausted",
                            propagation_source
                        );
                        return;
                    }
                    if let Err(error) =
                        self.event_tx
                            .try_send(NetworkEvent::ConsensusMessageReceived(
                                consensus_data.to_vec(),
                            ))
                    {
                        debug!(
                            "Dropped context-valid consensus message after node ingress queue filled: {}",
                            error
                        );
                    }
                }
            }
            Some(libp2p::swarm::SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                ..
            }) => {
                self.dialing_bootstrap_peers.remove(&peer_id);
                if !connection_slot_available(&self.connected_peers, &peer_id) {
                    debug!(
                        "Rejected peer {:?}: connection identity limit {} reached",
                        peer_id, MAX_CONNECTED_PEERS
                    );
                    let _ = self.swarm.close_connection(connection_id);
                    return;
                }
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .add_explicit_peer(&peer_id);
                self.handshake.session_nonce = self.handshake.session_nonce.wrapping_add(1);
                self.connected_peers.insert(peer_id);
                self.replay_published_consensus();
                self.handshake_pending = true;
                self.handshake_due = Some(Instant::now() + INITIAL_HANDSHAKE_DELAY);
                let _ = self.event_tx.try_send(NetworkEvent::PeerConnected(peer_id));
                debug!("Connection established with {:?}", peer_id);
            }
            Some(libp2p::swarm::SwarmEvent::ConnectionClosed { peer_id, .. }) => {
                self.dialing_bootstrap_peers.remove(&peer_id);
                self.connected_peers.remove(&peer_id);
                self.authenticated_peers.remove(&peer_id);
                self.pending_challenges.remove(&peer_id);
                self.sync_request_budget.remove(&peer_id);
                if self.connected_peers.is_empty() {
                    self.handshake_pending = false;
                    self.handshake_due = None;
                }
                let _ = self
                    .event_tx
                    .send(NetworkEvent::PeerDisconnected(peer_id))
                    .await;
            }
            Some(libp2p::swarm::SwarmEvent::OutgoingConnectionError { peer_id, error, .. }) => {
                if let Some(peer_id) = peer_id {
                    self.dialing_bootstrap_peers.remove(&peer_id);
                }
                debug!("Outbound connection failed for {:?}: {:?}", peer_id, error);
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

    async fn handle_handshake(&mut self, peer: PeerId, data: &[u8]) {
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
            Ok(peer_id) => peer_id,
            Err(_) => {
                debug!(
                    "Rejected handshake from {:?}: invalid advertised PeerId",
                    peer
                );
                return;
            }
        };
        match handshake.message {
            HandshakeMessage::Hello if handshake.peer_role == PeerRole::Validator => {
                // Validator handshakes are gossipsub messages and can be
                // delivered more than once or out of order.  Do not replace
                // an unexpired challenge on every repeated Hello: a response
                // to the previous challenge would otherwise become invalid
                // before it reaches us.  Once authenticated, the peer does
                // not need another challenge until its transport reconnects.
                if self.authenticated_peers.contains_key(&advertised_peer) {
                    return;
                }
                let now = Instant::now();
                if !challenge_slot_available(&mut self.pending_challenges, &advertised_peer, now) {
                    debug!(
                        "Rejected validator Hello while challenge table is full from {:?}",
                        peer
                    );
                    return;
                }
                let nonce = match self.pending_challenges.get(&advertised_peer) {
                    Some(challenge) if challenge.expires_at > now => challenge.nonce,
                    _ => {
                        let mut nonce = [0u8; 32];
                        rand::rngs::OsRng.fill_bytes(&mut nonce);
                        nonce
                    }
                };
                self.pending_challenges.insert(
                    advertised_peer,
                    PendingChallenge {
                        nonce,
                        expires_at: now + VALIDATOR_CHALLENGE_TTL,
                    },
                );
                let challenge = NetworkHandshake::challenge(
                    self.context,
                    self.handshake.peer_id.clone(),
                    handshake.peer_id,
                    nonce,
                );
                if !self.publish_handshake_message(challenge) {
                    debug!("Failed to publish validator challenge to {:?}", peer);
                }
                debug!("Issued validator handshake challenge to {:?}", peer);
            }
            HandshakeMessage::Hello => {
                if advertised_peer != peer {
                    debug!(
                        "Rejected FullNode handshake from {:?}: advertised PeerId is not the direct transport peer",
                        peer
                    );
                    return;
                }
                self.mark_authenticated(advertised_peer, handshake.peer_role)
                    .await;
            }
            HandshakeMessage::Challenge => {
                let local_peer = self.handshake.peer_id.clone();
                if handshake.peer_id != local_peer || handshake.peer_role != PeerRole::FullNode {
                    debug!("Rejected validator challenge addressed to another peer");
                    return;
                }
                if PeerId::from_bytes(&handshake.receiver_peer_id).is_err() {
                    debug!("Rejected validator challenge with invalid challenger PeerId");
                    return;
                }
                let Some(identity) = self.auth.local_validator.as_ref() else {
                    debug!("Ignoring validator challenge without local validator credentials");
                    return;
                };
                let Some(nonce) = handshake.receiver_nonce else {
                    debug!("Rejected validator challenge without a receiver nonce");
                    return;
                };
                let mut response = NetworkHandshake::new(self.context, PeerRole::Validator);
                response.message = HandshakeMessage::Response;
                response.peer_id = local_peer;
                response.receiver_peer_id = handshake.receiver_peer_id;
                response.receiver_nonce = Some(nonce);
                response.validator_id = Some(identity.validator_id);
                response.consensus_public_key = Some(identity.consensus_public_key.to_vec());
                let signature = match (identity.sign)(&response.validator_signing_bytes()) {
                    Ok(signature) => signature,
                    Err(error) => {
                        debug!("Failed to sign validator handshake response: {error}");
                        return;
                    }
                };
                response.signature = Some(signature.to_vec());
                if !self.publish_handshake_message(response) {
                    debug!(
                        "Failed to publish validator handshake response to {:?}",
                        peer
                    );
                }
            }
            HandshakeMessage::Response => {
                let local_peer = self.handshake.peer_id.clone();
                let Some(challenge) = self.pending_challenges.get(&advertised_peer) else {
                    debug!(
                        "Rejected unsolicited validator handshake response from {:?}",
                        peer
                    );
                    return;
                };
                if Instant::now() > challenge.expires_at {
                    self.pending_challenges.remove(&advertised_peer);
                    debug!(
                        "Rejected expired validator handshake response from {:?}",
                        peer
                    );
                    return;
                }
                if handshake.peer_role != PeerRole::Validator
                    || handshake.receiver_peer_id != local_peer
                    || handshake.receiver_nonce != Some(challenge.nonce)
                {
                    debug!(
                        "Rejected validator handshake response binding from {:?}",
                        peer
                    );
                    return;
                }
                let Some(validator_id) = handshake.validator_id else {
                    debug!(
                        "Rejected incomplete validator handshake response from {:?}",
                        peer
                    );
                    return;
                };
                let Some(public_key_bytes) = handshake.consensus_public_key.as_deref() else {
                    debug!(
                        "Rejected incomplete validator handshake response from {:?}",
                        peer
                    );
                    return;
                };
                let Ok(public_key) = <[u8; 33]>::try_from(public_key_bytes) else {
                    debug!(
                        "Rejected validator response with invalid key length from {:?}",
                        peer
                    );
                    return;
                };
                if self.auth.validator_public_keys.get(&validator_id) != Some(&public_key) {
                    debug!(
                        "Rejected validator response with unknown Genesis key from {:?}",
                        peer
                    );
                    return;
                }
                let verifying_key = match VerifyingKey::from_sec1_bytes(&public_key) {
                    Ok(key) => key,
                    Err(_) => {
                        debug!("Rejected malformed validator public key from {:?}", peer);
                        return;
                    }
                };
                let Some(signature_bytes) = handshake.signature.as_deref() else {
                    debug!(
                        "Rejected incomplete validator handshake response from {:?}",
                        peer
                    );
                    return;
                };
                let Ok(signature) = Signature::from_slice(signature_bytes) else {
                    debug!(
                        "Rejected malformed validator handshake signature from {:?}",
                        peer
                    );
                    return;
                };
                if signature.normalize_s().is_some()
                    || verifying_key
                        .verify(&handshake.validator_signing_bytes(), &signature)
                        .is_err()
                {
                    debug!(
                        "Rejected invalid validator handshake signature from {:?}",
                        peer
                    );
                    return;
                }
                self.pending_challenges.remove(&advertised_peer);
                self.mark_authenticated(advertised_peer, PeerRole::Validator)
                    .await;
            }
        }
    }

    async fn mark_authenticated(&mut self, peer: PeerId, role: PeerRole) {
        self.authenticated_peers.insert(peer, role);
        let _ = self.event_tx.try_send(NetworkEvent::PeerAuthenticated {
            peer_id: peer,
            role,
        });
        info!("Authenticated {:?} as {:?}", peer, role);
        if self.connected_peers.contains(&peer) {
            self.handshake_pending = true;
            if self.handshake_due.is_none() {
                self.handshake_due = Some(Instant::now() + HANDSHAKE_RETRY_DELAY);
            }
        }
    }

    fn publish_or_queue_consensus(&mut self, data: Vec<u8>) {
        // Publishing before a transport connection exists causes gossipsub to
        // remember the message id even though it returns InsufficientPeers.
        // A later retry is then incorrectly treated as a duplicate. Keep the
        // exact signed bytes until a connection is established instead.
        self.queue_consensus(data);
    }

    fn encode_consensus_gossip(&mut self, data: &[u8]) -> Vec<u8> {
        self.consensus_publish_nonce = self.consensus_publish_nonce.wrapping_add(1);
        let mut framed = Vec::with_capacity(
            CONSENSUS_GOSSIP_MAGIC.len() + CONSENSUS_GOSSIP_NONCE_BYTES + data.len(),
        );
        framed.extend_from_slice(CONSENSUS_GOSSIP_MAGIC);
        framed.extend_from_slice(&self.consensus_publish_nonce.to_be_bytes());
        framed.extend_from_slice(data);
        framed
    }

    fn decode_consensus_gossip(data: &[u8]) -> Option<&[u8]> {
        if data.starts_with(CONSENSUS_GOSSIP_MAGIC) {
            let payload_start = CONSENSUS_GOSSIP_MAGIC.len() + CONSENSUS_GOSSIP_NONCE_BYTES;
            return (data.len() > payload_start).then_some(&data[payload_start..]);
        }
        // Accept the unframed form during the transition so a V2 node can
        // still exchange envelopes with an older test harness. The envelope
        // validator remains the authority for protocol/version admission.
        Some(data)
    }

    fn replay_published_consensus(&mut self) {
        let replay = self
            .published_consensus
            .iter()
            .rev()
            .take(MAX_REPLAYED_CONSENSUS)
            .cloned()
            .collect::<Vec<_>>();
        for data in replay.into_iter().rev() {
            if self
                .pending_consensus
                .iter()
                .any(|pending| pending == &data)
            {
                continue;
            }
            if self.pending_consensus.len() >= 256 {
                self.pending_consensus.pop_front();
            }
            self.pending_consensus.push_back(data);
        }
    }

    fn queue_consensus(&mut self, data: Vec<u8>) {
        const MAX_PENDING_CONSENSUS: usize = 256;
        if self
            .published_consensus
            .iter()
            .any(|published| published == &data)
        {
            return;
        }
        if self
            .pending_consensus
            .iter()
            .any(|pending| pending == &data)
        {
            return;
        }
        if self.pending_consensus.len() >= MAX_PENDING_CONSENSUS {
            self.pending_consensus.pop_front();
        }
        debug!(
            "Queued consensus message for authenticated peers ({} bytes)",
            data.len()
        );
        self.pending_consensus.push_back(data);
    }

    fn flush_pending_consensus(&mut self) {
        if self.authenticated_peers.is_empty() {
            return;
        }
        debug!(
            "Flushing {} queued consensus messages to {} authenticated peers",
            self.pending_consensus.len(),
            self.authenticated_peers.len()
        );
        while let Some(data) = self.pending_consensus.pop_front() {
            let framed = self.encode_consensus_gossip(&data);
            let result = self
                .swarm
                .behaviour_mut()
                .gossipsub
                .publish(self.topics.consensus.clone(), framed);
            match result {
                Ok(_) => {
                    debug!("Published queued consensus message ({} bytes)", data.len());
                    const MAX_PUBLISHED_CONSENSUS: usize = 1024;
                    if self.published_consensus.len() >= MAX_PUBLISHED_CONSENSUS {
                        self.published_consensus.pop_front();
                    }
                    self.published_consensus.push_back(data);
                }
                Err(gossipsub::PublishError::Duplicate) => {
                    // The exact signed envelope has already been accepted by
                    // gossipsub, possibly through another local broadcast
                    // path. Treat this as idempotent success; retrying it at
                    // the queue head would starve every later consensus
                    // message indefinitely.
                    debug!(
                        "Dropping already-published consensus message ({} bytes)",
                        data.len()
                    );
                }
                Err(error) => {
                    error!("Broadcast queued consensus failed: {:?}", error);
                    self.pending_consensus.push_front(data);
                    break;
                }
            }
        }
    }
}

/// A FullNode may originate V2 synchronization requests and responses, but it
/// cannot originate a proposal, vote, or certificate. Incoming consensus
/// messages are deliberately not filtered by this local-role check: gossipsub
/// does not expose the original publisher to a relay, so the node must perform
/// the cryptographic admission checks instead.
fn local_consensus_broadcast_allowed(role: PeerRole, payload: &ConsensusMessage) -> bool {
    role == PeerRole::Validator
        || matches!(
            payload,
            ConsensusMessage::BlockRequest { .. }
                | ConsensusMessage::BlockResponse { .. }
                | ConsensusMessage::FinalityRequest { .. }
                | ConsensusMessage::FinalityResponse { .. }
        )
}

#[cfg(test)]
mod tests {
    use super::{
        challenge_slot_available, connection_slot_available, local_consensus_broadcast_allowed,
        sync_request_allowed, PendingChallenge, MAX_CONNECTED_PEERS,
        MAX_PENDING_VALIDATOR_CHALLENGES, MAX_SYNC_REQUESTS_PER_PEER,
    };
    use libp2p::identity::Keypair;
    use libp2p::PeerId;
    use norn_common::consensus_types::ConsensusMessage;
    use norn_common::types::{BlockId, Hash};
    use std::collections::{HashMap, HashSet};
    use std::time::{Duration, Instant};

    #[test]
    fn challenge_table_prunes_expired_entries_and_enforces_capacity() {
        let now = Instant::now();
        let mut pending = HashMap::new();
        pending.insert(
            PeerId::from(Keypair::generate_ed25519().public()),
            PendingChallenge {
                nonce: [0u8; 32],
                expires_at: now - Duration::from_secs(1),
            },
        );
        let live_peers = (0..MAX_PENDING_VALIDATOR_CHALLENGES)
            .map(|_| PeerId::from(Keypair::generate_ed25519().public()))
            .collect::<Vec<_>>();
        for peer in &live_peers {
            pending.insert(
                peer.clone(),
                PendingChallenge {
                    nonce: [1u8; 32],
                    expires_at: now + Duration::from_secs(1),
                },
            );
        }

        let new_peer = PeerId::from(Keypair::generate_ed25519().public());
        assert!(!challenge_slot_available(&mut pending, &new_peer, now));
        assert_eq!(pending.len(), MAX_PENDING_VALIDATOR_CHALLENGES);
        assert!(challenge_slot_available(&mut pending, &live_peers[0], now));
    }

    #[test]
    fn full_node_can_only_originate_v2_sync_requests() {
        let block_request = ConsensusMessage::BlockRequest {
            height: 1,
            round: 0,
            block_id: BlockId(Hash([1; 32])),
        };
        let finality_request = ConsensusMessage::FinalityRequest { height: 1 };
        let vote = ConsensusMessage::Vote(norn_common::consensus_types::SignedVote {
            protocol_version: Default::default(),
            chain_id: Default::default(),
            epoch: 0,
            height: 1,
            round: 0,
            step: norn_common::consensus_types::VoteStep::Prevote,
            block_id: None,
            stake_snapshot_hash: Default::default(),
            validator: Default::default(),
            signature: [0; 64],
        });

        assert!(local_consensus_broadcast_allowed(
            norn_common::chain_context::PeerRole::FullNode,
            &block_request
        ));
        assert!(local_consensus_broadcast_allowed(
            norn_common::chain_context::PeerRole::FullNode,
            &finality_request
        ));
        assert!(!local_consensus_broadcast_allowed(
            norn_common::chain_context::PeerRole::FullNode,
            &vote
        ));
        assert!(local_consensus_broadcast_allowed(
            norn_common::chain_context::PeerRole::Validator,
            &vote
        ));
    }

    #[test]
    fn connection_identity_limit_allows_existing_peers_but_rejects_new_ones() {
        let peers = (0..MAX_CONNECTED_PEERS)
            .map(|_| PeerId::from(Keypair::generate_ed25519().public()))
            .collect::<HashSet<_>>();
        let existing = peers.iter().next().expect("peer set is non-empty");
        let new_peer = PeerId::from(Keypair::generate_ed25519().public());

        assert!(connection_slot_available(&peers, existing));
        assert!(!connection_slot_available(&peers, &new_peer));
    }

    #[test]
    fn sync_request_budget_is_bounded_and_expires() {
        let now = Instant::now();
        let peer = PeerId::from(Keypair::generate_ed25519().public());
        let mut budget = HashMap::new();

        for _ in 0..MAX_SYNC_REQUESTS_PER_PEER {
            assert!(sync_request_allowed(&mut budget, peer, now));
        }
        assert!(!sync_request_allowed(&mut budget, peer, now));
        assert!(sync_request_allowed(
            &mut budget,
            peer,
            now + super::SYNC_REQUEST_WINDOW
        ));
    }
}
