//! Single-writer consensus event queue.
//!
//! Network callbacks and timeout tasks submit events here. The queue is the
//! only place that turns an event into a consensus action, so stale timers and
//! concurrent callback order cannot directly mutate consensus state.

use anyhow::{anyhow, Result};
use norn_common::consensus_types::{CommitCertificate, Proposal, SignedVote, VoteStep};
use norn_common::types::{BlockId, BlockV2};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutToken {
    pub height: u64,
    pub round: u32,
    pub step: VoteStep,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsensusDriverEvent {
    NetworkProposal { proposal: Proposal, block: BlockV2 },
    NetworkVote(SignedVote),
    NetworkCommit(CommitCertificate),
    LocalProposalReady { height: u64, block_id: BlockId },
    Timeout(TimeoutToken),
    VoteDurablySigned(SignedVote),
    VoteBroadcastResult { vote: SignedVote, accepted: bool },
    FinalityDurable(CommitCertificate),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsensusDriverAction {
    ValidateProposal,
    ValidateVote(SignedVote),
    ValidateCommit(CommitCertificate),
    BroadcastVote(SignedVote),
    ScheduleTimeout(TimeoutToken),
    CancelTimeout(TimeoutToken),
    BuildLocalProposal { height: u64, block_id: BlockId },
    PersistFinality(CommitCertificate),
    BroadcastCommit(CommitCertificate),
}

#[derive(Default)]
struct DriverState {
    active_timeout: Option<TimeoutToken>,
    generation: u64,
}

struct DriverRequest {
    event: ConsensusDriverEvent,
    reply: oneshot::Sender<Result<Vec<ConsensusDriverAction>>>,
}

#[derive(Clone)]
pub struct ConsensusDriver {
    tx: mpsc::Sender<DriverRequest>,
}

impl ConsensusDriver {
    pub fn start(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(anyhow!("consensus driver queue capacity must be non-zero"));
        }
        let (tx, mut rx) = mpsc::channel::<DriverRequest>(capacity);
        let state = Arc::new(Mutex::new(DriverState::default()));
        let worker_state = state.clone();
        tokio::spawn(async move {
            while let Some(request) = rx.recv().await {
                let result = process_event(&worker_state, request.event).await;
                let _ = request.reply.send(result);
            }
        });
        Ok(Self { tx })
    }

    pub async fn dispatch(
        &self,
        event: ConsensusDriverEvent,
    ) -> Result<Vec<ConsensusDriverAction>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(DriverRequest {
                event,
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow!("consensus driver queue is closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow!("consensus driver worker stopped"))?
    }
}

async fn process_event(
    state: &Mutex<DriverState>,
    event: ConsensusDriverEvent,
) -> Result<Vec<ConsensusDriverAction>> {
    let mut state = state.lock().await;
    let actions = match event {
        ConsensusDriverEvent::NetworkProposal { .. } => {
            vec![ConsensusDriverAction::ValidateProposal]
        }
        ConsensusDriverEvent::NetworkVote(vote) => {
            vec![ConsensusDriverAction::ValidateVote(vote)]
        }
        ConsensusDriverEvent::NetworkCommit(commit) => {
            vec![ConsensusDriverAction::ValidateCommit(commit)]
        }
        ConsensusDriverEvent::LocalProposalReady { height, block_id } => {
            state.generation = state.generation.saturating_add(1);
            vec![
                ConsensusDriverAction::BuildLocalProposal { height, block_id },
                ConsensusDriverAction::ScheduleTimeout(TimeoutToken {
                    height,
                    round: 0,
                    step: VoteStep::Prevote,
                    generation: state.generation,
                }),
            ]
        }
        ConsensusDriverEvent::Timeout(token) => {
            if state.active_timeout != Some(token) {
                return Ok(Vec::new());
            }
            vec![ConsensusDriverAction::ScheduleTimeout(token)]
        }
        ConsensusDriverEvent::VoteDurablySigned(vote) => {
            vec![ConsensusDriverAction::BroadcastVote(vote)]
        }
        ConsensusDriverEvent::VoteBroadcastResult { vote, accepted } => {
            if accepted {
                vec![ConsensusDriverAction::CancelTimeout(TimeoutToken {
                    height: vote.height,
                    round: vote.round,
                    step: vote.step,
                    generation: state.generation,
                })]
            } else {
                vec![ConsensusDriverAction::BroadcastVote(vote)]
            }
        }
        ConsensusDriverEvent::FinalityDurable(commit) => {
            vec![ConsensusDriverAction::BroadcastCommit(commit)]
        }
    };
    for action in &actions {
        match action {
            ConsensusDriverAction::ScheduleTimeout(token) => state.active_timeout = Some(*token),
            ConsensusDriverAction::CancelTimeout(token) if state.active_timeout == Some(*token) => {
                state.active_timeout = None
            }
            _ => {}
        }
    }
    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_timeout_tokens_are_ignored_after_a_new_proposal() {
        let driver = ConsensusDriver::start(8).unwrap();
        let first = driver
            .dispatch(ConsensusDriverEvent::LocalProposalReady {
                height: 1,
                block_id: BlockId(norn_common::types::Hash([1; 32])),
            })
            .await
            .unwrap();
        let first_token = first
            .iter()
            .find_map(|action| match action {
                ConsensusDriverAction::ScheduleTimeout(token) => Some(*token),
                _ => None,
            })
            .unwrap();

        let second = driver
            .dispatch(ConsensusDriverEvent::LocalProposalReady {
                height: 1,
                block_id: BlockId(norn_common::types::Hash([2; 32])),
            })
            .await
            .unwrap();
        let second_token = second
            .iter()
            .find_map(|action| match action {
                ConsensusDriverAction::ScheduleTimeout(token) => Some(*token),
                _ => None,
            })
            .unwrap();
        assert_ne!(first_token.generation, second_token.generation);

        let stale = driver
            .dispatch(ConsensusDriverEvent::Timeout(first_token))
            .await
            .unwrap();
        assert!(stale.is_empty());

        let current = driver
            .dispatch(ConsensusDriverEvent::Timeout(second_token))
            .await
            .unwrap();
        assert_eq!(
            current,
            vec![ConsensusDriverAction::ScheduleTimeout(second_token)]
        );
    }
}
