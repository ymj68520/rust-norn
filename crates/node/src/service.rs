use anyhow::{anyhow, Result};
use k256::ecdsa::signature::Signer;
use k256::ecdsa::SigningKey;
use norn_common::chain_context::{
    ChainContext, PeerRole, MAX_BLOCK_MESSAGE_BYTES, MAX_TRANSACTION_BATCH_MESSAGE_BYTES,
    MAX_TRANSACTION_MESSAGE_BYTES,
};
use norn_common::consensus_types::{
    CanonicalFinalizedTip, CommitCertificate, ConsensusEnvelope, ConsensusMessage,
    MAX_CONSENSUS_ENVELOPE_BYTES,
};
use norn_common::genesis::ProtocolResourceLimits;
use norn_common::types::ValidatorId;
use norn_core::blockchain::Blockchain;
use norn_core::consensus::driver::{
    CommitValidationResult, ConsensusActionExecutor, ConsensusDriver, ConsensusDriverAction,
    ConsensusDriverEvent, ConsensusDriverHandle, FinalityPreparationResult, PreparedFinality,
    ProposalValidationResult, RetryableConsensusActionError, TimeoutStep, VoteValidationResult,
};
use norn_core::consensus::povf::PoVFEngine;
use norn_core::consensus::producer::{BlockProducer, BlockProducerConfig};
use norn_core::consensus::safety_store::{ConsensusSigner, PersistentSafetyStore};
use norn_core::consensus::types::{ConsensusConfig, ProposalSigner};
use norn_core::evm::{CodeStorage, EVMConfig, EVMExecutor};
use norn_core::finality::{DurableCommitOutcome, FinalityStore};
use norn_core::state::merkle::StateRootCalculator;
use norn_core::state::{AccountStateConfig, AccountStateManager};
use norn_core::txpool::TxPool;
use norn_core::txpool_v2::TransactionV2Pool;
use norn_network::{NetworkAuthConfig, NetworkCommand, NetworkService, ValidatorHandshakeIdentity};
use norn_storage::SledDB;

use crate::config::{validate_validator_key_match, NetworkMode, NodeConfig, NodeRole};
use crate::keystore::NodeKeyStore;
use crate::manager::PeerManager;
use crate::syncer::BlockSyncer;
use crate::tx_handler::TxHandler;
use async_trait::async_trait;
use libp2p::identity::Keypair;
use norn_rpc::{create_ethereum_rpc, start_ethereum_rpc_server, start_rpc_server};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::signal;
use tracing::{debug, error, info, warn};

use crate::metrics::MetricsCollector;
use crate::monitoring::MonitoringServer;

pub struct EcdsaConsensusSigner {
    signing_key: SigningKey,
    validator_id: ValidatorId,
}

impl EcdsaConsensusSigner {
    pub fn new(signing_key: SigningKey, validator_id: ValidatorId) -> Self {
        Self {
            signing_key,
            validator_id,
        }
    }
}

impl ConsensusSigner for EcdsaConsensusSigner {
    fn sign_canonical_bytes(&self, bytes: &[u8]) -> Result<[u8; 64]> {
        let sig: k256::ecdsa::Signature = self
            .signing_key
            .try_sign(bytes)
            .map_err(|e| anyhow!("ECDSA signing failed: {:?}", e))?;
        let sig_canonical = sig.normalize_s().unwrap_or(sig);
        let bytes_ref = sig_canonical.to_bytes();
        let arr: [u8; 64] = bytes_ref
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("Invalid signature length"))?;
        Ok(arr)
    }
}

impl ProposalSigner for EcdsaConsensusSigner {
    fn validator_id(&self) -> ValidatorId {
        self.validator_id
    }

    fn sign_proposal(&self, sign_bytes: &[u8]) -> Result<[u8; 64]> {
        self.sign_canonical_bytes(sign_bytes)
    }
}

/// Shared finality coordinator used by the single-writer consensus driver and
/// by recovery paths.  It owns the durable finality ordering; callers must
/// not perform the individual state-machine/finality writes themselves.
#[derive(Clone)]
struct FinalityContext {
    chain_context: ChainContext,
    consensus: Arc<PoVFEngine>,
    finality_store: Arc<FinalityStore>,
    state_manager: Arc<AccountStateManager>,
    tx_pool_v2: Arc<TransactionV2Pool>,
    resource_limits: ProtocolResourceLimits,
    evm_executor: Arc<EVMExecutor>,
    activation_lock: Arc<tokio::sync::Mutex<()>>,
    network: Arc<NetworkService>,
}

enum FinalityPersistenceError {
    Persistence(anyhow::Error),
    Indeterminate(anyhow::Error),
    Activation(anyhow::Error),
}

impl FinalityContext {
    async fn prepare_commit(&self, commit: CommitCertificate) -> Result<FinalityPreparationResult> {
        if commit.protocol_version != self.chain_context.protocol_version
            || commit.chain_id != self.chain_context.chain_id
        {
            return Err(anyhow!(
                "UnsupportedProtocolVersion: finalized certificate is not for the active V2 chain"
            ));
        }
        let canonical_tip = self
            .finality_store
            .recover_canonical_tip()
            .await?
            .ok_or_else(|| anyhow!("canonical finalized tip is unavailable"))?;
        if canonical_tip.height > commit.height {
            let persisted = self
                .finality_store
                .recover_finalized_v2(commit.height)
                .await?
                .ok_or_else(|| anyhow!("replayed finalized certificate has no durable record"))?;
            self.verify_replayed_or_equivalent_certificate(&persisted, &commit)
                .await?;
            return Ok(FinalityPreparationResult::AlreadyDurable(commit));
        }
        let next_height = commit
            .height
            .checked_add(1)
            .ok_or_else(|| anyhow!("finalized height overflow"))?;
        let next_snapshot = self
            .consensus
            .state_machine
            .read()
            .await
            .snapshot_for_height(next_height)?;
        if canonical_tip.height > 0
            || canonical_tip.state_root != norn_common::types::Hash::default()
        {
            let current_root = StateRootCalculator::new(false)
                .calculate_from_manager(&self.state_manager)
                .await?;
            if current_root != canonical_tip.state_root {
                return Err(anyhow!(
                    "execution parent state root does not match canonical finalized tip"
                ));
            }
        }
        let durable_v2 = self
            .finality_store
            .recover_finalized_v2_with_state_and_checkpoint(commit.height)
            .await?;
        let (finalized, state_write_values, checkpoint) =
            if let Some((persisted, state_writes, checkpoint)) = durable_v2 {
                if persisted.block.header.height < 0
                    || persisted.block.header.height as u64 != commit.height
                    || persisted.commit.block_id != commit.block_id
                {
                    return Err(anyhow!(
                        "durable finalized payload conflicts with received certificate"
                    ));
                }
                if persisted.commit != commit {
                    self.verify_replayed_or_equivalent_certificate(&persisted, &commit)
                        .await?;
                    return Ok(FinalityPreparationResult::AlreadyDurable(commit));
                }
                let checkpoint = checkpoint.ok_or_else(|| {
                    anyhow!("durable V2 finality is missing canonical state checkpoint")
                })?;
                (persisted, state_writes, checkpoint)
            } else {
                if !self
                    .consensus
                    .pin_v2_candidate_for_finality(commit.height, commit.round, commit.block_id)
                    .await
                {
                    return Err(anyhow!(
                        "finality candidate is not retained for pending finality"
                    ));
                }
                let finalized = self.consensus.finalize_block_v2(commit).await?;
                let execution = self
                    .consensus
                    .execute_v2_block_for_finality(
                        &finalized.block,
                        &self.state_manager,
                        &self.resource_limits,
                        &self.chain_context,
                        self.evm_executor.code_storage(),
                    )
                    .await?;
                let state_writes = execution.overlay.canonical_persistence_values()?;
                let checkpoint = execution
                    .overlay
                    .canonical_state_checkpoint(
                        &self.state_manager,
                        self.evm_executor.code_storage(),
                    )
                    .await?;
                (finalized, state_writes, checkpoint)
            };
        Ok(FinalityPreparationResult::Prepared(PreparedFinality {
            finalized,
            state_write_values,
            checkpoint,
            next_snapshot,
        }))
    }

    async fn persist_prepared(
        &self,
        prepared: PreparedFinality,
    ) -> std::result::Result<(), FinalityPersistenceError> {
        // Multiple validators can independently form the same certificate.
        // Serialize durable commit plus in-memory activation so a duplicate
        // cannot observe the old height after the first activation has already
        // advanced the state machine.
        let _activation_guard = self.activation_lock.lock().await;
        let persistence_started = std::time::Instant::now();
        let persistence_transactions = prepared.finalized.block.transactions.len();
        let commit_status = match self
            .finality_store
            .commit_finalized_transaction_with_state_and_checkpoint_and_snapshot(
                &prepared.finalized,
                &prepared.state_write_values,
                Some(&prepared.checkpoint),
                Some(&prepared.next_snapshot),
            )
            .await
        {
            Ok(status) => status,
            Err(error) => {
                // A post-apply flush error is ambiguous. Re-read the exact
                // FinalizeTransactionId marker set before deciding whether
                // to retry, activate, or fail-stop.
                match self
                    .finality_store
                    .reconcile_finalized_transaction(
                        &prepared.finalized,
                        &prepared.state_write_values,
                        Some(&prepared.checkpoint),
                        Some(&prepared.next_snapshot),
                    )
                    .await
                {
                    Ok(DurableCommitOutcome::Applied)
                    | Ok(DurableCommitOutcome::AlreadyApplied) => {
                        norn_core::finality::FinalityCommitResult::AlreadyCommitted
                    }
                    Ok(DurableCommitOutcome::NotApplied) => {
                        return Err(FinalityPersistenceError::Persistence(error));
                    }
                    Ok(DurableCommitOutcome::Indeterminate) => {
                        return Err(FinalityPersistenceError::Indeterminate(anyhow!(
                            "finality persistence outcome is indeterminate after write error: {error}"
                        )));
                    }
                    Err(reconcile_error) => {
                        return Err(FinalityPersistenceError::Indeterminate(anyhow!(
                            "unable to reconcile finality write outcome: {error}; reconciliation failed: {reconcile_error}"
                        )));
                    }
                }
            }
        };

        if let Err(error) = self.activate_prepared(&prepared).await {
            return Err(FinalityPersistenceError::Activation(error));
        }

        info!(
            "Finalized V2 block {:?} at height {}; transactions={}; persistence_elapsed_ms={}; durable finality status {:?}",
            prepared.finalized.block.header.block_hash,
            prepared.finalized.block.header.height,
            persistence_transactions,
            persistence_started.elapsed().as_millis(),
            commit_status
        );
        // A validator can miss the Commit gossip while it is reconnecting or
        // while the gossipsub subscription is converging.  Once this node has
        // durably advanced its canonical tip, request the next certificate so
        // the ordered finality stream can repair that gap without waiting for
        // another peer-authentication event.
        self.request_next_finality().await;
        Ok(())
    }

    async fn request_next_finality(&self) {
        let Ok(Some(tip)) = self.finality_store.recover_canonical_tip().await else {
            return;
        };
        let Ok(height) = tip.next_height() else {
            warn!(
                "Cannot request next V2 finality after height {}",
                tip.height
            );
            return;
        };
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::FinalityRequest { height },
        };
        let Ok(bytes) = bincode::serialize(&envelope) else {
            warn!(
                "Failed to encode next V2 finality request at height {}",
                height
            );
            return;
        };
        if let Err(error) = self
            .network
            .control_tx
            .send(NetworkCommand::BroadcastConsensus(bytes))
            .await
        {
            warn!(
                "Failed to enqueue next V2 finality request at height {}: {}",
                height, error
            );
        }
    }

    async fn activate_prepared(&self, prepared: &PreparedFinality) -> Result<()> {
        let finalized = &prepared.finalized;
        let current_height = self.consensus.state_machine.read().await.height;
        if current_height > finalized.commit.height {
            let durable = self
                .finality_store
                .recover_finalized_v2(finalized.commit.height)
                .await?
                .ok_or_else(|| {
                    anyhow!(
                        "consensus is ahead of a finalized height with no durable finality record"
                    )
                })?;
            // Multiple valid quorum certificates can finalize the same block;
            // the durable store intentionally keeps the first certificate.
            // A repeated certificate with the same block identity is
            // therefore idempotent, while a different block remains a fatal
            // finality conflict.
            if durable.commit.block_id == finalized.commit.block_id {
                return Ok(());
            }
            return Err(anyhow!(
                "durable finality at height {} conflicts with repeated activation",
                finalized.commit.height
            ));
        }
        if current_height < finalized.commit.height {
            return Err(anyhow!(
                "cannot activate finalized height {} while consensus is at height {}",
                finalized.commit.height,
                current_height
            ));
        }
        norn_core::execution::overlay::ExecutionOverlay::apply_persisted_writes(
            &prepared.state_write_values,
            &self.state_manager,
            self.evm_executor.code_storage(),
        )
        .await
        .map_err(|error| anyhow!("failed to apply finalized canonical writes: {error}"))?;
        let recomputed_root = StateRootCalculator::new(false)
            .calculate_from_manager(&self.state_manager)
            .await?;
        if recomputed_root != finalized.block.header.state_root {
            return Err(anyhow!(
                "finalized canonical state root does not match block state_root"
            ));
        }
        self.state_manager
            .set_verified_state_root(recomputed_root)
            .await;
        // Prune the complete finalized nonce frontier before advancing the
        // consensus height. Otherwise the next proposer can race activation,
        // package stale nonces, and spend the round producing an invalid
        // block. The pool implementation performs one bounded scan rather
        // than one full scan per committed transaction.
        self.tx_pool_v2
            .remove_committed_batch(&finalized.block.transactions);
        self.consensus
            .record_finalized_v2_after_durable(&finalized)
            .await?;
        self.consensus
            .advance_after_finalized_v2(finalized, prepared.next_snapshot.clone())
            .await?;
        if let Err(error) = self
            .finality_store
            .clear_pending_proposal(finalized.commit.height, finalized.commit.round)
            .await
        {
            warn!(
                "Failed to clear finalized V2 pending proposal record: {}",
                error
            );
        }
        Ok(())
    }

    async fn verify_replayed_or_equivalent_certificate(
        &self,
        persisted: &norn_common::consensus_types::FinalizedBlockV2,
        received: &CommitCertificate,
    ) -> Result<()> {
        if persisted.commit.block_id != received.block_id
            || persisted.commit.height != received.height
            || persisted.commit.protocol_version != received.protocol_version
            || persisted.commit.chain_id != received.chain_id
            || persisted.commit.epoch != received.epoch
            || persisted.commit.stake_snapshot_hash != received.stake_snapshot_hash
        {
            return Err(anyhow!(
                "replayed finalized certificate conflicts with durable history"
            ));
        }
        if persisted.commit == *received {
            return Ok(());
        }
        let snapshot = self
            .finality_store
            .recover_snapshot(received.epoch)
            .await?
            .ok_or_else(|| anyhow!("snapshot for replayed finalized certificate is missing"))?;
        self.consensus
            .verify_commit_certificate_v2(&persisted.block, received, &snapshot)
            .map_err(|error| {
                anyhow!("equivalent finalized certificate failed verification: {error}")
            })
    }
}

pub struct NornNode {
    config: NodeConfig,
    blockchain: Arc<Blockchain>,
    tx_pool: Arc<TxPool>,
    #[allow(dead_code)]
    tx_pool_v2: Arc<TransactionV2Pool>,
    #[allow(dead_code)]
    network: Arc<NetworkService>,

    /// Consensus engine for PoVF BFT consensus
    consensus: Arc<PoVFEngine>,
    consensus_driver: ConsensusDriver,
    finality_store: Arc<FinalityStore>,
    pending_commits: Arc<
        tokio::sync::Mutex<HashMap<(u64, u32, norn_common::types::BlockId), CommitCertificate>>,
    >,

    /// Block producer
    block_producer: Option<Arc<BlockProducer>>,

    chain_context: ChainContext,
    resource_limits: ProtocolResourceLimits,

    peer_manager: Arc<PeerManager>,
    syncer: Arc<BlockSyncer>,
    tx_handler: Arc<TxHandler>,

    /// State manager for EVM
    state_manager: Arc<AccountStateManager>,

    /// EVM executor
    evm_executor: Arc<EVMExecutor>,

    // Temp holder for startup
    network_rx: Option<tokio::sync::mpsc::Receiver<norn_network::service::NetworkEvent>>,

    #[allow(dead_code)]
    metrics_collector: Option<Arc<MetricsCollector>>,
    #[allow(dead_code)]
    _monitoring_server: Option<MonitoringServer>,
    #[allow(dead_code)]
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

struct NodeConsensusActionExecutor {
    consensus: Arc<PoVFEngine>,
    signer: Option<Arc<EcdsaConsensusSigner>>,
    state_manager: Arc<AccountStateManager>,
    resource_limits: ProtocolResourceLimits,
    chain_context: ChainContext,
    code_storage: Arc<CodeStorage>,
    finality_context: Arc<FinalityContext>,
    network: Arc<NetworkService>,
    pending_commits: Arc<
        tokio::sync::Mutex<HashMap<(u64, u32, norn_common::types::BlockId), CommitCertificate>>,
    >,
    verification_slots: Arc<tokio::sync::Semaphore>,
}

impl NodeConsensusActionExecutor {
    async fn schedule_step_timeout(
        &self,
        handle: &ConsensusDriverHandle,
        height: u64,
        round: u32,
        step: TimeoutStep,
    ) -> Result<()> {
        let config = self.consensus.state_machine.read().await.config.clone();
        let timeout_ms = match step {
            TimeoutStep::Propose => config.timeout_propose_ms,
            TimeoutStep::PrevoteWait => config.timeout_prevote_ms,
            TimeoutStep::PrecommitWait => config.timeout_precommit_ms,
        };
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(1));
        handle
            .submit(ConsensusDriverEvent::StepStarted {
                height,
                round,
                step,
                deadline,
            })
            .await
    }

    async fn schedule_round_timeout(
        &self,
        handle: &ConsensusDriverHandle,
        height: u64,
        round: u32,
    ) -> Result<()> {
        let timeout_ms = self
            .consensus
            .state_machine
            .read()
            .await
            .config
            .timeout_propose_ms;
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(1));
        handle
            .submit(ConsensusDriverEvent::RoundStarted {
                height,
                round,
                deadline,
            })
            .await
    }

    async fn reconcile_candidate_retention(&self) -> Result<()> {
        if self.consensus.reconcile_v2_candidate_retention().await {
            Ok(())
        } else {
            Err(anyhow!(
                "consensus state references a V2 candidate that is not retained"
            ))
        }
    }
}

#[async_trait]
impl ConsensusActionExecutor for NodeConsensusActionExecutor {
    async fn execute(
        &self,
        action: ConsensusDriverAction,
        handle: ConsensusDriverHandle,
    ) -> Result<()> {
        match action {
            ConsensusDriverAction::ValidateProposal {
                request_id,
                context,
                proposal,
                block,
            } => {
                let permit = match self.verification_slots.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        handle
                            .submit(ConsensusDriverEvent::ProposalValidationCompleted {
                                request_id,
                                context,
                                result: ProposalValidationResult::Rejected(
                                    "verification task limit is saturated".into(),
                                ),
                            })
                            .await?;
                        return Ok(());
                    }
                };
                let consensus = self.consensus.clone();
                let state_manager = self.state_manager.clone();
                let limits = self.resource_limits.clone();
                let chain_context = self.chain_context;
                let code_storage = self.code_storage.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let validation_height = proposal.height;
                    let validation_transactions = block.transactions.len();
                    let validation_started = std::time::Instant::now();
                    let result = consensus
                        .verify_proposal_v2(
                            proposal,
                            block,
                            &state_manager,
                            &limits,
                            &chain_context,
                            &code_storage,
                        )
                        .await;
                    info!(
                        height = validation_height,
                        transactions = validation_transactions,
                        elapsed_ms = validation_started.elapsed().as_millis(),
                        accepted = result.is_ok(),
                        "Completed V2 proposal validation"
                    );
                    let result = match result {
                        Ok(candidate) => ProposalValidationResult::Accepted(candidate),
                        Err(error) => ProposalValidationResult::Rejected(error.to_string()),
                    };
                    let _ = handle
                        .submit(ConsensusDriverEvent::ProposalValidationCompleted {
                            request_id,
                            context,
                            result,
                        })
                        .await;
                });
            }
            ConsensusDriverAction::ValidateVote {
                request_id,
                context,
                vote,
            } => {
                let permit = match self.verification_slots.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        handle
                            .submit(ConsensusDriverEvent::VoteValidationCompleted {
                                request_id,
                                context,
                                vote: vote.clone(),
                                result: VoteValidationResult::Rejected(
                                    "verification task limit is saturated".into(),
                                ),
                            })
                            .await?;
                        return Ok(());
                    }
                };
                let consensus = self.consensus.clone();
                let vote_for_event = vote.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let result = match consensus.verify_vote(&vote).await {
                        Ok(()) => VoteValidationResult::Accepted,
                        Err(error) => VoteValidationResult::Rejected(error.to_string()),
                    };
                    if let VoteValidationResult::Rejected(error) = &result {
                        warn!(
                            "Consensus driver rejected vote validation at height {} round {} step {:?}: {}",
                            vote_for_event.height,
                            vote_for_event.round,
                            vote_for_event.step,
                            error
                        );
                    }
                    let _ = handle
                        .submit(ConsensusDriverEvent::VoteValidationCompleted {
                            request_id,
                            context,
                            vote: vote_for_event,
                            result,
                        })
                        .await;
                });
            }
            ConsensusDriverAction::ValidateCommit {
                request_id,
                context,
                commit,
            } => {
                let permit = match self.verification_slots.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        handle
                            .submit(ConsensusDriverEvent::CommitValidationCompleted {
                                request_id,
                                context,
                                commit: commit.clone(),
                                result: CommitValidationResult::Rejected(
                                    "verification task limit is saturated".into(),
                                ),
                            })
                            .await?;
                        return Ok(());
                    }
                };
                let consensus = self.consensus.clone();
                let finality_store = self.finality_context.finality_store.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let result = if let Some((_, block)) = consensus
                        .get_validated_candidate_for_commit(
                            commit.height,
                            commit.round,
                            commit.block_id,
                        )
                        .await
                    {
                        let snapshot = consensus.state_machine.read().await.snapshot.clone();
                        consensus
                            .verify_commit_certificate_v2(&block, &commit, &snapshot)
                            .map(|_| CommitValidationResult::Accepted)
                            .unwrap_or_else(|error| {
                                CommitValidationResult::Rejected(error.to_string())
                            })
                    } else if finality_store
                        .recover_finalized_v2(commit.height)
                        .await
                        .ok()
                        .flatten()
                        .is_some()
                    {
                        CommitValidationResult::Accepted
                    } else {
                        CommitValidationResult::Rejected(
                            "commit candidate is not available yet".into(),
                        )
                    };
                    let _ = handle
                        .submit(ConsensusDriverEvent::CommitValidationCompleted {
                            request_id,
                            context,
                            commit,
                            result,
                        })
                        .await;
                });
            }
            ConsensusDriverAction::ApplyValidatedProposal { candidate, .. } => {
                let candidate_key = (
                    candidate.proposal.height,
                    candidate.proposal.round,
                    candidate.proposal.block_id,
                );
                let proposal_height = candidate.proposal.height;
                let proposal_round = candidate.proposal.round;
                if let Some(signer) = self.signer.as_ref() {
                    let vote = self
                        .consensus
                        .apply_validated_proposal_v2(candidate, signer.as_ref())
                        .await?;
                    if let Some(vote) = vote {
                        let (follow_up, certificate) = self
                            .consensus
                            .handle_vote(vote.clone(), signer.as_ref())
                            .await?;
                        self.reconcile_candidate_retention().await?;
                        handle
                            .submit(ConsensusDriverEvent::VotePersisted(vote))
                            .await?;
                        let mut finality_formed = certificate.is_some();
                        if let Some(precommit) = follow_up {
                            let (_, local_certificate) = self
                                .consensus
                                .handle_vote(precommit.clone(), signer.as_ref())
                                .await?;
                            self.reconcile_candidate_retention().await?;
                            handle
                                .submit(ConsensusDriverEvent::VotePersisted(precommit))
                                .await?;
                            if let Some(certificate) = local_certificate {
                                finality_formed = true;
                                handle
                                    .submit(ConsensusDriverEvent::NetworkCommit(certificate))
                                    .await?;
                            }
                        }
                        if let Some(certificate) = certificate {
                            handle
                                .submit(ConsensusDriverEvent::NetworkCommit(certificate))
                                .await?;
                        }
                        if !finality_formed {
                            self.schedule_step_timeout(
                                &handle,
                                proposal_height,
                                proposal_round,
                                TimeoutStep::PrevoteWait,
                            )
                            .await?;
                        }
                    }
                } else {
                    self.consensus
                        .remember_validated_candidate(&candidate)
                        .await;
                }
                if let Some(commit) = self.pending_commits.lock().await.remove(&candidate_key) {
                    handle
                        .submit(ConsensusDriverEvent::NetworkCommit(commit))
                        .await?;
                }
            }
            ConsensusDriverAction::ApplyValidatedVote { vote, .. } => {
                let Some(signer) = self.signer.as_ref() else {
                    return Ok(());
                };
                let (follow_up, certificate) =
                    self.consensus.handle_vote(vote, signer.as_ref()).await?;
                self.reconcile_candidate_retention().await?;
                info!(
                    "Consensus driver applied vote; follow_up_precommit={} certificate={}",
                    follow_up.is_some(),
                    certificate.is_some()
                );
                if let Some(precommit) = follow_up {
                    let precommit_height = precommit.height;
                    let precommit_round = precommit.round;
                    let (_, local_certificate) = self
                        .consensus
                        .handle_vote(precommit.clone(), signer.as_ref())
                        .await?;
                    self.reconcile_candidate_retention().await?;
                    handle
                        .submit(ConsensusDriverEvent::VotePersisted(precommit))
                        .await?;
                    if let Some(certificate) = local_certificate {
                        handle
                            .submit(ConsensusDriverEvent::NetworkCommit(certificate))
                            .await?;
                    } else {
                        let state = self.consensus.state_machine.read().await;
                        let should_wait =
                            state.step != norn_core::consensus::types::ConsensusStep::Commit;
                        drop(state);
                        if should_wait {
                            self.schedule_step_timeout(
                                &handle,
                                precommit_height,
                                precommit_round,
                                TimeoutStep::PrecommitWait,
                            )
                            .await?;
                        }
                    }
                }
                if let Some(certificate) = certificate {
                    handle
                        .submit(ConsensusDriverEvent::NetworkCommit(certificate))
                        .await?;
                }
            }
            ConsensusDriverAction::PrepareFinality {
                request_id,
                context,
                commit,
            } => {
                let permit = match self.verification_slots.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        handle
                            .submit(ConsensusDriverEvent::FinalityPreparationCompleted {
                                request_id,
                                context,
                                result: FinalityPreparationResult::Rejected(
                                    "verification task limit is saturated".into(),
                                ),
                            })
                            .await?;
                        return Ok(());
                    }
                };
                let finality_context = self.finality_context.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let result = match finality_context.prepare_commit(commit).await {
                        Ok(result) => result,
                        Err(error) => FinalityPreparationResult::Rejected(error.to_string()),
                    };
                    let _ = handle
                        .submit(ConsensusDriverEvent::FinalityPreparationCompleted {
                            request_id,
                            context,
                            result,
                        })
                        .await;
                });
            }
            ConsensusDriverAction::PersistPreparedFinality { prepared, .. } => {
                let commit = prepared.finalized.commit.clone();
                let mut attempts = 0u8;
                loop {
                    match self
                        .finality_context
                        .persist_prepared(prepared.clone())
                        .await
                    {
                        Ok(()) => {
                            handle
                                .submit(ConsensusDriverEvent::FinalityPersisted(commit.clone()))
                                .await?;
                            break;
                        }
                        Err(FinalityPersistenceError::Persistence(error)) if attempts < 2 => {
                            attempts += 1;
                            warn!(
                                "Finality persistence was confirmed not applied; retrying exact transaction (attempt {}): {}",
                                attempts + 1,
                                error
                            );
                            continue;
                        }
                        Err(FinalityPersistenceError::Persistence(error)) => {
                            warn!("Finality persistence failed before activation: {}", error);
                            return Err(error);
                        }
                        Err(FinalityPersistenceError::Indeterminate(error)) => {
                            let message =
                                format!("finality persistence outcome is indeterminate: {error}");
                            error!("{message}");
                            let _ = handle
                                .submit(ConsensusDriverEvent::FinalityIndeterminate(
                                    message.clone(),
                                ))
                                .await;
                            return Err(anyhow!(message));
                        }
                        Err(FinalityPersistenceError::Activation(error)) => {
                            let message =
                                format!("finality activation failed after durable commit: {error}");
                            error!("{message}");
                            let _ = handle
                                .submit(ConsensusDriverEvent::ActivationFailed(message.clone()))
                                .await;
                            return Err(anyhow!(message));
                        }
                    }
                }
            }
            ConsensusDriverAction::HandleTimeout(token) => {
                let Some(signer) = self.signer.as_ref() else {
                    handle
                        .submit(ConsensusDriverEvent::TimeoutIgnored(token))
                        .await?;
                    return Ok(());
                };
                let mut timeout_is_current = true;
                let vote = {
                    let mut state_machine = self.consensus.state_machine.write().await;
                    if state_machine.height != token.height || state_machine.round != token.round {
                        timeout_is_current = false;
                        None
                    } else {
                        let step_matches = match token.step {
                            TimeoutStep::Propose => matches!(
                                state_machine.step,
                                norn_core::consensus::types::ConsensusStep::NewHeight
                                    | norn_core::consensus::types::ConsensusStep::NewRound
                                    | norn_core::consensus::types::ConsensusStep::Propose
                            ),
                            TimeoutStep::PrevoteWait => {
                                state_machine.step
                                    == norn_core::consensus::types::ConsensusStep::PrevoteWait
                            }
                            TimeoutStep::PrecommitWait => {
                                state_machine.step
                                    == norn_core::consensus::types::ConsensusStep::PrecommitWait
                            }
                        };
                        if !step_matches {
                            timeout_is_current = false;
                            None
                        } else {
                            match token.step {
                                TimeoutStep::Propose => {
                                    Some(state_machine.on_timeout_propose(signer.as_ref())?)
                                }
                                TimeoutStep::PrevoteWait => {
                                    Some(state_machine.on_timeout_prevote(signer.as_ref())?)
                                }
                                TimeoutStep::PrecommitWait => {
                                    state_machine.on_timeout_precommit()?;
                                    None
                                }
                            }
                        }
                    }
                };
                if !timeout_is_current {
                    handle
                        .submit(ConsensusDriverEvent::TimeoutIgnored(token))
                        .await?;
                    return Ok(());
                }
                // Only now does the driver advance generation. A timeout
                // that lost a race with Commit/finality has no effect on
                // pending finality preparation.
                handle
                    .submit(ConsensusDriverEvent::TimeoutApplied(token))
                    .await?;
                if let Some(vote) = vote {
                    handle
                        .submit(ConsensusDriverEvent::VotePersisted(vote))
                        .await?;
                    let next_step = match token.step {
                        TimeoutStep::Propose => TimeoutStep::PrevoteWait,
                        TimeoutStep::PrevoteWait => TimeoutStep::PrecommitWait,
                        TimeoutStep::PrecommitWait => unreachable!(),
                    };
                    self.schedule_step_timeout(&handle, token.height, token.round, next_step)
                        .await?;
                } else if token.step == TimeoutStep::PrecommitWait {
                    self.schedule_round_timeout(&handle, token.height, token.round + 1)
                        .await?;
                }
            }
            ConsensusDriverAction::BroadcastVote(vote) => {
                let envelope = ConsensusEnvelope {
                    wire_version: self.chain_context.wire_version,
                    protocol_version: vote.protocol_version,
                    chain_id: vote.chain_id,
                    genesis_hash: self.chain_context.genesis_hash,
                    payload: ConsensusMessage::Vote(vote.clone()),
                };
                let bytes = bincode::serialize(&envelope)
                    .map_err(|error| anyhow!("failed to encode Vote: {error}"))?;
                match self
                    .network
                    .control_tx
                    .try_send(NetworkCommand::BroadcastConsensus(bytes))
                {
                    Ok(()) => {}
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        return Err(anyhow!(RetryableConsensusActionError::with_max_retries(
                            "Vote broadcast command channel is temporarily full",
                            8,
                        )));
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                        return Err(anyhow!(
                            "Vote broadcast command channel is closed; network service stopped"
                        ));
                    }
                }
            }
            ConsensusDriverAction::ScheduleTimeout(token) => {
                let handle = handle.clone();
                tokio::spawn(async move {
                    tokio::time::sleep_until(token.deadline).await;
                    let _ = handle.submit(ConsensusDriverEvent::Timeout(token)).await;
                });
            }
            ConsensusDriverAction::BroadcastCommit(commit) => {
                let envelope = ConsensusEnvelope {
                    wire_version: self.chain_context.wire_version,
                    protocol_version: commit.protocol_version,
                    chain_id: commit.chain_id,
                    genesis_hash: self.chain_context.genesis_hash,
                    payload: ConsensusMessage::Commit(commit),
                };
                let bytes = bincode::serialize(&envelope)
                    .map_err(|error| anyhow!("failed to encode Commit: {error}"))?;
                self.network
                    .control_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                    .map_err(|error| {
                        anyhow!(RetryableConsensusActionError::new(format!(
                            "failed to enqueue Commit: {error}"
                        )))
                    })?;
            }
            ConsensusDriverAction::CancelTimeout(_) => {}
            ConsensusDriverAction::UnpinPendingFinality {
                height,
                round,
                block_id,
            } => {
                if !self
                    .consensus
                    .unpin_v2_candidate_for_finality(height, round, block_id)
                    .await
                {
                    warn!(
                        "stale finality preparation could not release candidate pin at height {}",
                        height
                    );
                }
            }
        }
        Ok(())
    }
}

impl NornNode {
    pub async fn new(config: NodeConfig, keypair: Keypair) -> Result<Self> {
        use crate::logging::LoggingConfig;
        let log_config: LoggingConfig = config.logging.clone().into();
        let log_guard = log_config.init()?;
        info!(
            "Logging initialized: format={:?}, level={}",
            log_config.format, log_config.level
        );

        let metrics_collector = if config.monitoring.prometheus_enabled {
            info!(
                "Initializing metrics collector on {}",
                config.monitoring.prometheus_address
            );
            Some(Arc::new(MetricsCollector::new()))
        } else {
            info!("Metrics collection disabled");
            None
        };

        if config.monitoring.health_check_enabled {
            if let Some(ref collector) = metrics_collector {
                let server = MonitoringServer::new(collector.clone());
                let address = config.monitoring.health_check_address.clone();
                info!("Monitoring server starting on {}", address);
                let address_log = address.clone();
                tokio::spawn(async move {
                    if let Err(e) = server.start(&address).await {
                        error!("Monitoring server failed: {}", e);
                    }
                });
                info!("Monitoring server started on {}", address_log);
            } else {
                warn!("Health check enabled but metrics collector is disabled");
            }
        } else {
            info!("Health check endpoint disabled");
        }

        let genesis_config = config.load_genesis_config()?;
        let genesis_snapshot = config.validate_genesis_for_role(&genesis_config)?;
        let genesis_snapshot_hash = genesis_snapshot.snapshot_hash;
        let chain_context = genesis_config.context();

        let db = Arc::new(SledDB::new(&config.data_dir)?);
        let blockchain = Blockchain::try_new_with_genesis(
            db.clone(),
            genesis_config.genesis_block.clone(),
            chain_context.genesis_hash,
        )
        .await?;

        let tx_pool = Arc::new(TxPool::new());
        let v2_pool_capacity = config.txpool.max_size.max(1);
        let max_v2_txs_per_block = config
            .txpool
            .v2_max_txs_per_block
            .max(1)
            .min(genesis_config.resource_limits.max_transactions_per_block as usize);
        if config.txpool.max_size == 0 || config.txpool.v2_max_txs_per_block == 0 {
            warn!("Invalid zero V2 txpool setting; clamping pool capacity and block cap to one");
        }
        info!(
            v2_pool_capacity,
            max_v2_txs_per_block,
            verification_queue = genesis_config.resource_limits.max_verification_queue,
            "Configured V2 transaction admission and proposal limits independently"
        );
        let tx_pool_v2 = Arc::new(TransactionV2Pool::new_with_capacity(v2_pool_capacity));

        let (local_validator_id, signer, vrf_key_pair) = if config.node_role == NodeRole::Validator
        {
            let keystore_dir = Path::new(&config.data_dir).join("keystore");
            let keystore = match config.network_mode {
                NetworkMode::Production => NodeKeyStore::open_existing(&keystore_dir)?,
                NetworkMode::Devnet | NetworkMode::Test => {
                    NodeKeyStore::open_or_create(&keystore_dir)?
                }
            };
            info!(
                "Loaded persistent validator keystore from {:?}",
                keystore_dir
            );

            let consensus_pubkey_bytes: [u8; 33] = keystore
                .consensus_key()
                .verifying_key()
                .to_sec1_bytes()
                .as_ref()
                .try_into()
                .map_err(|_| anyhow!("Invalid SEC1 public key length"))?;
            let vrf_pubkey_bytes = keystore.vrf_key().public_key_bytes();
            let local_validator_id = validate_validator_key_match(
                &genesis_snapshot,
                consensus_pubkey_bytes,
                vrf_pubkey_bytes,
            )?;

            let signer = Arc::new(EcdsaConsensusSigner::new(
                keystore.consensus_key().clone(),
                local_validator_id,
            ));
            (
                Some(local_validator_id),
                Some(signer),
                Some(keystore.vrf_key().clone()),
            )
        } else {
            info!("Starting as FullNode; validator private keys are not loaded");
            (None, None, None)
        };

        let defaults = ConsensusConfig::default();
        let consensus_config = ConsensusConfig {
            protocol_version: chain_context.protocol_version,
            chain_id: chain_context.chain_id,
            epoch: genesis_config.epoch,
            epoch_length: genesis_config.epoch_length,
            validator_update_delay: genesis_config.validator_update_delay,
            unbonding_delay: genesis_config.unbonding_delay,
            key_rotation_delay: genesis_config.key_rotation_delay,
            slashing_activation_delay: genesis_config.slashing_activation_delay,
            timeout_propose_ms: defaults.timeout_propose_ms,
            timeout_prevote_ms: defaults.timeout_prevote_ms,
            timeout_precommit_ms: defaults.timeout_precommit_ms,
            target_numerator: defaults.target_numerator,
            target_denominator: defaults.target_denominator,
            max_certificate_members: genesis_config.resource_limits.max_certificate_members,
            max_future_height: genesis_config.resource_limits.max_future_height,
            max_future_round: genesis_config.resource_limits.max_future_round,
            max_consensus_round: genesis_config.resource_limits.max_consensus_round,
            max_block_timestamp_step: genesis_config.resource_limits.max_block_timestamp_step,
        };

        let safety_path = Path::new(&config.data_dir).join("safety_store.log");
        let persistent_safety_store = Arc::new(PersistentSafetyStore::open(safety_path)?);
        let finality_store = Arc::new(FinalityStore::new_with_limits(
            db.clone(),
            genesis_config.resource_limits.clone(),
        ));
        let initialized_tip = finality_store
            .initialize_genesis_tip(
                &genesis_config.genesis_block,
                genesis_snapshot_hash,
                genesis_config.initial_randomness,
            )
            .await?;

        let consensus = Arc::new(
            PoVFEngine::new_with_parent_randomness_and_timestamp_and_limits_and_context(
                consensus_config,
                genesis_snapshot.clone(),
                genesis_config.initial_randomness,
                initialized_tip.timestamp,
                persistent_safety_store,
                local_validator_id,
                genesis_config.resource_limits.clone(),
                Some(chain_context),
            ),
        );
        consensus.attach_finality_store(finality_store.clone());
        {
            let sm = consensus.state_machine.read().await;
            info!(
                "Consensus V2 initial height={} round={} local_validator={:?} proposer={:?}",
                sm.height,
                sm.round,
                sm.local_validator_id,
                sm.get_current_proposer()
            );
        }
        let state_manager = Arc::new(AccountStateManager::new(AccountStateConfig::default()));
        let evm_config = EVMConfig::default();
        let evm_executor = Arc::new(EVMExecutor::new(state_manager.clone(), evm_config));

        if let Some((finalized, state_writes, checkpoint)) = finality_store
            .recover_finalized_tip_with_state_and_checkpoint()
            .await?
        {
            let checkpoint = checkpoint.ok_or_else(|| {
                anyhow!(
                    "durable finalized V2 state has no canonical state checkpoint; refusing startup"
                )
            })?;
            if checkpoint.state_root != finalized.block.header.state_root {
                return Err(anyhow!(
                    "durable canonical state root does not match finalized block"
                ));
            }
            state_manager
                .restore_canonical_state(
                    &checkpoint.accounts,
                    &checkpoint.storage,
                    checkpoint.state_root,
                )
                .await
                .map_err(|error| anyhow!("failed to restore canonical state: {error}"))?;
            evm_executor
                .code_storage()
                .restore_checkpoint(&checkpoint.code)
                .await
                .map_err(|error| anyhow!("failed to restore canonical code: {error}"))?;
            let recomputed_root = StateRootCalculator::new(false)
                .calculate_from_manager(&state_manager)
                .await?;
            if recomputed_root != checkpoint.state_root {
                return Err(anyhow!(
                    "recovered canonical state root recomputation mismatch"
                ));
            }
            let _ = state_writes;
            let next_snapshot = {
                let sm = consensus.state_machine.read().await;
                let next_height = finalized
                    .consensus_state
                    .height
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("finalized height overflow during recovery"))?;
                let next_epoch = sm.config.epoch_for_height(next_height)?;
                if let Some(snapshot) = finality_store.recover_snapshot(next_epoch).await? {
                    if snapshot.epoch != next_epoch {
                        return Err(anyhow!(
                            "durable next validator snapshot has an unexpected epoch"
                        ));
                    }
                    snapshot
                } else {
                    return Err(anyhow!(
                        "durable finalized V2 state is missing the next validator snapshot; refusing to derive it from in-memory pending changes"
                    ));
                }
            };
            let expected_tip = CanonicalFinalizedTip::from_finalized_with_next_snapshot(
                &finalized,
                Some(&next_snapshot),
            )
            .map_err(|error| anyhow!("failed to derive canonical recovery tip: {error}"))?;
            if initialized_tip != expected_tip {
                return Err(anyhow!(
                    "durable canonical tip conflicts with finalized block, state root, randomness, or next validator snapshot"
                ));
            }
            {
                let mut sm = consensus.state_machine.write().await;
                sm.restore_after_finalized(&finalized.consensus_state, next_snapshot)
                    .map_err(|error| {
                        anyhow!("failed to restore finalized consensus state: {error}")
                    })?;
                *consensus.current_height.write().await = sm.height;
            }
            info!(
                "Recovered finalized V2 state at height {}; consensus resumes at {}",
                finalized.consensus_state.height,
                finalized.consensus_state.height.saturating_add(1)
            );
        }

        // Restore Tendermint lock/valid-round state only after the canonical
        // finalized tip has been recovered. A state record from a finalized
        // older height is stale and is replaced by the current-height state;
        // a record ahead of the durable tip is a fail-closed corruption.
        let safety_store = consensus.state_machine.read().await.safety_store.clone();
        let durable_safety_state = safety_store
            .load_consensus_state()
            .map_err(|error| anyhow!("failed to load durable consensus safety state: {error}"))?;
        {
            let mut sm = consensus.state_machine.write().await;
            if let Some(state) = durable_safety_state {
                if state.height > sm.height {
                    return Err(anyhow!(
                        "durable consensus safety state is ahead of canonical consensus height"
                    ));
                }
                if state.height == sm.height {
                    sm.restore_durable_safety_state(state).map_err(|error| {
                        anyhow!("failed to restore durable consensus safety state: {error}")
                    })?;
                }
            }
            safety_store
                .persist_consensus_state(sm.durable_safety_state()?)
                .map_err(|error| anyhow!("failed to checkpoint consensus safety state: {error}"))?;
            info!(
                "Restored consensus safety state at height {} round {} step {:?}",
                sm.height, sm.round, sm.step
            );
        }

        if !consensus.reconcile_v2_candidate_retention().await {
            return Err(anyhow!(
                "durable consensus safety state references a V2 candidate that cannot be recovered"
            ));
        }

        let block_producer = match (config.node_role, vrf_key_pair, signer.clone()) {
            (NodeRole::Validator, Some(vrf_key_pair), Some(signer)) => {
                let producer_config = BlockProducerConfig {
                    is_validator: true,
                    block_interval: 1,
                    max_txs_per_block: genesis_config.resource_limits.max_transactions_per_block
                        as usize,
                    max_v2_txs_per_block,
                    max_gas_per_block: genesis_config.resource_limits.max_block_gas as i64,
                    max_block_bytes: genesis_config.resource_limits.max_block_bytes as usize,
                    max_transaction_bytes: genesis_config.resource_limits.max_transaction_bytes
                        as usize,
                    ..Default::default()
                };
                let mut producer = BlockProducer::new(
                    producer_config,
                    blockchain.clone(),
                    tx_pool.clone(),
                    vrf_key_pair,
                    state_manager.clone(),
                    Some(consensus.clone()),
                    Some(signer),
                );
                producer.attach_v2_pool(tx_pool_v2.clone());
                producer.attach_v2_code_storage(evm_executor.code_storage().clone());
                producer.attach_finality_store(finality_store.clone());
                Some(Arc::new(producer))
            }
            (NodeRole::FullNode, None, None) => None,
            _ => return Err(anyhow!("invalid node role/key initialization state")),
        };

        let peer_role = match config.node_role {
            NodeRole::Validator => PeerRole::Validator,
            NodeRole::FullNode => PeerRole::FullNode,
        };
        let validator_public_keys = genesis_snapshot
            .validators
            .iter()
            .map(|(validator_id, record)| (*validator_id, record.consensus_public_key.0))
            .collect();
        let local_validator = match (local_validator_id, signer.clone()) {
            (Some(validator_id), Some(signer)) => {
                let consensus_public_key = genesis_snapshot
                    .validators
                    .get(&validator_id)
                    .ok_or_else(|| anyhow!("local validator is missing from active snapshot"))?
                    .consensus_public_key
                    .0;
                let signer_for_handshake = signer.clone();
                Some(ValidatorHandshakeIdentity {
                    validator_id,
                    consensus_public_key,
                    sign: Arc::new(move |bytes| signer_for_handshake.sign_canonical_bytes(bytes)),
                })
            }
            (None, None) => None,
            _ => return Err(anyhow!("invalid local validator handshake identity")),
        };
        let mut network_svc = NetworkService::start_with_context_and_auth(
            config.network.clone(),
            keypair,
            chain_context,
            peer_role,
            NetworkAuthConfig {
                local_validator,
                validator_public_keys,
            },
        )
        .await?;
        let rx = std::mem::replace(&mut network_svc.event_rx, tokio::sync::mpsc::channel(1).1);
        let network = Arc::new(network_svc);

        let peer_manager = Arc::new(PeerManager::new(
            blockchain.clone(),
            tx_pool_v2.clone(),
            network.clone(),
            chain_context,
            genesis_config.resource_limits.max_transaction_bytes as usize,
            genesis_config.resource_limits.max_verification_tasks as usize,
        ));
        let syncer = Arc::new(BlockSyncer::new(blockchain.clone(), network.clone()));
        let tx_handler = Arc::new(TxHandler::new(
            tx_pool_v2.clone(),
            chain_context,
            genesis_config.resource_limits.max_transaction_bytes as usize,
            genesis_config.resource_limits.max_block_bytes as usize,
            genesis_config.resource_limits.max_verification_tasks as usize,
        ));

        let finality_context = Arc::new(FinalityContext {
            chain_context,
            consensus: consensus.clone(),
            finality_store: finality_store.clone(),
            state_manager: state_manager.clone(),
            tx_pool_v2: tx_pool_v2.clone(),
            resource_limits: genesis_config.resource_limits.clone(),
            evm_executor: evm_executor.clone(),
            activation_lock: Arc::new(tokio::sync::Mutex::new(())),
            network: network.clone(),
        });
        let pending_commits = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let consensus_executor = Arc::new(NodeConsensusActionExecutor {
            consensus: consensus.clone(),
            signer: signer.clone(),
            state_manager: state_manager.clone(),
            resource_limits: genesis_config.resource_limits.clone(),
            chain_context,
            code_storage: evm_executor.code_storage().clone(),
            finality_context: finality_context.clone(),
            network: network.clone(),
            pending_commits: pending_commits.clone(),
            verification_slots: Arc::new(tokio::sync::Semaphore::new(
                genesis_config.resource_limits.max_verification_tasks.max(1) as usize,
            )),
        });
        let consensus_driver = ConsensusDriver::start_with_executor(
            genesis_config.resource_limits.max_verification_queue.max(1) as usize,
            consensus_executor,
        )?;
        info!("Initialized disk-backed single-writer BFT consensus driver");

        Ok(Self {
            config,
            blockchain,
            tx_pool,
            tx_pool_v2,
            network,
            consensus,
            consensus_driver,
            finality_store,
            pending_commits,
            block_producer,
            chain_context,
            resource_limits: genesis_config.resource_limits,
            peer_manager,
            syncer,
            tx_handler,
            state_manager,
            evm_executor,
            network_rx: Some(rx),
            metrics_collector,
            _monitoring_server: None,
            _log_guard: log_guard,
        })
    }

    pub async fn start(mut self) -> Result<()> {
        info!("Starting Norn Node...");

        let rpc_addr = self.config.rpc_address;
        let eth_rpc_addr = {
            let port = rpc_addr.port() + 1000;
            format!("{}:{}", rpc_addr.ip(), port).parse::<std::net::SocketAddr>()?
        };

        let chain_ref = self.blockchain.clone();
        let tx_pool_ref = self.tx_pool.clone();
        let tx_pool_v2_ref = self.tx_pool_v2.clone();
        let finality_store_ref = self.finality_store.clone();
        let transaction_broadcast = self.network.command_tx.clone();
        let rpc_addr_clone = rpc_addr;
        tokio::spawn(async move {
            info!("gRPC Server listening on {}", rpc_addr_clone);
            if let Err(e) = start_rpc_server(
                rpc_addr_clone,
                chain_ref,
                tx_pool_ref,
                Some(tx_pool_v2_ref),
                finality_store_ref,
                Some(transaction_broadcast),
            )
            .await
            {
                error!("gRPC Server failed: {:?}", e);
            }
        });
        info!("gRPC Server started on {}", rpc_addr);

        let eth_rpc = create_ethereum_rpc(
            self.blockchain.clone(),
            self.state_manager.clone(),
            self.evm_executor.clone(),
            self.tx_pool.clone(),
            31337,
        );
        tokio::spawn(async move {
            info!("Ethereum JSON-RPC server listening on {}", eth_rpc_addr);
            if let Err(e) = start_ethereum_rpc_server(eth_rpc_addr, eth_rpc).await {
                error!("Ethereum JSON-RPC server failed: {:?}", e);
            }
        });
        info!("Ethereum JSON-RPC server started on {}", eth_rpc_addr);

        if self.config.consensus_start_delay_ms > 0 {
            info!(
                delay_ms = self.config.consensus_start_delay_ms,
                "Delaying first consensus round while the fresh validator mesh initializes"
            );
            tokio::time::sleep(std::time::Duration::from_millis(
                self.config.consensus_start_delay_ms,
            ))
            .await;
        }

        // All nodes start the same protocol round through the driver. This
        // gives the proposer wait phase a real deadline even when no local
        // proposal is produced and keeps timer ownership out of the producer.
        let (height, round, timeout_ms) = {
            let state = self.consensus.state_machine.read().await;
            (
                state.height,
                state.round,
                state.config.timeout_propose_ms.max(1),
            )
        };
        self.consensus_driver
            .dispatch(ConsensusDriverEvent::RoundStarted {
                height,
                round,
                deadline: tokio::time::Instant::now()
                    + std::time::Duration::from_millis(timeout_ms),
            })
            .await?;

        let syncer = self.syncer.clone();
        tokio::spawn(async move {
            syncer.start().await;
        });

        if let Some(producer) = self.block_producer.clone() {
            let chain_context = self.chain_context;
            let resource_limits = self.resource_limits.clone();
            let consensus = self.consensus.clone();
            let consensus_driver = self.consensus_driver.clone();
            let finality_store = self.finality_store.clone();
            let network_tx = self.network.control_tx.clone();
            tokio::spawn(async move {
                // Keep proposer hand-off latency well below a block interval.  A one-second
                // poll was visible directly in the inter-block time on small validators,
                // especially when leadership rotated immediately after finality.
                let mut timer = tokio::time::interval(std::time::Duration::from_millis(100));
                let mut produced_slot: Option<(u64, u32)> = None;
                loop {
                    timer.tick().await;
                    let Some(slot) = producer.proposal_slot().await else {
                        continue;
                    };
                    if produced_slot == Some(slot) {
                        continue;
                    }
                    let (proposal, block) = match finality_store
                        .recover_pending_proposal(slot.0, slot.1)
                        .await
                    {
                        Ok(Some((proposal, block))) => {
                            info!(
                                "Recovering pending V2 proposal at height {} round {}",
                                proposal.height, proposal.round
                            );
                            (proposal, block)
                        }
                        Ok(None) => {
                            let produced = match producer
                                .produce_v2_proposal_for_slot(
                                    &chain_context,
                                    &resource_limits,
                                    Some(slot),
                                )
                                .await
                            {
                                Ok(produced) => produced,
                                Err(error) => {
                                    warn!("V2 proposal production failed: {}", error);
                                    continue;
                                }
                            };
                            if let Err(error) = finality_store
                                .persist_pending_proposal(&produced.0, &produced.1)
                                .await
                            {
                                warn!(
                                    "V2 proposal was not durably recorded before voting: {}",
                                    error
                                );
                                continue;
                            }
                            (produced.0, produced.1)
                        }
                        Err(error) => {
                            warn!("Failed to recover pending V2 proposal: {}", error);
                            continue;
                        }
                    };
                    let current_slot = {
                        let state_machine = consensus.state_machine.read().await;
                        (state_machine.height, state_machine.round)
                    };
                    if current_slot != slot {
                        info!(
                            built_height = proposal.height,
                            built_round = proposal.round,
                            current_height = current_slot.0,
                            current_round = current_slot.1,
                            "Discarding stale local V2 proposal before persistence"
                        );
                        continue;
                    }
                    // The locally built proposal is already durably recorded
                    // and structurally validated. Broadcast it in parallel
                    // with this validator's pure execution check; every peer
                    // still performs full validation before voting.
                    let transaction_short_ids = block
                        .transactions
                        .iter()
                        .map(|transaction| transaction.transaction_id.relay_short_id())
                        .collect::<Vec<_>>();
                    let unique_short_ids = transaction_short_ids
                        .iter()
                        .copied()
                        .collect::<HashSet<_>>();
                    let payload = if unique_short_ids.len() == transaction_short_ids.len() {
                        ConsensusMessage::CompactProposalV2 {
                            proposal: proposal.clone(),
                            header: block.header.clone(),
                            consensus_data: block.consensus_data.clone(),
                            transaction_short_ids,
                        }
                    } else {
                        warn!(
                            "Short transaction ID collision in local proposal; broadcasting full block"
                        );
                        ConsensusMessage::ProposalV2 {
                            proposal: proposal.clone(),
                            block: block.clone(),
                        }
                    };
                    let envelope = ConsensusEnvelope {
                        wire_version: chain_context.wire_version,
                        protocol_version: proposal.protocol_version,
                        chain_id: proposal.chain_id,
                        genesis_hash: chain_context.genesis_hash,
                        payload,
                    };
                    let bytes = match bincode::serialize(&envelope) {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            warn!("Failed to encode local V2 proposal: {}", error);
                            continue;
                        }
                    };
                    if let Err(error) = network_tx
                        .send(NetworkCommand::BroadcastConsensus(bytes))
                        .await
                    {
                        warn!("Failed to enqueue local V2 proposal: {}", error);
                        continue;
                    }
                    produced_slot = Some(slot);
                    if let Err(error) = consensus_driver
                        .dispatch(ConsensusDriverEvent::LocalProposalBuilt { proposal, block })
                        .await
                    {
                        warn!(
                            "Failed to submit local V2 proposal to consensus driver: {}",
                            error
                        );
                    }
                }
            });
            info!("Block Producer started");
        } else {
            info!("Block Producer disabled for FullNode");
        }

        // A vote whose WAL completion was durable before a crash is still a
        // valid vote. Re-broadcast the exact persisted signature on startup;
        // never manufacture a replacement vote for the same signing slot.
        let recovered_votes = {
            let sm = self.consensus.state_machine.read().await;
            sm.safety_store.recover_signed_votes()
        };
        for vote in recovered_votes {
            if vote.protocol_version != self.chain_context.protocol_version
                || vote.chain_id != self.chain_context.chain_id
            {
                warn!(
                    "Discarding recovered vote from a different protocol/chain context: height={}, round={}, step={:?}",
                    vote.height, vote.round, vote.step
                );
                continue;
            }
            if let Err(error) = self
                .consensus_driver
                .dispatch(ConsensusDriverEvent::NetworkVote(vote.clone()))
                .await
            {
                warn!(
                    "Failed to submit recovered vote to consensus driver: {}",
                    error
                );
                continue;
            }
            if let Err(error) = self
                .consensus_driver
                .dispatch(ConsensusDriverEvent::VoteDurablySigned(vote))
                .await
            {
                warn!("Failed to rebroadcast recovered vote: {}", error);
            }
        }

        // Any node may have been offline when validators broadcast several
        // Commit certificates. Start an ordered finalized-record sync from
        // the next canonical height; each response requests the following
        // height after it has been verified and durably applied. Validators
        // use the same verify-and-recover path after a crash/partition; they
        // never treat a missing gossip replay as an implicit new proposal.
        if let Some(tip) = self.finality_store.recover_canonical_tip().await? {
            self.request_v2_finality(tip.next_height()?).await;
        }

        if let Some(rx) = self.network_rx.take() {
            self.run_loop(rx).await;
        }

        Ok(())
    }

    async fn request_v2_block(
        &self,
        height: u64,
        round: u32,
        block_id: norn_common::types::BlockId,
    ) {
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::BlockRequest {
                height,
                round,
                block_id,
            },
        };
        match bincode::serialize(&envelope) {
            Ok(bytes) => {
                if let Err(error) = self
                    .network
                    .control_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                {
                    warn!("Failed to request missing V2 block: {}", error);
                }
            }
            Err(error) => warn!("Failed to encode V2 block request: {}", error),
        }
    }

    fn dispatch_compact_v2_proposal(
        &self,
        proposal: norn_common::consensus_types::Proposal,
        header: norn_common::types::BlockHeader,
        consensus_data: norn_common::types::BlockConsensusData,
        transaction_short_ids: Vec<u64>,
    ) {
        let verified_pool = self.tx_pool_v2.clone();
        let relay_cache = self.tx_handler.relay_cache();
        let driver = self.consensus_driver.clone();
        let network_tx = self.network.control_tx.clone();
        let chain_context = self.chain_context;
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            let wanted = transaction_short_ids
                .iter()
                .copied()
                .collect::<HashSet<_>>();
            let mut missing = 0usize;
            for attempt in 0..=20u32 {
                let verified = verified_pool.get_by_relay_short_ids(&wanted);
                let relayed = relay_cache.get_by_relay_short_ids(&wanted);
                let mut transactions = Vec::with_capacity(transaction_short_ids.len());
                let mut missing_short_ids = Vec::new();
                missing = 0;
                for short_id in &transaction_short_ids {
                    if let Some(transaction) =
                        verified.get(short_id).or_else(|| relayed.get(short_id))
                    {
                        transactions.push(transaction.clone());
                    } else {
                        missing += 1;
                        missing_short_ids.push(*short_id);
                    }
                }
                if missing == 0 {
                    let block = norn_common::types::BlockV2 {
                        header,
                        transactions,
                        consensus_data,
                    };
                    info!(
                        "Reconstructed compact V2 proposal height={} transactions={} elapsed_ms={}",
                        proposal.height,
                        block.transactions.len(),
                        started.elapsed().as_millis()
                    );
                    if let Err(error) = driver
                        .dispatch(ConsensusDriverEvent::NetworkProposal { proposal, block })
                        .await
                    {
                        warn!("Failed to submit compact V2 proposal: {}", error);
                    }
                    return;
                }
                if attempt == 0
                    && missing_short_ids.len()
                        <= norn_common::consensus_types::MAX_COMPACT_REPAIR_TRANSACTIONS
                {
                    let request = ConsensusEnvelope {
                        wire_version: chain_context.wire_version,
                        protocol_version: chain_context.protocol_version,
                        chain_id: chain_context.chain_id,
                        genesis_hash: chain_context.genesis_hash,
                        payload: ConsensusMessage::CompactTransactionRequestV2 {
                            height: proposal.height,
                            round: proposal.round,
                            block_id: proposal.block_id,
                            transaction_short_ids: missing_short_ids,
                        },
                    };
                    match bincode::serialize(&request) {
                        Ok(bytes) => {
                            if let Err(error) = network_tx
                                .send(NetworkCommand::BroadcastConsensus(bytes))
                                .await
                            {
                                warn!("Failed to request compact transaction repair: {}", error);
                            }
                        }
                        Err(error) => {
                            warn!(
                                "Failed to encode compact transaction repair request: {}",
                                error
                            )
                        }
                    }
                }
                if missing > norn_common::consensus_types::MAX_COMPACT_REPAIR_TRANSACTIONS {
                    break;
                }
                if attempt < 20 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }

            // Do not request a multi-megabyte full block while the active
            // round is still exchanging votes. That fallback caused every
            // validator holding the candidate to broadcast a full response,
            // delaying a correct prevote by more than six seconds. A later
            // Commit for a genuinely missing candidate still uses the normal
            // BlockRequest recovery path.
            warn!(
                "Compact V2 proposal height={} still misses {} transaction bodies after repair window",
                proposal.height, missing
            );
        });
    }

    async fn respond_to_compact_transaction_request(
        &self,
        height: u64,
        round: u32,
        block_id: norn_common::types::BlockId,
        transaction_short_ids: Vec<u64>,
    ) {
        let candidate = if let Some(candidate) = self
            .consensus
            .get_validated_candidate_for_commit(height, round, block_id)
            .await
        {
            Some(candidate)
        } else {
            match self
                .finality_store
                .recover_pending_proposal(height, round)
                .await
            {
                Ok(Some((proposal, block))) if proposal.block_id == block_id => {
                    Some((proposal, block))
                }
                Ok(_) => None,
                Err(error) => {
                    warn!(
                        "Failed to recover pending proposal for compact body repair: {}",
                        error
                    );
                    None
                }
            }
        };
        let Some((proposal, block)) = candidate else {
            return;
        };
        let local_validator_id = self.consensus.state_machine.read().await.local_validator_id;
        if Some(proposal.proposer) != local_validator_id {
            // The request is broadcast, but exactly one validator owns the
            // proposal. Other validators stay silent to avoid N-fold response
            // amplification on the latency-sensitive control plane.
            return;
        }

        let wanted = transaction_short_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let mut found = HashMap::with_capacity(wanted.len());
        let mut ambiguous = HashSet::new();
        for transaction in block.transactions {
            let short_id = transaction.transaction_id.relay_short_id();
            if !wanted.contains(&short_id) || ambiguous.contains(&short_id) {
                continue;
            }
            if found.insert(short_id, transaction).is_some() {
                found.remove(&short_id);
                ambiguous.insert(short_id);
            }
        }
        let transactions = transaction_short_ids
            .into_iter()
            .filter_map(|short_id| found.remove(&short_id))
            .collect::<Vec<_>>();
        if transactions.is_empty() {
            return;
        }
        let count = transactions.len();
        // Keep each data-plane frame small enough for control-gossip votes to
        // interleave on low-bandwidth Raspberry Pi Wi-Fi links.
        const REPAIR_BATCH_TRANSACTIONS: usize = 256;
        for chunk in transactions.chunks(REPAIR_BATCH_TRANSACTIONS) {
            let gossip = match norn_common::types::TransactionV2Batch::encode(chunk.to_vec()) {
                Ok(gossip) => gossip,
                Err(error) => {
                    warn!(
                        "Failed to encode compact transaction repair batch: {}",
                        error
                    );
                    return;
                }
            };
            if let Err(error) = self
                .network
                .control_tx
                .send(NetworkCommand::BroadcastTransaction(gossip))
                .await
            {
                warn!(
                    "Failed to enqueue compact transaction repair batch: {}",
                    error
                );
                return;
            }
        }
        info!(
            "Responded to compact transaction repair height={} transactions={}",
            height, count
        );
    }

    async fn request_v2_finality(&self, height: u64) {
        if height == 0 {
            return;
        }
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::FinalityRequest { height },
        };
        match bincode::serialize(&envelope) {
            Ok(bytes) => {
                if let Err(error) = self
                    .network
                    .control_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                {
                    warn!(
                        "Failed to request missing V2 finality at height {}: {}",
                        height, error
                    );
                }
            }
            Err(error) => warn!("Failed to encode V2 finality request: {}", error),
        }
    }

    async fn respond_with_v2_block(
        &self,
        proposal: norn_common::consensus_types::Proposal,
        block: norn_common::types::BlockV2,
    ) {
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::BlockResponse { proposal, block },
        };
        match bincode::serialize(&envelope) {
            Ok(bytes) => {
                if let Err(error) = self
                    .network
                    .control_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                {
                    warn!("Failed to enqueue V2 block response: {}", error);
                }
            }
            Err(error) => warn!("Failed to encode V2 block response: {}", error),
        }
    }

    async fn respond_with_v2_finality(
        &self,
        finalized: norn_common::consensus_types::FinalizedBlockV2,
    ) {
        let envelope = ConsensusEnvelope {
            wire_version: self.chain_context.wire_version,
            protocol_version: self.chain_context.protocol_version,
            chain_id: self.chain_context.chain_id,
            genesis_hash: self.chain_context.genesis_hash,
            payload: ConsensusMessage::FinalityResponse { finalized },
        };
        match bincode::serialize(&envelope) {
            Ok(bytes) => {
                if let Err(error) = self
                    .network
                    .control_tx
                    .send(NetworkCommand::BroadcastConsensus(bytes))
                    .await
                {
                    warn!("Failed to enqueue V2 finality response: {}", error);
                }
            }
            Err(error) => warn!("Failed to encode V2 finality response: {}", error),
        }
    }

    async fn respond_to_v2_block_request(
        &self,
        height: u64,
        round: u32,
        block_id: norn_common::types::BlockId,
    ) {
        if let Some((proposal, block)) = self
            .consensus
            .get_validated_candidate_for_commit(height, round, block_id)
            .await
        {
            info!(
                "Responding to V2 block request at height {} from validated candidate",
                height
            );
            self.respond_with_v2_block(proposal, block).await;
            return;
        }

        // Candidate caches are intentionally pruned after durable finality.
        // The finality record remains an authoritative source for a missed
        // proposal, including its VRF proof and proposal context.
        match self.finality_store.recover_finalized_v2(height).await {
            Ok(Some(finalized))
                if finalized.commit.block_id == block_id && finalized.commit.round == round =>
            {
                info!(
                    "Responding to V2 block request at height {} from durable finality",
                    height
                );
                self.respond_with_v2_block(finalized.proposal, finalized.block)
                    .await;
            }
            Ok(Some(_)) => warn!(
                "Refused V2 block request with a block ID different from durable height {}",
                height
            ),
            Ok(None) => warn!(
                "No durable or in-memory V2 candidate available for requested height {} block {:?}",
                height, block_id
            ),
            Err(error) => warn!(
                "Failed to recover durable V2 block for requested height {}: {}",
                height, error
            ),
        }
    }

    async fn respond_to_v2_finality_request(&self, height: u64) {
        match self.finality_store.recover_finalized_v2(height).await {
            Ok(Some(finalized)) => self.respond_with_v2_finality(finalized).await,
            Ok(None) => debug!(
                "No durable V2 finality record exists at requested height {}",
                height
            ),
            Err(error) => warn!(
                "Failed to recover durable V2 finality at requested height {}: {}",
                height, error
            ),
        }
    }

    pub async fn run_loop(
        &mut self,
        mut network_events: tokio::sync::mpsc::Receiver<norn_network::service::NetworkEvent>,
    ) {
        loop {
            tokio::select! {
                event = network_events.recv() => {
                    match event {
                        Some(e) => {
                            match e {
                                norn_network::service::NetworkEvent::Listening(address) => {
                                    info!("Network listening at {:?}", address);
                                }
                                norn_network::service::NetworkEvent::PeerConnected(peer_id) => {
                                    info!("Network peer connected: {:?}", peer_id);
                                }
                                norn_network::service::NetworkEvent::DialFailed { address, reason } => {
                                    warn!("Network dial to {:?} failed: {}", address, reason);
                                }
                                norn_network::service::NetworkEvent::PeerAuthenticated { peer_id, role } => {
                                    info!("Authenticated network peer {:?} as {:?}", peer_id, role);
                                    if let Ok(Some(tip)) = self.finality_store.recover_canonical_tip().await {
                                        if let Ok(next_height) = tip.next_height() {
                                            self.request_v2_finality(next_height).await;
                                        }
                                    }
                                }
                                norn_network::service::NetworkEvent::PeerDisconnected(peer_id) => {
                                    info!("Network peer disconnected: {:?}", peer_id);
                                }
                                norn_network::service::NetworkEvent::BlockReceived(data) => {
                                    if data.len() > MAX_BLOCK_MESSAGE_BYTES {
                                        warn!("Rejected oversized block network message");
                                        continue;
                                    }
                                    self.peer_manager.handle_network_event(norn_network::service::NetworkEvent::BlockReceived(data)).await;
                                }
                                norn_network::service::NetworkEvent::TransactionReceived(data) => {
                                    let byte_limit = if data.starts_with(norn_common::types::TransactionV2Batch::MAGIC) {
                                        MAX_TRANSACTION_BATCH_MESSAGE_BYTES
                                    } else {
                                        MAX_TRANSACTION_MESSAGE_BYTES
                                    };
                                    if data.len() > byte_limit {
                                        warn!("Rejected oversized transaction network message");
                                        continue;
                                    }
                                    // Do not await CPU-bound signature verification here. The
                                    // handler owns a bounded worker queue; when it is full, drop
                                    // this gossip copy rather than starving Proposal/Prevote/
                                    // Precommit delivery. The proposer still retains accepted
                                    // transactions and includes them in the consensus proposal.
                                    if !self.tx_handler.try_enqueue(data) {
                                        debug!("Dropped transaction gossip because verification queue is saturated");
                                    }
                                }
                                norn_network::service::NetworkEvent::ConsensusMessageReceived(data) => {
                                    debug!(
                                        "Node received consensus event ({} bytes)",
                                        data.len()
                                    );
                                    if data.len() > MAX_CONSENSUS_ENVELOPE_BYTES {
                                        warn!("Rejected oversized consensus network message");
                                        continue;
                                    }

                                    let envelope = match ConsensusEnvelope::decode_and_validate_with_limits(
                                        &data,
                                        &self.chain_context,
                                        &self.resource_limits,
                                    ) {
                                        Ok(envelope) => envelope,
                                        Err(e) => {
                                            warn!("Rejected consensus envelope: {}", e);
                                            continue;
                                        }
                                    };
                                    match envelope.payload {
                                            ConsensusMessage::ProposalV2 { proposal, block } => {
                                                if let Err(error) = self
                                                    .consensus_driver
                                                    .dispatch(ConsensusDriverEvent::NetworkProposal {
                                                        proposal,
                                                        block,
                                                    })
                                                    .await
                                                {
                                                    warn!(
                                                        "Failed to submit V2 proposal to consensus driver: {}",
                                                        error
                                                    );
                                                    error!("Consensus driver stopped; entering node fail-stop");
                                                    break;
                                                }
                                }
                                             ConsensusMessage::CompactProposalV2 {
                                                 proposal,
                                                 header,
                                                 consensus_data,
                                                 transaction_short_ids,
                                             } => {
                                                 self.dispatch_compact_v2_proposal(
                                                     proposal,
                                                     header,
                                                     consensus_data,
                                                     transaction_short_ids,
                                                 );
                                             }
                                             ConsensusMessage::CompactTransactionRequestV2 {
                                                 height,
                                                 round,
                                                 block_id,
                                                 transaction_short_ids,
                                             } => {
                                                 self.respond_to_compact_transaction_request(
                                                     height,
                                                     round,
                                                     block_id,
                                                     transaction_short_ids,
                                                 )
                                                 .await;
                                             }
                                             ConsensusMessage::BlockRequest { height, round, block_id } => {
                                                 info!("Received V2 block request at height {} round {}", height, round);
                                                 self.respond_to_v2_block_request(height, round, block_id).await;
                                             }
                                             ConsensusMessage::BlockResponse { proposal, block } => {
                                                  info!("Received V2 block response at height {}", proposal.height);
                                                  if let Err(error) = self
                                                      .consensus_driver
                                                      .dispatch(ConsensusDriverEvent::NetworkProposal {
                                                          proposal,
                                                          block,
                                                      })
                                                      .await
                                                  {
                                                      warn!("Failed to submit V2 block response to consensus driver: {}", error);
                                                      error!("Consensus driver stopped; entering node fail-stop");
                                                      break;
                                                  }
                                              }
                                             ConsensusMessage::FinalityRequest { height } => {
                                                 self.respond_to_v2_finality_request(height).await;
                                             }
                                              ConsensusMessage::FinalityResponse { finalized } => {
                                                  let height = finalized.commit.height;
                                                  self.pending_commits
                                                      .lock()
                                                      .await
                                                      .insert(
                                                        (height, finalized.commit.round, finalized.commit.block_id),
                                                          finalized.commit,
                                                      );
                                                  if let Err(error) = self
                                                      .consensus_driver
                                                      .dispatch(ConsensusDriverEvent::NetworkProposal {
                                                          proposal: finalized.proposal,
                                                          block: finalized.block,
                                                      })
                                                      .await
                                                  {
                                                      warn!(
                                                          "Failed to submit V2 finalized-record response at height {}: {}",
                                                          height, error
                                                      );
                                                      error!("Consensus driver stopped; entering node fail-stop");
                                                      break;
                                                  }
                                              }
                                              ConsensusMessage::Vote(vote) => {
                                                 if let Err(error) = self
                                                     .consensus_driver
                                                     .dispatch(ConsensusDriverEvent::NetworkVote(vote))
                                                     .await
                                                 {
                                                     warn!("Failed to submit V2 vote to consensus driver: {}", error);
                                                     error!("Consensus driver stopped; entering node fail-stop");
                                                     break;
                                                 }
                                              }
                                              ConsensusMessage::Commit(commit_cert) => {
                                                  let candidate_available = self
                                                      .consensus
                                                      .get_validated_candidate_for_commit(
                                                         commit_cert.height,
                                                         commit_cert.round,
                                                         commit_cert.block_id,
                                                     )
                                                     .await
                                                     .is_some();
                                                 let already_durable = self
                                                     .finality_store
                                                     .recover_finalized_v2(commit_cert.height)
                                                     .await
                                                     .ok()
                                                     .flatten()
                                                     .is_some();
                                                  if !candidate_available && !already_durable {
                                                      self.pending_commits.lock().await.insert(
                                                          (commit_cert.height, commit_cert.round, commit_cert.block_id),
                                                          commit_cert.clone(),
                                                      );
                                                     self.request_v2_block(
                                                         commit_cert.height,
                                                         commit_cert.round,
                                                         commit_cert.block_id,
                                                     )
                                                     .await;
                                                     info!(
                                                         "Queued Commit for missing V2 candidate at height {}; requested Proposal/Block",
                                                         commit_cert.height
                                                      );
                                                      continue;
                                                  }
                                                  if let Err(error) = self
                                                      .consensus_driver
                                                      .dispatch(ConsensusDriverEvent::NetworkCommit(commit_cert))
                                                      .await
                                                  {
                                                      error!("Failed to submit Commit to consensus driver: {}", error);
                                                      error!("Consensus driver stopped; entering node fail-stop");
                                                      break;
                                                  }
                                              }
                                             _ => {
                                                 warn!("UnsupportedProtocolVersion: rejected legacy consensus payload");
                                             }
                                    }
                                }
                            }
                        }
                        None => break,
                    }
                }
                _ = signal::ctrl_c() => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }
    }
}
