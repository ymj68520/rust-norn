# Genesis schema v2

`genesis.json` is a versioned network identity document. The node accepts it
only when `schema_version` is `2`; unknown versions fail closed.

Required top-level fields:

- `schema_version`
- `protocol_version`
- `chain_id`
- `epoch`
- `epoch_length`
- `initial_randomness`
- `genesis_block`
- `validators`

Validator records contain `validator_id`, `consensus_public_key`,
`vrf_public_key`, and positive `voting_power`. Validator records are sorted by
`validator_id` for canonical hashing, so JSON list order does not affect the
network identity. Duplicate IDs, consensus keys, or VRF keys are rejected.

`genesis_path` in the node TOML configuration is resolved relative to the
TOML file when it is relative. Production nodes must provide this file;
Devnet/Test FullNodes may use the legacy fixed Genesis only as a non-consensus
fallback. Validators must always use a Genesis containing their validator
record.
