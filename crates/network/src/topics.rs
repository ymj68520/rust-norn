use libp2p::gossipsub::IdentTopic;
use norn_common::chain_context::ChainContext;

pub struct Topics {
    pub block: IdentTopic,
    pub transaction: IdentTopic,
    pub consensus: IdentTopic,
    pub handshake: IdentTopic,
}

impl Topics {
    /// Legacy topics retained for non-node library callers. Real nodes must
    /// use `for_context` so messages cannot cross chain identities.
    pub fn new() -> Self {
        Self {
            block: IdentTopic::new("norn/block"),
            transaction: IdentTopic::new("norn/tx"),
            consensus: IdentTopic::new("norn/consensus"),
            handshake: IdentTopic::new("norn/handshake"),
        }
    }

    pub fn for_context(context: &ChainContext) -> Self {
        let prefix = format!(
            "norn/v2/w{}-p{}-c{}-g{}",
            context.wire_version,
            context.protocol_version.0,
            hex::encode(context.chain_id.0 .0),
            hex::encode(context.genesis_hash.0),
        );
        Self {
            block: IdentTopic::new(format!("{prefix}/block")),
            transaction: IdentTopic::new(format!("{prefix}/tx")),
            consensus: IdentTopic::new(format!("{prefix}/consensus")),
            handshake: IdentTopic::new(format!("{prefix}/handshake")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::{ChainId, Hash, ProtocolVersion};

    fn context(genesis_byte: u8) -> ChainContext {
        ChainContext::new(
            2,
            ProtocolVersion(2),
            ChainId(Hash([3u8; 32])),
            Hash([genesis_byte; 32]),
        )
    }

    #[test]
    fn context_topics_are_distinct_by_genesis_identity() {
        let first = Topics::for_context(&context(4));
        let second = Topics::for_context(&context(5));
        assert_ne!(first.consensus.hash(), second.consensus.hash());
        assert_ne!(first.handshake.hash(), second.handshake.hash());
        assert_ne!(first.block.hash(), second.block.hash());
        assert_ne!(first.transaction.hash(), second.transaction.hash());
    }
}
