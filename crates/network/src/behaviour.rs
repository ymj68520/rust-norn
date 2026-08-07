use libp2p::gossipsub;
use libp2p::identify;
use libp2p::kad::{store::MemoryStore, Behaviour as KadBehaviour};
use libp2p::swarm::NetworkBehaviour;

#[derive(NetworkBehaviour)]
pub struct NornBehaviour {
    /// Latency-sensitive handshake and consensus control traffic.  Keep this
    /// field first: the derived `NetworkBehaviour` polls child behaviours in
    /// declaration order, so a continuously-ready bulk gossip stream must not
    /// starve consensus events.
    pub control_gossipsub: gossipsub::Behaviour,
    /// Bulk block and transaction propagation. Keeping this on a protocol
    /// stream separate from consensus prevents multi-megabyte transaction
    /// bursts from head-of-line blocking proposals and votes.
    pub gossipsub: gossipsub::Behaviour,
    pub kademlia: KadBehaviour<MemoryStore>,
    pub identify: identify::Behaviour,
}
