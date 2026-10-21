# Testnet go/no-go gates (#2159, #2160, #2165)

This checklist is the authoritative launch decision. `python3 scripts/testnet_gate_check.py`
automates the gate ids it knows, but its `ready` result cannot waive a row in this document.
If a listed gate is absent from the script, has no evidence, or cannot be evaluated, the launch
decision is **NO-GO** until the gap is implemented or explicitly resolved by a reviewed change.

For every gate registered in the script, the machine/human split is enforced in code. A machine
gate is derived from live evidence (an API call, a subprocess exit code, or file content). A human
gate is read only from `docs/security/attestations.yaml`; the script never creates an attestation
or infers one from a favourable machine signal. A checklist row not yet registered in the script
remains NO-GO, as described below.

## Launch review status — 2026-10-19

This is a requirements checklist, **not a current passing result**. Live gate evidence and human attestations have not been refreshed by this documentation change. Missing or stale evidence remains NO-GO under the rules below.

Two historical implementation notes in the machine table are stale: the identity checker and provenance verifier now exist on main. Their presence is not proof that a launch run passed. The binary-signing row remains required for any published binary; signed downloads are not yet published and signing is planned after launch. Do not infer a signed release from a source launch.

## Machine-checkable gates

Run the full registered set with `python3 scripts/testnet_gate_check.py --pretty`. The more
specific proving command or CI job below is authoritative for each row; a missing or stale run
is not a pass.

| gate id | required evidence | proving command or CI job |
|---|---|---|
| `threat-model-current` | `docs/security/THREAT_MODEL.md` exists and was reviewed within the configured freshness ceiling | `python3 scripts/testnet_gate_check.py --pretty` (`threat-model-current`) |
| `protocol-spec-current` | `docs/security/PROTOCOL_SPEC.md` exists and was reviewed within the configured freshness ceiling | `python3 scripts/testnet_gate_check.py --pretty` (`protocol-spec-current`) |
| `consensus-invariants-green` | consensus invariants are documented and the invariant harness detects planted violations | `cargo test --locked -p wrfcoin-shared-protocol --test consensus_invariants -- --nocapture`; CI: `rust-shared-native-parity.yml` / `Shared invariants gate` ([#720](https://github.com/wrfcoin/core4/issues/720)) |
| `no-open-critical-issues` | no open `protocol-critical`, `high-priority`, or `priority-high` issue in `wrfcoin/core4` | `python3 scripts/testnet_gate_check.py --pretty` (`no-open-critical-issues`) |
| `fuzz-campaign-green` | transaction and P2P parser targets are present and the latest full campaign on `main` succeeded | `python3 scripts/testnet_gate_check.py --pretty` (`fuzz-campaign-green`); CI: `fuzz-nightly.yml` / `fuzz` |
| `panic-policy-clean` | no new unjustified panic reachable from attacker-controlled input | `python3 scripts/check_no_panic_on_input.py`; CI: `input-panic-lint.yml` / `input-panic-lint` |
| `determinism-scan-clean` | no new unjustified nondeterministic construct in a deterministic path | `python3 scripts/check_determinism.py` |
| `supply-chain-clean` | the dependency audit succeeds, or every exception is linked, justified, and unexpired | `python3 scripts/testnet_gate_check.py --pretty` (`supply-chain-clean`); CI: `supply-chain-security.yml` / `Supply Chain Security Gate` |
| `testnet-identity-not-mainnet` | resolved testnet `chain_id`, `network_id`, genesis hash, network magic, and signing keys are distinct from mainnet | `python3 scripts/check_network_isolation.py`; **Historical note (stale):** #2161 now supplies that checker. The older command `python3 scripts/testnet_gate_check.py --chain-id "$WRFCOIN_CHAIN_ID" --network-id "$WRFCOIN_NETWORK_ID" --genesis-hash "$WRFCOIN_GENESIS_HASH" --pretty` provides only partial evidence; missing full identity evidence remains NO-GO |
| `faucet-rate-limits-active` | an over-limit request to the deployed faucet is rejected | `python3 scripts/testnet_gate_check.py --faucet-url "$TESTNET_FAUCET_URL" --pretty` (`faucet-rpc-limit-rejects-over-limit`) |
| `public-rpc-limits-active` | an over-limit request to the deployed public RPC is rejected | `python3 scripts/testnet_gate_check.py --rpc-url "$TESTNET_RPC_URL" --pretty` (`faucet-rpc-limit-rejects-over-limit`); run separately from the faucet probe |
| `release-signed-and-provenance-verifiable` | every published binary has a valid signature and independently verifiable provenance | `bash scripts/verify_release_signatures.sh artifacts`, then `bash scripts/verify_provenance.sh <artifact> <artifact.provenance.json>` for every artifact; CI: `release-signing.yml` / `verify-signatures`. **Historical note (stale):** the provenance verifier now exists. GPG-only signing remains **not** sufficient; fresh verification of each published artifact is required. |

The identity row includes the isolated genesis/key work tracked by [#721](https://github.com/wrfcoin/core4/issues/721).
Oracle operator trust bootstrap remains a launch gate under [#724](https://github.com/wrfcoin/core4/issues/724);
it is NO-GO while that issue is open. These gates complement, rather than replace, the broader launch
readiness work in [epic #1736](https://github.com/wrfcoin/core4/issues/1736).

## Human-attested gates

Every row requires `attested_by`, `attested_at`, and a concrete `evidence_url` in
`docs/security/attestations.yaml`. A role name, team name, or checkbox without a named person and ISO
date is not an attestation. A stale attestation must be renewed.

| gate id | required attestation |
|---|---|
| `monitoring-alerting-launch-critical` | a named operator confirms dashboards and alerts cover finality, peer count, block production, RPC error/rate-limit signals, disk, and process liveness |
| `incident-response-plan-reviewed` | the named on-call confirms the escalation path and `INCIDENT_RESPONSE_PLAN.md` runbook were rehearsed against the launch topology |
| `private-security-channel-monitored` | the named mailbox owner confirms the private channel in `SECURITY.md` is monitored and a test report received a response |
| `reset-disclaimer-published` | a named release owner confirms the chain-may-reset/no-balance-migration statement is visible on every user-facing surface |
| `disaster-recovery-runbook-reviewed` | a named operator reviewed `DISASTER_RECOVERY.md` against the deployed backup/restore path |
| `key-ceremony-witnessed` | a named independent witness attests that the testnet ceremony in [#721](https://github.com/wrfcoin/core4/issues/721) used isolated testnet keys |
| `oracle-bootstrap-reviewed` | a named security reviewer confirms the operator onboarding and data-verification controls from [#724](https://github.com/wrfcoin/core4/issues/724) are complete |
| `mobile-launch-criteria-met` | a named release owner confirms the mobile criteria (#1603) are met or every remaining gap has a linked blocking issue |

The current gate script predates several ids above. Until it registers them, their absence from script
output is not a pass; the signed checklist remains NO-GO.

## Must NOT be attached

These are hard prohibitions, not risk-acceptance checkboxes.

| prohibited attachment or claim | verification note |
|---|---|
| Real monetary value | Review faucet, wallet, explorer, and launch copy for sale, redemption, price, or value promises; any match blocks launch. |
| Production bridges | Inventory enabled bridge endpoints and contracts; every production/mainnet route must be absent or disabled. |
| Exchange listings | Obtain a named release-owner attestation that no exchange listing, market-making, or liquidity agreement exists. |
| Mainnet validator keys | Compare testnet ceremony fingerprints and HSM slots with the mainnet custody inventory; any reuse blocks launch. Never publish private key material as evidence. |
| Irreplaceable NFTs or assets | Review user-facing mint/import paths and terms; only explicitly disposable test assets may be accepted. |
| Any claim that testnet balances will migrate | Search README, SECURITY, wallet, explorer, faucet, docs, and announcements; the required statement is that balances **will not** migrate. |

## Incentivized testnet gate (#2165)

Run: `python3 scripts/testnet_gate_check.py --gate-set incentivized`

Strictly additive over the launch gate above — nothing in it is removed or weakened. Before
real incentives are attached, every gate above must still pass, **plus**:

| gate id | kind | what it checks |
|---|---|---|
| `external-audit-remediated-and-retested` | human | external audit complete, all critical/high findings remediated and retested — evidence is the report commit SHA and the remediation PR(s) |
| `public-bug-bounty-live` | human | public bug bounty live, with scope, severity rubric and payout table published |
| `extended-fuzzing-campaign-quantified` | human | extended fuzzing campaign completed — evidence states the CPU-hours run, not just "we ran fuzzing" |
| `validator-key-rotation-exercised` | human | a validator key-rotation exercise was performed on the live testnet using the crypto epic's runbook |
| `multiparty-upgrade-key-custody-in-place` | human | multi-party custody of upgrade keys is in place — the gate item that makes the TUF threshold design from the release epic real rather than nominal |
| `disaster-recovery-exercise-timed` | human | a full disaster-recovery exercise (restore from backups/snapshots) was completed and timed, with the result recorded |
| `adversarial-validator-ran-against-network` | human | at least one deliberately adversarial validator implementation has run **against the live network**, not only tested in isolation |
| `extended-run-without-unplanned-reset` | human | the network has run for the required duration (state the duration in the attestation's evidence) without an unplanned reset |

## Attesting a human gate

Add an entry to `docs/security/attestations.yaml`:

```yaml
- gate_id: incident-response-plan-reviewed
  attested_by: <your name or handle>
  attested_at: "2026-09-12"
  evidence_url: "<link to what you reviewed>"
```

An attestation older than 30 days (`--max-attestation-age-days` to change) reads `stale`, not
`pass`. A malformed entry (a missing required field, or a repeated `gate_id`) makes **every**
human gate `missing`, not just the broken one — a partially-unreadable attestation file is
never partially trusted. Re-run the script after editing to confirm the entry parsed.

## Wiring — read this before citing either workflow as coverage

- **`ci-readiness.yml`** runs this script as a **non-blocking report** step. That workflow's own
  header banner already records that it is parked (`workflow_dispatch` only — the `schedule` and
  `pull_request` triggers are both commented out per the 2026-07-31 Actions-budget decision), so
  today this report only appears when someone manually dispatches the workflow. It is not a gate
  a pull request goes through.
- **`deploy-gate.yml`** references this script's output in its summary. That workflow's own header
  banner already records that it gates **no actual deployment** (zero runs in its lifetime,
  `deploy-decision` accepts every job as `skipped`) — the real testnet deploy path is a local
  build pushed by SSH to the N5 host (`docs/security/EMERGENCY_RUNBOOK.md`), which this workflow
  does not touch. Referencing the gate script here makes the *readiness report* visible next to
  the workflow named for deployment; it does not make this workflow a deployment gate. Do not
  cite this wiring as deployment coverage — cite the script's own exit code, run directly against
  the target before a launch decision.
