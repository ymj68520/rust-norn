use norn_crypto::ecdsa::KeyPair;
use norn_crypto::transaction::TransactionSigner;
use norn_common::types::{Address, Transaction};

// Proto definition (simplified)
#[derive(Clone, PartialEq, ::prost::Message)]
struct ProtoTransaction {
    #[prost(string, tag = "1")]
    pub hash: ::prost::alloc::string::String,
    #[prost(string, tag = "2")]
    pub address: ::prost::alloc::string::String,
    #[prost(string, tag = "3")]
    pub receiver: ::prost::alloc::string::String,
    #[prost(uint64, tag = "4")]
    pub gas: u64,
    #[prost(uint64, tag = "5")]
    pub nonce: u64,
    #[prost(string, tag = "6")]
    pub event: ::prost::alloc::string::String,
    #[prost(string, tag = "7")]
    pub opt: ::prost::alloc::string::String,
    #[prost(string, tag = "8")]
    pub state: ::prost::alloc::string::String,
    #[prost(string, tag = "9")]
    pub data: ::prost::alloc::string::String,
    #[prost(uint64, tag = "10")]
    pub expire: u64,
    #[prost(uint64, tag = "11")]
    pub timestamp: u64,
    #[prost(string, tag = "12")]
    pub public: ::prost::alloc::string::String,
    #[prost(string, tag = "13")]
    pub signature: ::prost::alloc::string::String,
    #[prost(uint64, tag = "14")]
    pub height: u64,
    #[prost(string, tag = "15")]
    pub block_hash: ::prost::alloc::string::String,
    #[prost(uint64, tag = "16")]
    pub index: u64,
}

fn main() {
    let keypair = KeyPair::random();
    let mut signer = TransactionSigner::new(keypair);

    let receiver = Address::default();
    let tx = signer.create_transaction(
        receiver,
        b"test_event".to_vec(),
        b"test_opt".to_vec(),
        b"test_state".to_vec(),
        b"test_data".to_vec(),
        1000,
        chrono::Utc::now().timestamp() + 3600,
    ).unwrap();

    println!("=== Original Transaction ===");
    println!("Hash: {}", hex::encode(tx.body.hash.0));

    // 验证原始交易
    match norn_crypto::transaction::verify_transaction(&tx) {
        Ok(()) => println!("✅ Original transaction is valid"),
        Err(e) => println!("❌ Original transaction verification failed: {:?}", e),
    }

    // 转换为 protobuf
    let proto = ProtoTransaction {
        hash: hex::encode(tx.body.hash.0),
        address: hex::encode(tx.body.address.0),
        receiver: hex::encode(tx.body.receiver.0),
        gas: tx.body.gas as u64,
        nonce: tx.body.nonce as u64,
        event: hex::encode(&tx.body.event),
        opt: hex::encode(&tx.body.opt),
        state: hex::encode(&tx.body.state),
        data: hex::encode(&tx.body.data),
        expire: tx.body.expire as u64,
        timestamp: tx.body.timestamp as u64,
        public: hex::encode(tx.body.public.0),
        signature: hex::encode(&tx.body.signature),
        height: tx.body.height as u64,
        block_hash: hex::encode(tx.body.block_hash.0),
        index: tx.body.index as u64,
    };

    println!("\n=== After Protobuf Conversion ===");

    // 转换回Transaction
    use norn_common::types::*;

    let mut hash = Hash::default();
    if let Ok(bytes) = hex::decode(&proto.hash) {
        if bytes.len() == 32 {
            hash.0.copy_from_slice(&bytes);
        }
    }

    let mut address = Address::default();
    if let Ok(bytes) = hex::decode(&proto.address) {
        if bytes.len() == 20 {
            address.0.copy_from_slice(&bytes);
        }
    }

    let mut receiver_addr = Address::default();
    if let Ok(bytes) = hex::decode(&proto.receiver) {
        if bytes.len() == 20 {
            receiver_addr.0.copy_from_slice(&bytes);
        }
    }

    let mut public = PublicKey::default();
    if let Ok(bytes) = hex::decode(&proto.public) {
        if bytes.len() == 33 {
            public.0.copy_from_slice(&bytes);
        }
    }

    let mut block_hash = Hash::default();
    if let Ok(bytes) = hex::decode(&proto.block_hash) {
        if bytes.len() == 32 {
            block_hash.0.copy_from_slice(&bytes);
        }
    }

    let tx2 = Transaction {
        body: TransactionBody {
            hash,
            address,
            receiver: receiver_addr,
            gas: proto.gas as i64,
            nonce: proto.nonce as i64,
            event: hex::decode(&proto.event).unwrap_or_default(),
            opt: hex::decode(&proto.opt).unwrap_or_default(),
            state: hex::decode(&proto.state).unwrap_or_default(),
            data: hex::decode(&proto.data).unwrap_or_default(),
            expire: proto.expire as i64,
            height: proto.height as i64,
            index: proto.index as i64,
            block_hash,
            timestamp: proto.timestamp as i64,
            public,
            signature: hex::decode(&proto.signature).unwrap_or_default(),
        },
    };

    println!("Hash after round-trip: {}", hex::encode(tx2.body.hash.0));
    println!("Hashes match: {}", tx.body.hash.0 == tx2.body.hash.0);

    // 验证转换后的交易
    match norn_crypto::transaction::verify_transaction(&tx2) {
        Ok(()) => println!("✅ Round-trip transaction is valid"),
        Err(e) => println!("❌ Round-trip verification failed: {:?}", e),
    }
}
