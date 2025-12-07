use libp2p::gossipsub::IdentTopic;

pub struct Topics {
    pub block: IdentTopic,
    pub transaction: IdentTopic,
    pub consensus: IdentTopic,
}

impl Topics {
    pub fn new() -> Self {
        Self {
            block: IdentTopic::new("norn/block"),
            transaction: IdentTopic::new("norn/tx"),
            consensus: IdentTopic::new("norn/consensus"),
        }
    }
}
