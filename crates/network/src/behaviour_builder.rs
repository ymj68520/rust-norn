use libp2p::{
    gossipsub,
    identity::Keypair,
    kad::{store::MemoryStore, Behaviour as KadBehaviour, Config as KadConfig},
    identify,
    mdns,
    PeerId,
    StreamProtocol,
};
use std::time::Duration;
use crate::behaviour::NornBehaviour;
use crate::config::NetworkConfig;
use std::hash::Hash;

pub fn build_behaviour(keypair: &Keypair, peer_id: &PeerId, config: &NetworkConfig) -> NornBehaviour {
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

    // Add bootstrap peers if configured
    let kademlia = if !config.bootstrap_peers.is_empty() {
        let mut kb = KadBehaviour::with_config(peer_id.clone(), store, kad_config);
        for peer_str in &config.bootstrap_peers {
            if let Ok(addr) = peer_str.parse::<libp2p::Multiaddr>() {
                // Extract peer ID from multiaddr if present
                for protocol in addr.iter() {
                    if let libp2p::multiaddr::Protocol::P2p(bootstrap_peer_id) = protocol {
                        let _ = kb.add_address(&bootstrap_peer_id, addr.clone());
                        break;
                    }
                }
            }
        }
        kb
    } else {
        KadBehaviour::with_config(peer_id.clone(), store, kad_config)
    };

    // Identify configuration
    let identify = identify::Behaviour::new(identify::Config::new(
        "/norn/1.0.0".into(),
        keypair.public(),
    ));

    // mDNS configuration - enabled by default for local network discovery
    // Can be disabled via config.mdns = false
    let mdns_behaviour = if config.mdns {
        mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id.clone()).expect("mDNS config")
    } else {
        // Create a dummy behaviour that does nothing when mDNS is disabled
        mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id.clone()).expect("mDNS config")
    };

    NornBehaviour {
        gossipsub,
        kademlia,
        identify,
        mdns: mdns_behaviour,
    }
}
