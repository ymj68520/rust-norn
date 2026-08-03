# Candidate verification gate

The current implementation remains **Candidate**. It must not be described as
production-ready until every gate below is reproducible on a clean workspace.

## Required commands

```text
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked -j 1
cargo test -p norn-network --lib service::tests::stage7_authenticates_valid_peers_and_rejects_wrong_role_and_context -- --nocapture
cargo test -p norn-network --test stage7_process_test -- --nocapture
cargo test -p norn-core --lib finality -- --nocapture
cargo test -p norn-core --lib consensus::driver --locked -- --nocapture
cargo test -p norn-core --test four_node_bft_test -- --nocapture
cargo test -p norn --test four_process_v2_bft --locked -- --nocapture
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo audit --no-fetch --no-yanked
git diff --check
```

The consensus wire decoder also has a deterministic mutation corpus in
`norn-common::consensus_types::tests::consensus_wire_fuzz_corpus_is_panic_free`.
It flips every bit of a valid envelope and exercises 2,048 bounded
pseudo-random byte inputs. Any panic, unbounded allocation, or acceptance of
an invalid context is a gate failure. A CI job may additionally run a native
libFuzzer target against the same `decode_and_validate` entry point.

## Safety and recovery gates

- malformed, oversized, wrong-version, wrong-Genesis, and wrong-role network
  inputs fail closed before consensus processing;
- a Validator handshake is accepted only after a fresh challenge response
  verifies a Genesis consensus key, low-S signature, nonce, and bound PeerId;
- `propagation_source` is never treated as the original consensus publisher:
  context-valid relayed messages reach the V2 cryptographic validator, while a
  FullNode may originate only `BlockRequest` and `FinalityRequest` messages;
- the transport rejects new peer identities after the protocol connection
  ceiling is reached, and bounded ingress queues may drop gossip without
  blocking the network event loop;
- Validator and FullNode behavior is tested separately;
- a signer/WAL failure produces no broadcast and no state transition;
- a torn safety-WAL tail is recoverable;
- finality commit retry is idempotent after an apply/flush ambiguity;
- a conflicting block at an already finalized height is rejected;
- the finalized record, pending validator changes, and next validator snapshot
  are recovered as one protocol state; a missing next snapshot fails closed
  instead of being derived from process memory;
- restart recovery replays the exact durable vote, certificate, and overlay
  write-set;
- a worker completion that arrives after a real timeout/round change is
  discarded even if the request entry was not cleared by the test harness;
- worker completions are rechecked against generation, height, round, snapshot,
  and parent context before application;
- an internal Action error without a oneshot reply is observable and puts the
  driver into fail-stop instead of being silently dropped;
- only the idempotent `BroadcastCommit` enqueue path may return a typed
  retryable Action error; retry is bounded to three attempts with exponential
  backoff, while unknown errors and retry exhaustion fail-stop;
- V2 candidate proposal, block, and derived randomness are admitted as one
  bounded cache entry; byte, per-height, per-proposer, future-height,
  future-round, and TTL limits are enforced from protocol parameters or a
  versioned protocol invariant;
- a conflicting candidate for the same `(height, block_id)` is rejected,
  identical replay is idempotent, and candidates through finalized height are
  removed;
- four independent configurations derive the same snapshot hash and proposer
  sequence;
- the full workspace test suite is green.

These checks are completion gates, not evidence that an unreviewed deployment
is production-ready. Protocol upgrades continue to require a new versioned
Genesis or an explicitly specified activation-height migration.

The validator-change queue and epoch snapshot transition are deterministic and
durable, but they are not by themselves a governance transaction format. The
V2 transaction/system-action admission path must explicitly decode, authorize,
and queue `ValidatorChange` records before this feature can be considered live
chain governance.
