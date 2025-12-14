use crate::proto;
use norn_common::types::{Block, Transaction, TransactionBody, Hash};
use hex;

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

// Reverse conversions
impl From<proto::Transaction> for Transaction {
    fn from(p: proto::Transaction) -> Self {
        let mut hash = Hash::default();
        if let Ok(hash_bytes) = hex::decode(&p.hash) {
            if hash_bytes.len() == 32 {
                hash.0.copy_from_slice(&hash_bytes);
            }
        }

        Transaction {
            body: TransactionBody {
                hash,
                // Fill other fields with defaults for now
                ..Default::default()
            },
            // Fill signature and other fields
            ..Default::default()
        }
    }
}
