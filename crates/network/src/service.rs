use anyhow::Result;
use libp2p::identity::Keypair;
use libp2p::{PeerId, SwarmBuilder};
use tokio::sync::mpsc;
use tracing::info;
use crate::config::NetworkConfig;
use crate::event_loop::EventLoop;
use crate::transport::build_transport;
use crate::behaviour_builder::build_behaviour;

#[derive(Debug)] // Add Debug trait for easier debugging
pub enum NetworkCommand {
    BroadcastBlock(Vec<u8>),
    BroadcastTransaction(Vec<u8>),
    StartListening,
}

#[derive(Debug)] // Add Debug trait for easier debugging
pub enum NetworkEvent {
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
    pub async fn start(config: NetworkConfig, keypair: Keypair) -> Result<Self> {
        let local_peer_id = PeerId::from(keypair.public());
        info!("Local peer id: {:?}", local_peer_id);

        let transport = build_transport(&keypair)?;
        let behaviour = build_behaviour(&keypair, &local_peer_id);

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

        let event_loop = EventLoop::new(swarm, command_rx, event_tx);

        tokio::spawn(event_loop.run());

        Ok(Self {
            command_tx,
            event_rx,
            local_peer_id,
        })
    }
}