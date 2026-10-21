# Published Project Wycheproof corpus — vendored

Closes the blocker in **#2264**. These are the real published vectors that **#2126** asked for.
Vendored rather than fetched at test time because a network-dependent crypto test is a flaky crypto
test.

> **Convention for this file.** Every file this document names is either **in this tree** or **in
> open PR #2236**, and each mention says which. Nothing here asserts that a file exists without
> naming where. This rule exists because three separate sentences in earlier revisions described
> #2236's contents in the present tense as facts about this tree — see "What earlier revisions got
> wrong" at the end.

## Source and pin

- Repository: <https://github.com/C2SP/wycheproof> (Project Wycheproof, now under C2SP)
- Commit: **`b61843a9a5115bb758134b6a1f5d5e502d445342`** (2026-07-21)
- Path upstream: `testvectors_v1/`
- License: **Apache-2.0** — full text in `LICENSE` alongside these files

## Files

| File | Algorithm | Tests | Schema | SHA-256 |
|---|---|---|---|---|
| `ed25519_test.json` | EdDSA | **150** | `eddsa_verify_schema_v1.json` | `70471c053c711731f2195ef4875b60ea7f5d6793939d99058ac12da810cb8e00` |
| `ecdsa_secp256k1_sha256_test.json` | ECDSA | **476** | `ecdsa_verify_schema_v1.json` | `43db761c0a2eae71fb0755d355d5130e28ce64a5b07846cf27e7072082597a81` |
| `ecdsa_secp256k1_sha3_256_test.json` | ECDSA | **474** | `ecdsa_verify_schema_v1.json` | `4748f02694d2391576cf28c8cb3d13149739b5602eeb079deae4577e111afbc1` |
| `ecdsa_secp256r1_sha256_p1363_test.json` | ECDSA P-256 P1363 | **262** | `ecdsa_p1363_verify_schema_v1.json` | `c60de693930e386c3a5472d08081623ef8504decc54b38ac01ec6b2a2575c986` |

1,362 vectors, carrying 10 distinct flags for Ed25519, 17 for each secp256k1
ECDSA set, and 13 for the P-256 P1363 set.

Re-verify at any time:

```bash
sha256sum shared-protocol/tests/fixtures/wycheproof/upstream/*.json
```

## Why this matters — and what it complements

**Nothing described in this section exists in this tree.** Neither this head nor `main` contains a
Wycheproof harness or a local fixture; both live in **open PR #2236**, which is not merged. An
earlier draft of this file described that PR's contents in the present tense as though they were
tree facts — they were not, and the paths it named were already stale. This paragraph is a
description of an open proposal, and is marked as such so it cannot be misread again.

**In PR #2236 as of its current head**, the harness ships a locally-generated corpus at
`fixtures/wycheproof/local/ed25519_project_controls.json` — 16 Ed25519 cases produced with
`python-cryptography` plus nine NIST hash cases. (An earlier revision of that PR used
`fixtures/wycheproof/ed25519.json` with a sibling README; both are gone. Re-read #2236's live head
rather than trusting this sentence.)

That substitute is not equivalent to the published corpus, and the difference is the entire point
of Wycheproof. A locally-generated corpus exercises the cases its author thought to construct.
Wycheproof's value is the cases nobody thinks of — vectors curated from real CVEs and real
implementation bugs across many libraries. Those are precisely the blind spots a self-authored
corpus shares with the code it is testing.

The local controls should be **kept**, not deleted, if and when #2236 lands: their
`malleable_S_plus_L_*` and `identity_public_key` cases are targeted at WRFCoin's `verify_strict`
behaviour (#2043) and were built deliberately. They are a complement, not a competitor.

## Wiring status (updated for #2264 — supersedes "Not yet wired" below-the-line history)

**`ed25519_test.json` is wired.** `shared-protocol/tests/wycheproof.rs` (the #2236 harness, which
merged with the upstream-schema parsing built in, activating when these files landed) drives all
150 vectors through the **project wrappers** — `PublicKey::verify_signature` AND the typed
`Signature`/`verify` path, never `ed25519-dalek` directly — honouring `result` (with `acceptable`
routed through `ACCEPTABLE_POLICY`) and reporting per-file totals. As of #2264's closing change,
the run is **reconciled against this file's own `numberOfTests` in both directions** (declared
vs. groups carried; declared vs. actually executed), so a re-pin whose schema drift silently
drops groups fails loudly instead of shrinking the pass count, and failure output carries
`tcId` + upstream `flags` for traceability. The coverage ledger
(`unsupported_classes()`) computes its rows from what is on disk, enforced by
`coverage_ledger_is_complete_and_reconciles`.

Still true, and still open:

1. The two secp256k1 sets remain **data only**: `shared-protocol/src` has no secp256k1/k256
   signature-verification wrapper to drive them through, and the story forbids testing the
   upstream crate directly. The `k256/ECDSA` row stays in the ledger until a project wrapper
   exists (core4#2126).
2. These three sets contain **zero `acceptable` cases** (ed25519 88/62, secp256k1-sha256
   168/308, secp256k1-sha3-256 172/302), so `ACCEPTABLE_POLICY` is routed but not exercised by
   any vector. Vendoring an upstream set that carries `acceptable` is what would close that
   ledger row.

## What earlier revisions got wrong

This file was rejected in review four times, three of them for the same defect: describing files
from **unmerged PR #2236** in the present tense as though they existed in this tree. Recorded here
rather than quietly corrected, because a provenance document's only value is being trustworthy about
what it does and does not establish, and a reader deserves to know its history of not being.

| Revision | Claim | Reality |
|---|---|---|
| 1 | These sets exercise `ACCEPTABLE_POLICY` | They contain **zero** `acceptable` cases |
| 1 | Vendoring retires `k256` from `UNSUPPORTED_CLASSES` | No secp256k1 wrapper exists in `shared-protocol/src`; only `ed25519_test.json` is wirable |
| 3 | The scan and its printed help text "agree exactly" | The scan carried `--exclude-files '\.git/'`; the help text did not |
| 4 | "The harness currently ships `../ed25519.json`… Its own README says" | No harness, fixture or README exists here or on `main`; the path was stale against #2236 too |
| 4 | "The harness at `shared-protocol/tests/wycheproof.rs`" | That file does not exist |
| 5 | "for the reason the sibling README already gives" | There is no sibling README anywhere under `shared-protocol/tests/fixtures/` |

The pins, checksums, vector totals, result distributions and flag counts have reconciled
independently in **every** round. The defect was never the data — it was describing *surrounding
context* from memory while verifying only what had just changed. Each fix corrected one sentence,
which is why it took three rounds to find three sentences.

The convention stated at the top of this file is the structural fix: name where, or do not name.
