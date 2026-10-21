# Domain-tag registry

> **This file is checked against the code.**
> `shared-protocol/src/crypto/domain_registry.rs` embeds this document with `include_str!` and
> `tests::registry_doc_matches_code` asserts the table below matches `SIGNED_OBJECT_REGISTRY`
> row for row, in order. Editing one without the other fails the build.

Every object the WRFCoin protocol signs is listed here. The inventory is derived **from the source
tree**, not from call sites — that is the correction #2122 makes to the approach that let #2065
through. Coverage measured by "who calls the domain module" cannot see a signed type that never
calls it; the absence of a call site produces no evidence of a gap.

> **The table below is not the source of truth for completeness — the source tree is.**
> `shared-protocol/tests/signed_type_inventory.rs` scans every crate for types carrying a
> signature field and fails if one is neither registered here nor explicitly excluded with a
> reason. The first version of this document was hand-written and passed its own tests while
> omitting five live signed types and binding `FinalityVote` to an unsigned struct; a registry
> that cannot detect its own omissions is self-consistent and still wrong.

## Threat model

Domain separation is what stops a signature over one object being replayed as a signature over
another. Without a unique tag inside the preimage, an attacker who obtains any signature from a
victim key can attempt to reinterpret those bytes as a different object type — a wallet login
challenge as a transfer, an attestation envelope as a transaction (the #2065 shape).

Two properties make that impossible:

1. **Unique tags.** Every object kind has a distinct `DOMAIN_TAG`.
   `domain_registry::tests::all_domain_tags_are_unique` is a hard failure on collision.
2. **Unambiguous framing.** `domain_signing::domain_preimage` length-prefixes both the tag and
   the body, so concatenation cannot be re-split: `("ab","c")` and `("a","bc")` produce different
   bytes.
3. **Injectivity, enforced from both sides.** The registry check
   (`registry_is_injective_on_rust_type`) sees only registry *rows*. That is not enough: an `impl`
   can claim a tag with no matching row, which is how `FinalitySignal` kept
   `wrfcoin.finality_vote.v1` after the registry rebound that tag to the real `FinalityVote`,
   leaving two types answering to one tag. `impl_tags_are_injective_across_the_source` and
   `every_domain_impl_agrees_with_its_registry_row` scan the source for every
   `impl DomainSeparated for T` and fail on a collision or a drifted tag.

## Coverage states

| State | Meaning |
|---|---|
| `Implemented` | Implements `DomainSeparated`; no conflicting legacy signing path. |
| `MigrationDeferred` | Implements `DomainSeparated`, but the **live** signing path still uses a legacy preimage. Cutting over is consensus-breaking, so it is issue-tracked rather than silently applied. |
| `CrossCrate` | The type is real but lives in a crate that **depends on** `shared-protocol`, so its impl cannot live here (dependency cycle). Coherence permits the impl in the owning crate; the tag is reserved so nothing collides meanwhile. Tracked by #2249. |
| `Exception` | The protocol signs this conceptually, but no Rust type exists to implement against. Issue-tracked. |

`MigrationDeferred` is the honest state for most rows today. The trait impls added by #2122 are
**additive**: they define the audited preimage without changing any byte a running node currently
signs or verifies. Cutting the live paths over is a protocol-version bump — see #2123 and its
open PR #2232, which carries `docs/security/PREIMAGE_MIGRATION.md`. (That file is **not on
`main`**; it arrives when #2232 merges.)

## Registry

| Kind | Tag | Type | Coverage | Issue |
|---|---|---|---|---|
| Transaction | `wrfcoin.transaction.v1` | `blockchain::transaction::Transaction` | MigrationDeferred | #2226 |
| BlockHeader | `wrfcoin.block_header.v1` | `blockchain::block::Block` | MigrationDeferred | #2226 |
| FinalityVote | `wrfcoin.finality_vote.v1` | `consensus_engines::selection::bft_finality::FinalityVote` | CrossCrate | #2249 |
| ValidatorSetChange | `wrfcoin.validator_set_change.v1` | `consensus::validator_set::ValidatorSetChange` | Implemented | — |
| ProducerSetChange | `wrfcoin.producer_set_change.v1` | `consensus::producer_set::ProducerSetChange` | Implemented | — |
| SlashingEvidence | `wrfcoin.slashing_report.v1` | `security::slashing::SlashingReportPayload` | MigrationDeferred | #2226 |
| SnapshotPayload | `wrfcoin.snapshot_sync_payload.v2` | `blockchain::snapshot_sync::SnapshotSyncPayload` | MigrationDeferred | #2226 |
| WeatherObservation | `wrfcoin.weather_observation.v1` | `weather::validation::WeatherReading` | Implemented | — |
| OracleAttestation | `wrfcoin.oracle_attestation.v1` | `blockchain::transaction::OracleAttestationPayload` | MigrationDeferred | #2226 |
| AttestationEnvelope | `wrfcoin.calibration_attestation.v1` | `blockchain::transaction::CalibrationAttestationPayload` | MigrationDeferred | #2226 |
| GovernanceProposal | `wrfcoin.governance_proposal.v1` | `governance::proposal::Proposal` | Implemented | — |
| GovernanceVote | `wrfcoin.governance_vote.v1` | `blockchain::transaction::GovernanceVotePayload` | MigrationDeferred | #2229 |
| BridgeMessage | `wrfcoin.bridge_message.v1` | `cross_chain::types::BridgeMessage` | MigrationDeferred | #2226 |
| WalletLoginChallenge | `wrfcoin.wallet_login_challenge.v1` | `(no type)` | Exception | #2228 |
| ComputeReceipt | `wrfcoin.compute_receipt.v1` | `blockchain::transaction::ComputeReceipt` | MigrationDeferred | #2226 |
| ForecastVerificationReceipt | `wrfcoin.forecast_verification_receipt.v1` | `forecast_score::blockchain::ForecastVerificationReceipt` | MigrationDeferred | #2226 |
| DeviceAttestation | `wrfcoin.device_attestation.v1` | `sensor_attestation::DeviceAttestation` | CrossCrate | #2249 |
| StorageAttestation | `wrfcoin.storage_attestation.v1` | `keeper_proof::StorageAttestation` | CrossCrate | #2249 |
| OraclePacket | `wrfcoin.oracle_packet.v1` | `oracle_packet::OraclePacket` | CrossCrate | #2249 |
| UpgradeNotice | `wrfcoin.upgrade_notice.v1` | `governance::upgrade_notice::UpgradeNotice` | Implemented | — |
| FeatureScheduleExtension | `wrfcoin.feature_schedule_extension.v1` | `governance::schedule_extension::FeatureScheduleExtension` | Implemented | — |
| FaucetSinkPossession | `wrfcoin.faucet_sink_pop.v1` | `governance::faucet_sink_pop::FaucetSinkPossession` | Implemented | — |
| NativeAccountSubject | `wrfcoin.native_smart_account.tx.v1` | `accounts::native_smart_accounts::NativeAccountSubject` | Implemented | — |
| NativeSponsorSubject | `wrfcoin.native_smart_account.sponsor.v1` | `accounts::native_smart_accounts::NativeSponsorSubject` | Implemented | — |
| NativeSessionPossession | `wrfcoin.native_smart_account.session_possession.v1` | `accounts::native_smart_accounts::NativeSessionPossession` | Implemented | — |
| SlashEvidenceReport | `wrfcoin.slash_evidence_report.v1` | `security::slash_evidence::SlashEvidenceReport` | Implemented | — |
| NodeServiceReceipt | `wrfcoin.node_service_receipt.v1` | `consensus::node_service_receipt::NodeServiceReceipt` | Implemented | — |
| NodeServiceTicket | `wrfcoin.node_service_ticket.v1` | `consensus::node_share_contracts::NodeServiceTicket` | Implemented | — |
| NodeSettlementEndorsement | `wrfcoin.node_settlement_endorsement.v1` | `consensus::node_share_contracts::NodeSettlementEndorsement` | Implemented | — |
| NodeBondAuthorization | `wrfcoin.node_bond_authorization.v1` | `consensus::node_share_contracts::NodeBondAuthorization` | Implemented | — |
| NodeBondPossession | `wrfcoin.node_bond_possession.v1` | `consensus::node_share_contracts::NodeBondPossession` | Implemented | — |
| NodeReceiptSubmissionAuthorization | `wrfcoin.node_receipt_submission.v1` | `consensus::node_receipt_registry::NodeReceiptSubmissionAuthorization` | Implemented | — |

Native account subjects use explicit identity and V2 via their checked `preimage`
methods. They are signing foundations only, without transaction admission or
consensus activation. `wrfcoin.native_smart_account.address.v1` is an unsigned
address-derivation domain; its hash prefix uses the existing Address convention.

## External cross-repository reservations

These tags belong to formats outside the Rust object registry. They reserve the
namespace without inventing a Rust type or changing any native signing path.
`all_domain_tags_are_unique` includes these `CrossCrateReserved` rows: a future
native registry entry claiming one of them fails the uniqueness gate. The main
registry table above still matches `SIGNED_OBJECT_REGISTRY` row for row.

| External object | Reserved tag | Owning repository / format | Coverage | Tracking |
| --- | --- | --- | --- | --- |
| JoinArchiveManifest | `wrfcoin.join_archive.v1` | `wrfcoin/infra`: `wrfcoin.join_archive.signed.v1` JSON envelope | CrossCrateReserved | [infra #1057](https://github.com/wrfcoin/infra/issues/1057), [infra #1198](https://github.com/wrfcoin/infra/pull/1198) |

Only the infra archive adapter may use this tag. Its preimage is ASCII tag,
one zero separator, eight-byte BIG-ENDIAN payload byte length, and exact UTF-8
payload bytes. It is not a native `DomainSeparated` encoding. Native registered
tags are shorter than 256 bytes, so their eight-byte LITTLE-ENDIAN tag-length
prefix has zero at offsets 1 through 7; the archive has ASCII `72` at offset 1.
Native and archive preimages therefore cannot coincide, whatever their bodies
or chain/version bindings. Existing native action envelopes beginning
`wrfcoin-sign-v1|` differ at offset 7 (`-` versus the archive's `.`) as well.
The archive payload binds schema version, genesis hash, chain id and network id.

Implementation must exercise the same synthetic 9C-role key on both domains:
native `snapshot_sync_payload.v2` signatures fail archive verification, and
archive signatures fail core4 `verify_domain` for every registered tag/type.
Otherwise-valid target envelopes must reach cryptographic verification; schema
refusal alone is not that control. This test-only namespace reservation neither
authorizes deployed 9C reuse nor introduces a signer, verifier or protocol change.

Owner ruling (2026-10-03): the join-archive signer is a SEPARATE key; it is never the node's 9C key. The
synthetic 9C-role key above exists only so the cross-domain test can show that, even if the two were ever
confused, neither signature verifies in the other domain.

## Findings from this audit

### `FinalitySignal` is unsigned and holds no tag

`consensus::finality::FinalitySignal` briefly implemented `wrfcoin.finality_vote.v1`. It should
never have: it carries no `signature` field (#2226 records it as unsigned) and is an in-process
mpsc notification from the BFT gadget to the chain layer, never a wire object. The tag belongs to
`consensus_engines::selection::bft_finality::FinalityVote`, the live signed type, whose impl must
live in its own crate (#2249).

An unsigned type must not hold a domain tag, because a tag is a claim about what a signature
covers. The source-derived impl scan fails if the impl is reintroduced.

### Two enumerated objects have no type at all

`ValidatorSetChange` (#2227) and `WalletLoginChallenge` (#2228) are objects the protocol signs
conceptually, but no Rust type exists in the workspace to attach a tag to. Their tags are
**reserved** here so that whoever adds the type cannot pick a colliding one.

#### `WalletLoginChallenge` is EVM/secp256k1 today, not an ed25519 spending-key challenge (#2228)

The reserved tag anticipated a native wallet login signed by a user's ed25519 spending key — the
highest-value cross-domain-replay target, because such a signature shares a key and curve with
transfers. Investigation for #2228 found that is **not** how login works: every wallet-login flow is
**EVM secp256k1 / EIP-191 `personal_sign`**, verified via `ethers.verifyMessage` (`ecrecover`). The
backend mounts **two** such challenge flows, over different message strings:

- `/api/auth/nonce/:address` + `/api/auth/wallet-login` (`server/routes/wallet-auth-service.ts`,
  `buildWalletLoginMessage` / `verifyWalletSignature`) — the user signs
  `WRFCoin wallet login nonce: <32-byte hex nonce>`.
- `/api/wallet-auth/nonce/:address` + `/api/wallet-auth/verify` (`server/routes/wallet-auth-routes.js`)
  — the user signs `Wrfcoin Authentication\nWallet: <address>\nNonce: <nonce>`.

The mobile app (`lib/services/api/api_client.dart`) signs the first message; the **frontend also
signs the first message** — `src/services/walletAuthService.ts` (`loginWithSigner`) requests the
nonce, signs `buildWalletLoginMessage(nonce)` through the connected wallet, posts the signature,
and stores/uses the resulting JWT (pinned by `walletAuthService.test.ts`). It adds a **signer** of
the existing challenge, not a third message format. So the exact signed bytes differ by
endpoint, but the property that matters here is uniform: **all** of these are EVM secp256k1 /
EIP-191, a curve and key system **disjoint** from this crate's ed25519 `crypto::signature`. No
EIP-191 login signature can ever verify as an ed25519 `DomainSeparated` object, so the in-Rust
cross-domain-replay surface this tag guards against **does not currently exist**. (Neither EVM
message binds a domain tag, chain id, or expiry — a replay concern within the EVM/EIP-191 context,
but a backend/mobile/frontend matter, not a core4 ed25519 domain-separation gap.)

The row therefore stays `Exception` with `(no type)`: adding an ed25519 `DomainSeparated` type now
would model bytes nothing in production signs. The tag remains reserved so that a *future* native
ed25519 wallet login must adopt this domain-separated shape (and cannot collide), and bringing the
existing EVM login(s) into the audited system — or unifying login onto the ed25519 wallet key — is a
separate cross-repo migration that would flip this row to `Implemented` at that time.

### `GovernanceVotePayload` has no chain or network binding (#2229)

`GovernanceVotePayload::canonical_signing_message` commits to exactly:

```
governance-vote-v1|<proposal_id>|<approve>
```

There is no chain id, no network id, and no signer binding. A governance vote harvested from the
testnet is byte-identical to the same vote on mainnet, so it verifies on both. This is the #2065
shape, in governance rather than attestation. Filed separately as a security finding; the
chain-and-version binding lands with #2123.

**In progress (#2229), version-gated:** the production state applier calls
`verify_vote_signature_at_height(current_block)`, which selects the signing protocol from the
genesis-pinned activation schedule. Verification and signing helpers route through the type's
`DomainSeparated` impl **only at protocol version V2** (active from the #2034 fresh-genesis cut),
where the preimage binds the domain tag, signer, chain id and network id. At **V1** the exact legacy
`governance-vote-v1|<proposal_id>|<approve>` string is still used, so pre-cut signatures keep
verifying and the change is a clean version gate rather than a rolling V1/V2 asymmetry (an
always-domain cutover would, at V1, both reject legacy signatures *and* still omit the chain/network
binding). The row stays `MigrationDeferred` until the cut retires V1; it flips to `Implemented` when
V2 is the only active version.

### Most signed types predate the domain module

Nine of the thirteen rows are `MigrationDeferred`: they have a live signing path that does not go
through domain separation, and changing it invalidates previously produced signatures. They are
listed rather than migrated, per the #2122 acceptance criteria. #2226 tracks the cutover, which
must ride an explicit protocol-version bump — the failure mode in #2076 (a v1 signer against a v2
verifier silently halting the earn loop) is what happens otherwise.

## Adding a new signed object

1. Implement `DomainSeparated` in `shared-protocol/src/crypto/domain_impls.rs`. Build the preimage
   with `domain_preimage(Self::DOMAIN_TAG, &body)` and the `BodyBuilder` — do not hand-roll
   concatenation.
2. Add a variant to `SignedObjectKind` and a row to `SIGNED_OBJECT_REGISTRY`.
3. Add the row to the table above, in the same position.
4. Run `cargo test -p wrfcoin-shared-protocol crypto --lib`.

The preimage must be a deterministic function of the object's value: no wall-clock reads, no
unordered-map iteration (collect into a `BTreeMap` first), and no float arithmetic (`BodyBuilder`
bit-encodes `f64` and normalises NaN).

V2 receipt submission binds explicit genesis, sender account, nonce and canonical batch commitment under `wrfcoin.node_receipt_submission.v1`; offline owner signature, no mint/burn authority.
