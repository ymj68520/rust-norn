//! Time synchronization module
//!
//! Translates Go `node/time_syncer.go` to Rust.

use std::sync::{Arc, RwLock, Once};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{interval, Duration};
use rand::{self, Rng};
use tracing::{info, warn, debug};

use crate::metrics;

// ---------------------------------------------------------------------------
// SyncStatus
// ---------------------------------------------------------------------------

/// Time synchronization status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    Initial    = 1,
    Synced     = 2,
    Confirming = 3,
}

// ---------------------------------------------------------------------------
// TimeSyncMsg
// ---------------------------------------------------------------------------

/// Time synchronization message
#[derive(Debug, Clone, Default)]
pub struct TimeSyncMsg {
    pub code:        i8,
    pub req_time:    i64,
    pub rec_req_time: i64,
    pub rsp_time:    i64,
    pub rec_rsp_time: i64,
}

impl TimeSyncMsg {
    /// Serialize to bytes (LE encoding)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(40);
        bytes.push(self.code as u8);
        bytes.extend_from_slice(&self.req_time.to_le_bytes());
        bytes.extend_from_slice(&self.rec_req_time.to_le_bytes());
        bytes.extend_from_slice(&self.rsp_time.to_le_bytes());
        bytes.extend_from_slice(&self.rec_rsp_time.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        if bytes.len() < 40 {
            anyhow::bail!("TimeSyncMsg too short: {} bytes", bytes.len());
        }
        Ok(Self {
            code:         bytes[0] as i8,
            req_time:     i64::from_le_bytes(bytes[1..9].try_into()?),
            rec_req_time: i64::from_le_bytes(bytes[9..17].try_into()?),
            rsp_time:     i64::from_le_bytes(bytes[17..25].try_into()?),
            rec_rsp_time: i64::from_le_bytes(bytes[25..33].try_into()?),
        })
    }
}

// ---------------------------------------------------------------------------
// TimeSyncerConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TimeSyncerConfig {
    pub genesis:               bool,
    pub initial_delta:         i64,
    pub sync_interval_secs:    u64,
    pub confirm_threshold:     i32,
    pub available_threshold_ms: i64,
}

impl Default for TimeSyncerConfig {
    fn default() -> Self {
        Self {
            genesis:               false,
            initial_delta:         0,
            sync_interval_secs:    3,
            confirm_threshold:     5,
            available_threshold_ms: 1000,
        }
    }
}

// ---------------------------------------------------------------------------
// TimeSyncer
// ---------------------------------------------------------------------------

/// Inner state protected by `std::sync::RwLock`
struct TimeSyncerInner {
    status:       SyncStatus,
    delta:        i64,
    confirm_times: i32,
}

/// TimeSyncer manages clock synchronization across the network.
///
/// Port of Go `node/time_syncer.go`.
///
/// Uses `std::sync::RwLock` so the public API is synchronous (matching Go),
/// while the background sync routine is async (spawned as a Tokio task).
pub struct TimeSyncer {
    inner:      Arc<RwLock<TimeSyncerInner>>,
    config:     TimeSyncerConfig,
    start_once: Once,
}

impl TimeSyncer {
    // -- constructors --

    pub fn new(config: TimeSyncerConfig) -> Self {
        let status = if config.genesis { SyncStatus::Synced } else { SyncStatus::Initial };
        let inner = TimeSyncerInner { status, delta: config.initial_delta, confirm_times: 0 };
        metrics::time_syncer_status_set(status as i8);
        Self { inner: Arc::new(RwLock::new(inner)), config, start_once: Once::new() }
    }

    pub fn with_defaults(genesis: bool) -> Self {
        Self::new(TimeSyncerConfig { genesis, ..Default::default() })
    }

    // -- lifecycle --

    /// Start the background sync routine (only once, and only if not genesis).
    pub fn start(&self) {
        self.start_once.call_once(|| {
            if !self.config.genesis {
                let inner  = self.inner.clone();
                let config = self.config.clone();
                tokio::spawn(async move {
                    Self::sync_routine(inner, config).await;
                });
            } else {
                info!("Genesis node – time syncer already synced.");
            }
        });
    }

    // -- public API (synchronous, matches Go interface) --

    /// Get the current logical clock: `physical_time_ms + delta`
    pub fn get_logic_clock(&self) -> i64 {
        let inner = self.inner.read().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        now + inner.delta
    }

    /// Process an incoming time-sync *request*.
    ///
    /// Sets `msg.rsp_time` to the current logical clock and records the response.
    pub fn process_sync_request(&self, msg: &mut TimeSyncMsg) {
        msg.rsp_time = self.get_logic_clock();
        debug!("Processed time sync request, rsp_time={}", msg.rsp_time);
    }

    /// Process an incoming time-sync *response*.
    pub fn process_sync_respond(&self, msg: &TimeSyncMsg) {
        if msg.code != 0 {
            warn!("Remote peer respond time sync error: code={}", msg.code);
            return;
        }

        // NTP-like offset:  delta = (rsp - rec_rsp)/2 + (rec_req - req)/2
        let delta = (msg.rsp_time - msg.rec_rsp_time) / 2
                  + (msg.rec_req_time - msg.req_time) / 2;
        info!("Time sync delta = {}", delta);

        let mut inner = self.inner.write().unwrap();

        match inner.status {
            SyncStatus::Initial => {
                metrics::time_syncer_status_set(SyncStatus::Confirming as i8);
                inner.status = SyncStatus::Confirming;
                inner.delta += delta;
            }
            SyncStatus::Confirming | SyncStatus::Synced => {
                if (delta - inner.delta).abs() < self.config.available_threshold_ms {
                    inner.delta += delta;
                    metrics::time_sync_delta_set((delta - inner.delta + delta) as f64);
                    // Actually: report the raw delta
                    metrics::time_sync_delta_set(delta as f64);
                    if inner.status == SyncStatus::Confirming {
                        inner.confirm_times += 1;
                    }
                    debug!("Time syncer confirm times = {}", inner.confirm_times);
                } else {
                    inner.confirm_times = 0;
                    debug!("Time delta too large: {}ms, reset confirmations",
                           (delta - inner.delta).abs());
                }

                if inner.status == SyncStatus::Confirming
                    && inner.confirm_times >= self.config.confirm_threshold
                {
                    metrics::time_syncer_status_set(SyncStatus::Synced as i8);
                    inner.status = SyncStatus::Synced;
                    info!("Time syncer sync finished.");
                }
            }
        }
    }

    pub fn synced(&self) -> bool {
        let inner = self.inner.read().unwrap();
        inner.status == SyncStatus::Synced
    }

    pub fn get_status(&self) -> SyncStatus {
        let inner = self.inner.read().unwrap();
        inner.status
    }

    pub fn get_delta(&self) -> i64 {
        let inner = self.inner.read().unwrap();
        inner.delta
    }

    pub fn set_delta(&self, delta: i64) {
        let mut inner = self.inner.write().unwrap();
        inner.delta = delta;
    }

    // -- internal --

    async fn sync_routine(inner: Arc<RwLock<TimeSyncerInner>>, config: TimeSyncerConfig) {
        info!("Time syncer routine started, interval={}s", config.sync_interval_secs);
        let mut ticker = interval(Duration::from_secs(config.sync_interval_secs));

        loop {
            ticker.tick().await;

            // TODO: pick a random peer and send a time-sync request.
            // The actual peer I/O is handled by the node layer (see protocol.rs).
            let status = {
                let guard = inner.read().unwrap();
                guard.status
            };
            debug!("Time syncer tick – status={:?}", status);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_syncer_initial() {
        let s = TimeSyncer::with_defaults(false);
        assert_eq!(s.get_status(), SyncStatus::Initial);
        assert!(!s.synced());
    }

    #[test]
    fn test_time_syncer_genesis() {
        let s = TimeSyncer::with_defaults(true);
        assert_eq!(s.get_status(), SyncStatus::Synced);
        assert!(s.synced());
    }

    #[test]
    fn test_logic_clock() {
        let s = TimeSyncer::new(TimeSyncerConfig {
            genesis: true,
            initial_delta: 1000,
            ..Default::default()
        });
        let clock = s.get_logic_clock();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        assert!((clock - (now + 1000)).abs() < 200);
    }

    #[test]
    fn test_sync_respond_initial_to_confirming() {
        let s = TimeSyncer::new(TimeSyncerConfig {
            genesis: false,
            available_threshold_ms: 1000,
            confirm_threshold: 3,
            ..Default::default()
        });

        let msg = TimeSyncMsg {
            code: 0, req_time: 1000, rec_req_time: 1001,
            rsp_time: 2000, rec_rsp_time: 2001,
        };
        s.process_sync_respond(&msg);
        assert_eq!(s.get_status(), SyncStatus::Confirming);
    }

    #[test]
    fn test_sync_respond_to_synced() {
        let s = TimeSyncer::new(TimeSyncerConfig {
            genesis: false,
            available_threshold_ms: 1000,
            confirm_threshold: 2,
            ..Default::default()
        });

        for i in 0..3 {
            let base = 1000 + i * 1000;
            let msg = TimeSyncMsg {
                code: 0,
                req_time:     base,
                rec_req_time: base + 1,
                rsp_time:     base + 1000,
                rec_rsp_time: base + 1001,
            };
            s.process_sync_respond(&msg);
        }
        assert_eq!(s.get_status(), SyncStatus::Synced);
    }

    #[test]
    fn test_time_sync_msg_roundtrip() {
        let original = TimeSyncMsg {
            code: 0, req_time: 12345, rec_req_time: 12346,
            rsp_time: 22345, rec_rsp_time: 22346,
        };
        let bytes = original.to_bytes();
        let decoded = TimeSyncMsg::from_bytes(&bytes).unwrap();
        assert_eq!(original.code, decoded.code);
        assert_eq!(original.req_time, decoded.req_time);
    }
}
