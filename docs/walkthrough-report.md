# Rust-Norn V2 Safety Hardening Walkthrough

## Scope

This report records the published V2 follow-up on top of commit `e3c4745`, committed as `fe692408fe29e84261bca5b1c2bb4a35a2de434a`, plus the current uncommitted P0/P1 hardening work on `78fabac`. The project remains a Candidate prototype; this report does not declare production readiness.

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

### Current P0/P1 follow-up

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
- Valid-round re-proposals now preserve the exact previously accepted block:
  the proposal may advance to a later consensus round while the block header
  retains its original valid-round, which is part of the block ID. Wire,
  finalized-record, and pending-proposal admission all enforce this relation
  and reject impossible round combinations.

## Verification

Passed:

```text
cargo fmt --all -- --check
git diff --check
cargo test -p norn-core --test four_node_bft_test --locked
cargo test --workspace --locked -j 1
```

The current uncommitted changes additionally pass the focused driver,
SafetyStore, state-machine, and wire-validation suites, the finality-timeout
race regression, the standalone four-node BFT test, and the four-process
Validator/FullNode recovery test, including proposer restart and valid-round
re-proposal.

The workspace run passed. It included the four-process Validator/FullNode BFT recovery test, 258 `norn-core` unit tests, 40 `norn-common` tests, the standalone four-node BFT test, all crate tests, and doc tests.

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
Published follow-up implementation commit: `fe692408fe29e84261bca5b1c2bb4a35a2de434a`
Current P0/P1 follow-up: uncommitted on `78fabac`
```
