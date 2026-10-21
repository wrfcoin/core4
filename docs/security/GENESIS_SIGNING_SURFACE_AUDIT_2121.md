# Genesis signing-surface audit (#2121)

Status: current audit result, 2026-07-28. This is a living cut artifact, not a
claim that epic #2121 is complete.

## Thesis and scope

Every object that can be signed at genesis or admitted from block 1 must:

1. have a unique domain;
2. bind the active chain and network;
3. be discoverable by the source-derived signed-type inventory, with pinned
   bytes for every live hand-built authorization preimage; and
4. be signable through the production path by the key that must hold the
   authority.

“Implemented `DomainSeparated`” alone is not a pass. Several registry entries
have an additive typed preimage while production still signs different bytes.
This audit reports the live preimage and the real signer separately.

The status terms below are:

- **PASS** — the live path has the property.
- **PARTIAL** — an additive definition, helper, or constructor exists, but the
  live production seam does not yet prove the property.
- **NO** — the live path lacks the property.
- **N/A** — the type is not independently signed in production.
- **CUT-OR-NEVER** — fix before fresh genesis or keep the surface disabled;
  changing the preimage later requires another coordinated reset.
- **CUT** — must be demonstrated before the cut, but does not itself change
  consensus bytes.
- **POST-CUT** — safe to leave disabled or unexercised at genesis.

## Current answer

| Signed surface and live path | Domain-separated | Chain-bound | Inventoried / pinned | Production signability | Disposition |
|---|---|---|---|---|---|
| `Block` producer signature over the V2 block preimage | PASS — `wrfcoin.block_header.v1` | PASS — V2 frame binds chain, network, and version | PASS — registry, golden, and one-encoding guards | PARTIAL — `load_block_signer -> PopConsensusAdapter -> sign_at_height` is proven in software; physical primary and standby keys have no owner transcript | **CUT**: run both YubiKeys after #2407 |
| `Transaction` input signatures over `wrfcoin_tx_sighash_v2` | PASS | PASS — chain and network are in the live sighash | PARTIAL — type is inventoried and typed golden exists; the live sighash is a separate construction | PASS for the Rust software sign/verify path; external-signer parity must be checked per client | Keep enabled; external clients remain a cut checklist item |
| `FinalityVote::content_hash` | PASS — `wrfcoin-finality-vote-v2|` | **NO** — live bytes bind block, height, epoch, committee hash, and validator, but not chain/network | PARTIAL — type and unique tag are inventoried; additive `DomainSeparated` body also omits epoch and committee hash and is not the live verifier | PARTIAL — local Ed25519 construction exists; no chain-bound production preimage exists to prove | **CUT-OR-NEVER** (#2226/#2249): migrate the live vote bytes; do not switch to the incomplete additive body |
| `ValidatorSetChange` | PASS in the additive typed frame | PASS in the additive V2 frame | PASS — registry and golden | **PARTIAL** — no production detached-signing call was found | POST-CUT unless activated; require a real envelope before use |
| `ProducerSetChangeEnvelope` signing a `ProducerSetChange` | PASS — distinct producer-set tag | PASS — V2 frame | PASS — registry, golden, and cross-domain test | PASS in software through the real envelope; governance-key hardware ceremony is not demonstrated | **CUT** for genesis authority provenance; seeding itself is already wired and rejects a seeded-empty set |
| `OracleRegistrationPayload` governance authorization | PASS — `wrfcoin_oracle_registration_auth_v1` | PASS | PASS — source inventory and byte-pinned golden | PARTIAL — production verifier and software signer are exercised; final governance/oracle hardware keys are not | **CUT**: block 1 must carry final N=3, K=2 from genesis metadata, hardcoded nowhere |
| `RotationPlan` proof of possession | PASS on this branch — `wrfcoin.validator_key_rotation_pop.v2` | PASS on this branch | PASS — source scanner recognizes `proof`; byte-pinned golden | PASS in software for old-key proof generation and verification | Land before any rotation; no quorum-change migration is in scope |
| `SignedSnapshotSyncPayload` | PASS — live snapshot action domain | PASS — live action envelope binds chain/network | PASS — typed registry and golden | PASS in software through `load_snapshot_signer`; physical slot 9C is not demonstrated | **CUT** if snapshot signing is enabled at genesis; sequence after #2407 |
| `CommitPayload` authorization | PASS — `wrfcoin_commit_auth_v2` | PASS | PASS — first-class authorization surface and golden | PASS in the Rust collector/verifier construction | Keep enabled |
| Transparent-transfer authorization | PASS in Rust — `wrfcoin_transparent_transfer_auth_v2` | PASS in Rust | PASS — first-class authorization surface and golden | **PARTIAL** — Rust path passes; the hand-mirrored JS faucet signer recorded on #2226/#2254 used the old tag and omitted chain binding | **CUT-OR-NEVER** for any external signer used at genesis: prove the exact golden bytes before enabling |
| System-transfer authorization | PASS on this branch — `wrfcoin_system_transfer_auth_v2` | PASS on this branch | PASS — first-class authorization surface and golden | PASS in the Rust software authority path | Land before enabling system transfers |
| `StakeAuthPayload` | PASS — `wrfcoin_stake_auth_v2` | PASS | PASS — first-class authorization surface and golden | PASS in the Rust software sign/verify path | Keep enabled |
| `GamificationClaimPayload` | PASS — `wrfcoin_gamification_auth_v1` | PASS | PASS — live signed type is explicitly inventoried and golden | PASS in the Rust software sign/verify path | Already correct |
| `RewardClaimPayload` MLSAG authorization | PASS on this branch — `wrfcoin_reward_claim_auth_v2` | PASS on this branch | PASS — first-class authorization surface and golden | PASS for canonical binary bytes through the real privacy signing and verification routes; full client composition remains to be exercised | Land before claims are enabled. The committed amount now uses the post-#2493 denomination |
| `ConfidentialTransferPayload` MLSAG authorization | PASS on this branch — `wrfcoin_confidential_transfer_auth_v2` | PASS on this branch | PASS — first-class authorization surface and golden | PASS — the production privacy API accepts exact binary `message_hex`, signs it, and verifies it | Land before confidential transfers are enabled |
| `SelectiveDisclosureProofPayload.verifier_attestation` | PASS on this branch — `wrfcoin_selective_disclosure_auth_v2` | PASS on this branch | PASS — scanner recognizes the live signature field; byte-pinned golden | PASS in software; no physical oracle-key transcript yet | **CUT** only if disclosures are accepted from block 1 |
| `GovernanceVotePayload::canonical_signing_message` | PASS for object kind — `governance-vote-v1|` | **NO** — live bytes contain only proposal id and decision | PARTIAL — type is inventoried; additive chain-bound typed golden is not the live verifier | PARTIAL — software bytes can be signed, but the bytes are not chain-scoped | **CUT-OR-NEVER** (#2229/#2226): migrate before governance voting is enabled |
| `GovernanceProposal` | N/A — submitted through an outer transaction | N/A — outer transaction supplies live binding | PASS — additive typed definition is inventoried and golden | N/A — no separate live proposal signer | Do not claim a detached guarantee; outer transaction is the authority |
| `SlashingReportPayload` reporter authorization | PASS — live slashing action domain | PASS — live action envelope binds chain/network | PASS — typed registry and golden | PASS in the production slashing submitter software path | Already correct |
| `OracleAttestationPayload` | PASS — live oracle-attestation action domain | PASS | PASS — typed registry and golden | PASS in software; physical three-oracle signability is not demonstrated | **CUT** for the final N=3 operator set |
| `CalibrationAttestationPayload` | PASS — distinct calibration action domain | PASS | PASS — typed registry and golden | PASS in the Rust software sign/verify path | Already correct |
| `ComputeReceipt` | PASS — distinct compute-receipt action domain | PASS | PASS — typed registry and golden | PASS through the type's software sign/verify path; no hardware role is required by current design | Already correct |
| `ForecastVerificationReceipt` | PASS — distinct forecast-verification action domain | PASS | PASS — typed registry and golden | PASS through the type's software sign/verify path; physical oracle proof remains part of the N=3 ceremony | **CUT** if accepted from block 1 |
| `WeatherReading` | N/A as a detached object — signed through an oracle envelope | PASS through the enclosing live action | PASS — indirect-signing explanation, typed registry, and golden | N/A independently; oracle signer is the production authority | Already correct as an indirect object |
| `DeviceAttestation` | **PARTIAL** — a unique typed tag is reserved/implemented, but the live signer uses the shared oracle-attestation action | PASS — live action envelope binds chain/network | PASS for discovery and cross-crate impl; no live-type golden | PASS through the type constructor, not an external production service | POST-CUT unless activated; migrate to its unique live type tag before use |
| `StorageAttestation` | **PARTIAL** — a unique typed tag is reserved/implemented, but the live signer uses the shared oracle-attestation action | PASS | PASS for discovery and cross-crate impl; no live-type golden | PASS through the type constructor | POST-CUT unless activated; migrate to its unique live type tag before use |
| `OraclePacket` | **PARTIAL** — a unique typed tag is reserved/implemented, but live variants reuse oracle-attestation or forecast-verification actions | PASS | PASS for discovery and cross-crate impl; no live-type golden | PASS through the type constructor | POST-CUT unless activated; migrate to its unique live type tag before use |
| `BridgeMessage` | PARTIAL — additive typed tag only | PARTIAL — additive V2 frame only | PASS — registry and golden | **NO production signer found**; the type has no signature field | POST-CUT / disabled. Do not describe this as live until an envelope and caller exist |
| EVM wallet-login challenges | Separate EIP-191 key/domain system | Not WRFCoin-chain-bound | Reserved exception, tracked by #2228 | Signable by connected EVM wallets | Outside the genesis Ed25519 surface |

## Ranked gaps

### P0 — CUT-OR-NEVER preimage gaps

1. **Finality vote chain binding.** The live verifier signs
   `wrfcoin-finality-vote-v2|block_hash|height|epoch|validator_set_hash|validator_id`
   without chain or network. The additive typed body is not a safe drop-in
   because it omits `epoch` and `validator_set_hash`. Bob owns the remaining
   #2226 wiring; this audit must not duplicate it.
2. **Governance vote chain binding.** The live verifier signs
   `governance-vote-v1|proposal_id|approve`. The additive typed definition is
   correct but not live. Either migrate before genesis or keep governance
   voting disabled.
3. **External transparent-transfer parity.** The Rust v2 preimage is correct,
   but #2226/#2254 records a JS faucet mirror on the old tag with no chain
   fields. Every external signer used at genesis must reproduce the pinned Rust
   bytes before it is enabled.

### P1 — CUT production-key proof

1. **Producer keys.** The software production path is proven for both compiled
   public keys. The physical private halves on YubiKey serials 35278470 and
   35278471 have not produced transcripts. #2407 does not prevent signing, but
   its default management key can invalidate a prior transcript by allowing the
   PIV slot to be replaced. Correct order: resolve #2407, verify slots 9A/9C,
   then run both owner-only `producer_key_pop` proofs.
2. **Oracle keys.** The owner decision is final: N=3, K=2, final set from block
   1, no later quorum-change migration. Provision the Mac mini and demonstrate
   the three oracle signing keys plus spares through the production role.
3. **Genesis authority keys.** Record which key signs the producer-set envelope
   and the block-1 oracle registration, then prove those exact production
   signers. A software signature from an arbitrary test key is not custody
   evidence.

### P2 — land the field-binding fixes before enabling their surfaces

This branch closes five live preimage gaps:

- reward claim: chain/network, key image, amount commitment, range proof, and
  destination;
- validator rotation proof: chain/network and both keys at the effective
  height;
- system transfer: chain/network plus sender, destination, amount, and nonce;
- selective disclosure: chain/network, scope, expiry, disclosed-fields digest,
  and oracle attestation;
- confidential transfer: chain/network and every consensus field except the
  signature containers, with the MLSAG key image tied to the payload.

Each has a field-binding regression test. The confidential-transfer proof also
crosses the production HTTP seam using non-UTF-8 canonical bytes.

## Structural finding: semantic-absence collapse

The recurring class is **semantic-absence collapse**: “missing,” “empty,” or
“unchecked” is represented as an ordinary successful value, so a guard becomes
a no-op without a distinct failure state.

Confirmed examples are:

- an unseeded producer history (`[]`) deliberately means “not enforced,” while
  a seeded empty `ProducerSetChange` must be distinguishable and rejected;
- state-root enforcement previously depended on callers not swallowing an
  applier error; PR #2503 now classifies every checked and reconstruction-only
  finalization call and exercises the production call sites;
- an inventory row or additive preimage can exist while no production signer
  uses it (`BridgeMessage`, the deferred finality/governance paths).

The audit therefore never treats registry presence, an empty collection, or an
unchecked helper return as enforcement evidence.

### Completeness gate cannot pass on an absent corpus

The signed-type and `DomainSeparated` implementation inventories share one
fail-closed production-source enumerator. It:

- proves its root is the Cargo workspace rather than a nearby directory;
- fails on every directory-entry or Rust-source read error instead of silently
  continuing with a partial tree;
- requires at least 400 production Rust files; and
- requires seven named source sentinels spanning shared-protocol,
  consensus-engines, forecast-score, keeper-proof, oracle-packet, and
  sensor-attestation.

The separate inventory target also remains named in
`scripts/integration_test_ratchet.json`, whose contract rejects new, removed,
renamed, or unexecuted integration targets. A large partial scan, an empty
registry loop, or an omitted test binary is therefore not accepted as a
complete audit.

## Evidence and open verification

Author execution on the audit branch:

- `cargo test -p wrfcoin-shared-protocol --lib`:
  **1564 passed, 0 failed**.
- `cargo test -p wrfcoin-shared-protocol --test signed_type_inventory`:
  **17 passed, 0 failed**.
- `cargo test -p wrfcoin-shared-protocol --test signing_preimage_v2_golden`:
  **4 passed, 0 failed**.
- `cargo test -p wrfcoin-shared-protocol --test cross_domain_replay`:
  **14 passed, 0 failed**.
- confidential-transfer state filter: **8 passed, 0 failed**.
- confidential-transfer preimage filter: **1 passed, 0 failed**.
- production privacy route binary-preimage filter: **1 passed, 0 failed**;
  full privacy API group: **9 passed, 0 failed**.

Executed RED controls:

- replacing only the mandatory `oracle-packet/src/lib.rs` corpus sentinel with
  `oracle-packet/src/NOT_A_SOURCE.rs` made the completeness gate fail
  **0 passed, 1 failed**; the failure named the missing production file;
- removing output commitments only from the confidential-transfer preimage made
  the field-binding regression fail **0 passed, 1 failed**;
- treating `message_hex` as UTF-8 text made the production route regression
  fail **0 passed, 1 failed** and showed the wrong signed bytes;
- the preceding branch commits separately record RED controls for reward
  destination, rotation chain identity, system-transfer chain identity, and
  selective-disclosure scope.

GitHub Actions results after 08:00Z are intentionally absent: Actions minutes
are exhausted and runnerless, zero-step runs are non-evidence. The pushed audit
head still requires the independent second local execution under the current
two-execution standard.

## Cut conclusion

The signing surface is **not yet sound enough to cut**. The code gaps are
bounded: live finality/governance chain binding, external-signer parity, and
landing the field-binding batch. The longer pole is production key custody:
two producer YubiKey transcripts, three oracle nodes at K=2, the Mac mini, and
spares. No quorum-change migration should be built; genesis is cut once with
the final set.
