# Norn Consensus V2 State-Transition Rules

This document fixes the consensus rules implemented by
`norn-core::consensus::state_machine::TendermintStateMachine`. It is part of
the Candidate V2 protocol specification. A node must reject an object that is
outside the active chain context instead of attempting legacy interpretation.

## Context and invariant

Every proposal and vote is bound to exactly one
`(protocol_version, chain_id, epoch, height, round, step,
stake_snapshot_hash)` context. The epoch is derived from the Genesis
`epoch_length` schedule, the active snapshot must have that epoch, and the
parent randomness is the `next_randomness` persisted with the previous
finalized state. The proposer is selected from the active snapshot using the
fixed V2 proposer-seed formula.

The local state machine may advance a step or height only after the required
side effect has been acknowledged. A failed WAL write or signer operation does
not advance state and does not produce a broadcastable vote.

## Transition table

| Event | Required checks | State transition | Emitted action/result |
| --- | --- | --- | --- |
| Enter height | Finalized block and consensus state are durable; finalized height equals current height; snapshot hash matches; next randomness is present | Set `height = finalized.height + 1`, `round = 0`, clear locks and valid-round data, install the scheduled snapshot, then enter `Propose` | None |
| Enter round | Round is the current or explicitly selected next round; no conflicting height context | Set `round`; set `step = Propose` | None |
| Proposal accepted | Height, round, epoch, protocol, chain, parent hash, parent randomness, snapshot hash, proposer selection, canonical ECDSA signature, VRF proof/output, and block-header hash all match | No lock is changed by proposal processing | `VoteIntent(Prevote(block))`; after WAL + signer completion, `VotePersisted` |
| Proposal rejected | Any proposal check fails, including invalid proposer, signature, VRF, context, or block header | No lock or valid-round field is changed | `VoteIntent(Prevote(NIL))`; after persistence/signing, `VotePersisted` |
| Prevote(block) received | Vote has the exact active context, valid validator/signature, and unique validator identity; block is bounded by protocol limits | Add to the vote pool. No state change until a POLKA is formed | None, unless local node must emit precommit |
| Prevote(NIL) received | Same vote/context checks, with `block_id = NIL` | Add to the vote pool. NIL never creates a block lock or commit certificate | None |
| Prevote POLKA(block) | More than two-thirds of the active snapshot stake prevoted the same block; certificate members are within the Genesis limit | First obtain local `VotePersisted(Precommit(block))` when this node is a validator; only then set `locked_block = block`, `locked_round = round`, `valid_block = block`, `valid_round = round`, and retain the prevote certificate | Local `VoteIntent(Precommit(block))`; or no local vote for a FullNode |
| Prevote POLKA(NIL) | More than two-thirds prevoted NIL | Do not set or replace a lock; remain eligible for the next round | None |
| Proposal while locked | A different block is accepted only when it carries a valid prevote certificate for that block at `valid_round >= locked_round` and the certificate round matches | If unlock proof is absent or invalid, prevote NIL. If valid, prevote the proposed block; lock changes only after the resulting POLKA/precommit path | Prevote intent as above |
| Prevote timeout | Timeout belongs to current height/round and no valid block POLKA has advanced the state | Keep lock fields unchanged; set `step = PrevoteWait` only after the NIL precommit vote is durably signed | `VoteIntent(Precommit(NIL))`; then `VotePersisted` |
| Precommit(block) received | Same context/signature/uniqueness checks; block certificate has bounded members | Add to the vote pool | None until quorum |
| Precommit(NIL) received | Same checks, with NIL block | Add to the vote pool; NIL cannot finalize a block | None |
| Precommit quorum(block) | More than two-thirds active stake precommitted the same block; certificate is canonical and within Genesis limits | Set `step = Commit` only after the certificate is formed and verified; do not increment height in the consensus state machine yet | `CommitCertificate` for the finality driver |
| Precommit timeout | Current round has no finality certificate | Enter `round + 1`, set `step = Propose`; retain a valid/locked block according to the rules above | None |
| Finality commit failure | Storage batch, flush, or finalized-state persistence fails or returns an indeterminate result | Keep current height/round and the signed commit/finality intent available for deterministic retry; never choose another block | No new vote or Commit broadcast |
| Finality commit acknowledged | Finalized block, certificate, transaction result, state root, and `FinalizedConsensusState` are durable and mutually consistent | Advance exactly once to the next height using persisted `next_randomness` | Broadcast Commit/recovery event as appropriate |

## Intent/Ack ordering

The vote path is:

```text
state machine builds VoteIntent
    -> SafetyStore writes and syncs intent
    -> signer produces signature
    -> SafetyStore writes and syncs SignedVote completion
    -> state machine applies VotePersisted
    -> network driver broadcasts the exact SignedVote
```

The following rules are mandatory:

- WAL failure: no signing, no broadcast, no step/lock transition.
- Signer failure: no broadcast; the state remains retryable, but the safety
  slot remains bound to the original intent and cannot be used for another
  block.
- Broadcast failure: the already signed vote remains authoritative. A retry
  may only replay that exact vote; it must not sign a different block.
- Restart: completed WAL records are recovered and re-broadcast as the exact
  persisted signatures. An intent without a completion record is not treated
  as a broadcastable vote.

## Resource and fail-closed rules

The Genesis `ProtocolResourceLimits` are consensus parameters, not local
preferences. They bound block bytes, transaction count and bytes, block and
transaction gas, overlay writes, certificate members, future height/round
windows, and verification concurrency/queue size. A proposal, vote pool,
certificate, execution overlay, or verification task over its bound is
rejected without changing consensus state.

Unknown protocol versions, malformed canonical encodings, invalid signatures,
invalid VRF proofs, mismatched snapshots, and mismatched Genesis identities
are all fail-closed conditions.
