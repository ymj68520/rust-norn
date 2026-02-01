use norn_crypto::ecdsa::KeyPair;
use norn_common::types::{Address, Transaction, TransactionBody, Hash};
use rand::Rng;

fn main() -> anyhow::Result<()> {
    println!("=== Norn Transaction Generator ===\n");

    // 生成随机密钥对
    let keypair = KeyPair::random();
    let pub_key = keypair.public_key();
    let encoded_point = pub_key.to_encoded_point(true);
    let pub_key_bytes = encoded_point.as_bytes();
    let pub_key_hex = hex::encode(pub_key_bytes);
    println!("✅ Generated keypair");
    println!("   Public key: {}", pub_key_hex);

    // 从公钥创建地址（简化版：取公钥后20字节）
    let mut addr_bytes = [0u8; 20];
    addr_bytes.copy_from_slice(&pub_key_bytes[..20.min(pub_key_bytes.len())]);
    let sender = Address(addr_bytes);

    // 创建随机接收地址
    let mut receiver_bytes = [0u8; 20];
    rand::thread_rng().fill(&mut receiver_bytes);
    let receiver = Address(receiver_bytes);

    println!("\n✅ Creating test transaction...");
    println!("   From: {}", hex::encode(sender.0));
    println!("   To: {}", hex::encode(receiver.0));

    // 创建随机哈希
    let mut hash_bytes = [0u8; 32];
    rand::thread_rng().fill(&mut hash_bytes);
    let hash = Hash(hash_bytes);

    // 创建PublicKey结构（从VerifyingKey）
    let mut public_key_bytes = [0u8; 33];
    public_key_bytes.copy_from_slice(&pub_key_bytes[..33.min(pub_key_bytes.len())]);
    let public = norn_common::types::PublicKey(public_key_bytes);

    // 创建简单的交易
    let tx = Transaction {
        body: TransactionBody {
            hash,
            address: sender,
            receiver,
            gas: 21000,
            nonce: 0,
            event: vec![],
            opt: vec![],
            state: vec![],
            data: vec![1, 2, 3, 4],
            expire: 0,
            height: 0,
            index: 0,
            block_hash: Hash::default(),
            timestamp: chrono::Utc::now().timestamp(),
            public,
            signature: vec![],
            tx_type: norn_common::types::TransactionType::Native,
            chain_id: Some(31337),
            value: Some("1000".to_string()),
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
            access_list: None,
            gas_price: None,
        },
    };

    println!("\n=== Transaction Details ===");
    println!("Hash: {}", hex::encode(tx.body.hash.0));
    println!("Gas: {}", tx.body.gas);
    println!("Nonce: {}", tx.body.nonce);
    println!("Timestamp: {}", tx.body.timestamp);
    println!("Chain ID: {:?}", tx.body.chain_id);
    println!("Value: {:?}", tx.body.value);

    // 模拟签名
    let message = b"test transaction message";
    let signature = keypair.sign(message);
    println!("\n✅ Signature: {}", hex::encode(&signature[..]));

    // 生成批量测试交易
    println!("\n=== Generating Batch Transactions ===");
    for i in 1..=5 {
        let mut hash_bytes = [0u8; 32];
        rand::thread_rng().fill(&mut hash_bytes);
        let batch_hash = Hash(hash_bytes);

        let mut receiver_bytes = [0u8; 20];
        rand::thread_rng().fill(&mut receiver_bytes);
        let batch_receiver = Address(receiver_bytes);

        let batch_tx = Transaction {
            body: TransactionBody {
                hash: batch_hash,
                address: sender,
                receiver: batch_receiver,
                gas: 21000,
                nonce: i,
                data: vec![i as u8],
                value: Some((1000 * (i + 1)).to_string()),
                timestamp: chrono::Utc::now().timestamp(),
                public,
                signature: vec![],
                tx_type: norn_common::types::TransactionType::Native,
                chain_id: Some(31337),
                ..Default::default()
            },
        };
        println!("  Tx #{}: hash={}, nonce={}, value={:?}",
            i,
            hex::encode(&batch_tx.body.hash.0[..8]),
            batch_tx.body.nonce,
            batch_tx.body.value
        );
    }

    println!("\n✅ Transaction generation complete!");
    println!("   Total transactions generated: 6");

    Ok(())
}
