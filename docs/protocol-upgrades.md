# Protocol upgrade policy

The Candidate v4 implementation uses a new network and a new canonical
Genesis document for protocol-incompatible changes.

This is the selected upgrade policy for the current implementation baseline:

1. A new protocol object version is introduced for incompatible wire or
   signing changes (`TransactionV2`, `BlockHeaderV2`, and
   `ConsensusEnvelopeV2` where applicable).
2. The new object version is activated by a new Genesis and chain identity.
3. Nodes do not infer an object version from the serialized shape and do not
   attempt to reinterpret legacy bytes as the new format.
4. Unknown schema, wire, or protocol versions fail closed.
5. Existing databases must match the configured canonical Genesis identity;
   they are not silently migrated across protocol versions.

An activation-height migration (the alternative policy) is intentionally not
used for this baseline. It may be introduced later only as a separately
specified protocol upgrade with explicit old/new validation rules and a
migration test suite.
