use libp2p::gossipsub;
use libp2p::identify;
use libp2p::kad::{store::MemoryStore, Behaviour as KadBehaviour};
use libp2p::swarm::NetworkBehaviour;

#[derive(NetworkBehaviour)]
pub struct NornBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kademlia: KadBehaviour<MemoryStore>,
    pub identify: identify::Behaviour,
}
