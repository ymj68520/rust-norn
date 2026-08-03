//! Deterministic, side-effect-free execution overlay.
//!
//! Transaction execution writes here first. Reads observe the overlay before
//! the immutable base state, and the resulting write set is emitted in a
//! canonical address/key order. This prevents transaction order or hash-map
//! iteration order from changing the state transition.

use crate::evm::{
    CodeStorage, CodeStorageCheckpoint, EVMCodeChange, EVMConfig, EVMContext, EVMExecutor,
};
use crate::state::merkle::StateRootCalculator;
use crate::state::{AccountState, AccountStateManager, AccountType, StorageItem};
use norn_common::error::{NornError, Result};
use norn_common::genesis::ProtocolResourceLimits;
use norn_common::types::Address;
use norn_common::types::{TransactionId, TransactionType, TransactionV2};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct V2ExecutionContext {
    pub block_number: u64,
    pub block_timestamp: u64,
    pub block_coinbase: Address,
    pub block_gas_limit: u64,
    pub code_storage: Arc<CodeStorage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OverlayWrite {
    Account {
        address: Address,
        old_state: Option<AccountState>,
        new_state: AccountState,
    },
    Storage {
        address: Address,
        key: Vec<u8>,
        old_value: Option<Vec<u8>>,
        new_value: Vec<u8>,
    },
    Code {
        address: Address,
        old_hash: Option<norn_common::types::Hash>,
        new_hash: norn_common::types::Hash,
        code: Vec<u8>,
        deleted: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    #[error("execution overlay write-set limit exceeded")]
    WriteSetLimitExceeded,
    #[error("overlay account address mismatch")]
    AddressMismatch,
}

#[derive(Debug, Clone)]
struct AccountEntry {
    address: Address,
    old_state: Option<AccountState>,
    new_state: AccountState,
}

#[derive(Debug, Clone)]
struct StorageEntry {
    address: Address,
    key: Vec<u8>,
    old_value: Option<Vec<u8>>,
    new_value: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CodeEntry {
    address: Address,
    old_hash: Option<norn_common::types::Hash>,
    new_hash: norn_common::types::Hash,
    code: Vec<u8>,
    deleted: bool,
}

#[derive(Debug, Clone)]
pub struct ExecutionOverlay {
    accounts: BTreeMap<[u8; 20], AccountEntry>,
    storage: BTreeMap<([u8; 20], Vec<u8>), StorageEntry>,
    code: BTreeMap<[u8; 20], CodeEntry>,
    max_writes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V2ExecutionResult {
    pub transaction_id: TransactionId,
    pub gas_used: u64,
}

#[derive(Debug, Clone)]
pub struct V2BlockExecution {
    pub overlay: ExecutionOverlay,
    pub results: Vec<V2ExecutionResult>,
    pub gas_used: u64,
}

/// Full canonical state checkpoint written with a finalized V2 block. The
/// granular keys are emitted from this value, while the root record provides
/// one bounded recovery object that does not depend on a prior error result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalStateCheckpoint {
    pub state_root: norn_common::types::Hash,
    pub accounts: Vec<AccountState>,
    pub storage: Vec<(Address, Vec<StorageItem>)>,
    pub code: CodeStorageCheckpoint,
}

/// Canonical commitment to deterministic execution results.
pub fn calculate_v2_execution_data_hash(results: &[V2ExecutionResult]) -> norn_common::types::Hash {
    let mut hasher = Sha256::new();
    hasher.update(b"NORN_EXECUTION_RESULTS_V2");
    for result in results {
        hasher.update(result.transaction_id.0 .0);
        hasher.update(result.gas_used.to_be_bytes());
    }
    norn_common::types::Hash(hasher.finalize().into())
}

impl ExecutionOverlay {
    pub fn new(max_writes: usize) -> Result<Self> {
        if max_writes == 0 {
            return Err(NornError::Config(
                "execution overlay write limit must be non-zero".into(),
            ));
        }
        Ok(Self {
            accounts: BTreeMap::new(),
            storage: BTreeMap::new(),
            code: BTreeMap::new(),
            max_writes,
        })
    }

    pub fn max_writes(&self) -> usize {
        self.max_writes
    }

    pub fn write_count(&self) -> usize {
        self.accounts.len() + self.storage.len() + self.code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.write_count() == 0
    }

    pub async fn get_account(
        &self,
        base: &AccountStateManager,
        address: &Address,
    ) -> Result<AccountState> {
        if let Some(entry) = self.accounts.get(&address.0) {
            return Ok(entry.new_state.clone());
        }
        Ok(base
            .get_account(address)
            .await?
            .unwrap_or_else(|| AccountState {
                address: *address,
                balance: BigUint::from(0u64),
                nonce: 0,
                code_hash: None,
                storage_root: Default::default(),
                account_type: AccountType::Normal,
                created_at: 0,
                updated_at: 0,
                deleted: false,
            }))
    }

    pub async fn get_storage(
        &self,
        base: &AccountStateManager,
        address: &Address,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        if let Some(entry) = self.storage.get(&(address.0, key.to_vec())) {
            return Ok(Some(entry.new_value.clone()));
        }
        base.get_storage(address, key).await
    }

    pub async fn write_account(
        &mut self,
        base: &AccountStateManager,
        address: Address,
        mut new_state: AccountState,
    ) -> Result<()> {
        if new_state.address != address {
            return Err(NornError::Internal(
                OverlayError::AddressMismatch.to_string(),
            ));
        }
        if !self.accounts.contains_key(&address.0) && self.write_count() >= self.max_writes {
            return Err(NornError::Internal(
                OverlayError::WriteSetLimitExceeded.to_string(),
            ));
        }
        let old_state = if let Some(entry) = self.accounts.get(&address.0) {
            entry.old_state.clone()
        } else {
            base.get_account(&address).await?
        };
        new_state.address = address;
        self.accounts.insert(
            address.0,
            AccountEntry {
                address,
                old_state,
                new_state,
            },
        );
        Ok(())
    }

    pub async fn write_storage(
        &mut self,
        base: &AccountStateManager,
        address: Address,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<()> {
        let storage_key = (address.0, key.clone());
        if !self.storage.contains_key(&storage_key) && self.write_count() >= self.max_writes {
            return Err(NornError::Internal(
                OverlayError::WriteSetLimitExceeded.to_string(),
            ));
        }
        let old_value = if let Some(entry) = self.storage.get(&storage_key) {
            entry.old_value.clone()
        } else {
            base.get_storage(&address, &key).await?
        };
        self.storage.insert(
            storage_key,
            StorageEntry {
                address,
                key,
                old_value,
                new_value: value,
            },
        );
        Ok(())
    }

    /// Add or replace the code write for an address while retaining the
    /// original pre-transaction code hash.
    pub async fn write_code(
        &mut self,
        base: &AccountStateManager,
        change: EVMCodeChange,
    ) -> anyhow::Result<()> {
        if !self.code.contains_key(&change.address.0) && self.write_count() >= self.max_writes {
            return Err(anyhow::anyhow!(
                "execution overlay write-set limit exceeded"
            ));
        }
        let old_hash = if let Some(entry) = self.code.get(&change.address.0) {
            entry.old_hash
        } else {
            base.get_account(&change.address)
                .await?
                .and_then(|account| account.code_hash)
        };
        self.code.insert(
            change.address.0,
            CodeEntry {
                address: change.address,
                old_hash,
                new_hash: change.code_hash,
                code: change.code,
                deleted: change.deleted,
            },
        );
        Ok(())
    }

    /// Return the exact same write sequence on every node.
    pub fn ordered_writes(&self) -> Vec<OverlayWrite> {
        let mut writes = Vec::with_capacity(self.write_count());
        writes.extend(self.accounts.values().map(|entry| OverlayWrite::Account {
            address: entry.address,
            old_state: entry.old_state.clone(),
            new_state: entry.new_state.clone(),
        }));
        writes.extend(self.storage.values().map(|entry| OverlayWrite::Storage {
            address: entry.address,
            key: entry.key.clone(),
            old_value: entry.old_value.clone(),
            new_value: entry.new_value.clone(),
        }));
        writes.extend(self.code.values().map(|entry| OverlayWrite::Code {
            address: entry.address,
            old_hash: entry.old_hash,
            new_hash: entry.new_hash,
            code: entry.code.clone(),
            deleted: entry.deleted,
        }));
        writes
    }

    /// Serialize the canonical write sequence for the finality batch. These
    /// values are written before the live state manager is changed, so a
    /// flush failure leaves a complete durable replay description.
    pub fn canonical_persistence_values(&self) -> anyhow::Result<Vec<Vec<u8>>> {
        self.ordered_writes()
            .iter()
            .map(|write| norn_common::utils::codec::serialize(write))
            .collect()
    }

    pub async fn canonical_state_checkpoint(
        &self,
        base: &AccountStateManager,
        code_storage: &CodeStorage,
    ) -> anyhow::Result<CanonicalStateCheckpoint> {
        let projected = base.clone();
        self.apply_account_storage_writes(&projected).await?;
        let state_root = StateRootCalculator::new(false)
            .calculate_from_manager(&projected)
            .await?;

        let mut accounts = projected
            .accounts_map()
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        accounts.sort_by_key(|account| account.address.0);

        let mut storage = projected
            .storage_map()
            .iter()
            .map(|entry| {
                let mut items = entry.value().values().cloned().collect::<Vec<_>>();
                items.sort_by(|left, right| left.key.cmp(&right.key));
                (*entry.key(), items)
            })
            .collect::<Vec<_>>();
        storage.sort_by_key(|(address, _)| address.0);

        let projected_code = code_storage.fork().await?;
        for write in self.ordered_writes() {
            if let OverlayWrite::Code {
                address,
                new_hash,
                code,
                deleted,
                ..
            } = write
            {
                projected_code
                    .apply_code_change(address, new_hash, code, deleted)
                    .await?;
            }
        }

        Ok(CanonicalStateCheckpoint {
            state_root,
            accounts,
            storage,
            code: projected_code.checkpoint().await,
        })
    }

    /// Replay state writes recovered from a durable finalized transaction.
    pub async fn apply_persisted_writes(
        writes: &[Vec<u8>],
        base: &AccountStateManager,
        code_storage: &CodeStorage,
    ) -> anyhow::Result<()> {
        // Decode the complete durable write-set before mutating live state. A
        // malformed later record must not leave a partially replayed overlay.
        let decoded_writes: Vec<OverlayWrite> = writes
            .iter()
            .map(|encoded| norn_common::utils::codec::deserialize(encoded))
            .collect::<anyhow::Result<_>>()?;
        for write in decoded_writes {
            match write {
                OverlayWrite::Account {
                    address, new_state, ..
                } => base.set_account(&address, new_state).await?,
                OverlayWrite::Storage {
                    address,
                    key,
                    new_value,
                    ..
                } => {
                    if new_value.is_empty() {
                        base.delete_storage(&address, &key).await?;
                    } else {
                        base.set_storage(&address, key, new_value).await?;
                    }
                }
                OverlayWrite::Code {
                    address,
                    new_hash,
                    code,
                    deleted,
                    ..
                } => {
                    code_storage
                        .apply_code_change(address, new_hash, code, deleted)
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// Apply the prepared writes in canonical order. Atomic persistence is a
    /// later finality-stage concern; this method intentionally performs no
    /// signing or network side effects.
    pub async fn apply(&self, base: &AccountStateManager) -> Result<()> {
        if !self.code.is_empty() {
            return Err(NornError::Internal(
                "code writes require apply_with_code_storage".into(),
            ));
        }
        self.apply_account_storage_writes(base).await
    }

    /// Apply account/storage writes and the content-addressed code write-set.
    pub async fn apply_with_code_storage(
        &self,
        base: &AccountStateManager,
        code_storage: &CodeStorage,
    ) -> Result<()> {
        self.apply_account_storage_writes(base).await?;
        for entry in self.code.values() {
            code_storage
                .apply_code_change(
                    entry.address,
                    entry.new_hash,
                    entry.code.clone(),
                    entry.deleted,
                )
                .await
                .map_err(|error| NornError::Internal(error.to_string()))?;
        }
        Ok(())
    }

    async fn apply_account_storage_writes(&self, base: &AccountStateManager) -> Result<()> {
        for write in self.ordered_writes() {
            match write {
                OverlayWrite::Account {
                    address, new_state, ..
                } => {
                    base.set_account(&address, new_state).await?;
                }
                OverlayWrite::Storage {
                    address,
                    key,
                    new_value,
                    ..
                } => {
                    if new_value.is_empty() {
                        base.delete_storage(&address, &key).await?;
                    } else {
                        base.set_storage(&address, key, new_value).await?;
                    }
                }
                OverlayWrite::Code { .. } => {}
            }
        }
        Ok(())
    }

    /// Calculate the post-execution state root without mutating the live
    /// state.  The manager is cloned first, then the canonical overlay write
    /// sequence is applied to that isolated projection.
    pub async fn projected_state_root(
        &self,
        base: &AccountStateManager,
    ) -> Result<norn_common::types::Hash> {
        let projected = base.clone();
        self.apply_account_storage_writes(&projected).await?;
        StateRootCalculator::new(false)
            .calculate_from_manager(&projected)
            .await
    }

    /// Execute one V2 transaction against the deterministic overlay.
    /// Unsupported transaction shapes fail closed instead of falling back to
    /// direct state mutation.
    pub async fn execute_transaction_v2(
        &mut self,
        base: &AccountStateManager,
        tx: &TransactionV2,
        context: Option<&V2ExecutionContext>,
    ) -> anyhow::Result<V2ExecutionResult> {
        norn_crypto::transaction::verify_transaction_v2(tx)
            .map_err(|e| anyhow::anyhow!("invalid TransactionV2: {}", e))?;
        if tx.tx_type == TransactionType::EVM {
            let context = context.ok_or_else(|| {
                anyhow::anyhow!("EVM TransactionV2 requires a deterministic block context")
            })?;
            return self.execute_evm_transaction_v2(base, tx, context).await;
        }
        if tx.tx_type != TransactionType::Native
            || !tx.data.is_empty()
            || !tx.event.is_empty()
            || !tx.opt.is_empty()
            || !tx.state.is_empty()
            || !tx.access_list.is_empty()
        {
            return Err(anyhow::anyhow!(
                "unsupported TransactionV2 execution shape; EVM/contract payloads require the full deterministic EVM overlay"
            ));
        }
        let receiver = tx.receiver.ok_or_else(|| {
            anyhow::anyhow!(
                "contract creation is not available in the deterministic V2 transfer executor"
            )
        })?;
        if tx.gas_limit < 21_000 {
            return Err(anyhow::anyhow!(
                "TransactionV2 gas limit below transfer minimum"
            ));
        }

        let sender_before = self.get_account(base, &tx.sender).await?;
        if sender_before.nonce != tx.nonce {
            return Err(anyhow::anyhow!(
                "TransactionV2 nonce mismatch: expected {}, got {}",
                sender_before.nonce,
                tx.nonce
            ));
        }
        let receiver_before = self.get_account(base, &receiver).await?;
        let gas_cost = BigUint::from(tx.gas_limit) * BigUint::from(tx.max_fee_per_gas);
        let total_cost = gas_cost.clone() + BigUint::from(tx.value);
        if sender_before.balance < total_cost {
            return Err(anyhow::anyhow!(
                "TransactionV2 sender balance is insufficient"
            ));
        }

        let gas_used = 21_000u64;
        let actual_gas = BigUint::from(gas_used) * BigUint::from(tx.max_fee_per_gas);
        let refund = gas_cost - actual_gas;

        let mut sender_after = sender_before.clone();
        sender_after.balance -= total_cost;
        sender_after.balance += refund;
        sender_after.nonce = sender_after
            .nonce
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("TransactionV2 nonce overflow"))?;

        self.write_account(base, tx.sender, sender_after).await?;
        if receiver != tx.sender {
            let mut receiver_after = receiver_before;
            receiver_after.balance += BigUint::from(tx.value);
            self.write_account(base, receiver, receiver_after).await?;
        }

        Ok(V2ExecutionResult {
            transaction_id: tx.transaction_id,
            gas_used,
        })
    }

    async fn execute_evm_transaction_v2(
        &mut self,
        base: &AccountStateManager,
        tx: &TransactionV2,
        context: &V2ExecutionContext,
    ) -> anyhow::Result<V2ExecutionResult> {
        let receiver = tx.receiver;
        let projected = Arc::new(base.clone());
        // Code is stored outside AccountStateManager.  Account/storage writes
        // from earlier transactions still need to be visible to this EVM
        // invocation, while code writes are supplied by the isolated code
        // store below.
        self.apply_account_storage_writes(&projected).await?;
        let before_accounts = projected.accounts_map();
        let before_storage = projected.storage_map();

        let mut evm_config = EVMConfig::default();
        let chain_id_bytes: [u8; 8] = tx.chain_id.0 .0[24..]
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid V2 chain ID length"))?;
        evm_config.chain_id = u64::from_be_bytes(chain_id_bytes).max(1);
        evm_config.block_gas_limit = context.block_gas_limit;
        let executor = EVMExecutor::with_code_storage_collecting_code_changes(
            projected.clone(),
            evm_config,
            context.code_storage.clone(),
        );
        let evm_context = EVMContext {
            block_number: context.block_number,
            block_timestamp: context.block_timestamp,
            block_coinbase: context.block_coinbase,
            block_gas_limit: context.block_gas_limit,
            tx_gas_price: tx.max_fee_per_gas,
            tx_nonce: Some(tx.nonce),
        };
        let (result, code_changes) = executor
            .execute_with_revm_and_code_changes(
                tx.sender,
                receiver,
                tx.value,
                tx.data.clone(),
                tx.gas_limit,
                &evm_context,
            )
            .await
            .map_err(|e| anyhow::anyhow!("V2 EVM execution failed: {e}"))?;
        if !result.success {
            return Err(anyhow::anyhow!(
                "V2 EVM transaction reverted; failed transactions are not admitted to this block path: error={:?}, output=0x{}",
                result.error,
                hex::encode(&result.output)
            ));
        }
        if result.gas_used > tx.gas_limit {
            return Err(anyhow::anyhow!("V2 EVM execution exceeded transaction gas"));
        }
        if receiver.is_some() && !code_changes.is_empty() {
            return Err(anyhow::anyhow!(
                "nested EVM code creation/destruction is not yet supported by V2 overlay"
            ));
        }
        if receiver.is_none() && code_changes.len() != 1 {
            return Err(anyhow::anyhow!(
                "V2 contract creation must produce exactly one code write"
            ));
        }
        for change in code_changes {
            context
                .code_storage
                .apply_code_change(
                    change.address,
                    change.code_hash,
                    change.code.clone(),
                    change.deleted,
                )
                .await
                .map_err(|e| anyhow::anyhow!("failed to update projected code storage: {e}"))?;
            self.write_code(base, change).await?;
        }

        let after_accounts = projected.accounts_map();
        for entry in after_accounts.iter() {
            let address = *entry.key();
            let new_state = entry.value().clone();
            let changed = before_accounts
                .get(&address)
                .map(|old| old.value() != &new_state)
                .unwrap_or(true);
            if changed {
                self.write_account(base, address, new_state).await?;
            }
        }

        let after_storage = projected.storage_map();
        for entry in after_storage.iter() {
            let address = *entry.key();
            let before_slots = before_storage
                .get(&address)
                .map(|slots| slots.value().clone())
                .unwrap_or_default();
            for (key, item) in entry.value() {
                let changed = before_slots
                    .get(key)
                    .map(|old| old.value != item.value)
                    .unwrap_or(true);
                if changed {
                    self.write_storage(base, address, key.clone(), item.value.clone())
                        .await?;
                }
            }
            for (key, _) in before_slots {
                if !entry.value().contains_key(&key) {
                    self.write_storage(base, address, key, Vec::new()).await?;
                }
            }
        }

        Ok(V2ExecutionResult {
            transaction_id: tx.transaction_id,
            gas_used: result.gas_used,
        })
    }
}

/// Execute a V2 block without mutating the base state. Any error discards the
/// in-memory overlay and therefore cannot leave a partially applied block.
pub async fn execute_v2_block(
    base: &AccountStateManager,
    transactions: &[TransactionV2],
    limits: &ProtocolResourceLimits,
    context: Option<&V2ExecutionContext>,
) -> anyhow::Result<V2BlockExecution> {
    limits.validate()?;
    if transactions.len() > limits.max_transactions_per_block as usize {
        return Err(anyhow::anyhow!("TransactionV2 count exceeds Genesis limit"));
    }
    let execution_context = if let Some(context) = context {
        let code_storage = context
            .code_storage
            .fork()
            .await
            .map_err(|e| anyhow::anyhow!("failed to fork code storage: {e}"))?;
        Some(V2ExecutionContext {
            code_storage: Arc::new(code_storage),
            ..context.clone()
        })
    } else {
        None
    };
    let mut overlay = ExecutionOverlay::new(limits.max_overlay_writes as usize)?;
    let mut results = Vec::with_capacity(transactions.len());
    let mut gas_used = 0u64;
    for tx in transactions {
        let tx_bytes = bincode::serialize(tx)?;
        if tx_bytes.len() > limits.max_transaction_bytes as usize {
            return Err(anyhow::anyhow!("TransactionV2 exceeds Genesis byte limit"));
        }
        if tx.gas_limit > limits.max_transaction_gas {
            return Err(anyhow::anyhow!("TransactionV2 exceeds Genesis gas limit"));
        }
        let next_gas = gas_used
            .checked_add(21_000)
            .ok_or_else(|| anyhow::anyhow!("block gas overflow"))?;
        if next_gas > limits.max_block_gas {
            return Err(anyhow::anyhow!("block gas limit exceeded"));
        }
        let result = overlay
            .execute_transaction_v2(base, tx, execution_context.as_ref())
            .await?;
        gas_used = gas_used
            .checked_add(result.gas_used)
            .ok_or_else(|| anyhow::anyhow!("block gas overflow"))?;
        results.push(result);
    }
    Ok(V2BlockExecution {
        overlay,
        results,
        gas_used,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AccountStateConfig, AccountType};
    use norn_common::types::{ChainId, ProtocolVersion, PublicKey, TransactionId, TransactionType};
    use norn_crypto::ecdsa::KeyPair;
    use norn_crypto::transaction::{sign_transaction_v2, verify_transaction_v2};
    use num_bigint::BigUint;
    use sha2::{Digest, Sha256};

    fn account(address: Address, balance: u64) -> AccountState {
        AccountState {
            address,
            balance: BigUint::from(balance),
            nonce: 0,
            code_hash: None,
            storage_root: Default::default(),
            account_type: AccountType::Normal,
            created_at: 0,
            updated_at: 0,
            deleted: false,
        }
    }

    #[tokio::test]
    async fn overlay_reads_own_writes_and_orders_keys() {
        let base = AccountStateManager::new(AccountStateConfig::default());
        let first = Address([2; 20]);
        let second = Address([1; 20]);
        base.set_account(&first, account(first, 10)).await.unwrap();
        base.set_account(&second, account(second, 20))
            .await
            .unwrap();

        let mut overlay = ExecutionOverlay::new(4).unwrap();
        let mut updated = overlay.get_account(&base, &first).await.unwrap();
        updated.balance += BigUint::from(5u64);
        overlay.write_account(&base, first, updated).await.unwrap();
        overlay
            .write_storage(&base, first, vec![2], vec![9])
            .await
            .unwrap();
        overlay
            .write_storage(&base, second, vec![1], vec![8])
            .await
            .unwrap();

        assert_eq!(
            overlay.get_account(&base, &first).await.unwrap().balance,
            BigUint::from(15u64)
        );
        let writes = overlay.ordered_writes();
        assert!(matches!(writes[0], OverlayWrite::Account { address, .. } if address == first));
        assert!(matches!(writes[1], OverlayWrite::Storage { address, .. } if address == second));
        assert!(matches!(writes[2], OverlayWrite::Storage { address, .. } if address == first));
    }

    #[tokio::test]
    async fn overlay_enforces_write_limit() {
        let base = AccountStateManager::new(AccountStateConfig::default());
        let mut overlay = ExecutionOverlay::new(1).unwrap();
        let address = Address([1; 20]);
        overlay
            .write_storage(&base, address, vec![1], vec![1])
            .await
            .unwrap();
        assert!(overlay
            .write_storage(&base, address, vec![2], vec![2])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn v2_block_execution_is_side_effect_free_until_apply() {
        let base = AccountStateManager::new(AccountStateConfig::default());
        let keypair = KeyPair::random();
        let mut address_hash = Sha256::new();
        address_hash.update(keypair.public_key().to_encoded_point(true).as_bytes());
        let sender = Address(address_hash.finalize()[..20].try_into().unwrap());
        let receiver = Address([4; 20]);
        base.set_account(&sender, account(sender, 100_000))
            .await
            .unwrap();
        base.set_account(&receiver, account(receiver, 0))
            .await
            .unwrap();

        let mut tx = TransactionV2 {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(norn_common::types::Hash([1; 32])),
            nonce: 0,
            sender,
            receiver: Some(receiver),
            value: 10,
            gas_limit: 21_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 1,
            data: vec![],
            event: vec![],
            opt: vec![],
            state: vec![],
            expire: None,
            timestamp: 1,
            tx_type: TransactionType::Native,
            access_list: vec![],
            public_key: PublicKey::default(),
            signature: [0; 64],
            transaction_id: TransactionId::default(),
        };
        sign_transaction_v2(&keypair, &mut tx).unwrap();
        verify_transaction_v2(&tx).unwrap();

        let execution = execute_v2_block(
            &base,
            &[tx.clone()],
            &ProtocolResourceLimits::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(execution.results.len(), 1);
        assert_eq!(
            base.get_account(&sender).await.unwrap().unwrap().balance,
            BigUint::from(100_000u64)
        );
        assert_eq!(execution.overlay.write_count(), 2);

        execution.overlay.apply(&base).await.unwrap();
        assert_eq!(
            base.get_account(&receiver).await.unwrap().unwrap().balance,
            BigUint::from(10u64)
        );

        let mut self_transfer = tx.clone();
        self_transfer.receiver = Some(sender);
        self_transfer.value = 7;
        self_transfer.nonce = 1;
        sign_transaction_v2(&keypair, &mut self_transfer).unwrap();
        execute_v2_block(
            &base,
            &[self_transfer],
            &ProtocolResourceLimits::default(),
            None,
        )
        .await
        .unwrap()
        .overlay
        .apply(&base)
        .await
        .unwrap();
        assert_eq!(
            base.get_account(&sender).await.unwrap().unwrap().balance,
            BigUint::from(57_983u64)
        );

        let mut unsupported = tx;
        unsupported.data = vec![0x01];
        sign_transaction_v2(&keypair, &mut unsupported).unwrap();
        assert!(execute_v2_block(
            &base,
            &[unsupported],
            &ProtocolResourceLimits::default(),
            None,
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn v2_evm_transfer_is_side_effect_free_until_apply() {
        let base = AccountStateManager::new(AccountStateConfig::default());
        let keypair = KeyPair::random();
        let mut address_hash = Sha256::new();
        address_hash.update(keypair.public_key().to_encoded_point(true).as_bytes());
        let sender = Address(address_hash.finalize()[..20].try_into().unwrap());
        let receiver = Address([5; 20]);
        let initial_balance = 100_000_000u64;
        base.set_account(&sender, account(sender, initial_balance))
            .await
            .unwrap();
        base.set_account(&receiver, account(receiver, 0))
            .await
            .unwrap();

        let mut tx = TransactionV2 {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(norn_common::types::Hash([1; 32])),
            nonce: 0,
            sender,
            receiver: Some(receiver),
            value: 10,
            gas_limit: 100_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 1,
            data: vec![],
            event: vec![],
            opt: vec![],
            state: vec![],
            expire: None,
            timestamp: 1,
            tx_type: TransactionType::EVM,
            access_list: vec![],
            public_key: PublicKey::default(),
            signature: [0; 64],
            transaction_id: TransactionId::default(),
        };
        sign_transaction_v2(&keypair, &mut tx).unwrap();

        let context = V2ExecutionContext {
            block_number: 1,
            block_timestamp: 2,
            block_coinbase: Address([0; 20]),
            block_gas_limit: 10_000_000,
            code_storage: Arc::new(CodeStorage::new()),
        };
        let execution = execute_v2_block(
            &base,
            &[tx],
            &ProtocolResourceLimits::default(),
            Some(&context),
        )
        .await
        .unwrap();

        assert_eq!(
            base.get_account(&sender).await.unwrap().unwrap().balance,
            BigUint::from(initial_balance)
        );
        assert_eq!(
            base.get_account(&receiver).await.unwrap().unwrap().balance,
            BigUint::from(0u64)
        );
        assert!(execution.gas_used >= 21_000);

        execution.overlay.apply(&base).await.unwrap();
        let sender_after = base.get_account(&sender).await.unwrap().unwrap();
        let receiver_after = base.get_account(&receiver).await.unwrap().unwrap();
        assert_eq!(sender_after.nonce, 1);
        assert_eq!(receiver_after.balance, BigUint::from(10u64));
        assert!(sender_after.balance < BigUint::from(initial_balance - 10));
    }

    #[tokio::test]
    async fn v2_evm_contract_creation_is_isolated_until_apply() {
        let base = AccountStateManager::new(AccountStateConfig::default());
        let keypair = KeyPair::random();
        let mut address_hash = Sha256::new();
        address_hash.update(keypair.public_key().to_encoded_point(true).as_bytes());
        let sender = Address(address_hash.finalize()[..20].try_into().unwrap());
        base.set_account(&sender, account(sender, 1_000_000_000))
            .await
            .unwrap();

        // Init code copies a five-byte runtime from offset 0x0c and returns
        // it.  The runtime returns an empty value when called.
        let init_code = vec![
            0x60, 0x05, 0x60, 0x0c, 0x60, 0x00, 0x39, 0x60, 0x05, 0x60, 0x00, 0xf3, 0x60, 0x00,
            0x60, 0x00, 0xf3,
        ];
        let runtime_code = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
        let mut tx = TransactionV2 {
            protocol_version: ProtocolVersion(2),
            chain_id: ChainId(norn_common::types::Hash([1; 32])),
            nonce: 0,
            sender,
            receiver: None,
            value: 0,
            gas_limit: 500_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 1,
            data: init_code,
            event: vec![],
            opt: vec![],
            state: vec![],
            expire: None,
            timestamp: 1,
            tx_type: TransactionType::EVM,
            access_list: vec![],
            public_key: PublicKey::default(),
            signature: [0; 64],
            transaction_id: TransactionId::default(),
        };
        sign_transaction_v2(&keypair, &mut tx).unwrap();

        let code_storage = Arc::new(CodeStorage::new());
        let context = V2ExecutionContext {
            block_number: 1,
            block_timestamp: 2,
            block_coinbase: Address([0; 20]),
            block_gas_limit: 10_000_000,
            code_storage: code_storage.clone(),
        };
        let execution = execute_v2_block(
            &base,
            &[tx],
            &ProtocolResourceLimits::default(),
            Some(&context),
        )
        .await
        .unwrap();

        let contract = execution
            .overlay
            .ordered_writes()
            .into_iter()
            .find_map(|write| match write {
                OverlayWrite::Code {
                    address,
                    deleted: false,
                    ..
                } => Some(address),
                _ => None,
            })
            .expect("CREATE must produce a code write");
        assert_eq!(code_storage.get_code_hash(&contract).await.unwrap(), None);
        assert_eq!(
            base.get_account(&contract).await.unwrap(),
            None,
            "base state must not observe CREATE before finality"
        );

        execution
            .overlay
            .apply_with_code_storage(&base, code_storage.as_ref())
            .await
            .unwrap();
        let deployed_code = code_storage.get_code_by_address(&contract).await.unwrap();
        assert_eq!(deployed_code, Some(runtime_code));
        assert_eq!(
            base.get_account(&contract)
                .await
                .unwrap()
                .unwrap()
                .code_hash
                .is_some(),
            true
        );
    }
}
