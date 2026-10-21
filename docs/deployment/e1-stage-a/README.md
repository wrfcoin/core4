# E1 Stage A — DRAFT owner preparation

Refs #3214, Refs #3277. LEAD Round15 narrows Stage A to
`station_owner_slash_identity` alone, sequence **2**, after GO and the completed
E0/E0f activation checks. This packet contains no signed or submit-ready artifact.
It does not authorize signing, submission, a genesis edit or a deployment.

## Read-only owner audit

Run `audit_owners.py` independently against each participating node after checking
its reviewed binary and common-height chain identity. Supply the node's existing
Operator credential on stdin through the owner's credential source:

```sh
python3 docs/deployment/e1-stage-a/audit_owners.py --base "$E1_NODE_BASE" < "$E1_OPERATOR_TOKEN_FILE"
```

The node base is an explicit HTTPS origin; HTTP is accepted for loopback tests.
The helper sends only GET, disables redirects and proxies, and never retries.

**Read budget (owner ruling 2026-10-03, option A): at most 30 node reads per run.**
The ceiling is hard: `--max-reads` above 30 is refused (`READ_BOUND_EXCEEDS_30`), and
reads are paced with a fixed delay between consecutive reads (`--delay`, default 2s,
minimum 1s). The audit calls the deployed `/transactions/station/registrations?limit=100`
keyset route (up to eight pages), then `/addresses/{canonical_owner}` once per unique
active owner, then closes with one more registry-context read. That is usually more than
30 reads, so pass a per-node state file and re-run until it finishes:

```sh
python3 docs/deployment/e1-stage-a/audit_owners.py --base "$E1_NODE_BASE" --state "$E1_STATE_FILE" < "$E1_OPERATOR_TOKEN_FILE"
```

A run that spends its budget prints `{"status":"PARTIAL",...}` and exits 3; run it again
later (an owner-chosen window, never in a loop) to resume from the saved cursor/owner
results. Each resumed run re-reads the registry context first and STOPs
`CANONICAL_STATE_MOVED` if height, root or total changed; the saved state is then stale,
so delete it and start a fresh audit in a quieter window. A saved state is untrusted input: it is rejected (`STATE_INVALID` / `REGISTRY_INCOMPLETE`) unless its
station ids are unique and increasing, the cursor and context are coherent, a finished registry has
exactly the recorded total and every checked owner belongs to an active station, so a resumed or
partial audit can never PASS or NO-GO on an incomplete registry. The state file holds only
station and account identifiers (mode 0600, no token, no coordinates), is bound to one
`--base` and must not be shared between nodes; it is removed when the audit finishes.
Without `--state` the whole audit must fit in one run or it STOPs `READ_BUDGET`.
Exit codes: 0 AUDIT-PASS, 1 NO-GO, 2 STOP, 3 PARTIAL. `request_count` in the result is the
total node reads across all runs; `reads_this_run` is this run's.

Exhaustion, incomplete counts, cursor disorder, changed height/root/total,
authentication refusal or malformed responses return STOP, never partial PASS.
No block scan or per-station coordinate request is made.

The source station-list response contains coordinates. They are discarded in
memory; neither responses nor credential/error text are logged or persisted.
Output contains counts and station/account identifiers, including the canonical
station-to-owner pairing. Inactive stations remain in the completeness count but
their owners are excluded. `wrf…` and `0x…` addresses normalize like the deployed
slashing helper. Invalid owners and missing accounts are NO-GO. Zero-balance
owners are conservatively NO-GO economic debt; account existence alone is not
proof of a funded penalty. No amounts are emitted. AUDIT-PASS is this audit's
result only, never permission to activate.

Account responses lack a height/root, so this is a bracketed observation, not an
atomic server snapshot. It refuses an advancing tip; choose another owner-run
window rather than relax the guard. A transient reorg that returns to the exact
same context cannot be excluded by these routes. Compare all three audits and
common-height hashes/roots, recheck at signing and before submission. No live
audit has been performed by SOL-E.

Route/schema pins exist on release `65134a64492135b6ac81344dd0d2edd8dfaf1309` and
candidate main `9efd50b116eddcd02bd34f465fbbba4701c977a5`:
`native-chain/src/bin/launch_api/routes/station.rs` (`list_station_registrations`),
`routes/chain.rs` (`get_address_compat`), `routes/mod.rs` (GET registrations and
addresses). The account success envelope is `ok:true,data:{address,balance,…}`;
missing accounts require `ok:false,error.code:NOT_FOUND` with HTTP404.

## Offline drills and economic GO canary

Use the repository's cached Rust1.98 toolchain, one job at a time, build jobs3,
test threads3 and the resource-rules memory gate before Cargo. Never use a live
node or deployment volume.

```sh
CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 cargo +1.98.0 test --offline -p wrfcoin-shared-protocol --test station_owner_slash_identity_activation_3214 -- --nocapture
CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 cargo +1.98.0 test --offline -p wrfcoin-native-chain --features full-mining,p2p,hsm,upgrade-notice-fetch --bin launch_api e1_stage_a_three_node_activation_wal_and_reorg_drill -- --nocapture
WRFCOIN_E1_STAGE_A_REQUIRE_PENALTY=1 CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 cargo +1.98.0 test --offline -p wrfcoin-native-chain --features full-mining,p2p,hsm,upgrade-notice-fetch --bin launch_api e1_stage_a_three_node_activation_wal_and_reorg_drill -- --nocapture
python3 -m unittest discover -s docs/deployment/e1-stage-a -p 'test_*.py' -v
```

The kernel controls test H−1/H/H+1 liability, canonical/bystander selection,
malformed/missing owners/accounts, zero balance, queue finalization/idempotence
and the production 100-of-110 circuit cap. The native drill owns an isolated
process and pinned past clock. A real producer carries three synthetic signed
extensions: two E0/E0f-like prior features, then sequence2 containing only the
owner-identity gate at carrying3+2000. The first two are a synthetic history,
not copies of the owner's artifacts. Two independent chain/storage peers apply
wire-serialized blocks through the actual P2P applier. These are three local
runtime states; this drill does not exercise TCP delivery or deployed binaries.

At H−1/H/H+1 it checks root convergence, actual quorum-backed ordinary earning,
physical-violation pruning, rejected-block atomicity, absence of poisoned WAL
rows, checkpoint+WAL recovery and effective overlays. A longer alternate branch
crosses H from the durable H−2 checkpoint through the real reorg path; orphaned
earnings roll back, then both databases reopen to the expected branch roots.

The economic canary requires a durable debit of the registered bad station's
funded owner. Current source prunes the invalid reveal and its temporary trigger;
helper-only debit is insufficient. A RED canary means Stage A remains NO-GO for
economic enforcement even if the characterization/convergence checks pass. Do
not patch consensus behavior or substitute an evidence-report transaction in
this preparation: reviewed B4 Option C is a separate, unscheduled release path.
Executed results, baseline limits and exact source hashes belong in the PR and
SOL-E ledger, not an unexecuted PASS claim here.

## Unsigned extension artifact plan

Owner inputs, refreshed after GO: all participating reviewed binary/release pins;
chain777/testnet, actual canonical genesis hash, immutable installed governance
authority; exact canonical applied count2 and E0/E0f entries; latest common
hash/root; T=max(fresh three-node tips), L=latest scheduled feature height≥33000.
Require owner audit and economic canary GO before any signature request.

Set `H_A=ceil_500(max(L+500,T+2300))`. For actual carrying height c recheck
`H_A−500000 ≤ c ≤ H_A−2000`; expiry is H_A−2000. The planning cushion is not
permission to retry or reuse a stale signature. Recompute a later height and
seek review if necessary. Do not guess height from elapsed wall time.

The future `FeatureScheduleExtension` payload has chain/network/genesis above,
sequence2, exactly one entry `{name:station_owner_slash_identity,height:H_A}`,
empty `version_steps`, and the separately reviewed minimum binary protocol and
informational `release_commit` pin. The audited B3 source uses protocol3 and
chain signing v1; extension signatures are explicit v2. These are distinct.
No numeric activation height, governance signature or submission command is
included in this packet. Keep the eventual payload DRAFT until CR-1 and the
LEAD/owner review the exact artifact and deployed release.

Owner execution later must prove V1–V9 at actual inclusion, canonical sequence,
authority, absence of previously scheduled name and monotonic height. HTTP201
means admission only. Submit once with persistent unknown-outcome handling;
after timeout/bad receipt stop without retry. Compare exact carried entries,
sequence and effective schedule on all nodes, then common-height hashes and
canonical roots at H−1/H/H+1 plus receipt-backed earning/clean oracle cycles.
Carry Stage B only after Stage A's observation passes. A carried name cannot be
moved/cancelled/re-armed; recovery requires a reviewed forward plan, not an
automatic binary downgrade. Current prep changes no schedule, state layout,
checkpoint format, production route or environment default.
