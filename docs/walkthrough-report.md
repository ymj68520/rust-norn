# Rust-Norn V2 Safety Hardening Walkthrough

## Scope

This report records the published V2 follow-up on top of commit `e3c4745`,
including `fe692408fe29e84261bca5b1c2bb4a35a2de434a` and the published
integration commit `1c35717` (parent `78fabac`). It also records the current
uncommitted next-round hardening in the worktree. The project remains a
Candidate prototype; this report does not declare production readiness.

## Implemented in this follow-up

### Candidate retention and bounded cache behavior

`V2CandidateCache` now tracks explicit retention state:

- `Normal` candidates are subject to TTL and capacity eviction.
- `valid_block`, `locked_block`, and pending-finality candidates are pinned.
- Retention is reconciled with the consensus state and finalized heights are cleaned up.
- A required pinned candidate that cannot be retained causes a fail-closed error instead of silent eviction.
- The producer reconciles retention before using a valid-round candidate.

### SignedVote and ConsensusAction retry policy

The driver now retries only explicitly typed, idempotent action failures with a finite exponential backoff. Commit broadcast uses three retries; SignedVote broadcast uses eight retries for temporary backpressure. Closed channels and serialization failures are terminal. The old independent `VoteBroadcastResult` loop was removed, so a failed vote broadcast cannot retry forever or create a replacement signature.

### State-machine and recovery hardening

- A late block Polka after a durable local NIL precommit does not create a second precommit.
- Timeout handling rejects tokens for the wrong consensus step.
- Stale V2 proposal work for an older height/round is discarded rather than fail-stopping the active height.
- Durable finality requests the next finality record after local advancement, improving recovery after missed Commit gossip.
- FullNode may originate the four V2 synchronization request/response payloads, but not proposal, vote, or certificate broadcasts.
- FullNode synchronization and recovery paths now have explicit response/request logging.

### Published P0/P1 follow-up (`1c35717`)

The uncommitted follow-up addresses the latest review findings:

- Finality preparation records the candidate height and block ID, cancels the
  active timeout before dispatching preparation, and releases pins for stale,
  mismatched, replaced, or finalized preparation requests. Timeout generation
  is advanced only after the executor confirms that the token matched the live
  state-machine step, so an old timeout cannot invalidate a legal finality
  preparation.
- The safety WAL now persists the Tendermint round/step, lock, valid-round
  block, and valid-round certificate. A companion framed state log is recovered
  with torn-tail truncation, and startup validates the recovered context before
  restoring it. Vote intent, signature completion, and the safety state carried
  by that vote remain fail-closed across restart.
- Retry backoff is scheduled as a delayed driver event rather than a sleep in
  the single-writer actor. High-priority timeout and consensus events therefore
  remain processable while an idempotent broadcast is waiting for its bounded
  retry.

### Current V5 worktree changes

- `BlockHeader.block_builder` is now the stable original block builder, while
  `Proposal.proposer` remains the current-round proposer. A valid-round
  re-proposal by validator B can therefore carry the exact block built by A;
  the block ID and header commitments remain unchanged.
- Fresh proposals require `block_builder == proposer` and equal proposal/header
  rounds. Re-proposals require a certificate with `valid_round < proposal.round`,
  matching certificate round/block ID, and header round equal to `valid_round`.
  Equal, greater, missing, and mismatched valid-round inputs fail closed.
- Complete validated candidates (Proposal, BlockV2, and derived randomness) are
  persisted in FinalityStore. Immutable block records and round-specific
  proposal attempts are separate, keyed respectively by `(height, block_id)`
  and `(height, round, block_id)`. Finalization retrieves the exact certificate
  round instead of using the latest cached Proposal. Recovery restores every
  indexed attempt before production resumes; record versions and checksums
  reject torn or unknown durable material.
- `FinalizedConsensusState::from_v2` now accepts the Proposal explicitly:
  fresh finality requires equal Proposal/header rounds, while valid-round
  re-proposal finality requires `commit.round == proposal.round` and
  `block.header.round == proposal.valid_round`. The old invalid requirement
  `commit.round == block.header.round` is gone.
- Persistent SafetyStore sequence allocation is serialized across vote and
  companion-state WAL transactions; a concurrent regression test verifies
  monotonic recovery without sequence reuse.
- Protocol activation is explicit: the active network uses Genesis schema 5,
  protocol version 5, wire version 5, `NORN_BLOCK_HEADER_V5`,
  `NORN_GENESIS_V5`, V5 transaction IDs, and V5 consensus signing domains.
  Legacy V2/V3/V4 Genesis/configuration is rejected; there is no mixed-height
  deserialization fallback.
- The V5 activation carries immutable `BlockConsensusData`
  with the original builder's VRF preout/proof and builder round. Its
  `consensus_data_hash` commits this material with the execution commitment,
  and finality derives `next_randomness` with `NORN_CHAIN_RANDOMNESS_V5` from
  the parent seed, one builder VRF output, builder context, and snapshot. The
  formula excludes `block_id`, so block-content grinding cannot search future
  seeds. A single builder may still selectively abort after seeing an
  unfavorable VRF; this is not claimed to be unbiased beacon randomness.
- Historical proposal attempts may leave memory after their exact durable
  `(height, round, block_id)` record is retained. Exact Commit lookup reloads
  and revalidates the attempt; required locked/valid immutable block bodies
  remain available. Durable block-index read-modify-write is serialized with
  finality cleanup. A separate durable per-height attempt index counts
  competing block IDs as well as rounds, so attempts cannot bypass the limit
  by changing block identity. Genesis now bounds attempts per height, durable
  attempt bytes, and the maximum consensus round; exceeding a bound fails
  closed.
- Canonical finalized state contains block/state/timestamp/randomness/snapshot
  fields but no certificate hash. Finality evidence remains in the durable
  Proposal/Commit record, and an equivalent certificate at another round is
  independently verified without rewriting canonical state.
- V5 block timestamps are non-negative, strictly greater than the canonical
  parent, and no more than the Genesis `max_block_timestamp_step`; local time
  is used only when proposing.

## Verification

Passed:

```text
cargo fmt --all -- --check
git diff --check
cargo test -p norn-core --test four_node_bft_test --locked
cargo test --workspace --locked -j 1
```

The current V5 changes additionally pass the focused wire-validation,
candidate-cache, exact-round finality, durable-candidate, Genesis activation,
concurrent SafetyStore, V5 randomness, V5 timestamp, and durable-attempt-bound
regressions. `cargo check --workspace --all-features --locked` and
`cargo test --workspace --all-features --locked -j 1` also pass. The focused
common/core run includes 265 `norn-core` unit tests and 43 `norn-common` tests.

`cargo audit --no-fetch --no-yanked` remains a failing security gate: it reports two `hickory-proto 0.25.2` vulnerabilities and 13 allowed maintenance/unsoundness warnings. One Hickory advisory has no fixed upgrade; the other requires `hickory-proto >=0.26.1`. Clippy with `-D warnings` also remains open because of existing warnings outside this follow-up.

## Remaining work

The following items remain outside this follow-up and must be completed before any production claim:

1. Add the canonical on-chain governance transaction/admission path for validator changes, jailing, slashing, and key rotation.
2. Derive durable network authorization from the finalized validator snapshot, including epoch changes and key rotation.
3. Replace the current plaintext-oriented NodeKeyStore with authenticated, permission-checked production storage and an HSM/TPM integration boundary.
4. Remove or explicitly isolate legacy VDF and `BlockBuffer` paths behind a legacy-only feature.
5. Close the audit and clippy gates, and add Byzantine, crash/I/O, disk-full, restart, multiprocess fault, fuzz, model-checking, and soak coverage.

Until these items are independently reviewed and accepted, the status remains:

```text
Candidate prototype
Not production-ready
Published follow-up implementation commits: `fe692408fe29e84261bca5b1c2bb4a35a2de434a`, `1c35717`
Current V5 hardening: uncommitted on `6e6edc0`
```
