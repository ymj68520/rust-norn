# Candidate verification gate

The current implementation remains **Candidate**. It must not be described as
production-ready until every gate below is reproducible on a clean workspace.

## Required commands

```text
cargo fmt --all -- --check
cargo test --workspace -j 1
cargo test -p norn-network --lib service::tests::stage7_authenticates_valid_peers_and_rejects_wrong_role_and_context -- --nocapture
cargo test -p norn-network --test stage7_process_test -- --nocapture
cargo test -p norn-core --lib finality -- --nocapture
cargo test -p norn-core --test four_node_bft_test -- --nocapture
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
- Validator and FullNode behavior is tested separately;
- a signer/WAL failure produces no broadcast and no state transition;
- a torn safety-WAL tail is recoverable;
- finality commit retry is idempotent after an apply/flush ambiguity;
- a conflicting block at an already finalized height is rejected;
- restart recovery replays the exact durable vote, certificate, and overlay
  write-set;
- four independent configurations derive the same snapshot hash and proposer
  sequence;
- the full workspace test suite is green.

These checks are completion gates, not evidence that an unreviewed deployment
is production-ready. Protocol upgrades continue to require a new versioned
Genesis or an explicitly specified activation-height migration.
