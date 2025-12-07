use crate::proto;
use norn_common::types::{Block, Transaction};

impl From<Block> for proto::Block {
    fn from(b: Block) -> Self {
        proto::Block {
            header: Some(proto::BlockHeader {
                timestamp: b.header.timestamp as u64,
                height: b.header.height as u64,
                block_hash: hex::encode(b.header.block_hash.0),
                prev_block_hash: hex::encode(b.header.prev_block_hash.0),
                // Fill others
                ..Default::default()
            }),
            transactions: b.transactions.into_iter().map(|t| t.into()).collect(),
        }
    }
}

impl From<Transaction> for proto::Transaction {
    fn from(t: Transaction) -> Self {
        proto::Transaction {
            hash: hex::encode(t.body.hash.0),
            // Fill others
            ..Default::default()
        }
    }
}
