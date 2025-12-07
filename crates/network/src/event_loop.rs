use libp2p::{Swarm, gossipsub};
use libp2p::futures::StreamExt;
use crate::behaviour::NornBehaviour;
use crate::topics::Topics;
use super::service::{NetworkCommand, NetworkEvent};
use tokio::sync::mpsc;
use tracing::{info, error};

pub struct EventLoop {
    swarm: Swarm<NornBehaviour>,
    command_rx: mpsc::Receiver<NetworkCommand>,
    event_tx: mpsc::Sender<NetworkEvent>,
    topics: Topics,
}

impl EventLoop {
    pub fn new(
        swarm: Swarm<NornBehaviour>,
        command_rx: mpsc::Receiver<NetworkCommand>,
        event_tx: mpsc::Sender<NetworkEvent>,
    ) -> Self {
        Self {
            swarm,
            command_rx,
            event_tx,
            topics: Topics::new(),
        }
    }

    pub async fn run(mut self) {
        // Subscribe to topics
        let _ = self.swarm.behaviour_mut().gossipsub.subscribe(&self.topics.block);
        let _ = self.swarm.behaviour_mut().gossipsub.subscribe(&self.topics.transaction);
        
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
            }
        }
    }

    async fn handle_command(&mut self, command: NetworkCommand) {
        match command {
            NetworkCommand::BroadcastBlock(data) => {
                if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(self.topics.block.clone(), data) {
                    error!("Broadcast block failed: {:?}", e);
                }
            },
            NetworkCommand::BroadcastTransaction(data) => {
                if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(self.topics.transaction.clone(), data) {
                    error!("Broadcast transaction failed: {:?}", e);
                }
            },
            NetworkCommand::StartListening => {
                // Handled via external setup or if we want to start listener dynamically
            }
        }
    }

    async fn handle_swarm_event(&mut self, event: Option<libp2p::swarm::SwarmEvent<crate::behaviour::NornBehaviourEvent>>) {
        // Simplified handling
        match event {
            Some(libp2p::swarm::SwarmEvent::Behaviour(crate::behaviour::NornBehaviourEvent::Gossipsub(
                gossipsub::Event::Message { propagation_source: _, message_id: _, message }
            ))) => {
                if message.topic == self.topics.block.hash() {
                    let _ = self.event_tx.send(NetworkEvent::BlockReceived(message.data)).await;
                } else if message.topic == self.topics.transaction.hash() {
                    let _ = self.event_tx.send(NetworkEvent::TransactionReceived(message.data)).await;
                }
            },
            Some(libp2p::swarm::SwarmEvent::NewListenAddr { address, .. }) => {
                info!("Listening on {:?}", address);
            },
            _ => {}
        }
    }
}
