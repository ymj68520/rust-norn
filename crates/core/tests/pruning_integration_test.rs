//! State pruning integration tests
//!
//! This test demonstrates the complete state pruning workflow.

use norn_core::state::{StateHistory, PruningConfig, StatePruningManager};
use norn_common::types::{Address, Hash};
use num_bigint::BigUint;
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::test]
async fn test_complete_pruning_workflow() {
    // Create a state history with max 100 snapshots
    let history = Arc::new(StateHistory::new(100));

    // Create pruning manager with default config
    let config = PruningConfig::default();
    let manager = StatePruningManager::new(config.clone(), history.clone());

    // Create some test snapshots
    let mut accounts = HashMap::new();
    let address = Address([1u8; 20]);
    accounts.insert(address, AccountState {
        address,
        balance: BigUint::from(1000u64),
        nonce: 0,
        code_hash: None,
        storage_root: Hash::default(),
        account_type: AccountType::Normal,
        created_at: 0,
        updated_at: 0,
        deleted: false,
    });

    // Create snapshots at blocks 10000, 11000, 12000, ..., 30000 (21 snapshots)
    // This ensures that when we're at block 30000, we'll keep blocks 20000-30000 (10k max)
    // and prune blocks 10000-19000 (10 snapshots)
    for i in 0..21 {
        let block_number = 10000 + (i * 1000);
        history.create_snapshot(
            block_number,
            Hash([i as u8; 32]),
            accounts.clone(),
            Hash([0u8; 32]),
        ).await.unwrap();
    }

    // Verify we have 21 snapshots
    let snapshots = history.get_all_snapshots().await;
    assert_eq!(snapshots.len(), 21);

    // At block 30000, we should prune blocks older than (30000 - 10000) = 20000
    // But we must keep min 1000 blocks, so cutoff = max(20000, 29000) = 29000
    // This should prune blocks 10000-28000 (19 snapshots), keep 29000-30000 (2 snapshots)
    let result = manager.prune_old_states(30000).await.unwrap();
    assert_eq!(result.snapshots_pruned, 19);
    assert_eq!(result.blocks_freed, 19);

    // Verify we now have only 2 snapshots remaining (29000, 30000)
    let snapshots = history.get_all_snapshots().await;
    assert_eq!(snapshots.len(), 2);

    // Check pruning statistics
    let stats = manager.get_stats().await;
    assert_eq!(stats.total_prunings, 1);
    assert_eq!(stats.snapshots_pruned, 19);
    assert!(stats.bytes_saved > 0);
}

#[tokio::test]
async fn test_pruning_with_intervals() {
    let history = Arc::new(StateHistory::new(100));

    // Create config with prune_interval = 200 blocks
    let config = PruningConfig {
        prune_interval: 200,
        ..Default::default()
    };
    let manager = StatePruningManager::new(config, history.clone());

    // Test should_prune logic
    assert!(!manager.should_prune(100).await);  // Before interval
    assert!(!manager.should_prune(199).await);  // Just before interval
    assert!(manager.should_prune(200).await);   // At interval
    assert!(manager.should_prune(400).await);   // After interval
}

#[tokio::test]
async fn test_aggressive_pruning() {
    let history = Arc::new(StateHistory::new(100));

    // Use aggressive pruning (keep only 100-1000 blocks)
    let config = PruningConfig::aggressive();
    let manager = StatePruningManager::new(config, history.clone());

    // Create snapshots at blocks 100, 200, ..., 2000
    let mut accounts = HashMap::new();
    let address = Address([1u8; 20]);
    accounts.insert(address, AccountState {
        address,
        balance: BigUint::from(1000u64),
        nonce: 0,
        code_hash: None,
        storage_root: Hash::default(),
        account_type: AccountType::Normal,
        created_at: 0,
        updated_at: 0,
        deleted: false,
    });

    for i in 0..20 {
        let block_number = (i + 1) * 100;
        history.create_snapshot(
            block_number,
            Hash([i as u8; 32]),
            accounts.clone(),
            Hash([0u8; 32]),
        ).await.unwrap();
    }

    // At block 5000, aggressive pruning should remove almost everything
    // (keep only last 100 blocks max)
    let result = manager.prune_old_states(5000).await.unwrap();
    assert!(result.snapshots_pruned > 15); // Should prune most snapshots
}

#[tokio::test]
async fn test_archival_mode_no_pruning() {
    let history = Arc::new(StateHistory::new(u32::MAX as usize));

    // Archival mode disables pruning
    let config = PruningConfig::archival();
    let manager = StatePruningManager::new(config, history.clone());

    // Should never prune
    assert!(!manager.should_prune(1000).await);
    assert!(!manager.should_prune(10000).await);
    assert!(!manager.should_prune(u64::MAX).await);

    // Even at very high block numbers, nothing should be pruned
    let result = manager.prune_old_states(100000).await.unwrap();
    assert_eq!(result.snapshots_pruned, 0);
}

// Import types for the test
use norn_core::state::{AccountState, AccountType};
