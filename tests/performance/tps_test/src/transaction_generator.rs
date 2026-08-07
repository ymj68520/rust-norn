use norn_common::types::{
    AccessListItem, Address, ChainId, Hash, ProtocolVersion, Transaction, TransactionId,
    TransactionType, TransactionV2,
};
use norn_crypto::ecdsa::KeyPair;
use norn_crypto::transaction::{sign_transaction_v2, TransactionSigner};
use rand::Rng;
use sha2::{Digest, Sha256};

/// 交易生成器
pub struct TransactionGenerator {
    signer: TransactionSigner,
}

impl TransactionGenerator {
    /// 创建新的交易生成器
    pub fn new() -> Self {
        let keypair = KeyPair::random();
        let signer = TransactionSigner::new(keypair);
        Self { signer }
    }

    /// 生成随机交易
    pub fn generate_random_transaction(&mut self) -> Transaction {
        let mut rng = rand::thread_rng();

        // 生成随机接收地址
        let receiver = self.generate_random_address();

        // 生成随机数据
        let event_size = rng.gen_range(10..100);
        let event = (0..event_size)
            .map(|_| rng.gen_range(b'A'..b'Z'))
            .collect::<Vec<_>>();

        let opt_size = rng.gen_range(5..50);
        let opt = (0..opt_size)
            .map(|_| rng.gen_range(b'a'..b'z'))
            .collect::<Vec<_>>();

        let state_size = rng.gen_range(5..50);
        let state = (0..state_size)
            .map(|_| rng.gen_range(b'0'..b'9'))
            .collect::<Vec<_>>();

        let data_size = rng.gen_range(50..500);
        let data = (0..data_size)
            .map(|_| {
                let charset = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
                charset[rng.gen_range(0..charset.len())]
            })
            .collect::<Vec<_>>();

        // 生成随机 gas 和过期时间
        let gas = rng.gen_range(1000..100000);
        let expire = chrono::Utc::now().timestamp() + rng.gen_range(300..3600);

        // 创建交易
        let tx = self
            .signer
            .create_transaction(receiver, event, opt, state, data, gas, expire)
            .expect("Failed to create transaction");

        tx
    }

    /// 生成随机地址
    fn generate_random_address(&self) -> Address {
        let mut rng = rand::thread_rng();
        let mut addr = Address::default();
        rng.fill(&mut addr.0);
        addr
    }

    /// 批量生成交易
    pub fn generate_batch(&mut self, count: usize) -> Vec<Transaction> {
        (0..count)
            .map(|_| self.generate_random_transaction())
            .collect()
    }

    /// 生成固定大小的交易（用于测试）
    pub fn generate_fixed_size_transaction(&mut self, data_size: usize) -> Transaction {
        let receiver = self.generate_random_address();
        let event = vec![b'E'; 20];
        let opt = vec![b'O'; 10];
        let state = vec![b'S'; 10];
        let data = vec![b'D'; data_size];
        let gas = 50000;
        let expire = chrono::Utc::now().timestamp() + 3600;

        self.signer
            .create_transaction(receiver, event, opt, state, data, gas, expire)
            .expect("Failed to create transaction")
    }
}

impl Default for TransactionGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Generates valid protocol-V5 native transfers for the V2 consensus path.
pub struct V2TransactionGenerator {
    keypair: KeyPair,
    sender: Address,
    receiver: Address,
    nonce: u64,
}

impl V2TransactionGenerator {
    pub fn new() -> Self {
        Self::for_node(1)
    }

    /// Use a stable, node-specific signer so the benchmark can pre-fund the
    /// account on every validator without reusing a nonce across nodes.
    pub fn for_node(node_id: u8) -> Self {
        let seed = format!("{:02x}", node_id.max(1)).repeat(32);
        let keypair = KeyPair::from_private_key_hex(&seed)
            .expect("node-specific benchmark key must be a valid P-256 key");
        let encoded = keypair.public_key().to_encoded_point(true);
        let digest = Sha256::digest(encoded.as_bytes());
        let sender = Address(digest[..20].try_into().expect("address length"));
        Self {
            keypair,
            sender,
            // A benchmark must not turn every transfer into an account-creation
            // workload. All generated zero-value native transfers target one
            // deterministic sink, so the measured steady-state cost is the
            // transaction pool, execution, and BFT path.
            receiver: Address([0x42; 20]),
            nonce: 0,
        }
    }

    /// Derive a deterministic benchmark signer for one sender lane. Each lane
    /// owns its nonce sequence, allowing concurrent RPCs across accounts
    /// without reordering transactions from the same account.
    pub fn for_stream(node_id: u8, stream_id: u8) -> Self {
        if stream_id == 0 {
            return Self::for_node(node_id);
        }
        // Keep stream zero backward-compatible with `for_node`, but domain
        // separate every additional lane. The old `0101` pattern for node 1,
        // stream 1 expanded to exactly the same 32-byte scalar as stream 0
        // (`01` repeated 32 times), which created duplicate nonces that could
        // be admitted yet never become canonical.
        let seed = format!("{:02x}{:02x}", node_id.max(1) ^ 0xA5, stream_id).repeat(16);
        let keypair = KeyPair::from_private_key_hex(&seed)
            .expect("benchmark stream key must be a valid P-256 key");
        let encoded = keypair.public_key().to_encoded_point(true);
        let digest = Sha256::digest(encoded.as_bytes());
        let sender = Address(digest[..20].try_into().expect("address length"));
        Self {
            keypair,
            sender,
            receiver: Address([0x42; 20]),
            nonce: 0,
        }
    }

    pub fn sender(&self) -> Address {
        self.sender
    }

    /// Set the next nonce used by this sender lane.  Benchmarks normally start
    /// from zero on a fresh chain, but an explicit starting value lets a later
    /// run reuse pre-funded accounts without resubmitting already-finalized
    /// transactions.
    pub fn with_starting_nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }

    pub fn generate_random_transaction(&mut self) -> TransactionV2 {
        let mut tx = TransactionV2 {
            protocol_version: ProtocolVersion(5),
            chain_id: ChainId(Hash([1u8; 32])),
            nonce: self.nonce,
            sender: self.sender,
            receiver: Some(self.receiver),
            value: 0,
            gas_limit: 21_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 0,
            data: Vec::new(),
            event: Vec::new(),
            opt: Vec::new(),
            state: Vec::new(),
            expire: None,
            timestamp: chrono::Utc::now().timestamp() as u64,
            tx_type: TransactionType::Native,
            access_list: Vec::<AccessListItem>::new(),
            public_key: Default::default(),
            signature: [0u8; 64],
            transaction_id: TransactionId::default(),
        };
        sign_transaction_v2(&self.keypair, &mut tx).expect("sign TransactionV2");
        self.nonce += 1;
        tx
    }
}

impl Default for V2TransactionGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_random_transaction() {
        let mut generator = TransactionGenerator::new();
        let tx = generator.generate_random_transaction();

        assert_ne!(tx.body.hash, norn_common::types::Hash::default());
        assert!(tx.body.gas > 0);
        assert!(tx.body.expire > chrono::Utc::now().timestamp());
    }

    #[test]
    fn test_generate_batch() {
        let mut generator = TransactionGenerator::new();
        let batch = generator.generate_batch(10);

        assert_eq!(batch.len(), 10);
        for tx in batch {
            assert_ne!(tx.body.hash, norn_common::types::Hash::default());
        }
    }

    #[test]
    fn test_generate_fixed_size_transaction() {
        let mut generator = TransactionGenerator::new();
        let tx = generator.generate_fixed_size_transaction(1000);

        assert_eq!(tx.body.data.len(), 1000);
    }

    #[test]
    fn v2_sender_lanes_preserve_the_primary_signer_and_are_distinct() {
        assert_eq!(
            V2TransactionGenerator::for_node(1).sender(),
            V2TransactionGenerator::for_stream(1, 0).sender()
        );
        let senders = (0..8)
            .map(|stream_id| V2TransactionGenerator::for_stream(1, stream_id).sender())
            .collect::<Vec<_>>();
        for (index, sender) in senders.iter().enumerate() {
            assert!(senders[..index].iter().all(|prior| prior != sender));
        }
    }

    #[test]
    fn v2_sender_lane_can_resume_from_a_canonical_nonce() {
        let mut generator = V2TransactionGenerator::for_stream(1, 3).with_starting_nonce(75);

        assert_eq!(generator.generate_random_transaction().nonce, 75);
        assert_eq!(generator.generate_random_transaction().nonce, 76);
    }
}
