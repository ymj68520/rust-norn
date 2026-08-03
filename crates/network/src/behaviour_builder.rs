use crate::behaviour::NornBehaviour;
use crate::config::NetworkConfig;
use anyhow::{bail, Result};
use libp2p::{
    gossipsub, identify,
    identity::Keypair,
    kad::{store::MemoryStore, Behaviour as KadBehaviour, Config as KadConfig},
    PeerId, StreamProtocol,
};
use std::hash::Hash;
use std::time::Duration;

pub fn build_behaviour(
    keypair: &Keypair,
    peer_id: &PeerId,
    config: &NetworkConfig,
) -> Result<NornBehaviour> {
    if config.mdns {
        bail!("mDNS is disabled in protocol V2; configure explicit bootstrap_peers");
    }
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
    )
    .expect("Correct configuration");

    // Kademlia configuration
    let store = MemoryStore::new(peer_id.clone());
    let kad_config = KadConfig::new(StreamProtocol::new("/norn/kad/1.0.0"));

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

    // V2 uses explicit bootstrap peers only. This keeps peer discovery
    // deterministic and avoids an unauthenticated local-discovery ingress.
    Ok(NornBehaviour {
        gossipsub,
        kademlia,
        identify,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mdns_configuration_is_rejected_in_v2() {
        let keypair = Keypair::generate_ed25519();
        let peer_id = PeerId::from_public_key(&keypair.public());
        let mut config = NetworkConfig::default();
        config.mdns = true;

        assert!(build_behaviour(&keypair, &peer_id, &config).is_err());
    }
}
