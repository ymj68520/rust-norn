# TransactionV2 and deterministic execution

`TransactionV2` is the protocol transaction object selected by the new
Genesis/network policy. It is not inferred from legacy transaction bytes.

The signed preimage contains the protocol version, chain ID, sender, receiver,
nonce, value, fee bounds, payload fields, expiry, timestamp, access list, and
public key. It does not contain:

- block height;
- transaction index;
- block hash;
- the signature itself;
- the derived transaction ID.

The signature is made over the canonical preimage. The transaction ID is then
`SHA-256("NORN_TRANSACTION_ID_V2" || signing_bytes || signature)`. A block
Merkle leaf is derived from that ID, so the transaction and block hashes have
no circular dependency.

`ExecutionOverlay` reads the immutable base state, records writes in memory,
and emits them in ascending address/storage-key order. `execute_v2_block`
rejects invalid signatures, nonce/balance violations, gas/byte/count limits,
and overlay write-set overflow before any base-state mutation. Applying the
overlay is a separate operation and is intentionally outside transaction
execution.

`BlockV2` commits the ordered transaction IDs through a domain-separated
Merkle root, commits the projected post-overlay state root in the header, and
derives the block ID from the header only. `ProposalV2` carries this block as
an explicit consensus payload; it is never decoded as the legacy `Block`.
Block production selects transactions without removing them from the V2 pool.
Removal is reserved for the later finalized-commit path, so a rejected or
crashed proposal cannot drop a valid transaction.

Legacy `Transaction` remains an explicit compatibility adapter for code that
has not migrated. It is not accepted on the V2 transaction wire, and legacy
bytes are never reinterpreted as V2 messages. Block height, transaction index,
and block hash belong to receipts/query results derived after finalization,
not to a submitted transaction object.
