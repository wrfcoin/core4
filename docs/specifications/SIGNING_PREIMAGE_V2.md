# Signing Preimage v2 — chain-id / network-id domain separation (#2034)

> **Status: SUPERSEDED as the normative record (Round 90 cut prep).**
> The normative artifact is now the **executable golden-vector fixture**
> `shared-protocol/tests/fixtures/golden/signing_preimage_v2.json`, generated and
> byte-verified by `shared-protocol/tests/signing_preimage_v2_golden.rs` over the
> `GENESIS_CUT_PLAN.md` §3.1 composed frame, one case + one tampered twin for
> **every** `MigrationDeferred{#2226}` registry row (9 composed-frame types,
> ComputeReceipt and ForecastVerificationReceipt included) **plus** first-class
> `money_path_v2` cases for the live authorization-message signers (commit,
> transparent-transfer, gamification, oracle-registration — each under its own
> domain tag). Prose — this file included — is descriptive; **the pinned bytes
> win**. If prose and fixture disagree, file a bug against the prose. Fixture
> `schema` is bumped on any deliberate re-derive (now `…golden/2` after #2371
> folded `producer_pubkey` into the BlockHeader case).
>
> §"Pre-cut money-path preimages" below remains the accurate record of what live
> signers (faucet, collector, SDK) produce **today**: the #2064 hand-rolled `_v2`
> constructions. Those collapse into the registry tags at the #2034 cut
> (GENESIS_CUT_PLAN.md §3.3(a)); from the cut onward the fixture's frame is the
> only preimage construction.

## Required verification command

The gate is the three golden suites, targeted explicitly:

```bash
cargo test -p wrfcoin-shared-protocol \
  --test signing_preimage_v2_golden \
  --test signed_type_inventory \
  --test cross_domain_replay
```

They must all be green. The bare package command (`cargo test -p
wrfcoin-shared-protocol`) is **not** the gate: it currently dies on the
pre-existing `earn_loop_oracle_gate_e2e` failure (2 tests, reproduces on clean
`main` — tracked separately, unrelated to signing preimages) before Cargo
launches these binaries. Target the three suites directly until that e2e is
fixed.

## The golden-vector contract (normative)

Every consumer, per case, must: rebuild the object from the case's `input` JSON,
reproduce `canonical_body_hex`, `preimage_v1_hex`, `preimage_v2_hex` and
`signature_v2_hex` **byte-for-byte** (ed25519 is RFC-8032 deterministic; keys
derive from the pinned `signer_seed`), verify the signature — and rebuild from
`tampered_input`, obtaining a **different** preimage and a **failed**
verification under the golden signature. The tampered half is what makes an
echo-the-expected-values harness impossible.

Regeneration is a consensus decision:
`WRFCOIN_REGEN_GOLDEN_VECTORS=1 cargo test -p wrfcoin-shared-protocol --test
signing_preimage_v2_golden`, review every changed byte, bump the fixture
`schema` together with the change that moved them (e.g. #2371 re-derives the
BlockHeader case when `producer_pubkey` joins the body).

## Four-consumer test plan (cross-repo; Elsa/Frank/Dan wire the non-Rust three)

| # | Consumer | What it must assert against the fixture |
|---|----------|------------------------------------------|
| 1 | **core4 Rust** — `signing_preimage_v2_golden.rs` (**live now**) | Full contract, all cases; plus `signed_type_inventory.rs` recomputes the pinned bytes per row (`every_2226_migration_row_produces_its_pinned_golden_bytes`) and per authorization-message surface (`every_authorization_message_surface_has_byte_pinned_golden_coverage`), so byte drift fails the audit, not only the fixture binary |
| 2 | **infra faucet** (`signTransferAuthorization`) | **Live now:** the fixture's dedicated `transparent_transfer_authorization_v2` case (`frame: "money_path_v2"`) — recompute preimage + signature from `input` with `TRANSPARENT_TRANSFER_AUTH_DOMAIN` + u32-LE lengths, byte-for-byte, plus the tampered half. Cut-ready (gated on the cut): the composed-frame `Transaction` case |
| 3 | **infra collector** (commit-authorization signer — Dan's new test) | **Live now:** the dedicated `commit_authorization_v2` case (`frame: "money_path_v2"`, fields chain_id/network_id/station_id/commit_id/contribution_type) — byte-for-byte + tampered half; the collector's v2→v1 fallback keeps its own existing coverage. Cut-ready: composed-frame `Transaction` (+`OracleAttestation` when the collector signs them) |
| 4 | **core4 JS SDK** (`sdks/extensions/javascript` — `canonicalSigningMessage`, `compute-receipt` signer) | Cut-ready test: recompute the `ComputeReceipt` case (the SDK's own signed type) + at least one other case byte-for-byte; the JS `canonicalSigningMessage` must emit the §3.1 frame exactly |

Rules for consumers 2–4: read the fixture **from core4** (vendored copy pinned
by hash, or repo checkout) — never re-declare expected bytes locally; implement
both halves (golden + tampered); fail loudly on a fixture `schema` they do not
recognize, so a re-derive forces every consumer to re-look.

### Why `signer_secret_hex` is acknowledged in `.secrets.baseline`

`shared-protocol/tests/fixtures/golden/signing_preimage_v2.json` carries
**thirteen** `signer_secret_hex` values — one per case, every case in the
fixture. Secret scanning flags each twice (`Hex High Entropy String` and
`Secret Keyword`), and those 26 hits are **acknowledged in `.secrets.baseline`,
not suppressed as false positives.** A field named `signer_secret_hex` holding a
64-hex ed25519 key is a *true* positive; calling it anything else would teach
the gate the one lesson it must never learn.

#### The thirteen fields and who actually consumes each

| Fixture line | `kind` | Frame | Rust harness | Re-signs in JS |
|---|---|---|---|---|
| 73 | `Transaction` | composed | `common/mod.rs:84` | — |
| 232 | `BlockHeader` | composed | `common/mod.rs:84` | — |
| 370 | `SlashingEvidence` | composed | `common/mod.rs:84` | — |
| 430 | `SnapshotPayload` | composed | `common/mod.rs:84` | — |
| 458 | `OracleAttestation` | composed | `common/mod.rs:84` | — |
| 489 | `AttestationEnvelope` | composed | `common/mod.rs:84` | — |
| 525 | `BridgeMessage` | composed | `common/mod.rs:84` | — |
| 560 | `ComputeReceipt` | composed | `common/mod.rs:84` | **yes** — spec:140 |
| 594 | `ForecastVerificationReceipt` | composed | `common/mod.rs:84` | **yes** — spec:140 |
| 625 | `commit_authorization_v2` | `money_path_v2` | `common/mod.rs:197` | — |
| 651 | `transparent_transfer_authorization_v2` | `money_path_v2` | `common/mod.rs:197` | — |
| 677 | `gamification_authorization_v1` | `money_path_v2` | `common/mod.rs:197` | — |
| 708 | `oracle_registration_authorization_v1` | `money_path_v2` | `common/mod.rs:197` | — |

**All thirteen** are consumed by Rust — the nine composed-frame cases through
`check_case` (`common/mod.rs:84`), the four money-path cases through
`check_money_path_case` (`common/mod.rs:197`); both assert `sk.to_hex()` still
equals the pinned value.

**Exactly two** are consumed by a non-Rust signer.
`sdks/extensions/javascript/__tests__/signing-preimage-v2-parity.spec.ts`
filters the fixture to `ComputeReceipt` and `ForecastVerificationReceipt`
(`isTargetCase`, lines 60–64), so its re-sign at line 140 reads **only lines 560
and 594**. It does not read the other eleven. Do not attribute those to the JS
suite.

**Two more are consumed outside this repo, and all thirteen are duplicated
there.** `infra` vendors the whole fixture at
`infra/testnet/fixtures/core4/signing_preimage_v2.json` (schema
`wrfcoin.signing-preimage-v2.golden/2`); all 13 `signer_secret_hex` values in
that copy are **byte-identical** to the ones here. Two are live test inputs:

| core4 line | `kind` | infra consumer |
|---|---|---|
| 625 | `commit_authorization_v2` | `infra/testnet/services/synthetic-collector/test-commit-authorization-golden.cjs:16,29` — filters to this `kind`, then re-signs from `signer_secret_hex` |
| 651 | `transparent_transfer_authorization_v2` | `infra/testnet/services/faucet/signing-golden.test.js:13,25,71` — asserts the field is a raw 64-hex ed25519 seed, then signs with it |

So rows 2 and 3 of the four-consumer plan above are **live for this field**, not
cut-ready: the faucet and collector golden tests are real non-Rust signers
consuming these keys today. (An earlier revision of this section claimed zero
outside consumers and no `infra` reference at all. That was wrong — it was
measured against a stale local `infra` checkout rather than `infra` `main`.)

Counted by *distinct signer implementation*, the field therefore has four live
consumers: Rust (all 13), the core4 JS SDK (560/594), the infra collector (625),
and the infra faucet (651).

#### Why they are acknowledged rather than removed

* **Synthetic and deterministic.** Each is generated from the adjacent
  `signer_seed` via `common::keypair()` (`StdRng::seed_from_u64`), and the
  harnesses above re-derive and assert it, so drift is a test failure.
* **Never used anywhere real.** No live network, no ceremony, no HSM. Not
  reachable from any production code path — the fixture is `tests/` only. Within
  **this repo**, all thirteen values occur nowhere but this fixture; they are
  also present, byte-identical, in the vendored `infra` copy named above, which
  is likewise test-only. Scope any "occurs nowhere else" claim to a named repo:
  an unscoped one is false across the vendoring boundary.
* **A uniform, version-stable cross-language interface — this is the real
  trade.** `signer_secret_hex` is the raw 32-byte ed25519 seed (RFC 8032),
  directly usable by Node crypto or any other language's ed25519. Deriving from
  `signer_seed` instead is *reproducible* outside Rust — at the currently locked
  `rand 0.8.6` / `rand_core 0.6.4`, `StdRng` is `ChaCha12Rng` and
  `seed_from_u64` is a specified expansion, and all 13 keys have been
  reproduced from their seeds in Node. But `StdRng` is **explicitly not a stable
  reproducibility contract**: `rand` reserves the right to change the underlying
  algorithm across releases, and this workspace's lockfile already carries
  `rand` 0.8.6, 0.9.4 and 0.10.1 side by side. A non-Rust consumer deriving from
  the seed would be re-implementing a pinned internal that is allowed to move
  under it. Carrying the seed bytes keeps the fixture portable and stable across
  `rand` upgrades, and carrying them on *every* case — not just the two the JS
  suite currently signs — is what keeps the interface uniform as consumers 2–4
  come online.
* **Removal would fail loudly, not silently.** Recorded because an earlier
  revision of this section claimed the opposite: deleting a key breaks its
  consumers noisily. Dropping the line-73 `Transaction` key fails the Rust
  golden test at `common/mod.rs:84`; dropping the line-560 `ComputeReceipt` key
  fails JS at spec line 140 (`hex string expected, got undefined`). So keeping
  these thirteen is a **deliberate choice for portability and uniformity**, not
  protection against a silent loss of coverage. Removing them is a legitimate
  alternative; it costs the JS suite its signer coverage and costs the fixture
  its version-stable portable interface, and it must be done by deleting the
  field and its consumers — never by renaming it to dodge the detector.
* **An inline `pragma: allowlist secret` is not sufficient** — measured: it
  suppresses the entropy hit but the `Secret Keyword` hit still fires, so the
  baseline entry is the only complete instrument available.

**If you regenerate this fixture, all thirteen sets of baseline entries move
with it.** Keep them acknowledged, keep this table current — including the
consumer columns — and do not rename the field to dodge the detector.

`shared-protocol/src/blockchain/transaction.rs` remains the **source of truth
for the pre-cut money-path preimages** documented below; if Rust and this
document ever disagree, **Rust wins** — file a bug against this doc.

## Why

Before #2034 the value-transfer preimages carried a static domain tag but **no
network discriminator**, so a signature produced on testnet was byte-identical
to a mainnet authorization for the same key/nonce (EIP-155 replay class). v2
folds the runtime chain identity `(chain_id, network_id)` into every money-path
preimage so a signature is bound to exactly one network.

**Activation = hard cut at a fresh genesis** (no dual-verify ramp). Every signer
and verifier must emit/accept v2 only; the testnet is relaunched fresh so no v1
history needs to verify. A v1 and a v2 signer cannot interoperate — coordinate
the cutover at relaunch.

## Chain identity source

| Field | Env var | Fallback (mainnet) | Testnet value |
|-------|---------|--------------------|---------------|
| `chain_id`   | `WRFCOIN_CHAIN_ID`   | `wrfcoin-main` | `777` |
| `network_id` | `WRFCOIN_NETWORK_ID` | `mainnet`      | `testnet` |

Resolved by `crypto::domain_signing::signing_chain_id()` /
`signing_network_id()`. These are the **same** env vars the p2p handshake and the
#2035 genesis boot assertion read — one identity across sign, verify, and peer.
An empty/unset var falls back to the mainnet constant. Every signer MUST set both
vars to the deployment's values.

## Common encoding primitives

- Integers: little-endian.
- `str(s)`: `u32_le(len(s_bytes))` ‖ `s_bytes` (UTF-8).
- `chain_domain`: `str(chain_id)` ‖ `str(network_id)` — folded immediately after
  the domain tag in every preimage (`push_chain_domain`).

## Pre-cut money-path preimages (live until the #2226 collapse at the cut)

Each is the exact byte sequence that is ed25519-signed (the `*_with_domain`
functions in Rust build these; the public wrappers resolve the env identity and
delegate).

### 1. Value-transfer sighash — `Transaction::signing_hash`
Tag `wrfcoin_tx_sighash_v2` (raw ASCII, no length prefix) ‖ `chain_domain` ‖
`str(tx_type)` ‖ `u32_le(version)`… (full body unchanged from v1; see Rust). The
signed message is `hash_bytes(preimage)` (SHA3-256), then ed25519 over that hash.

### 2. Transparent transfer — `transparent_transfer_authorization_message`
`b"wrfcoin_transparent_transfer_auth_v2"` ‖ `chain_domain` ‖ `str(from)` ‖
`str(to)` ‖ `u64_le(amount)` ‖ `u64_le(nonce)`. Signed directly (no pre-hash).

### 3. Stake / unstake — `stake_authorization_message`
`b"wrfcoin_stake_auth_v2"` ‖ `chain_domain` ‖ `str(account)` ‖ `u64_le(amount)` ‖
`u8(is_unstake)` ‖ `u64_le(nonce)`.

### 4. Contribution commit — `commit_authorization_message`
`b"wrfcoin_commit_auth_v2"` ‖ `chain_domain` ‖ `str(station_id)` ‖
`str(commit_id)` ‖ `str(contribution_type)`.

## Golden vectors

Reproduce these exactly. See the `*_2034` tests in `transaction.rs`.

**Transparent transfer**, tuple `chain_id="777"`, `network_id="testnet"`,
`from="wrf_from"`, `to="wrf_to"`, `amount=1000`, `nonce=5`:

```
777266636f696e5f7472616e73706172656e745f7472616e736665725f617574685f7632
0300000037373707000000746573746e6574080000007772665f66726f6d060000007772
665f746fe8030000000000000500000000000000
```
(concatenate; whitespace here is for wrapping only). Decoded:
`"wrfcoin_transparent_transfer_auth_v2"` ‖ `03000000 373737` (`"777"`) ‖
`07000000 746573746e6574` (`"testnet"`) ‖ `08000000 7772665f66726f6d`
(`"wrf_from"`) ‖ `06000000 7772665f746f` (`"wrf_to"`) ‖ `e803000000000000`
(1000) ‖ `0500000000000000` (5).

## Cross-repo signer checklist (must land before relaunch)

- [ ] **core4** (Rust) — this PR (source of truth).
- [ ] **infra collector** — commit/reveal signer: adopt `commit_auth_v2` +
      `chain_domain`; set `WRFCOIN_CHAIN_ID=777` / `WRFCOIN_NETWORK_ID=testnet`.
- [ ] **backend faucet / wallet** — transparent-transfer signer: adopt
      `transparent_transfer_auth_v2` + `chain_domain`; validate against the
      golden vector.
- [ ] **TS/SDK signers** — mirror all four preimages; add a golden-vector test.

## Follow-up (NOT in this PR)

- Attestation envelope (`domain_signing.rs`) callers still hardwire
  `DEFAULT_CHAIN_ID`/`DEFAULT_NETWORK_ID` to mainnet across ~9 crates
  (oracle-packet, sensor-attestation, keeper-proof, forecast-score, p2p/node,
  snapshot_sync, security/slashing, transaction.rs attestation paths). Swap those
  to `signing_chain_id()`/`signing_network_id()` — tracked as a #2034 follow-up.
- Fold a network magic into the **block hash** preimage (issue #2034 3rd bullet)
  — separate, heavier fork; split to its own issue.
