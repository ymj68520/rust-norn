//! 4-Node BFT Integration Test Suite
//! Validates 4-node real BFT consensus, VRF selection, quorum aggregation, CommitCertificate verification, and single-tree persistence.

use k256::ecdsa::signature::Signer;
use k256::ecdsa::SigningKey;
use rand::thread_rng;
use std::sync::Arc;
use tempfile::TempDir;

use norn_common::consensus_types::{Proposal, StakeSnapshot, ValidatorRecord, VoteStep};
use norn_common::traits::DBInterface;
use norn_common::types::{
    Block, BlockHeader, BlockId, ChainId, ConsensusPublicKey, Hash, ProtocolVersion, ValidatorId,
    VrfPublicKey,
};
use norn_core::consensus::povf::PoVFEngine;
use norn_core::consensus::safety_store::{ConsensusSigner, PersistentSafetyStore};
use norn_core::consensus::types::{ConsensusConfig, ElectionMath};
use norn_crypto::vrf::{VRFCalculator, VRFKeyPair, VrfContext};
use norn_storage::sled::SledDB;

struct EcdsaSigner {
    validator_id: ValidatorId,
    key: SigningKey,
}

impl ConsensusSigner for EcdsaSigner {
    fn sign_canonical_bytes(&self, canonical_bytes: &[u8]) -> anyhow::Result<[u8; 64]> {
        let sig: k256::ecdsa::Signature = self.key.sign(canonical_bytes);
        let sig_norm = sig.normalize_s().unwrap_or(sig);
        let mut arr = [0u8; 64];
        arr.copy_from_slice(sig_norm.to_bytes().as_slice());
        Ok(arr)
    }
}

impl EcdsaSigner {
    fn sign_proposal(&self, canonical_bytes: &[u8]) -> anyhow::Result<[u8; 64]> {
        self.sign_canonical_bytes(canonical_bytes)
    }
}

#[tokio::test]
async fn test_four_node_bft_consensus_and_finality() {
    let protocol_version = ProtocolVersion(2);
    let chain_id = ChainId(Hash([1u8; 32]));

    // 1. Generate 4 validator keypairs (ECDSA consensus key + Schnorrkel VRF key)
    let mut signers = Vec::new();
    let mut vrf_keys = Vec::new();
    let mut validator_records = Vec::new();

    for i in 1..=4 {
        let mut val_id_bytes = [0u8; 32];
        val_id_bytes[0] = i;
        let validator_id = ValidatorId(val_id_bytes);

        let ecdsa_key = SigningKey::random(&mut thread_rng());
        let ecdsa_pub_bytes: [u8; 33] = ecdsa_key
            .verifying_key()
            .to_sec1_bytes()
            .as_ref()
            .try_into()
            .unwrap();
        let consensus_pub_key = ConsensusPublicKey(ecdsa_pub_bytes);

        let vrf_keypair = VRFKeyPair::generate();
        let vrf_pub_bytes = vrf_keypair.public_key_bytes();
        let vrf_public_key = VrfPublicKey(vrf_pub_bytes);

        signers.push(EcdsaSigner {
            validator_id,
            key: ecdsa_key,
        });
        vrf_keys.push(vrf_keypair);
        validator_records.push(ValidatorRecord {
            validator_id,
            consensus_public_key: consensus_pub_key,
            vrf_public_key,
            voting_power: 10,
            jailed_until_epoch: None,
            slashed: false,
        });
    }

    // 2. Form initial StakeSnapshot (30/40 voting power > 2/3 required for BFT quorum)
    let snapshot =
        StakeSnapshot::from_genesis(1, validator_records.clone()).expect("Valid snapshot");
    assert_eq!(snapshot.validators.len(), 4);

    // 3. Determine deterministic proposer for height 1 round 0
    let parent_randomness = Hash([0x42; 32]);
    let expected_proposer = ElectionMath::select_deterministic_proposer(
        &chain_id,
        1,
        1,
        0,
        &parent_randomness,
        &snapshot,
    )
    .expect("Proposer selected");

    let proposer_index = signers
        .iter()
        .position(|s| s.validator_id == expected_proposer)
        .expect("Proposer found");
    let proposer_signer = &signers[proposer_index];
    let proposer_vrf_key = &vrf_keys[proposer_index];

    // 4. Proposer builds Block Header and Proposal
    let prev_hash = Hash::default();
    let header = BlockHeader {
        protocol_version: protocol_version.clone(),
        chain_id: chain_id.clone(),
        height: 1,
        epoch: 1,
        round: 0,
        timestamp: 1700000000,
        prev_block_hash: prev_hash,
        block_hash: Hash::default(),
        merkle_root: Hash::default(),
        state_root: Hash::default(),
        block_builder: expected_proposer,
        stake_snapshot_hash: snapshot.snapshot_hash.clone(),
        parent_randomness,
        gas_limit: 10000000,
        base_fee: 1000000000,
        consensus_data_hash: Hash::default(),
    };

    let mut block = Block {
        header,
        transactions: vec![],
    };
    block.header.block_hash = block.header.calculate_hash().expect("Calculate block hash");
    let block_id = BlockId(block.header.block_hash);

    let vrf_context = VrfContext {
        protocol_version: protocol_version.clone(),
        chain_id: chain_id.clone(),
        epoch: 1,
        height: 1,
        round: 0,
        parent_block_hash: prev_hash,
        stake_snapshot_hash: snapshot.snapshot_hash.clone(),
        validator_id: expected_proposer,
    };

    let vrf_output =
        VRFCalculator::calculate_with_context(proposer_vrf_key, &vrf_context).expect("VRF output");

    let mut proposal = Proposal {
        protocol_version: protocol_version.clone(),
        chain_id: chain_id.clone(),
        epoch: 1,
        height: 1,
        round: 0,
        valid_round: None,
        valid_round_certificate: None,
        block_id,
        parent_block_hash: prev_hash,
        stake_snapshot_hash: snapshot.snapshot_hash.clone(),
        proposer: expected_proposer,
        vrf_preout: vrf_output.preout.0,
        vrf_proof: vrf_output.proof.0,
        signature: [0u8; 64],
    };

    let proposal_bytes = proposal.canonical_bytes();
    proposal.signature = proposer_signer
        .sign_proposal(&proposal_bytes)
        .expect("Sign proposal");

    // 5. Instantiate 4 PoVF Engines with isolated single-tree Sled DBs
    let mut engines = Vec::new();
    let mut temp_dirs = Vec::new();

    for i in 0..4 {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("sled.db");
        let sled_db = SledDB::new(&db_path).unwrap();

        let safety_path = temp_dir.path().join("safety.log");
        let safety_store = Arc::new(PersistentSafetyStore::open(&safety_path).unwrap());

        let consensus_config = ConsensusConfig {
            protocol_version: protocol_version.clone(),
            chain_id: chain_id.clone(),
            epoch: 1,
            epoch_length: 100,
            validator_update_delay: 1,
            unbonding_delay: 1,
            key_rotation_delay: 1,
            slashing_activation_delay: 1,
            timeout_propose_ms: 3000,
            timeout_prevote_ms: 2000,
            timeout_precommit_ms: 2000,
            target_numerator: 1,
            target_denominator: 3,
            max_certificate_members: 4,
            max_future_height: 2,
            max_future_round: 2,
        };

        let engine = PoVFEngine::new_with_parent_randomness(
            consensus_config,
            snapshot.clone(),
            parent_randomness,
            safety_store,
            Some(signers[i].validator_id),
        );

        engines.push((engine, sled_db));
        temp_dirs.push(temp_dir);
    }

    // 6. Broadcast Proposal -> Nodes receive proposal and cast Prevotes
    let mut prevotes = Vec::new();
    for i in 0..4 {
        let (engine, _) = &engines[i];
        let vote_opt = engine
            .handle_proposal(proposal.clone(), block.clone(), &signers[i])
            .await
            .expect("Handle proposal");
        let vote = vote_opt.expect("Cast prevote");
        assert_eq!(vote.step, VoteStep::Prevote);
        assert_eq!(vote.block_id, Some(block_id));
        prevotes.push(vote);
    }

    // 7. Process Prevotes -> Nodes reach Polka (2/3 quorum) and cast Precommits
    let mut precommits = Vec::new();
    let mut commit_certs = Vec::new();

    for i in 0..4 {
        let (engine, _) = &engines[i];
        for prevote in &prevotes {
            let (vote_opt, cert_opt) = engine
                .handle_vote(prevote.clone(), &signers[i])
                .await
                .expect("Handle prevote");

            if let Some(precommit) = vote_opt {
                assert_eq!(precommit.step, VoteStep::Precommit);
                assert_eq!(precommit.block_id, Some(block_id));
                precommits.push(precommit);
            }
            if let Some(cert) = cert_opt {
                commit_certs.push(cert);
            }
        }
    }

    // Deduplicate precommits
    precommits.sort_by_key(|v| v.validator.0);
    precommits.dedup_by_key(|v| v.validator.0);
    assert_eq!(precommits.len(), 4);

    // 8. Process Precommits -> Nodes achieve > 2/3 Precommit quorum and generate CommitCertificate
    for i in 0..4 {
        let (engine, _) = &engines[i];
        for precommit in &precommits {
            let (_, cert_opt) = engine
                .handle_vote(precommit.clone(), &signers[i])
                .await
                .expect("Handle precommit");

            if let Some(cert) = cert_opt {
                commit_certs.push(cert);
            }
        }
    }

    assert!(!commit_certs.is_empty());
    let commit_cert = commit_certs.remove(0);

    // 9. Verify CommitCertificate & single-tree Sled persistence
    for (engine, sled_db) in &engines {
        engine
            .verify_commit_certificate(&block, &commit_cert, &snapshot)
            .expect("Verify CommitCertificate");

        let key = format!("block/{}", hex::encode(block.header.block_hash.0)).into_bytes();
        let val = bincode::serialize(&block).unwrap();
        sled_db
            .insert(&key, &val)
            .await
            .expect("Atomic insert block");

        let fetched = sled_db.get(&key).await.unwrap().expect("Fetched block");
        let fetched_block: Block = bincode::deserialize(&fetched).unwrap();
        assert_eq!(fetched_block.header.block_hash, block.header.block_hash);
    }
}
