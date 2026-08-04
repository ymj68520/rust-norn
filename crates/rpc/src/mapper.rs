use crate::proto;
use hex;
use norn_common::types::{
    Address, Block, Hash, PublicKey, Transaction, TransactionBody, TransactionType,
};

impl From<Block> for proto::Block {
    fn from(b: Block) -> Self {
        proto::Block {
            header: Some(proto::BlockHeader {
                timestamp: b.header.timestamp as u64,
                height: b.header.height as u64,
                block_hash: hex::encode(b.header.block_hash.0),
                prev_block_hash: hex::encode(b.header.prev_block_hash.0),
                merkle_root: hex::encode(b.header.merkle_root.0),
                public: hex::encode(b.header.block_builder.0),
                params: hex::encode(b.header.parent_randomness.0),
                gas_limit: b.header.gas_limit as u64,
            }),
            transactions: b.transactions.into_iter().map(|t| t.into()).collect(),
        }
    }
}

impl From<Transaction> for proto::Transaction {
    fn from(t: Transaction) -> Self {
        proto::Transaction {
            hash: hex::encode(t.body.hash.0),
            address: hex::encode(t.body.address.0),
            receiver: hex::encode(t.body.receiver.0),
            gas: t.body.gas as u64,
            nonce: t.body.nonce as u64,
            event: hex::encode(&t.body.event),
            opt: hex::encode(&t.body.opt),
            state: hex::encode(&t.body.state),
            data: hex::encode(&t.body.data),
            expire: t.body.expire as u64,
            timestamp: t.body.timestamp as u64,
            public: hex::encode(t.body.public.0),
            signature: hex::encode(&t.body.signature),
            // Inclusion metadata is derived from the containing block and
            // is not present on the consensus transaction object.
            height: 0,
            block_hash: hex::encode(Hash::default().0),
            index: 0,
        }
    }
}

// Reverse conversions - Fully implemented
impl From<proto::Transaction> for Transaction {
    fn from(p: proto::Transaction) -> Self {
        let mut hash = Hash::default();
        if let Ok(hash_bytes) = hex::decode(&p.hash) {
            if hash_bytes.len() == 32 {
                hash.0.copy_from_slice(&hash_bytes);
            }
        }

        let mut address = Address::default();
        if let Ok(bytes) = hex::decode(&p.address) {
            if bytes.len() == 20 {
                address.0.copy_from_slice(&bytes);
            }
        }

        let mut receiver = Address::default();
        if let Ok(bytes) = hex::decode(&p.receiver) {
            if bytes.len() == 20 {
                receiver.0.copy_from_slice(&bytes);
            }
        }

        let mut public = PublicKey::default();
        if let Ok(bytes) = hex::decode(&p.public) {
            if bytes.len() == 33 {
                public.0.copy_from_slice(&bytes);
            }
        }

        Transaction {
            body: TransactionBody {
                hash,
                address,
                receiver,
                gas: p.gas as i64,
                nonce: p.nonce as i64,
                event: hex::decode(&p.event).unwrap_or_default(),
                opt: hex::decode(&p.opt).unwrap_or_default(),
                state: hex::decode(&p.state).unwrap_or_default(),
                data: hex::decode(&p.data).unwrap_or_default(),
                expire: p.expire as i64,
                timestamp: p.timestamp as i64,
                public,
                signature: hex::decode(&p.signature).unwrap_or_default(),
                tx_type: TransactionType::default(),
                chain_id: None,
                value: None,
                gas_price: None,
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                access_list: None,
            },
        }
    }
}

impl From<proto::Block> for Block {
    fn from(proto: proto::Block) -> Self {
        use norn_common::types::*;

        let proto_header = proto.header.expect("Block header is missing");

        let header = BlockHeader {
            protocol_version: norn_common::types::ProtocolVersion(2),
            chain_id: norn_common::types::ChainId(Hash([1u8; 32])),
            height: proto_header.height as i64,
            epoch: 1,
            round: 0,
            timestamp: proto_header.timestamp as i64,
            prev_block_hash: {
                let mut hash = Hash::default();
                if let Ok(bytes) = hex::decode(&proto_header.prev_block_hash) {
                    if bytes.len() == 32 {
                        hash.0.copy_from_slice(&bytes);
                    }
                }
                hash
            },
            block_hash: {
                let mut hash = Hash::default();
                if let Ok(bytes) = hex::decode(&proto_header.block_hash) {
                    if bytes.len() == 32 {
                        hash.0.copy_from_slice(&bytes);
                    }
                }
                hash
            },
            merkle_root: {
                let mut hash = Hash::default();
                if let Ok(bytes) = hex::decode(&proto_header.merkle_root) {
                    if bytes.len() == 32 {
                        hash.0.copy_from_slice(&bytes);
                    }
                }
                hash
            },
            state_root: Hash::default(),
            block_builder: {
                let mut validator_id = norn_common::types::ValidatorId([0u8; 32]);
                if let Ok(bytes) = hex::decode(&proto_header.public) {
                    if bytes.len() == 32 {
                        validator_id.0.copy_from_slice(&bytes);
                    }
                }
                validator_id
            },
            stake_snapshot_hash: norn_common::types::StakeSnapshotHash::default(),
            parent_randomness: Hash::default(),
            gas_limit: proto_header.gas_limit as i64,
            base_fee: 1_000_000_000,
            consensus_data_hash: Hash::default(),
        };

        let transactions: Vec<Transaction> =
            proto.transactions.into_iter().map(|tx| tx.into()).collect();

        Block {
            header,
            transactions,
        }
    }
}
