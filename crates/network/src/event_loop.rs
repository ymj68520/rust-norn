use super::service::{NetworkCommand, NetworkEvent};
use crate::behaviour::NornBehaviour;
use crate::topics::Topics;
use libp2p::futures::StreamExt;
use libp2p::PeerId;
use libp2p::{gossipsub, kad, Swarm};
use norn_common::chain_context::{
    ChainContext, NetworkHandshake, PeerRole, MAX_BLOCK_MESSAGE_BYTES, MAX_HANDSHAKE_BYTES,
    MAX_TRANSACTION_MESSAGE_BYTES,
};
use norn_common::consensus_types::ConsensusEnvelope;
use norn_common::types::TransactionV2;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

const CONSENSUS_GOSSIP_MAGIC: &[u8] = b"NORN_CONSENSUS_GOSSIP_V2";
const CONSENSUS_GOSSIP_NONCE_BYTES: usize = std::mem::size_of::<u64>();
const MAX_REPLAYED_CONSENSUS: usize = 64;

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
    pending_inbound_consensus: VecDeque<(PeerId, Vec<u8>)>,
    connected_peers: HashSet<PeerId>,
    dialing_bootstrap_peers: HashSet<PeerId>,
    handshake_pending: bool,
    handshake_due: Option<Instant>,
    consensus_publish_nonce: u64,
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
            pending_inbound_consensus: VecDeque::new(),
            connected_peers: HashSet::new(),
            dialing_bootstrap_peers: HashSet::new(),
            handshake_pending: false,
            handshake_due: None,
            consensus_publish_nonce: 0,
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
                || self.dialing_bootstrap_peers.contains(&peer_id)
            {
                continue;
            }
            self.dialing_bootstrap_peers.insert(peer_id);
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
        let handshake = self.handshake.clone();
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
        if self.publish_handshake() {
            debug!("Handshake broadcast accepted by gossipsub");
            self.handshake_pending = false;
            self.handshake_due = None;
        } else {
            // The first gossipsub publish can fail while the remote subscription
            // is still being established, but the message id is retained briefly.
            // Retry only after that cache has expired instead of creating a hot
            // duplicate loop.
            self.handshake_due = Some(now + Duration::from_secs(3));
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
                    // FullNodes may request missing V2 finalized data, but only
                    // authenticated validators may inject proposals, votes,
                    // commits, or responses into the consensus stream.
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
                    let is_block_request = matches!(
                        decoded_envelope.payload,
                        norn_common::consensus_types::ConsensusMessage::BlockRequest { .. }
                            | norn_common::consensus_types::ConsensusMessage::FinalityRequest { .. }
                    );
                    if !is_block_request
                        && self.authenticated_peers.get(&propagation_source)
                            != Some(&PeerRole::Validator)
                    {
                        self.queue_inbound_consensus(propagation_source, consensus_data.to_vec());
                        return;
                    }
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::ConsensusMessageReceived(
                            consensus_data.to_vec(),
                        ))
                        .await;
                }
            }
            Some(libp2p::swarm::SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                self.dialing_bootstrap_peers.remove(&peer_id);
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .add_explicit_peer(&peer_id);
                self.handshake.session_nonce = self.handshake.session_nonce.wrapping_add(1);
                self.connected_peers.insert(peer_id);
                self.replay_published_consensus();
                self.handshake_pending = true;
                self.handshake_due = Some(Instant::now() + Duration::from_secs(2));
                let _ = self.event_tx.try_send(NetworkEvent::PeerConnected(peer_id));
                debug!("Connection established with {:?}", peer_id);
            }
            Some(libp2p::swarm::SwarmEvent::ConnectionClosed { peer_id, .. }) => {
                self.dialing_bootstrap_peers.remove(&peer_id);
                self.connected_peers.remove(&peer_id);
                self.authenticated_peers.remove(&peer_id);
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
        if handshake.peer_role == PeerRole::Validator {
            let mut queued = Vec::new();
            self.pending_inbound_consensus.retain(|(source, data)| {
                if source == &advertised_peer {
                    queued.push(data.clone());
                    false
                } else {
                    true
                }
            });
            for data in queued {
                let _ = self
                    .event_tx
                    .send(NetworkEvent::ConsensusMessageReceived(data))
                    .await;
            }
        } else {
            self.pending_inbound_consensus
                .retain(|(source, _)| source != &advertised_peer);
        }
        info!("Authenticated {:?} as {:?}", peer, handshake.peer_role);
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

    fn queue_inbound_consensus(&mut self, peer: PeerId, data: Vec<u8>) {
        const MAX_PENDING_INBOUND_CONSENSUS: usize = 256;
        if self
            .pending_inbound_consensus
            .iter()
            .any(|(source, pending)| source == &peer && pending == &data)
        {
            return;
        }
        if self.pending_inbound_consensus.len() >= MAX_PENDING_INBOUND_CONSENSUS {
            self.pending_inbound_consensus.pop_front();
        }
        self.pending_inbound_consensus.push_back((peer, data));
    }
}
