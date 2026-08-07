use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tonic::transport::Channel;
use tonic::Request;

use norn_common::types::{Address, Hash, Transaction, TransactionType, TransactionV2};

// 生成的 protobuf 代码
pub mod blockchain {
    tonic::include_proto!("blockchain");
}

use blockchain::blockchain_service_client::BlockchainServiceClient;
use blockchain::{
    Empty, GetBlockReq, SendTransactionV2Req, SendTransactionWithDataReq, SendTransactionsV2Req,
    Transaction as ProtoTransaction,
};

/// RPC 客户端
#[derive(Clone)]
pub struct BlockchainRpcClient {
    client: BlockchainServiceClient<Channel>,
}

impl BlockchainRpcClient {
    /// 连接到 RPC 服务器
    pub async fn connect(addr: &str) -> Result<Self> {
        let client = BlockchainServiceClient::connect(format!("http://{}", addr))
            .await
            .context("Failed to connect to RPC server")?;

        Ok(Self { client })
    }

    /// 获取当前区块高度
    pub async fn get_block_number(&mut self) -> Result<i64> {
        let request = Request::new(Empty {});

        let response = self
            .client
            .get_block_number(request)
            .await
            .context("Failed to get block number")?;

        Ok(response.into_inner().number as i64)
    }

    /// 根据高度获取区块
    pub async fn get_block_by_number(
        &mut self,
        height: i64,
    ) -> Result<Option<norn_common::types::Block>> {
        let request = Request::new(GetBlockReq {
            number: height as u64,
            hash: String::new(),
            full: true,
        });

        match self.client.get_block_by_number(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if let Some(proto_block) = resp.body {
                    Ok(Some(proto_block.into()))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                if e.code() == tonic::Code::NotFound {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("Failed to get block: {}", e))
                }
            }
        }
    }

    /// 根据哈希获取区块
    pub async fn get_block_by_hash(
        &mut self,
        hash: &Hash,
    ) -> Result<Option<norn_common::types::Block>> {
        let request = Request::new(GetBlockReq {
            number: 0,
            hash: hex::encode(hash.0),
            full: true,
        });

        match self.client.get_block_by_hash(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if let Some(proto_block) = resp.body {
                    Ok(Some(proto_block.into()))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                if e.code() == tonic::Code::NotFound {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("Failed to get block: {}", e))
                }
            }
        }
    }

    /// 发送交易（带完整数据）
    pub async fn send_transaction_with_data(&mut self, tx: &Transaction) -> Result<String> {
        let proto_tx: ProtoTransaction = tx.clone().into();

        let request = Request::new(SendTransactionWithDataReq {
            transaction: Some(proto_tx),
        });

        let response = self
            .client
            .send_transaction_with_data(request)
            .await
            .context("Failed to send transaction")?;

        Ok(response.into_inner().tx_hash)
    }

    /// Send a canonical, signed protocol-V2 transaction.
    pub async fn send_transaction_v2(&mut self, tx: &TransactionV2) -> Result<String> {
        let request = Request::new(SendTransactionV2Req {
            transaction: bincode::serialize(tx).context("Failed to encode TransactionV2")?,
        });

        let response = self
            .client
            .send_transaction_v2(request)
            .await
            .context("Failed to send TransactionV2")?;

        Ok(response.into_inner().tx_hash)
    }

    /// Submit one pre-signed V2 batch. The validator verifies the batch on a
    /// bounded CPU pool and emits one gossip message for the whole batch.
    pub async fn send_transactions_v2(
        &mut self,
        transactions: &[TransactionV2],
    ) -> Result<Vec<String>> {
        let encoded = transactions
            .iter()
            .map(|tx| bincode::serialize(tx).context("Failed to encode TransactionV2"))
            .collect::<Result<Vec<_>>>()?;
        let response = self
            .client
            .send_transactions_v2(Request::new(SendTransactionsV2Req {
                transactions: encoded,
            }))
            .await
            .context("Failed to send TransactionV2 batch")?;
        Ok(response.into_inner().tx_hashes)
    }

    /// Fund a benchmark signer through the node's development faucet.
    ///
    /// This is bootstrap-only: it directly mutates local state, so callers
    /// must invoke it on every validator before any BFT proposal is produced.
    pub async fn fund_account(eth_rpc_addr: &str, address: &Address) -> Result<()> {
        let mut stream = TcpStream::connect(eth_rpc_addr)
            .await
            .with_context(|| format!("Failed to connect to Ethereum RPC {}", eth_rpc_addr))?;
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"dev_faucet","params":["0x{}","100000000000"]}}"#,
            hex::encode(address.0)
        );
        let request = format!(
            "POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            eth_rpc_addr,
            body.len(),
            body
        );
        stream.write_all(request.as_bytes()).await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        let response = String::from_utf8_lossy(&response);
        if !response.contains(r#""result":true"#) {
            anyhow::bail!("dev_faucet failed at {}: {}", eth_rpc_addr, response);
        }
        Ok(())
    }

    /// 发送批量交易
    pub async fn send_batch(
        &mut self,
        transactions: &[Transaction],
    ) -> Result<Vec<Result<String>>> {
        let mut results = Vec::with_capacity(transactions.len());

        for tx in transactions {
            let result = self.send_transaction_with_data(tx).await;
            results.push(result);
        }

        Ok(results)
    }
}

// 实现 From<ProtoTransaction> for Transaction
impl From<ProtoTransaction> for Transaction {
    fn from(proto: ProtoTransaction) -> Self {
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

        let mut receiver = Address::default();
        if let Ok(bytes) = hex::decode(&proto.receiver) {
            if bytes.len() == 20 {
                receiver.0.copy_from_slice(&bytes);
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

        Transaction {
            body: TransactionBody {
                hash,
                address,
                receiver,
                gas: proto.gas as i64,
                nonce: proto.nonce as i64,
                event: hex::decode(&proto.event).unwrap_or_default(),
                opt: hex::decode(&proto.opt).unwrap_or_default(),
                state: hex::decode(&proto.state).unwrap_or_default(),
                data: hex::decode(&proto.data).unwrap_or_default(),
                expire: proto.expire as i64,
                timestamp: proto.timestamp as i64,
                public,
                signature: hex::decode(&proto.signature).unwrap_or_default(),
                tx_type: TransactionType::default(),
                chain_id: None,
                value: None,
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                access_list: None,
                gas_price: None,
            },
        }
    }
}

// 实现 From<Transaction> for ProtoTransaction
impl From<Transaction> for ProtoTransaction {
    fn from(tx: Transaction) -> Self {
        ProtoTransaction {
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
            height: 0,
            block_hash: hex::encode(Hash::default().0),
            index: 0,
        }
    }
}

// 实现 From<ProtoBlock> for Block
impl From<blockchain::Block> for norn_common::types::Block {
    fn from(proto: blockchain::Block) -> Self {
        use norn_common::types::*;

        let proto_header = proto.header.expect("Block header is missing");

        let header = BlockHeader {
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
            height: proto_header.height as i64,
            protocol_version: ProtocolVersion::default(),
            chain_id: ChainId::default(),
            epoch: 0,
            round: 0,
            block_builder: ValidatorId::default(),
            stake_snapshot_hash: StakeSnapshotHash::default(),
            parent_randomness: Hash::default(),
            gas_limit: proto_header.gas_limit as i64,
            base_fee: 0,
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
