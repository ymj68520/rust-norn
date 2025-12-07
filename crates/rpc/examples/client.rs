use norn_rpc::proto::blockchain_service_client::BlockchainServiceClient;
use norn_rpc::proto::SendTransactionReq;
use tonic::Request;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to the server
    let mut client = BlockchainServiceClient::connect("http://127.0.0.1:50051").await?;

    // Construct the request
    let request = Request::new(SendTransactionReq {
        r#type: "set".to_string(),
        receiver: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
        key: "test_key".to_string(),
        value: "test_value".to_string(),
    });

    println!("Sending transaction...");
    
    // Send the request
    // Note: The server implementation currently returns "Unimplemented" error for logic, 
    // but the RPC call itself should succeed in reaching the server.
    // If it returns Err(Status), that's also a "success" in terms of testing connectivity.
    match client.send_transaction(request).await {
        Ok(response) => println!("RESPONSE={:?}", response),
        Err(e) => println!("RPC Error (Expected if unimplemented): {:?}", e),
    }

    Ok(())
}
