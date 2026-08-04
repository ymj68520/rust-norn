# Genesis schema v5

`genesis.json` is a versioned network identity document. The node accepts it
only when `schema_version` is `5`; unknown versions fail closed.

Required top-level fields:

- `schema_version`
- `protocol_version`
- `chain_id`
- `epoch`
- `epoch_length`
- `initial_randomness`
- `resource_limits`
- `genesis_block`
- `validators`

`resource_limits` is canonical Genesis data and contains the maximum block
bytes, transaction count, block gas, transaction bytes, transaction gas,
execution-overlay writes, certificate members, future height/round windows,
and verification task/queue bounds, plus maximum consensus round, durable
proposal-attempt count/bytes per height, and parent-relative block timestamp
step. Nodes may not increase these values locally.

The active default permits consensus rounds `0..=63` and therefore requires
at least 64 durable proposal attempts per height. Genesis validation rejects a
round/attempt configuration that cannot durably cover every permitted round;
`max_block_timestamp_step` must also fit in the signed `i64` block timestamp
range.

Validator records contain `validator_id`, `consensus_public_key`,
`vrf_public_key`, and positive `voting_power`. Validator records are sorted by
`validator_id` for canonical hashing, so JSON list order does not affect the
network identity. Duplicate IDs, consensus keys, or VRF keys are rejected.

Epoch assignment is deterministic: height `1` starts at Genesis `epoch`, and
the epoch increases by one after every `epoch_length` finalized blocks. The
stake snapshot used at an epoch boundary must be derived from the finalized
state and carry the new epoch in its canonical hash. The next height's
`parent_randomness` must equal the `next_randomness` recorded by the preceding
`FinalizedConsensusState`; nodes cannot choose either value locally.

`genesis_path` in the node TOML configuration is resolved relative to the
TOML file when it is relative. Production nodes must provide this file;
Devnet/Test FullNodes may use the legacy fixed Genesis only as a non-consensus
fallback. Validators must always use a Genesis containing their validator
record.
