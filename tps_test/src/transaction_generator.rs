use norn_crypto::ecdsa::KeyPair;
use norn_crypto::transaction::TransactionSigner;
use norn_common::types::{Address, Transaction};
use rand::Rng;

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
}
