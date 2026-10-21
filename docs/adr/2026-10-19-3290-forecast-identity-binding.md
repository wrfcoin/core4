# ADR: protocol-level forecast identity binding

Status: **PROPOSED — POST-FREEZE**. Design only; no activation height, consensus code, transaction schema change or deployment is authorized by this document.

Refs: core4#3290, core4#3604, core4#3114, core4#3277.

## Problem and current boundary

The HTTP routes bind a forecaster to the authenticated bearer's wallet. Peer transactions do not pass through that route. The current commit authorization signs chain/network, station, commit id and contribution type, but neither a canonical forecaster address nor the transaction sender. Verification against a caller-supplied key proves possession of that key, not the right to name another forecaster.

Source anchors: [commit authorization](../../shared-protocol/src/blockchain/transaction.rs), [mempool admission](../../shared-protocol/src/mempool/pool.rs), [transition extraction](../../native-chain/src/bin/launch_api/transition_extractor.rs), and [canonical Address derivation](../../shared-protocol/src/common/types.rs).

## Proposed choice

Use direct key-derived identity: the canonical 20-byte forecaster must equal `Address::from_public_key(auth_pubkey)`. Compare bytes, not case-folded strings or display spellings. The transaction sender and credited identity must be that same address. Malformed addresses are rejected; hashing an arbitrary label into an address is forbidden.

A forecaster-to-key registry is deferred: it introduces registration, rotation, recovery and historical-key state that this defect does not require. Account delegation and smart-account authorization are separate future designs. Direct binding means a key change creates a different forecaster identity unless a later reviewed migration explicitly says otherwise.

## Versioned authorization

Keep the existing `COMMIT_AUTH_DOMAIN` and helper byte-for-byte. Introduce a separately reviewed forecast-only V2 domain and explicit authorization version; never append fields to V1 in place or infer V2 merely from signature length. Proposed V2 encoding, to be frozen by cross-language golden vectors:

1. A distinct fixed domain tag and version discriminator.
2. Chain id and network id using the existing length-prefixed domain encoding.
3. The canonical forecaster's 20 address bytes.
4. Station id, commit id and the literal forecast contribution type, each length-prefixed using the existing integer width/endianness.

The key must derive the address and the signature must cover all these bytes. A valid signature over a victim's address using an unrelated key is rejected even if cryptographically valid. Contribution commits retain their existing protocol and domain.

The V2 wire representation must use a separately discriminated payload or transaction envelope with frozen serialization vectors. Do not add a serde-defaulted field to a bincode-serialized legacy struct and assume old bytes still decode. Parser availability can ship before activation, but V2 execution is refused before activation; V1 forecast authorization retains historical semantics while the gate is OFF.

## Feature registration and activation

Propose `ConsensusFeature::ForecastIdentityBindingV2` with stable name `forecast_identity_binding_v2`, paired in `ConsensusFeature::ALL` and `KNOWN_FEATURES`. These are proposed names, not registered features today.

The registration slice is unscheduled and `schedulable = false` until every admission, block-validity, state and replay consumer is wired. No genesis default, environment override or implicit testnet activation. Prove gate-OFF parity first. A later reviewed release may permit scheduling through the existing schedule-extension governance path only after all prerequisite evidence is accepted. It must retain the path's identity binding, notice interval, duplicate/unknown-feature refusal and readiness checks. No historical schedule rewrite and no direct database edit.

The operator proposes a future height H only after the freeze and separate owner approval. Unsupported binaries must refuse the announced feature according to existing upgrade support checks. Monitor capability before H; a rollback to a binary lacking activated semantics is not safe. This ADR does not set H or authorize a governance transaction.

## Admission and block validity

Use one height-aware validation function for HTTP submission, gossip/mempool admission, block assembly, block validation and replay/transition extraction. At H and later, new forecast commits require V2 plus exact address/key/sender equality. Invalid peer-supplied commits must invalidate the candidate block, not merely disappear from extracted transitions while the block is accepted.

Admission evaluates the candidate inclusion height using the canonical chain schedule, not wall time. Revalidate pending transactions at H and after reorgs; a transaction admitted at H-1 has no right to enter a block at H under old rules. Keep legacy contribution behavior unchanged.

Forecast reveals must match the identity and authorization version recorded by their accepted commit, as well as the existing commitment and manifest/receipt checks. Proposed crossing-H policy: commitments accepted in blocks below H retain their historical reveal/payee rules; new commits at or above H cannot select V1. Preserve enough canonical commitment provenance to distinguish the cases deterministically across restart and snapshot restore. If existing state lacks that provenance, introduce and test a versioned migration before enabling scheduling. Never derive the policy from mempool arrival time.

## Evidence required before activation

- Golden vectors: V1 unchanged; V2 fixed bytes across Rust/CLI and a separate implementation. Mutate chain, network, address, key, station, commit id, type and version independently; every mutation is refused.
- Reproduce #3290's unrelated-key attack through HTTP, direct mempool insertion, P2P gossip and a constructed block. At/after H all four paths refuse it; include a legitimate key-derived forecaster control.
- Exercise H-1/H/H+1, duplicate/unknown scheduling, unsupported binaries, empty/invalid addresses, case/display variants, wrong sender, key substitution and V1/V2 downgrade attempts.
- Revalidate a queued V1 commit across H; test a reorg crossing H in both directions and restart at each boundary.
- Replay one fixed pre-H corpus through old and new binaries with the feature absent: identical acceptance, serialized state, state roots, receipts and reward accounting. Include a default-OFF fresh chain and restore from an old snapshot.
- Replay a scheduled corpus across H: both implementations agree below H; after H only explicitly enumerated identity/version refusals differ. Valid V2 commits/reveals produce deterministic roots across independent runs. Historical pending commitments use the documented crossing-H rule.
- Prove block-invalid versus silently-skipped behavior with mutation controls that remove each verifier call. Gate on executed nonvacuous tests, not merely successful compilation or queued CI.

## Separate API protection

Phase A5 quotas are node-local traffic protection. They cannot establish cryptographic forecaster identity, prevent direct gossip, or substitute for this feature. A wallet may belong to a person, agent or service; no agent-specific protocol identity is introduced.

Outstanding implementation decisions are the exact V2 wire discriminator/domain bytes and any commitment-provenance migration required by the crossing-H policy. Review these before code lands; do not silently choose an activation height or reinterpret legacy forecast identities.
