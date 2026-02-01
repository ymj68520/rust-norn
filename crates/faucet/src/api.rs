//! HTTP API for faucet service

use super::service::{DispenseResponse, FaucetService, FaucetStatus};
use super::error::FaucetResult;
use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info};

/// Dispense request
#[derive(Debug, Deserialize)]
pub struct DispenseRequest {
    pub address: String,
    pub captcha: Option<String>,
}

/// API error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    pub timestamp: String,
}

/// Success response
#[derive(Debug, Serialize)]
pub struct SuccessResponse<T> {
    pub data: T,
    pub timestamp: String,
}

/// Dispense handler
pub async fn dispense_handler(
    State(service): State<Arc<FaucetService>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<DispenseRequest>,
) -> impl IntoResponse {
    info!("Dispense request from {}: address={}", addr, request.address);

    // Parse address
    let address_bytes = if request.address.starts_with("0x") {
        hex::decode(&request.address[2..])
    } else {
        hex::decode(&request.address)
    };

    let address_bytes = match address_bytes {
        Ok(bytes) if bytes.len() == 20 => bytes,
        _ => {
            return Json(ErrorResponse {
                error: "INVALID_ADDRESS".to_string(),
                message: "Invalid address format".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            })
            .into_response();
        }
    };

    let mut addr_array = [0u8; 20];
    addr_array.copy_from_slice(&address_bytes);

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("Unknown")
        .to_string();

    let ip_addr = addr.ip();

    // Call service
    match service
        .dispense(norn_common::types::Address(addr_array), ip_addr, user_agent)
        .await
    {
        Ok(response) => Json(SuccessResponse {
            data: response,
            timestamp: chrono::Utc::now().to_rfc3339(),
        })
        .into_response(),
        Err(e) => {
            error!("Dispense error: {:?}", e);
            e.into_response()
        }
    }
}

/// Status handler
pub async fn status_handler(
    State(service): State<Arc<FaucetService>>,
) -> FaucetResult<Json<SuccessResponse<FaucetStatus>>> {
    let status = service.get_status().await?;
    Ok(Json(SuccessResponse {
        data: status,
        timestamp: chrono::Utc::now().to_rfc3339(),
    }))
}

/// Health check handler
pub async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Root handler with info
pub async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "Norn Faucet",
        "version": "0.1.0",
        "description": "Production-grade faucet service for Norn blockchain",
        "endpoints": {
            "POST /api/dispense": "Request tokens",
            "GET /api/status": "Get faucet status",
            "GET /health": "Health check",
            "GET /metrics": "Prometheus metrics"
        }
    }))
}
