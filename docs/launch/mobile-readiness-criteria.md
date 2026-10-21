# Mobile Testnet Readiness Criteria

**Issue:** [#1603 — Encode mobile pre-launch readiness criteria for testnet go/no-go](https://github.com/wrfcoin/core4/issues/1603)
**Status:** Criteria documented and gaps tracked (2026-09-12; criterion 2, the settlement note and the re-check procedure updated 2026-09-24); mobile-specific fix work belongs in `wrfcoin/mobile`, not here
**Scope note:** This document defines and tracks the minimum bar. It does not implement any mobile feature.

---

## The three minimum criteria (#1603's acceptance criteria)

| # | Criterion | Current status | Evidence / gap |
|---|---|---|---|
| 1 | Sensor data submit end-to-end (mobile → backend → chain) | **Partially automated** | `wrfcoin/mobile`'s `testnet-smoke.yml` CI (green as of 2026-09-11, run `34645447889`) exercises `integration_test/testnet_smoke_test.dart`'s `weather submission accepts shape-valid payload` case — a real HTTP round trip to the configured backend, shape-checked. It does **not** confirm the submission settles on-chain, and doesn't run on a physical device. **Settlement dependency:** on-chain settlement of a submission needs the oracle quorum, 2 of the 3 registered attesters (only 2 were online as of 2026-09-24). A single attester outage therefore stops finalization, as it did on 2026-09-22, so a green submit round trip never implies settlement. **No live automated evidence newer than 2026-09-11:** every `testnet-smoke` run since then is a 0-step job (hosted Actions minutes exhausted), e.g. run `35960756395` (2026-09-24), job `advisory mobile testnet smoke`, conclusion `failure` with 0 steps. That is VOID, not RED and not GREEN. [`wrfcoin/mobile#44`](https://github.com/wrfcoin/mobile/issues/44) (open) tracks the deeper physical-device round trip: on-device capture, offline queueing, reconnection sync, and confirming the backend actually received and processed the submission. |
| 2 | Wallet send transaction end-to-end | **Partial: unit/widget coverage; no live smoke** | Since the 2026-09-12 snapshot, mobile `main` gained client-signed ed25519 sends and ed25519 login: [`wrfcoin/mobile#287`](https://github.com/wrfcoin/mobile/pull/287) (signs base-unit transfers for `/wallet/send`), [`#290`](https://github.com/wrfcoin/mobile/pull/290) (ed25519 login), [`#284`](https://github.com/wrfcoin/mobile/pull/284) (honest transaction history), all merged 2026-09-24, and [`#293`](https://github.com/wrfcoin/mobile/pull/293) (refuses an expired ed25519 login challenge on the device clock), still open at the time of writing. Unit/widget coverage: `test/features/wallet/data/wallet_send_signed_transfer_test.dart` and `test/features/wallet/presentation/screens/send_screen_signed_send_test.dart`. What is still missing is a live send in the smoke test, which covers wallet **balance fetch** only (`wallet balance fetch returns shape-valid JSON`); [`wrfcoin/mobile#253`](https://github.com/wrfcoin/mobile/issues/253) tracks that. A live send also needs the deployed backend to include `wrfcoin/backend#1189` (client-signed transfers) and `#1208` (ed25519 login), and settles only under the oracle quorum noted in criterion 1. |
| 3 | P2P node handshake visible from mobile client | **Covered, via the right layer for a phone app** | A mobile client does not — and should not — run its own libp2p stack; it consumes the backend/native-chain HTTP API. The smoke test's `node status exposes block height or ready state` case reads `/node/status` (or equivalent) and asserts a height/ready field is present, which is the mobile-appropriate proxy for "the node this app talks to has live chain/network connectivity." No further gap tracked under this criterion unless a reviewer wants literal peer-count visibility surfaced in the app UI — if so, that would be a `wrfcoin/mobile` feature request, not a readiness gap. |

## What "done" means for this issue

Per #1603's own acceptance criteria: a checklist is recorded (this document), every criterion
that isn't currently met has a linked open issue (criterion 2 → `wrfcoin/mobile#253`; criterion
1's deeper validation → `wrfcoin/mobile#44`, already open and unrelated to this pass), and the
issue is closed once that's true — which this document does, rather than re-scoping #1603 into
mobile feature work (explicitly out of scope per the issue's own notes).

## How this feeds the launch gate

`docs/security/TESTNET_GO_NO_GO.md` (#2160/#2165) reads a `mobile-launch-criteria-met` human
attestation gate from `docs/security/attestations.yaml`. Attesting that gate should point its
`evidence_url` at this document (or at the closed #1603) plus confirmation that
`wrfcoin/mobile#44` and `#253` are either closed or judged non-blocking for the launch in
question — a `mobile-launch-criteria-met` attestation should not be filed while a criterion's
linked gap issue is still open and judged blocking.

## Re-checking this later

This table is a snapshot (2026-09-12). Before relying on it for an actual launch decision,
re-run the searches it's built from rather than trusting the table by itself:

```bash
gh issue view 44 --repo wrfcoin/mobile --json state
gh issue view 253 --repo wrfcoin/mobile --json state
gh pr view 293 --repo wrfcoin/mobile --json state
# Latest smoke run, then its jobs WITH step counts: the conclusion alone is not evidence.
RUN=$(gh run list --repo wrfcoin/mobile --workflow testnet-smoke.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh api repos/wrfcoin/mobile/actions/runs/$RUN/jobs --jq '.jobs[] | "\(.id) \(.name) \(.conclusion) steps=\(.steps|length)"'
# For a single job: gh api repos/wrfcoin/mobile/actions/jobs/<job-id> --jq '.steps|length'
```

Read the result by step count, not conclusion. A job with **0 steps** never ran (no runner
or no hosted minutes). It is **VOID**: no evidence either way, whatever its conclusion says.
It is not a RED failure and not a GREEN pass. Only a job with a nonzero step count and
conclusion `success` is live evidence for criterion 1; a nonzero-step `failure` is a real RED.
