# Network wire v2

Consensus traffic is accepted only when all of the following are bound to the
local `ChainContext`:

- `wire_version`;
- `protocol_version`;
- `chain_id`;
- complete canonical `genesis_hash`.

The same identity is included in the libp2p topic namespace:

```text
norn/v2/w{wire}-p{protocol}-c{chain_id}-g{genesis_hash}/{topic}
```

The handshake topic carries the same fields plus the peer role (`Validator`
or `FullNode`) and the sender's canonical libp2p `PeerId` bytes. The receiver
requires the advertised PeerId to equal the authenticated gossipsub transport
source. This identity binding is required because gossipsub deduplicates by
payload: without it, two validators with the same role/context handshake
would collapse into one message. A peer must complete this handshake before
its consensus messages are forwarded to the node, and only an authenticated
`Validator` may originate consensus traffic. A mismatched handshake, role,
or unknown wire payload is rejected without entering consensus.

Outbound bootstrap addresses and explicit Dial commands must include
`/p2p/<PeerId>`. Addresses without an explicit peer identity are rejected;
this prevents a successful TCP connection from being mistaken for an
authenticated consensus peer. After a reconnect the handshake is published
again and the prior authentication entry is removed on connection close.

Consensus bytes are checked against a hard 2 MiB wire ceiling before bincode
decoding. Handshakes are limited to 1 KiB, block messages to 8 MiB, and
transaction messages to 256 KiB. These are protocol safety ceilings; later
Genesis resource parameters may tighten them but node-local configuration may
not increase them.

After decoding, `ConsensusEnvelope` checks its own context and the payload's
protocol, chain, height, epoch, round, snapshot, block, certificate, and vote
contexts. `ProposalV2` additionally validates the V2 block hash, Merkle root,
canonical transaction IDs, and hard resource ceilings before it can reach the
node loop. Each node then rechecks its Genesis-specific resource limits before
passing the payload to the V2 consensus adapter. The adapter re-executes the
currently supported V2 transaction set against the local immutable state
projection and compares both `state_root` and the domain-separated
execution-result commitment before invoking the Tendermint lock/unlock and
WAL-backed vote path. A failed execution or commitment mismatch cannot produce
a block vote. V2 candidates are stored in a separate typed map. Cryptographic
membership and quorum remain separate validation stages; durable overlay
application remains part of atomic finality. EVM/contract-shaped V2
transactions currently fail closed until their deterministic overlay executor
is implemented; they are never treated as native transfers.
