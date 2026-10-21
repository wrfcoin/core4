# External security audit — RFP and scope (#2163)

This document is the scope package handed to a candidate audit firm. It exists alongside
#1617 (the original RFP tracking issue — vendor selection, budget, engagement letter) rather
than replacing it: #1617 owns the human-process checklist and firm selection; this document
owns the technical scope a firm needs to price and staff the engagement correctly. Do not
duplicate the process checklist here, and do not let this document drift from #1617 — if the
in-scope crate list changes, update both.

**The core mistake this document exists to prevent**: hiring only a smart-contract auditor.
WRFCoin's core4 repository is a Rust L1 blockchain — consensus, networking, storage and
cryptography — not a Solidity application with a thin backend. A firm whose staff can review
the Solidity in `wrfcoin/smart-contracts` (a separate repository) competently may have no one on the bench who can review a BFT finality
gadget, a libp2p gossip layer, or a Merkle-proof-based sampling scheme. Confirm the specific
named reviewers for each area below before signing, not just the firm's marketing page.

## Required expertise

A firm must answer each question below with a **specific prior engagement**, not a general
capability claim ("we have blockchain experience" is not an answer to any of these).

1. **Rust systems programming.** Which production Rust codebases has the assigned reviewer
   audited, and what class of memory-safety-adjacent or logic bug did they find (Rust's type
   system prevents memory corruption, not integer overflow, unbounded recursion, or a
   TOCTOU race across an `async` yield point)?
2. **BFT consensus protocols.** Has the reviewer formally or informally analyzed a
   Byzantine-fault-tolerant finality gadget (Tendermint-family, HotStuff-family, or a
   from-scratch design) for safety and liveness under the assumptions it claims? Name the
   protocol and the finding, if any.
3. **P2P networking and DoS surface.** Has the reviewer audited a libp2p (or comparable)
   gossip/discovery stack for eclipse attacks, Sybil resistance, or resource-exhaustion via
   unbounded peer-supplied input? Name the codebase.
4. **Applied cryptographic protocol review.** Has the reviewer verified a sigma-protocol or
   zero-knowledge construction's security reduction, checked a Fiat-Shamir transcript's domain
   separation, or reviewed a ring-signature/stealth-address scheme for linkability leaks? Name
   the construction.
5. **Database / storage crash-consistency.** Has the reviewer audited a system's
   checkpoint/WAL or snapshot design for a state a crash mid-write can leave unrecoverable, or
   for a state-root/checksum that can silently diverge from the data it claims to describe?
6. **Token economics and incentive design.** Has the reviewer analyzed an emission schedule,
   reward-distribution formula, or slashing mechanism for a manipulable edge case (integer
   truncation bias, a timing-dependent payout, a Sybil-exploitable reward path)?
7. **Validator / node operations security.** Has the reviewer reviewed key-custody design for
   a validator or block-producer fleet (HSM/PKCS#11 integration, key-rotation procedure,
   multi-party custody), rather than only application-level secrets management? This
   includes **oracle attester key custody**: attester keys live on hardware tokens
   (YubiKey PIV via PKCS#11) or a SoftHSM fallback, and finalization needs a quorum of
   them to be online, so custody and availability are part of liveness, not just secrecy
   (see the Stage 2 "Oracle attestation and quorum" row).

A firm need not staff all seven with a single person, but every area must have a named
reviewer with a specific answer, not a generalist covering all seven by title alone.

## Stage 1 — architecture review (before code freeze)

**Question the reviewer must answer**: *is the protocol safe under the stated Byzantine and
network assumptions?* This is a paper-and-model review, not a code review — it happens before
Stage 2's commit freeze so a design-level finding doesn't require re-reviewing already-frozen
code.

**Deliverables handed to the reviewer**:
- The protocol specification and threat model (`docs/security/PROTOCOL_SPEC.md`,
  `docs/security/THREAT_MODEL.md`).
- The consensus invariant list and any TLA+/formal models from the security-model epic
  (`docs/plans/2026-02-21-consensus-invariants-model-check.md` and any successor artifacts —
  confirm current location with whoever owns the security-model epic before the engagement
  starts, since this is exactly the kind of doc that moves).
- A list of already-known, already-closed findings so the reviewer doesn't re-discover them at
  the firm's hourly rate: #716 (privacy/economic-model audit scope, closed — testnet does not
  require it, mainnet does), #717 (cryptographer review of `range_proof.rs`, `ring_signatures.rs`,
  `stealth_addresses.rs`, `pedersen.rs`; the linked concern about 22 `unwrap()` calls in
  `ring_signatures.rs` was #708, closed).

**Stage 1 does not require a frozen commit** — the specification and models are the review
subject, and they can (and should) still be corrected before Stage 2's freeze if Stage 1 finds
a design flaw.

## Stage 2 — implementation review (against a frozen RC commit)

**Scope, mapped to real crate paths** (verify each path still exists at freeze time — this
repo's layout has moved before, e.g. `core4/src`, `core4/frontend`, `core4/backend` were all
removed after extraction; do not assume this list is current without a `git ls-tree` check on
the freeze commit itself):

| Area | Path(s) |
|---|---|
| Consensus | `consensus-engines/`, `shared-protocol/src/consensus/` (`proof_of_publishing.rs` and siblings) |
| State transition | `shared-protocol/src/blockchain/` (`state.rs`, `transaction.rs`) |
| Networking and serialization | `p2p/`, wire-format code under `native-chain/src/bin/launch_api/p2p_events.rs` |
| Cryptographic wrappers | `shared-protocol/src/crypto/` (signing, domain separation, chain binding) |
| Privacy / ZK | `privacy/` (`range_proof.rs`, `ring_signatures.rs`, `stealth_addresses.rs`, `pedersen.rs`, `key_image_registry.rs`, `mlsag_budget.rs`), `zk-circuits/` |
| Storage | `native-chain/src/storage.rs` and `native-chain/src/storage/` (checkpoint/WAL, snapshot format) |
| Genesis | genesis cut tooling and the genesis bundle format (`docs/security/GENESIS_CUT_RUNBOOK.md`, `docs/security/GENESIS_CUT_PLAN.md` name the current procedure and pins) |
| Validator lifecycle | producer/validator registration, key ceremony and rotation (`docs/security/PRODUCER_CEREMONY_PACKET.md`, `docs/security/KEY_ROTATION_RUNBOOK.md`) |
| Upgrades and governance | the upgrade-notice / activation-schedule machinery (`shared-protocol/src/governance/`, `ConsensusFeature` activation in `shared-protocol/src/crypto/chain_binding.rs`-adjacent modules) |
| RPC | `native-chain/src/bin/launch_api/routes/` and `docs/security/RPC_SURFACE.md` (the authoritative route/method-policy inventory) |
| CLI and key management | `tools/cli/WrfcoinCLI/`, key-file and HSM/PKCS#11 signer code under `shared-protocol/src/crypto/signer/` |
| Oracle attestation and quorum | The path through which contributions actually finalize. The node runs the Proof-of-Publication consensus adapter (`PopConsensusAdapter`, `native-chain/src/bin/launch_api/main.rs`); a contribution reveal is rewarded only once a quorum of registered oracle attesters has attested it. Code: `native-chain/src/bin/launch_api/routes/oracle.rs`, the attester service `native-chain/src/bin/oracle_attester.rs`, `native-chain/src/bin/oracle_key_pop.rs`, `native-chain/src/oracle/`, `shared-protocol/src/oracle/attestation.rs`, `shared-protocol/src/crypto/genesis_oracle.rs`, `oracle-packet/`. Custody and procedure: `docs/security/ORACLE_KEY_CEREMONY.md`, `docs/security/GENESIS_ORACLE_CEREMONY.md`, `docs/oracle-attester-deployment.md`. Review attestation verification, quorum counting and replay, **and** attester key custody and availability (YubiKey/SoftHSM): on 2026-09-22 the testnet stopped finalizing when one attester host lost its hardware tokens and the quorum fell below threshold. That outage is a liveness failure mode the review must cover. |
| Build / release workflows | `.github/workflows/` (release signing, supply-chain, SBOM generation — confirm current names against `.github/workflows/WORKFLOW_JUSTIFICATION.md` at freeze time, since workflows here have been renamed/parked/retired before) |

**Explicitly out of scope for this L1 review** (per #1617, still owned by that issue's process,
not duplicated here): `wrfcoin/smart-contracts` (Solidity, separate repository and audit
track); `wrfcoin/defi-protocols` (a separate repository with 13 Rust workspace crates (12 tested protocol crates plus
a `defi-node` crate), ~19K lines, including a fixed reward-stacking exploit, defi-protocols#47;
in testnet scope only as a read/quote simulator per its ADR 0001, accepted 2026-09-18, so it
needs its own audit track before any live use) and `wrfcoin/storage-providers` (a separate
repository with 7 tested crates: IPFS/Filecoin/Arweave clients, dedup registry, retention
scheduler; own audit track). Neither is empty; both are outside this L1 engagement because they
are separate codebases. The mobile app is a separate engagement,
`backend/`/`frontend/` (separate Node.js services, own security review track).

## Deliverables

- Findings, each with a severity rating and a concrete reproduction (a test case, an attack
  transcript, or a step-by-step trigger condition) — a finding without a reproduction is not
  actionable and should be returned for clarification before the engagement closes.
- A remediation retest: after fixes land, the firm re-verifies each critical/high finding
  against the fix, not just against the firm's own memory of the original report.
- A published report or, if the firm requires confidentiality, a detailed public summary that
  still names every critical/high finding's class and remediation status. Either form **must
  name the exact commit SHA reviewed** — a report that says "the current version of the code"
  becomes unverifiable the moment another commit lands, and an audit an operator cannot map to
  a specific commit is not evidence for a launch decision.

## Freeze procedure

1. Tag the release-candidate commit (`git tag rc-audit-<date> <sha>` or the project's normal
   release-tagging convention — confirm against `docs/security/BUILD_PROVENANCE.md` and
   `RELEASE_SIGNING` tooling once #2151 lands) and record the tag and SHA in this file's PR (or
   a follow-up comment on #2163) so the engagement's scope is traceable from the issue itself.
2. Hand the firm the tagged commit, not a branch name or `main` — a branch can move under the
   auditor without anyone intending it to.
3. **Any change after the freeze is either explicitly excluded from Stage 2's scope, or
   triggers a re-scope** — a firm reviewing commit `X` cannot be assumed to have also reviewed
   whatever landed on `main` after `X` was tagged, even a "trivial" fix. If a post-freeze change
   touches anything in the Stage 2 scope table above, treat it as needing its own review pass
   (either folded into a re-scope of this engagement, or tracked as a follow-up finding against
   the next audit).
4. On completion, post the report (or its public summary) and the reviewed SHA as a comment on
   #1617, and open a tracking issue for every critical/high finding per that issue's own
   acceptance criteria.

## Links

- #1617 — the RFP process tracker (vendor selection, budget, engagement letter); this document
  is its technical scope companion, not a replacement.
- #716, #717 (closed) — prior privacy/economic-model and cryptographer review scoping; the
  concerns they raised inform Stage 2's privacy/ZK scope above.
- #708 (closed) — the `ring_signatures.rs` `unwrap()` finding #717 flagged; resolved, kept here
  as evidence the specific concern was already tracked and closed rather than silently dropped.
- Epic #2158 — [Epic] Staged testnet launch gates & external audit commissioning.
