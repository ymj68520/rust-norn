//! Protocol handlers module
//!
//! Translates Go `node/request.go`, `node/respond.go`, and `node/handler_funcs.go`
//! to Rust.

use std::sync::Arc;
use tracing::{info, warn, debug, error};

use crate::time_syncer::{TimeSyncer, SyncStatus, TimeSyncMsg};

// ========================================================================
// Sync Status Message
// ========================================================================

/// Sync status message exchanged between peers
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncStatusMsg {
    /// Latest block height (-1 if not synced)
    pub latest_height: i64,
    /// Latest block hash
    pub latest_hash: norn_common::types::Hash,
    /// Buffered start height
    pub buffered_start_height: i64,
    /// Buffered end height (-1 if no buffered blocks)
    pub buffered_end_height: i64,
}

impl Default for SyncStatusMsg {
    fn default() -> Self {
        Self {
            latest_height:        -1,
            latest_hash:          norn_common::types::Hash::default(),
            buffered_start_height: 0,
            buffered_end_height:  -1,
        }
    }
}

// ========================================================================
// Block sync state
// ========================================================================

/// Block syncer states (from Go `node/block_syncer.go`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockSyncState {
    Paused       = 0x00,
    BlockSyncing = 0x01,
    BufferSyncing = 0x02,
    Synced       = 0x03,
}

// ========================================================================
// Protocol Context
// ========================================================================

/// Context shared by protocol handlers.
/// This is a lightweight cloneable handle (all fields are Arcs).
#[derive(Clone)]
pub struct ProtocolContext {
    pub chain:      Arc<norn_core::blockchain::Blockchain>,
    pub tx_pool:    Arc<norn_core::txpool::TxPool>,
    pub time_syncer: Arc<TimeSyncer>,
    pub genesis:    bool,
}

impl ProtocolContext {
    pub fn new(
        chain:      Arc<norn_core::blockchain::Blockchain>,
        tx_pool:    Arc<norn_core::txpool::TxPool>,
        time_syncer: Arc<TimeSyncer>,
    ) -> Self {
        Self { chain, tx_pool, time_syncer, genesis: false }
    }

    pub fn with_genesis(mut self, genesis: bool) -> Self {
        self.genesis = genesis;
        self
    }
}

// ========================================================================
// Request helpers  (Go: node/request.go)
// ========================================================================

pub struct ProtocolRequest;

impl ProtocolRequest {
    /// Request a block by hash
    pub fn block_with_hash(_block_hash: &norn_common::types::Hash, _peer_id: &str) {
        // TODO: send via NetworkService
        debug!("REQ GetBlockBodies for peer {}", _peer_id);
    }

    /// Request sync status with current height
    pub fn sync_status(height: i64, _peer_id: &str) {
        let bytes = height.to_le_bytes();
        debug!("REQ SyncStatus height={} peer={}", height, _peer_id);
        // TODO: send bytes via NetworkService
    }

    /// Request blocks at a specific height
    pub fn sync_get_block(height: i64, _peer_id: &str) {
        let bytes = height.to_le_bytes();
        debug!("REQ SyncGetBlock height={} peer={}", height, _peer_id);
        // TODO: send bytes via NetworkService
    }

    /// Send a time sync request
    pub fn time_sync(msg: &TimeSyncMsg, _peer_id: &str) {
        let _bytes = msg.to_bytes();
        debug!("REQ TimeSync peer={}", _peer_id);
        // TODO: send bytes via NetworkService
    }
}

// ========================================================================
// Response helpers  (Go: node/respond.go)
// ========================================================================

pub struct ProtocolResponse;

impl ProtocolResponse {
    /// Respond with a block
    pub fn get_block_bodies(_block: &norn_common::types::Block, _peer_id: &str) {
        debug!("RSP BlockBodies peer={}", _peer_id);
        // TODO: serialize + send via NetworkService
    }

    /// Respond with a pooled transaction
    pub fn get_pooled_transaction(_tx: &norn_common::types::Transaction, _peer_id: &str) {
        debug!("RSP PooledTransaction peer={}", _peer_id);
    }

    /// Respond with a sync block
    pub fn sync_get_block(_block: &norn_common::types::Block, _peer_id: &str) {
        debug!("RSP SyncBlock peer={}", _peer_id);
    }

    /// Respond with sync status
    pub fn get_sync_status(_msg: &SyncStatusMsg, _peer_id: &str) {
        debug!("RSP SyncStatus peer={}", _peer_id);
    }

    /// Respond to time sync
    pub fn time_sync(_msg: &TimeSyncMsg, _peer_id: &str) {
        debug!("RSP TimeSync peer={}", _peer_id);
    }
}

// ========================================================================
// Handler functions  (Go: node/handler_funcs.go)
// ========================================================================

pub struct ProtocolHandlers;

impl ProtocolHandlers {
    /// Handle StatusMsg – extract remote height
    pub fn handle_status_msg(_payload: &[u8]) {
        if _payload.len() >= 8 {
            let height = i64::from_le_bytes([
                _payload[0], _payload[1], _payload[2], _payload[3],
                _payload[4], _payload[5], _payload[6], _payload[7],
            ]);
            debug!("Remote height = {}", height);
        }
    }

    /// Handle NewBlockMsg
    pub async fn handle_new_block_msg(
        _ctx: &ProtocolContext,
        payload: &[u8],
    ) {
        let _block: norn_common::types::Block =
            match norn_common::utils::codec::deserialize(payload) {
                Ok(b) => b,
                Err(e) => { warn!("Deserialize block failed: {}", e); return; }
            };
        debug!("Received new block from peer");
        // TODO: add to syncer / chain
    }

    /// Handle BlockBodiesMsg
    pub async fn handle_block_msg(
        _ctx: &ProtocolContext,
        payload: &[u8],
    ) {
        let _block: norn_common::types::Block =
            match norn_common::utils::codec::deserialize(payload) {
                Ok(b) => b,
                Err(e) => { warn!("Deserialize block failed: {}", e); return; }
            };
        debug!("Received block bodies from peer");
    }

    /// Handle SyncStatusReq
    pub fn handle_sync_status_req(_ctx: &ProtocolContext) -> SyncStatusMsg {
        SyncStatusMsg::default()
    }

    /// Handle SyncStatusMsg
    pub fn handle_sync_status_msg(_payload: &[u8]) {
        let _status: SyncStatusMsg =
            match norn_common::utils::codec::deserialize(_payload) {
                Ok(s) => s,
                Err(e) => { warn!("Deserialize SyncStatusMsg failed: {}", e); return; }
            };
        debug!("Received sync status: height={}", _status.latest_height);
    }

    /// Handle SyncGetBlocksMsg
    pub fn handle_sync_get_blocks_msg(
        _ctx: &ProtocolContext,
        payload: &[u8],
        _peer_id: &str,
    ) {
        if payload.len() < 8 { return; }
        let height = i64::from_le_bytes([
            payload[0], payload[1], payload[2], payload[3],
            payload[4], payload[5], payload[6], payload[7],
        ]);
        debug!("Peer requested blocks at height={}", height);
        // TODO: fetch block from chain and respond
    }

    /// Handle SyncBlocksMsg
    pub async fn handle_sync_block_msg(
        _ctx: &ProtocolContext,
        payload: &[u8],
    ) {
        let _block: norn_common::types::Block =
            match norn_common::utils::codec::deserialize(payload) {
                Ok(b) => b,
                Err(e) => { warn!("Deserialize sync block failed: {}", e); return; }
            };
        debug!("Received sync block from peer");
    }

    /// Handle TimeSyncReq
    pub fn handle_time_sync_req(
        ctx: &ProtocolContext,
        payload: &[u8],
        peer_id: &str,
    ) {
        let mut msg: TimeSyncMsg =
            match TimeSyncMsg::from_bytes(payload) {
                Ok(m) => m,
                Err(e) => { warn!("Deserialize TimeSyncMsg failed: {}", e); return; }
            };
        ctx.time_syncer.process_sync_request(&mut msg);
        ProtocolResponse::time_sync(&msg, peer_id);
    }

    /// Handle TimeSyncRsp
    pub fn handle_time_sync_rsp(
        ctx: &ProtocolContext,
        payload: &[u8],
        _peer_id: &str,
    ) {
        let msg: TimeSyncMsg =
            match TimeSyncMsg::from_bytes(payload) {
                Ok(m) => m,
                Err(e) => { warn!("Deserialize TimeSyncRsp failed: {}", e); return; }
            };
        // Set rec_rsp_time to local logic clock before processing
        let mut msg = msg;
        msg.rec_rsp_time = ctx.time_syncer.get_logic_clock();
        ctx.time_syncer.process_sync_respond(&msg);
    }
}

// ========================================================================
// VRF verification  (Go: node/handler_funcs.go :: verifyBlockVRF)
// ========================================================================

/// Verify a block's VRF proof.
/// Port of Go `verifyBlockVRF`.
pub fn verify_block_vrf(_block: &norn_common::types::Block) -> bool {
    // TODO: Implement proper VRF verification using norn_crypto
    debug!("VRF verification (placeholder) for block");
    true
}
