//! PoVF Orchestration Engine wrapping Tendermint BFT State Machine

use super::safety_store::{ConsensusSafetyStore, ConsensusSigner};
use super::state_machine::TendermintStateMachine;
use super::types::ConsensusConfig;
use norn_common::consensus_types::{
    CommitCertificate, FinalizedBlock, Proposal, SignedVote, StakeSnapshot,
};
use norn_common::error::{NornError, Result};
use norn_common::types::{Block, Hash, ValidatorId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    Proposal(Proposal),
    Vote(SignedVote),
}

pub struct PoVFEngine {
    pub state_machine: Arc<RwLock<TendermintStateMachine>>,
    pub finalized_blocks: Arc<RwLock<HashMap<Hash, FinalizedBlock>>>,
    pub current_height: Arc<RwLock<u64>>,
}

impl PoVFEngine {
    pub fn new(
        config: ConsensusConfig,
        snapshot: StakeSnapshot,
        safety_store: Arc<dyn ConsensusSafetyStore>,
        local_validator_id: Option<ValidatorId>,
    ) -> Self {
        let state_machine = TendermintStateMachine::new(config, snapshot, safety_store, local_validator_id);
        Self {
            state_machine: Arc::new(RwLock::new(state_machine)),
            finalized_blocks: Arc::new(RwLock::new(HashMap::new())),
            current_height: Arc::new(RwLock::new(1)),
        }
    }

    pub async fn handle_proposal(
        &self,
        proposal: Proposal,
        block: Block,
        signer: &dyn ConsensusSigner,
    ) -> Result<Option<SignedVote>> {
        let mut sm = self.state_machine.write().await;
        let vote = sm
            .handle_proposal(&proposal, &block, signer)
            .map_err(|e| NornError::ConsensusError(e.to_string()))?;
        Ok(Some(vote))
    }

    pub async fn handle_vote(
        &self,
        vote: SignedVote,
        signer: &dyn ConsensusSigner,
    ) -> Result<Option<FinalizedBlock>> {
        let mut sm = self.state_machine.write().await;
        if let Some(commit_cert) = sm
            .handle_vote(vote, signer)
            .map_err(|e| NornError::ConsensusError(e.to_string()))?
        {
            info!(
                "BFT Consensus reached CommitCertificate for height {} round {}",
                commit_cert.height, commit_cert.round
            );
            drop(sm);
            return Ok(None);
        }
        Ok(None)
    }

    pub async fn finalize_block(&self, block: Block, commit: CommitCertificate) -> Result<()> {
        let hash = block.header.block_hash;
        info!("Finalizing block {:?} at height {}", hash, block.header.height);

        let finalized = FinalizedBlock {
            block,
            commit,
        };

        {
            let mut fb = self.finalized_blocks.write().await;
            fb.insert(hash, finalized);
        }

        {
            let mut h = self.current_height.write().await;
            *h = *h + 1;
        }

        Ok(())
    }

    pub async fn get_current_height(&self) -> u64 {
        *self.current_height.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::safety_store::MemorySafetyStore;

    struct DummySigner;
    impl ConsensusSigner for DummySigner {
        fn sign_canonical_bytes(&self, _bytes: &[u8]) -> anyhow::Result<[u8; 64]> {
            Ok([1u8; 64])
        }
    }

    #[tokio::test]
    async fn test_povf_engine_creation() {
        let config = ConsensusConfig::default();
        let snapshot = StakeSnapshot::default();
        let safety_store = Arc::new(MemorySafetyStore::new());

        let engine = PoVFEngine::new(config, snapshot, safety_store, None);
        assert_eq!(engine.get_current_height().await, 1);
    }
}
