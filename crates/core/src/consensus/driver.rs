//! Single-writer consensus event driver.
//!
//! The driver is the only component allowed to order consensus events.  Heavy
//! proposal, vote and commit validation is delegated to workers through
//! [`ConsensusActionExecutor`].  Workers must return a completion event with
//! the request's context token; the driver drops results that no longer match
//! the active request.

use super::povf::ValidatedCandidate;
use crate::execution::overlay::CanonicalStateCheckpoint;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use norn_common::consensus_types::{
    CommitCertificate, FinalizedBlockV2, Proposal, SignedVote, StakeSnapshot,
};
use norn_common::types::{BlockV2, Hash, StakeSnapshotHash};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, warn};

const HIGH_PRIORITY_BUDGET: usize = 32;
const MAX_ACTION_RETRIES: usize = 3;
const ACTION_RETRY_BASE: Duration = Duration::from_millis(50);

/// An action may opt into bounded retry only when the action is idempotent
/// (currently Commit/Vote broadcast). Unknown errors remain fatal and stop
/// the single consensus driver.
#[derive(Debug)]
pub struct RetryableConsensusActionError {
    reason: String,
    max_retries: usize,
}

impl RetryableConsensusActionError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self::with_max_retries(reason, MAX_ACTION_RETRIES)
    }

    pub fn with_max_retries(reason: impl Into<String>, max_retries: usize) -> Self {
        Self {
            reason: reason.into(),
            max_retries,
        }
    }

    fn max_retries(&self) -> usize {
        self.max_retries
    }
}

impl std::fmt::Display for RetryableConsensusActionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "retryable consensus action failure: {}",
            self.reason
        )
    }
}

impl std::error::Error for RetryableConsensusActionError {}

fn action_retry_delay(retry_number: usize) -> Duration {
    let multiplier = 1u32 << retry_number.saturating_sub(1).min(5);
    ACTION_RETRY_BASE.saturating_mul(multiplier)
}

/// Timer phases are intentionally independent from vote phases.  A timer is
/// about waiting for a state transition; it is not itself a vote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeoutStep {
    Propose,
    PrevoteWait,
    PrecommitWait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeoutToken {
    pub height: u64,
    pub round: u32,
    pub step: TimeoutStep,
    pub generation: u64,
    pub deadline: tokio::time::Instant,
}

/// Context captured when work is submitted to a worker.  The driver checks
/// every completion against the exact request and token before applying it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConsensusContextToken {
    pub height: u64,
    pub round: u32,
    pub generation: u64,
    pub parent_block_hash: Hash,
    pub stake_snapshot_hash: StakeSnapshotHash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValidationRequestId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalValidationResult {
    Accepted(ValidatedCandidate),
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoteValidationResult {
    Accepted,
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitValidationResult {
    Accepted,
    Rejected(String),
}

/// The output of the worker-only finality preparation stage.  It contains no
/// live consensus mutation; the driver may persist and activate it only after
/// the request/context token has been checked.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedFinality {
    pub finalized: FinalizedBlockV2,
    pub state_write_values: Vec<Vec<u8>>,
    pub checkpoint: CanonicalStateCheckpoint,
    pub next_snapshot: StakeSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FinalityPreparationResult {
    Prepared(PreparedFinality),
    AlreadyDurable(CommitCertificate),
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConsensusDriverEvent {
    NetworkProposal {
        proposal: Proposal,
        block: BlockV2,
    },
    /// A locally produced proposal must enter the same validation/apply path
    /// as a network proposal.
    LocalProposalBuilt {
        proposal: Proposal,
        block: BlockV2,
    },
    NetworkVote(SignedVote),
    NetworkCommit(CommitCertificate),
    /// Begin a fresh Tendermint round. The producer, startup recovery path,
    /// or timeout executor supplies the protocol-selected deadline; the
    /// driver owns replacing any older timer and advancing its generation.
    RoundStarted {
        height: u64,
        round: u32,
        deadline: tokio::time::Instant,
    },
    /// Begin waiting for the next vote phase after a durable local vote.
    StepStarted {
        height: u64,
        round: u32,
        step: TimeoutStep,
        deadline: tokio::time::Instant,
    },
    Timeout(TimeoutToken),
    /// The timeout executor confirmed that the state-machine step matched
    /// the timer and applied the timeout transition.
    TimeoutApplied(TimeoutToken),
    /// The timeout executor observed that the timer was no longer applicable
    /// (for example because finality had already been formed).
    TimeoutIgnored(TimeoutToken),
    ProposalValidationCompleted {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        result: ProposalValidationResult,
    },
    VoteValidationCompleted {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        vote: SignedVote,
        result: VoteValidationResult,
    },
    CommitValidationCompleted {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        commit: CommitCertificate,
        result: CommitValidationResult,
    },
    FinalityPreparationCompleted {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        result: FinalityPreparationResult,
    },
    /// A vote has passed the SafetyStore WAL/signing acknowledgement.
    VotePersisted(SignedVote),
    VoteDurablySigned(SignedVote),
    /// FinalityStore has completed the durable transaction.
    FinalityPersisted(CommitCertificate),
    FinalityDurable(CommitCertificate),
    /// Durable commit succeeded but in-memory activation failed.  This is a
    /// safety fault, not an ordinary network error.
    ActivationFailed(String),
    /// Neither a complete old nor a complete new finality transaction could
    /// be proven after a persistence error. The node must fail-stop and
    /// reconcile from durable state on restart.
    FinalityIndeterminate(String),
    /// Re-submit one exact idempotent action after a non-blocking backoff.
    RetryAction {
        action: ConsensusDriverAction,
        attempt: usize,
        max_retries: usize,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConsensusDriverAction {
    ValidateProposal {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        proposal: Proposal,
        block: BlockV2,
    },
    ValidateVote {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        vote: SignedVote,
    },
    ValidateCommit {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        commit: CommitCertificate,
    },
    ApplyValidatedProposal {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        candidate: ValidatedCandidate,
    },
    ApplyValidatedVote {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        vote: SignedVote,
    },
    PrepareFinality {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        commit: CommitCertificate,
    },
    PersistPreparedFinality {
        request_id: ValidationRequestId,
        context: ConsensusContextToken,
        prepared: PreparedFinality,
    },
    HandleTimeout(TimeoutToken),
    BroadcastVote(SignedVote),
    ScheduleTimeout(TimeoutToken),
    CancelTimeout(TimeoutToken),
    BroadcastCommit(CommitCertificate),
    /// Release a pending-finality cache pin after a preparation result was
    /// discarded as stale by the single-writer driver.
    UnpinPendingFinality {
        height: u64,
        round: u32,
        block_id: norn_common::types::BlockId,
    },
}

/// A worker/executor receives actions only from the driver.  It may spawn
/// heavy validation work, but every result must be submitted back through the
/// supplied handle.  This keeps the event loop responsive without creating a
/// second consensus writer.
#[async_trait]
pub trait ConsensusActionExecutor: Send + Sync {
    async fn execute(
        &self,
        action: ConsensusDriverAction,
        handle: ConsensusDriverHandle,
    ) -> Result<()>;
}

#[derive(Default)]
struct NoopActionExecutor;

#[async_trait]
impl ConsensusActionExecutor for NoopActionExecutor {
    async fn execute(
        &self,
        _action: ConsensusDriverAction,
        _handle: ConsensusDriverHandle,
    ) -> Result<()> {
        Ok(())
    }
}

struct DriverRequest {
    event: ConsensusDriverEvent,
    reply: Option<tokio::sync::oneshot::Sender<Result<Vec<ConsensusDriverAction>>>>,
}

#[derive(Clone)]
pub struct ConsensusDriverHandle {
    high_tx: mpsc::Sender<DriverRequest>,
    low_tx: mpsc::Sender<DriverRequest>,
}

impl ConsensusDriverHandle {
    pub async fn submit(&self, event: ConsensusDriverEvent) -> Result<()> {
        let tx = if is_high_priority(&event) {
            &self.high_tx
        } else {
            &self.low_tx
        };
        tx.send(DriverRequest { event, reply: None })
            .await
            .map_err(|_| anyhow!("consensus driver queue is closed"))
    }
}

#[derive(Clone)]
pub struct ConsensusDriver {
    handle: ConsensusDriverHandle,
}

#[derive(Default)]
struct DriverState {
    active_timeout: Option<TimeoutToken>,
    generation: u64,
    next_request_id: u64,
    active_height: Option<u64>,
    active_round: Option<u32>,
    active_parent_block_hash: Option<Hash>,
    active_snapshot_hash: Option<StakeSnapshotHash>,
    pending: HashMap<ValidationRequestId, ConsensusContextToken>,
    pending_finality: HashMap<ValidationRequestId, PendingFinality>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingFinality {
    context: ConsensusContextToken,
    height: u64,
    round: u32,
    block_id: norn_common::types::BlockId,
}

impl ConsensusDriver {
    /// Start a driver with a no-op executor.  This remains useful for pure
    /// driver tests; production nodes use `start_with_executor`.
    pub fn start(capacity: usize) -> Result<Self> {
        Self::start_with_executor(capacity, Arc::new(NoopActionExecutor))
    }

    pub fn start_with_executor(
        capacity: usize,
        executor: Arc<dyn ConsensusActionExecutor>,
    ) -> Result<Self> {
        if capacity == 0 {
            return Err(anyhow!("consensus driver queue capacity must be non-zero"));
        }
        let high_capacity = capacity.max(8);
        let (high_tx, mut high_rx) = mpsc::channel::<DriverRequest>(high_capacity);
        let (low_tx, mut low_rx) = mpsc::channel::<DriverRequest>(capacity);
        let handle = ConsensusDriverHandle { high_tx, low_tx };
        let worker_handle = handle.clone();
        tokio::spawn(async move {
            let mut state = DriverState::default();
            let mut high_count = 0usize;
            loop {
                let request = receive_next(&mut high_rx, &mut low_rx, &mut high_count).await;
                let Some(request) = request else { break };
                let fail_stop = matches!(
                    &request.event,
                    ConsensusDriverEvent::ActivationFailed(_)
                        | ConsensusDriverEvent::FinalityIndeterminate(_)
                );
                let retry_context = match &request.event {
                    ConsensusDriverEvent::RetryAction {
                        attempt,
                        max_retries,
                        ..
                    } => Some((*attempt, *max_retries)),
                    _ => None,
                };
                let result = process_event(&mut state, request.event).await;
                match result {
                    Ok(actions) => {
                        let mut execution_error = None;
                        'actions: for action in actions.iter().cloned() {
                            let mut retries = retry_context.map_or(0, |(attempt, _)| attempt);
                            loop {
                                match executor
                                    .execute(action.clone(), worker_handle.clone())
                                    .await
                                {
                                    Ok(()) => break,
                                    Err(error) => {
                                        let Some(retryable) =
                                            error.downcast_ref::<RetryableConsensusActionError>()
                                        else {
                                            execution_error = Some(error);
                                            break 'actions;
                                        };
                                        let retry_limit = retry_context
                                            .map(|(_, max_retries)| max_retries)
                                            .map_or(retryable.max_retries(), |max_retries| {
                                                max_retries.min(retryable.max_retries())
                                            });
                                        if retries < retry_limit {
                                            retries += 1;
                                            let delay = action_retry_delay(retries);
                                            warn!(
                                                attempt = retries,
                                                max_retries = retry_limit,
                                                delay_ms = delay.as_millis() as u64,
                                                "retrying idempotent consensus action after transient failure"
                                            );
                                            let retry_handle = worker_handle.clone();
                                            let retry_action = action.clone();
                                            tokio::spawn(async move {
                                                tokio::time::sleep(delay).await;
                                                let _ = retry_handle
                                                    .submit(ConsensusDriverEvent::RetryAction {
                                                        action: retry_action,
                                                        attempt: retries,
                                                        max_retries: retry_limit,
                                                    })
                                                    .await;
                                            });
                                            break;
                                        } else {
                                            execution_error = Some(error);
                                            break 'actions;
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(error) = execution_error {
                            let message = error.to_string();
                            if let Some(reply) = request.reply {
                                let _ = reply.send(Err(anyhow!(message.clone())));
                            }
                            error!(
                                "ConsensusDriver action execution failed; entering fail-stop: {}",
                                message
                            );
                            break;
                        }
                        if let Some(reply) = request.reply {
                            let _ = reply.send(Ok(actions));
                        }
                    }
                    Err(error) => {
                        if let Some(reply) = request.reply {
                            let _ = reply.send(Err(error));
                        }
                        if fail_stop {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self { handle })
    }

    pub async fn dispatch(
        &self,
        event: ConsensusDriverEvent,
    ) -> Result<Vec<ConsensusDriverAction>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let tx = if is_high_priority(&event) {
            &self.handle.high_tx
        } else {
            &self.handle.low_tx
        };
        tx.send(DriverRequest {
            event,
            reply: Some(reply_tx),
        })
        .await
        .map_err(|_| anyhow!("consensus driver queue is closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow!("consensus driver worker stopped"))?
    }

    pub fn handle(&self) -> ConsensusDriverHandle {
        self.handle.clone()
    }
}

fn is_high_priority(event: &ConsensusDriverEvent) -> bool {
    matches!(
        event,
        ConsensusDriverEvent::Timeout(_)
            | ConsensusDriverEvent::TimeoutApplied(_)
            | ConsensusDriverEvent::TimeoutIgnored(_)
            | ConsensusDriverEvent::RoundStarted { .. }
            | ConsensusDriverEvent::StepStarted { .. }
            | ConsensusDriverEvent::VotePersisted(_)
            | ConsensusDriverEvent::VoteDurablySigned(_)
            | ConsensusDriverEvent::FinalityPersisted(_)
            | ConsensusDriverEvent::FinalityDurable(_)
            | ConsensusDriverEvent::ProposalValidationCompleted { .. }
            | ConsensusDriverEvent::VoteValidationCompleted { .. }
            | ConsensusDriverEvent::CommitValidationCompleted { .. }
            | ConsensusDriverEvent::FinalityPreparationCompleted { .. }
            | ConsensusDriverEvent::ActivationFailed(_)
            | ConsensusDriverEvent::FinalityIndeterminate(_)
            | ConsensusDriverEvent::Shutdown
    )
}

async fn receive_next(
    high_rx: &mut mpsc::Receiver<DriverRequest>,
    low_rx: &mut mpsc::Receiver<DriverRequest>,
    high_count: &mut usize,
) -> Option<DriverRequest> {
    if *high_count < HIGH_PRIORITY_BUDGET {
        tokio::select! {
            biased;
            request = high_rx.recv() => {
                if request.is_some() { *high_count += 1; }
                request
            }
            request = low_rx.recv() => {
                *high_count = 0;
                request
            }
        }
    } else {
        tokio::select! {
            biased;
            request = low_rx.recv() => {
                *high_count = 0;
                request
            }
            request = high_rx.recv() => {
                if request.is_some() { *high_count = 1; }
                request
            }
        }
    }
}

fn next_request_id(state: &mut DriverState) -> ValidationRequestId {
    state.next_request_id = state.next_request_id.saturating_add(1).max(1);
    ValidationRequestId(state.next_request_id)
}

fn proposal_context(state: &DriverState, proposal: &Proposal) -> ConsensusContextToken {
    ConsensusContextToken {
        height: proposal.height,
        round: proposal.round,
        generation: state.generation,
        parent_block_hash: proposal.parent_block_hash,
        stake_snapshot_hash: proposal.stake_snapshot_hash,
    }
}

fn vote_context(state: &DriverState, vote: &SignedVote) -> ConsensusContextToken {
    ConsensusContextToken {
        height: vote.height,
        round: vote.round,
        generation: state.generation,
        parent_block_hash: state.active_parent_block_hash.unwrap_or_default(),
        stake_snapshot_hash: vote.stake_snapshot_hash,
    }
}

fn commit_context(state: &DriverState, commit: &CommitCertificate) -> ConsensusContextToken {
    ConsensusContextToken {
        height: commit.height,
        round: commit.round,
        generation: state.generation,
        parent_block_hash: state.active_parent_block_hash.unwrap_or_default(),
        stake_snapshot_hash: commit.stake_snapshot_hash,
    }
}

fn replace_timeout(
    state: &mut DriverState,
    height: u64,
    round: u32,
    step: TimeoutStep,
    deadline: tokio::time::Instant,
) -> Vec<ConsensusDriverAction> {
    if state.active_height != Some(height) {
        state.active_parent_block_hash = None;
        state.active_snapshot_hash = None;
    }
    state.active_height = Some(height);
    state.active_round = Some(round);
    state.generation = state.generation.saturating_add(1);
    state.pending.clear();
    let stale_finality = state
        .pending_finality
        .values()
        .map(|pending| (pending.height, pending.round, pending.block_id))
        .collect::<Vec<_>>();
    state.pending_finality.clear();
    let token = TimeoutToken {
        height,
        round,
        step,
        generation: state.generation,
        deadline,
    };
    let mut actions = stale_finality
        .into_iter()
        .map(
            |(height, round, block_id)| ConsensusDriverAction::UnpinPendingFinality {
                height,
                round,
                block_id,
            },
        )
        .collect::<Vec<_>>();
    actions.reserve(2);
    if let Some(previous) = state.active_timeout.replace(token) {
        actions.push(ConsensusDriverAction::CancelTimeout(previous));
    }
    actions.push(ConsensusDriverAction::ScheduleTimeout(token));
    actions
}

fn register_context(state: &mut DriverState, context: &ConsensusContextToken) -> bool {
    if let Some(height) = state.active_height {
        if height != context.height || state.active_round != Some(context.round) {
            return false;
        }
    } else {
        state.active_height = Some(context.height);
        state.active_round = Some(context.round);
    }

    if let Some(snapshot_hash) = state.active_snapshot_hash {
        if snapshot_hash != context.stake_snapshot_hash {
            return false;
        }
    } else {
        state.active_snapshot_hash = Some(context.stake_snapshot_hash);
    }

    if let Some(parent_block_hash) = state.active_parent_block_hash {
        if parent_block_hash != context.parent_block_hash {
            return false;
        }
    } else if context.parent_block_hash != Hash::default() {
        state.active_parent_block_hash = Some(context.parent_block_hash);
    }
    true
}

fn context_is_current(state: &DriverState, context: &ConsensusContextToken) -> bool {
    context.generation == state.generation
        && state.active_height == Some(context.height)
        && state.active_round == Some(context.round)
        && state.active_snapshot_hash.map_or(true, |snapshot_hash| {
            snapshot_hash == context.stake_snapshot_hash
        })
        && state
            .active_parent_block_hash
            .map_or(true, |parent_block_hash| {
                parent_block_hash == context.parent_block_hash
            })
}

fn invalidate_after_finality(
    state: &mut DriverState,
    finalized_height: u64,
) -> Vec<ConsensusDriverAction> {
    state.generation = state.generation.saturating_add(1);
    state.pending.clear();
    let stale_finality = state
        .pending_finality
        .iter()
        .filter_map(|(request_id, pending)| {
            (pending.height <= finalized_height).then_some((*request_id, *pending))
        })
        .collect::<Vec<_>>();
    let actions = stale_finality
        .iter()
        .map(|(_, pending)| ConsensusDriverAction::UnpinPendingFinality {
            height: pending.height,
            round: pending.round,
            block_id: pending.block_id,
        })
        .collect::<Vec<_>>();
    for (request_id, _) in stale_finality {
        state.pending_finality.remove(&request_id);
    }
    let next_height = finalized_height.saturating_add(1);
    if state
        .active_height
        .map_or(true, |active_height| active_height <= finalized_height)
    {
        state.active_height = Some(next_height);
        state.active_round = Some(0);
        state.active_parent_block_hash = None;
        state.active_snapshot_hash = None;
    }
    actions
}

async fn process_event(
    state: &mut DriverState,
    event: ConsensusDriverEvent,
) -> Result<Vec<ConsensusDriverAction>> {
    let actions = match event {
        ConsensusDriverEvent::NetworkProposal { proposal, block }
        | ConsensusDriverEvent::LocalProposalBuilt { proposal, block } => {
            let request_id = next_request_id(state);
            let context = proposal_context(state, &proposal);
            if !register_context(state, &context) {
                return Ok(Vec::new());
            }
            state.pending.insert(request_id, context);
            vec![ConsensusDriverAction::ValidateProposal {
                request_id,
                context,
                proposal,
                block,
            }]
        }
        ConsensusDriverEvent::NetworkVote(vote) => {
            let request_id = next_request_id(state);
            let context = vote_context(state, &vote);
            if !register_context(state, &context) {
                return Ok(Vec::new());
            }
            state.pending.insert(request_id, context);
            vec![ConsensusDriverAction::ValidateVote {
                request_id,
                context,
                vote,
            }]
        }
        ConsensusDriverEvent::NetworkCommit(commit) => {
            let request_id = next_request_id(state);
            let context = commit_context(state, &commit);
            if !register_context(state, &context) {
                return Ok(Vec::new());
            }
            state.pending.insert(request_id, context);
            vec![ConsensusDriverAction::ValidateCommit {
                request_id,
                context,
                commit,
            }]
        }
        ConsensusDriverEvent::RetryAction { action, .. } => vec![action],
        ConsensusDriverEvent::RoundStarted {
            height,
            round,
            deadline,
        } => replace_timeout(state, height, round, TimeoutStep::Propose, deadline),
        ConsensusDriverEvent::StepStarted {
            height,
            round,
            step,
            deadline,
        } => replace_timeout(state, height, round, step, deadline),
        ConsensusDriverEvent::Timeout(token) => {
            if state.active_timeout != Some(token) {
                return Ok(Vec::new());
            }
            // The executor validates the real state-machine step before the
            // timeout is allowed to invalidate the active context.
            vec![ConsensusDriverAction::HandleTimeout(token)]
        }
        ConsensusDriverEvent::TimeoutApplied(token) => {
            if state.active_timeout == Some(token) {
                state.active_timeout = None;
                state.generation = state.generation.saturating_add(1);
            }
            Vec::new()
        }
        ConsensusDriverEvent::TimeoutIgnored(token) => {
            if state.active_timeout == Some(token) {
                state.active_timeout = None;
                vec![ConsensusDriverAction::CancelTimeout(token)]
            } else {
                Vec::new()
            }
        }
        ConsensusDriverEvent::ProposalValidationCompleted {
            request_id,
            context,
            result,
        } => {
            if state.pending.remove(&request_id) != Some(context)
                || !context_is_current(state, &context)
            {
                return Ok(Vec::new());
            }
            match result {
                ProposalValidationResult::Accepted(candidate) => {
                    vec![ConsensusDriverAction::ApplyValidatedProposal {
                        request_id,
                        context,
                        candidate,
                    }]
                }
                ProposalValidationResult::Rejected(_) => Vec::new(),
            }
        }
        ConsensusDriverEvent::VoteValidationCompleted {
            request_id,
            context,
            vote,
            result,
        } => {
            if state.pending.remove(&request_id) != Some(context)
                || !context_is_current(state, &context)
            {
                return Ok(Vec::new());
            }
            match result {
                VoteValidationResult::Accepted => {
                    vec![ConsensusDriverAction::ApplyValidatedVote {
                        request_id,
                        context,
                        vote,
                    }]
                }
                VoteValidationResult::Rejected(_) => Vec::new(),
            }
        }
        ConsensusDriverEvent::CommitValidationCompleted {
            request_id,
            context,
            commit,
            result,
        } => {
            if state.pending.remove(&request_id) != Some(context)
                || !context_is_current(state, &context)
            {
                return Ok(Vec::new());
            }
            match result {
                CommitValidationResult::Accepted => {
                    state.pending_finality.insert(
                        request_id,
                        PendingFinality {
                            context,
                            height: commit.height,
                            round: commit.round,
                            block_id: commit.block_id,
                        },
                    );
                    let mut actions = Vec::with_capacity(2);
                    if let Some(previous) = state.active_timeout.take() {
                        actions.push(ConsensusDriverAction::CancelTimeout(previous));
                    }
                    actions.push(ConsensusDriverAction::PrepareFinality {
                        request_id,
                        context,
                        commit,
                    });
                    actions
                }
                CommitValidationResult::Rejected(_) => Vec::new(),
            }
        }
        ConsensusDriverEvent::FinalityPreparationCompleted {
            request_id,
            context,
            result,
        } => {
            let Some(pending) = state.pending_finality.remove(&request_id) else {
                return Ok(Vec::new());
            };
            if pending.context != context {
                // The completion is not authoritative for the request that
                // installed the pin.  It must nevertheless release that
                // request's pin; otherwise an out-of-order worker result can
                // permanently consume candidate-cache capacity.
                return Ok(vec![ConsensusDriverAction::UnpinPendingFinality {
                    height: pending.height,
                    round: pending.round,
                    block_id: pending.block_id,
                }]);
            }
            if !context_is_current(state, &context) {
                return Ok(vec![ConsensusDriverAction::UnpinPendingFinality {
                    height: pending.height,
                    round: pending.round,
                    block_id: pending.block_id,
                }]);
            }
            match result {
                FinalityPreparationResult::Prepared(prepared) => {
                    vec![ConsensusDriverAction::PersistPreparedFinality {
                        request_id,
                        context,
                        prepared,
                    }]
                }
                FinalityPreparationResult::AlreadyDurable(commit) => {
                    let mut actions = invalidate_after_finality(state, commit.height);
                    actions.push(ConsensusDriverAction::BroadcastCommit(commit));
                    actions
                }
                FinalityPreparationResult::Rejected(_) => {
                    vec![ConsensusDriverAction::UnpinPendingFinality {
                        height: pending.height,
                        round: pending.round,
                        block_id: pending.block_id,
                    }]
                }
            }
        }
        ConsensusDriverEvent::VotePersisted(vote)
        | ConsensusDriverEvent::VoteDurablySigned(vote) => {
            vec![ConsensusDriverAction::BroadcastVote(vote)]
        }
        ConsensusDriverEvent::FinalityPersisted(commit)
        | ConsensusDriverEvent::FinalityDurable(commit) => {
            let mut actions = invalidate_after_finality(state, commit.height);
            actions.reserve(2);
            if let Some(previous) = state.active_timeout.take() {
                actions.push(ConsensusDriverAction::CancelTimeout(previous));
            }
            actions.push(ConsensusDriverAction::BroadcastCommit(commit));
            actions
        }
        ConsensusDriverEvent::ActivationFailed(error) => {
            return Err(anyhow!("durable finality activation failed: {error}"));
        }
        ConsensusDriverEvent::FinalityIndeterminate(error) => {
            return Err(anyhow!(
                "durable finality outcome is indeterminate: {error}"
            ));
        }
        ConsensusDriverEvent::Shutdown => return Ok(Vec::new()),
    };

    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::BlockId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::sync::Notify;

    fn proposal(height: u64, round: u32, id: u8) -> Proposal {
        Proposal {
            protocol_version: Default::default(),
            chain_id: Default::default(),
            epoch: 0,
            height,
            round,
            valid_round: None,
            valid_round_certificate: None,
            block_id: BlockId(Hash([id; 32])),
            parent_block_hash: Hash([9; 32]),
            stake_snapshot_hash: StakeSnapshotHash([8; 32]),
            proposer: norn_common::types::ValidatorId([7; 32]),
            vrf_preout: [1; 32],
            vrf_proof: [2; 64],
            signature: [3; 64],
        }
    }

    fn commit(height: u64, round: u32, id: u8) -> CommitCertificate {
        CommitCertificate {
            protocol_version: Default::default(),
            chain_id: Default::default(),
            epoch: 0,
            height,
            round,
            block_id: BlockId(Hash([id; 32])),
            stake_snapshot_hash: StakeSnapshotHash([8; 32]),
            precommits: Vec::new(),
        }
    }

    #[tokio::test]
    async fn stale_worker_results_are_ignored_after_context_changes() {
        let driver = ConsensusDriver::start(8).unwrap();
        let p = proposal(1, 0, 1);
        let actions = driver
            .dispatch(ConsensusDriverEvent::NetworkProposal {
                proposal: p.clone(),
                block: BlockV2::default(),
            })
            .await
            .unwrap();
        let (request_id, context) = match &actions[0] {
            ConsensusDriverAction::ValidateProposal {
                request_id,
                context,
                ..
            } => (*request_id, *context),
            other => panic!("unexpected action: {other:?}"),
        };
        let stale = driver
            .dispatch(ConsensusDriverEvent::ProposalValidationCompleted {
                request_id,
                context: ConsensusContextToken {
                    generation: context.generation.saturating_add(1),
                    ..context
                },
                result: ProposalValidationResult::Rejected("stale".into()),
            })
            .await
            .unwrap();
        assert!(stale.is_empty());
    }

    #[tokio::test]
    async fn stale_worker_result_is_ignored_after_real_round_change() {
        let driver = ConsensusDriver::start(8).unwrap();
        let p = proposal(1, 0, 1);
        let actions = driver
            .dispatch(ConsensusDriverEvent::NetworkProposal {
                proposal: p.clone(),
                block: BlockV2::default(),
            })
            .await
            .unwrap();
        let (request_id, context) = match &actions[0] {
            ConsensusDriverAction::ValidateProposal {
                request_id,
                context,
                ..
            } => (*request_id, *context),
            other => panic!("unexpected action: {other:?}"),
        };

        let round_actions = driver
            .dispatch(ConsensusDriverEvent::RoundStarted {
                height: 1,
                round: 1,
                deadline: tokio::time::Instant::now(),
            })
            .await
            .unwrap();
        assert!(round_actions.iter().any(|action| matches!(
            action,
            ConsensusDriverAction::ScheduleTimeout(TimeoutToken { round: 1, .. })
        )));

        let stale = driver
            .dispatch(ConsensusDriverEvent::ProposalValidationCompleted {
                request_id,
                context,
                result: ProposalValidationResult::Accepted(ValidatedCandidate {
                    proposal: p,
                    block: BlockV2::default(),
                    derived_randomness: Hash([4; 32]),
                }),
            })
            .await
            .unwrap();
        assert!(stale.is_empty());
    }

    #[tokio::test]
    async fn completion_rechecks_generation_even_if_pending_entry_survives() {
        let mut state = DriverState::default();
        let p = proposal(1, 0, 1);
        let actions = process_event(
            &mut state,
            ConsensusDriverEvent::NetworkProposal {
                proposal: p.clone(),
                block: BlockV2::default(),
            },
        )
        .await
        .unwrap();
        let (request_id, context) = match &actions[0] {
            ConsensusDriverAction::ValidateProposal {
                request_id,
                context,
                ..
            } => (*request_id, *context),
            other => panic!("unexpected action: {other:?}"),
        };
        state.generation = state.generation.saturating_add(1);

        let stale = process_event(
            &mut state,
            ConsensusDriverEvent::ProposalValidationCompleted {
                request_id,
                context,
                result: ProposalValidationResult::Accepted(ValidatedCandidate {
                    proposal: p,
                    block: BlockV2::default(),
                    derived_randomness: Hash([5; 32]),
                }),
            },
        )
        .await
        .unwrap();
        assert!(stale.is_empty());
    }

    #[tokio::test]
    async fn completion_rechecks_snapshot_and_parent_even_if_pending_entry_survives() {
        let mut state = DriverState::default();
        let first = proposal(1, 0, 1);
        let second = proposal(1, 0, 2);
        let first_actions = process_event(
            &mut state,
            ConsensusDriverEvent::NetworkProposal {
                proposal: first.clone(),
                block: BlockV2::default(),
            },
        )
        .await
        .unwrap();
        let (first_request_id, first_context) = match &first_actions[0] {
            ConsensusDriverAction::ValidateProposal {
                request_id,
                context,
                ..
            } => (*request_id, *context),
            other => panic!("unexpected action: {other:?}"),
        };

        let second_actions = process_event(
            &mut state,
            ConsensusDriverEvent::NetworkProposal {
                proposal: second.clone(),
                block: BlockV2::default(),
            },
        )
        .await
        .unwrap();
        let (second_request_id, second_context) = match &second_actions[0] {
            ConsensusDriverAction::ValidateProposal {
                request_id,
                context,
                ..
            } => (*request_id, *context),
            other => panic!("unexpected action: {other:?}"),
        };

        state.active_snapshot_hash = Some(StakeSnapshotHash([6; 32]));
        let stale_snapshot = process_event(
            &mut state,
            ConsensusDriverEvent::ProposalValidationCompleted {
                request_id: first_request_id,
                context: first_context,
                result: ProposalValidationResult::Accepted(ValidatedCandidate {
                    proposal: first,
                    block: BlockV2::default(),
                    derived_randomness: Hash([4; 32]),
                }),
            },
        )
        .await
        .unwrap();
        assert!(stale_snapshot.is_empty());

        state.active_parent_block_hash = Some(Hash([5; 32]));
        let stale_parent = process_event(
            &mut state,
            ConsensusDriverEvent::ProposalValidationCompleted {
                request_id: second_request_id,
                context: second_context,
                result: ProposalValidationResult::Accepted(ValidatedCandidate {
                    proposal: second,
                    block: BlockV2::default(),
                    derived_randomness: Hash([4; 32]),
                }),
            },
        )
        .await
        .unwrap();
        assert!(stale_parent.is_empty());
    }

    #[tokio::test]
    async fn network_flood_cannot_consume_internal_queue_capacity() {
        let driver = ConsensusDriver::start(1).unwrap();
        let p = proposal(1, 0, 1);
        let _ = driver
            .dispatch(ConsensusDriverEvent::NetworkProposal {
                proposal: p,
                block: BlockV2::default(),
            })
            .await
            .unwrap();
        let token = TimeoutToken {
            height: 1,
            round: 0,
            step: TimeoutStep::Propose,
            generation: 1,
            deadline: tokio::time::Instant::now(),
        };
        let actions = driver
            .dispatch(ConsensusDriverEvent::Timeout(token))
            .await
            .unwrap();
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn timeout_lifecycle_replaces_timer_and_advances_generation() {
        let driver = ConsensusDriver::start(8).unwrap();
        let round_actions = driver
            .dispatch(ConsensusDriverEvent::RoundStarted {
                height: 1,
                round: 0,
                deadline: tokio::time::Instant::now(),
            })
            .await
            .unwrap();
        let first = round_actions
            .iter()
            .find_map(|action| match action {
                ConsensusDriverAction::ScheduleTimeout(token) => Some(*token),
                _ => None,
            })
            .expect("round start must schedule Propose timeout");
        assert_eq!(first.step, TimeoutStep::Propose);

        let timeout_actions = driver
            .dispatch(ConsensusDriverEvent::Timeout(first))
            .await
            .unwrap();
        assert!(timeout_actions
            .iter()
            .any(|action| matches!(action, ConsensusDriverAction::HandleTimeout(token) if *token == first)));
        driver
            .dispatch(ConsensusDriverEvent::TimeoutApplied(first))
            .await
            .unwrap();

        let second_actions = driver
            .dispatch(ConsensusDriverEvent::StepStarted {
                height: 1,
                round: 0,
                step: TimeoutStep::PrevoteWait,
                deadline: tokio::time::Instant::now(),
            })
            .await
            .unwrap();
        let second = second_actions
            .iter()
            .find_map(|action| match action {
                ConsensusDriverAction::ScheduleTimeout(token) => Some(*token),
                _ => None,
            })
            .expect("step start must schedule PrevoteWait timeout");
        assert_eq!(second.step, TimeoutStep::PrevoteWait);
        assert!(second.generation > first.generation);
        assert!(driver
            .dispatch(ConsensusDriverEvent::Timeout(first))
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn accepted_finality_cancels_old_timeout_before_executor_can_apply_it() {
        let mut state = DriverState::default();
        let round_actions = process_event(
            &mut state,
            ConsensusDriverEvent::RoundStarted {
                height: 1,
                round: 0,
                deadline: tokio::time::Instant::now(),
            },
        )
        .await
        .unwrap();
        let timeout = round_actions
            .iter()
            .find_map(|action| match action {
                ConsensusDriverAction::ScheduleTimeout(token) => Some(*token),
                _ => None,
            })
            .expect("round start must schedule a timeout");

        let commit_actions = process_event(
            &mut state,
            ConsensusDriverEvent::NetworkCommit(commit(1, 0, 7)),
        )
        .await
        .unwrap();
        let (request_id, context) = match &commit_actions[0] {
            ConsensusDriverAction::ValidateCommit {
                request_id,
                context,
                ..
            } => (*request_id, *context),
            other => panic!("unexpected action: {other:?}"),
        };

        let finality_actions = process_event(
            &mut state,
            ConsensusDriverEvent::CommitValidationCompleted {
                request_id,
                context,
                commit: commit(1, 0, 7),
                result: CommitValidationResult::Accepted,
            },
        )
        .await
        .unwrap();
        assert!(finality_actions.iter().any(|action| matches!(
            action,
            ConsensusDriverAction::CancelTimeout(token) if *token == timeout
        )));
        assert!(finality_actions.iter().any(|action| matches!(
            action,
            ConsensusDriverAction::PrepareFinality { request_id: id, .. } if *id == request_id
        )));

        let rejected_actions = process_event(
            &mut state,
            ConsensusDriverEvent::FinalityPreparationCompleted {
                request_id,
                context,
                result: FinalityPreparationResult::Rejected("candidate unavailable".into()),
            },
        )
        .await
        .unwrap();
        assert!(rejected_actions.iter().any(|action| matches!(
            action,
            ConsensusDriverAction::UnpinPendingFinality {
                height: 1,
                block_id,
                ..
            }
                if *block_id == BlockId(Hash([7; 32]))
        )));

        // A timeout already queued by the timer task must not be allowed to
        // race the finality preparation after the driver cancelled it.
        assert!(
            process_event(&mut state, ConsensusDriverEvent::Timeout(timeout))
                .await
                .unwrap()
                .is_empty()
        );
    }

    struct RecordingExecutor {
        actions: Arc<Mutex<Vec<ConsensusDriverAction>>>,
    }

    #[async_trait]
    impl ConsensusActionExecutor for RecordingExecutor {
        async fn execute(
            &self,
            action: ConsensusDriverAction,
            _handle: ConsensusDriverHandle,
        ) -> Result<()> {
            self.actions.lock().unwrap().push(action);
            Ok(())
        }
    }

    #[tokio::test]
    async fn driver_executes_actions_through_the_single_executor() {
        let actions = Arc::new(Mutex::new(Vec::new()));
        let driver = ConsensusDriver::start_with_executor(
            8,
            Arc::new(RecordingExecutor {
                actions: actions.clone(),
            }),
        )
        .unwrap();

        driver
            .dispatch(ConsensusDriverEvent::NetworkProposal {
                proposal: proposal(1, 0, 1),
                block: BlockV2::default(),
            })
            .await
            .unwrap();

        assert!(actions
            .lock()
            .unwrap()
            .iter()
            .any(|action| matches!(action, ConsensusDriverAction::ValidateProposal { .. })));
    }

    struct BlockingExecutor {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    struct FailingExecutor {
        failed: Arc<Notify>,
    }

    struct RetryingExecutor {
        attempts: Arc<AtomicUsize>,
        failures: usize,
    }

    #[async_trait]
    impl ConsensusActionExecutor for RetryingExecutor {
        async fn execute(
            &self,
            _action: ConsensusDriverAction,
            _handle: ConsensusDriverHandle,
        ) -> Result<()> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt < self.failures {
                return Err(anyhow!(RetryableConsensusActionError::new(
                    "injected transient broadcast failure"
                )));
            }
            Ok(())
        }
    }

    #[async_trait]
    impl ConsensusActionExecutor for FailingExecutor {
        async fn execute(
            &self,
            _action: ConsensusDriverAction,
            _handle: ConsensusDriverHandle,
        ) -> Result<()> {
            self.failed.notify_one();
            Err(anyhow!("injected action failure"))
        }
    }

    #[async_trait]
    impl ConsensusActionExecutor for BlockingExecutor {
        async fn execute(
            &self,
            _action: ConsensusDriverAction,
            _handle: ConsensusDriverHandle,
        ) -> Result<()> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(())
        }
    }

    fn vote(height: u64, id: u8) -> SignedVote {
        SignedVote {
            protocol_version: Default::default(),
            chain_id: Default::default(),
            epoch: 0,
            height,
            round: 0,
            step: norn_common::consensus_types::VoteStep::Prevote,
            block_id: Some(BlockId(Hash([id; 32]))),
            stake_snapshot_hash: StakeSnapshotHash([8; 32]),
            validator: norn_common::types::ValidatorId([id; 32]),
            signature: [3; 64],
        }
    }

    #[tokio::test]
    async fn high_priority_queue_isolated_from_a_full_network_queue() {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let driver = ConsensusDriver::start_with_executor(
            1,
            Arc::new(BlockingExecutor {
                started: started.clone(),
                release: release.clone(),
            }),
        )
        .unwrap();
        let handle = driver.handle();

        handle
            .submit(ConsensusDriverEvent::NetworkProposal {
                proposal: proposal(1, 0, 1),
                block: BlockV2::default(),
            })
            .await
            .unwrap();
        started.notified().await;

        handle
            .submit(ConsensusDriverEvent::NetworkVote(vote(1, 2)))
            .await
            .unwrap();
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(25),
            handle.submit(ConsensusDriverEvent::NetworkVote(vote(1, 3)))
        )
        .await
        .is_err());

        tokio::time::timeout(
            std::time::Duration::from_millis(25),
            handle.submit(ConsensusDriverEvent::VotePersisted(vote(1, 4))),
        )
        .await
        .unwrap()
        .unwrap();
        release.notify_waiters();
    }

    #[tokio::test]
    async fn internal_action_failure_stops_driver_instead_of_being_dropped() {
        let failed = Arc::new(Notify::new());
        let driver = ConsensusDriver::start_with_executor(
            8,
            Arc::new(FailingExecutor {
                failed: failed.clone(),
            }),
        )
        .unwrap();
        let notification = failed.notified();
        driver
            .handle()
            .submit(ConsensusDriverEvent::NetworkProposal {
                proposal: proposal(1, 0, 1),
                block: BlockV2::default(),
            })
            .await
            .unwrap();
        notification.await;
        tokio::task::yield_now().await;

        let stopped = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            driver.dispatch(ConsensusDriverEvent::Shutdown),
        )
        .await
        .expect("driver shutdown request must resolve after fail-stop");
        assert!(stopped.is_err());
    }

    #[tokio::test]
    async fn retryable_action_failure_is_retried_then_succeeds() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let driver = ConsensusDriver::start_with_executor(
            8,
            Arc::new(RetryingExecutor {
                attempts: attempts.clone(),
                failures: 2,
            }),
        )
        .unwrap();
        driver
            .handle()
            .submit(ConsensusDriverEvent::NetworkProposal {
                proposal: proposal(1, 0, 1),
                block: BlockV2::default(),
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert!(driver
            .dispatch(ConsensusDriverEvent::Shutdown)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn retryable_action_failure_is_bounded_and_stops_driver() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let driver = ConsensusDriver::start_with_executor(
            8,
            Arc::new(RetryingExecutor {
                attempts: attempts.clone(),
                failures: usize::MAX,
            }),
        )
        .unwrap();
        driver
            .handle()
            .submit(ConsensusDriverEvent::NetworkProposal {
                proposal: proposal(1, 0, 1),
                block: BlockV2::default(),
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(attempts.load(Ordering::SeqCst), 4);
        assert!(driver
            .dispatch(ConsensusDriverEvent::Shutdown)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn retry_backoff_does_not_block_following_high_priority_events() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let driver = ConsensusDriver::start_with_executor(
            8,
            Arc::new(RetryingExecutor {
                attempts,
                failures: 8,
            }),
        )
        .unwrap();
        let started = tokio::time::Instant::now();
        driver
            .dispatch(ConsensusDriverEvent::NetworkProposal {
                proposal: proposal(1, 0, 1),
                block: BlockV2::default(),
            })
            .await
            .unwrap();
        assert!(started.elapsed() < Duration::from_millis(100));

        let actions = driver
            .dispatch(ConsensusDriverEvent::RoundStarted {
                height: 1,
                round: 0,
                deadline: tokio::time::Instant::now(),
            })
            .await
            .unwrap();
        assert!(actions.iter().any(|action| matches!(
            action,
            ConsensusDriverAction::ScheduleTimeout(TimeoutToken {
                step: TimeoutStep::Propose,
                ..
            })
        )));
    }

    #[tokio::test]
    async fn activation_failure_stops_the_driver() {
        let driver = ConsensusDriver::start(8).unwrap();
        let result = driver
            .dispatch(ConsensusDriverEvent::ActivationFailed("boom".into()))
            .await;
        assert!(result.is_err());

        let stopped = driver.dispatch(ConsensusDriverEvent::Shutdown).await;
        assert!(stopped.is_err());
    }
}
