use libp2p::{
    gossipsub,
    identity::Keypair,
    kad::{store::MemoryStore, Behaviour as KadBehaviour, Config as KadConfig},
    identify,
    PeerId,
    StreamProtocol,
};
use std::time::Duration;
use crate::behaviour::NornBehaviour;
use std::hash::Hash;

pub fn build_behaviour(keypair: &Keypair, peer_id: &PeerId) -> NornBehaviour {
    // Gossipsub configuration
    let message_id_fn = |message: &gossipsub::Message| {
        let mut s = std::collections::hash_map::DefaultHasher::new();
        use std::hash::Hasher;
        message.data.hash(&mut s);
        gossipsub::MessageId::from(s.finish().to_string())
    };

    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(1))
        .validation_mode(gossipsub::ValidationMode::Strict)
        .message_id_fn(message_id_fn)
        .build()
        .expect("Valid config");

    let gossipsub = gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(keypair.clone()),
        gossipsub_config,
    ).expect("Correct configuration");

    // Kademlia configuration
    let store = MemoryStore::new(peer_id.clone());
    let mut kad_config = KadConfig::default();
    kad_config.set_protocol_names(vec![StreamProtocol::new("/norn/kad/1.0.0")]);
    let kademlia = KadBehaviour::with_config(peer_id.clone(), store, kad_config);

    // Identify configuration
    let identify = identify::Behaviour::new(identify::Config::new(
        "/norn/1.0.0".into(),
        keypair.public(),
    ));

    NornBehaviour {
        gossipsub,
        kademlia,
        identify,
    }
}
