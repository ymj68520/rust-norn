use crate::proto;
use crate::proto::blockchain_service_server::BlockchainService;
use crate::proto::{GetBlockReq, GetBlockResp, GetTransactionReq, GetTransactionResp, SendTransactionReq, SendTransactionResp};
use tonic::{Request, Response, Status};
use std::sync::Arc;
use norn_core::blockchain::Blockchain;
use norn_common::types::Hash;
use hex;
use tracing::{info, error};

pub struct BlockchainRpcImpl {
    chain: Arc<Blockchain>,
}

impl BlockchainRpcImpl {
    pub fn new(chain: Arc<Blockchain>) -> Self {
        Self { chain }
    }
}

#[tonic::async_trait]
impl BlockchainService for BlockchainRpcImpl {
    async fn get_block_by_hash(
        &self,
        request: Request<GetBlockReq>,
    ) -> Result<Response<GetBlockResp>, Status> {
        let req = request.into_inner();
        let hash_bytes = hex::decode(&req.hash).map_err(|_| Status::invalid_argument("Invalid hash"))?;
        
        let mut h = Hash::default();
        if hash_bytes.len() != 32 {
             return Err(Status::invalid_argument("Hash length must be 32"));
        }
        h.0.copy_from_slice(&hash_bytes);

        if let Some(block) = self.chain.get_block_by_hash(&h).await {
            Ok(Response::new(GetBlockResp {
                timestamp: block.header.timestamp as u64,
                body: Some(block.into()), // Uses From implementation
            }))
        } else {
            Err(Status::not_found("Block not found"))
        }
    }

    async fn get_block_by_number(
        &self,
        request: Request<GetBlockReq>,
    ) -> Result<Response<GetBlockResp>, Status> {
        let req = request.into_inner();
        if let Some(block) = self.chain.get_block_by_height(req.number as i64).await {
             Ok(Response::new(GetBlockResp {
                timestamp: block.header.timestamp as u64,
                body: Some(block.into()), 
            }))
        } else {
            Err(Status::not_found("Block not found"))
        }
    }

    async fn get_transaction_by_hash(
        &self,
        request: Request<GetTransactionReq>,
    ) -> Result<Response<GetTransactionResp>, Status> {
        let req = request.into_inner();
        let hash_bytes = hex::decode(&req.hash).map_err(|_| Status::invalid_argument("Invalid hash"))?;
        
        let mut h = Hash::default();
        if hash_bytes.len() != 32 {
             return Err(Status::invalid_argument("Hash length must be 32"));
        }
        h.0.copy_from_slice(&hash_bytes);

        if let Some(tx) = self.chain.get_transaction_by_hash(&h).await {
            Ok(Response::new(GetTransactionResp {
                body: Some(tx.into()),
            }))
        } else {
            Err(Status::not_found("Transaction not found"))
        }
    }

    async fn send_transaction(
        &self,
        request: Request<SendTransactionReq>,
    ) -> Result<Response<SendTransactionResp>, Status> {
        let req = request.into_inner();
        info!("Received SendTransaction request: type={} receiver={} key={}", req.r#type, req.receiver, req.key);
        
        let db_key = req.key.as_bytes().to_vec();
        let db_val = req.value.as_bytes().to_vec();

        // Generate a dummy transaction hash
        let tx_hash_bytes = norn_common::types::Hash::default().0; 
        let tx_hash_str = hex::encode(tx_hash_bytes);

        // Use a dummy sender address (e.g., default Address)
        let sender_address = norn_common::types::Address::default().0.to_vec();
        
        let task = norn_core::data_processor::DataTask {
            command_type: req.r#type, 
            hash: norn_common::types::Hash(tx_hash_bytes), // Use the dummy hash
            height: 0, // Placeholder
            address: sender_address, 
            key: db_key,
            value: db_val,
        };
        
        // Submit to DataProcessor (it returns (), not a Result)
        self.chain.data_processor.submit_task(task).await;

        info!("Submitted DataTask for transaction: {}", tx_hash_str);

        Ok(Response::new(SendTransactionResp { tx_hash: tx_hash_str }))
    }
}