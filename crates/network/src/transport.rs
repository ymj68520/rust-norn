use libp2p::core::upgrade::Version;
use libp2p::{noise, tcp, yamux, Transport};
use libp2p::identity::Keypair;
use std::time::Duration;

pub fn build_transport(keypair: &Keypair) -> std::io::Result<libp2p::core::transport::Boxed<(libp2p::PeerId, libp2p::core::muxing::StreamMuxerBox)>> {
    let noise_config = noise::Config::new(&keypair).unwrap();
    let yamux_config = yamux::Config::default();

    let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
        .upgrade(Version::V1)
        .authenticate(noise_config)
        .multiplex(yamux_config)
        .timeout(Duration::from_secs(20))
        .boxed();

    Ok(transport)
}
