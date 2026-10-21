# Genesis 888 local evidence

Base: `ab40f108a254e010024440f2dfd99066b8b17f87`. Workstation execution was
explicitly authorized for this round. Synthetic rehearsals and the subsequent
real-chain follower replay are distinguished below. This is workstation
evidence, not hosted CI, CT102, owner-signature, or deployment evidence.

## Initial synthetic replay differential (supporting evidence only)

At initial PR head `a5b318d48693c9e79c12c9c570cc4e3806cb33bc`, the same synthetic WAL fixture was compiled against an independent archived
baseline and the candidate, with separate Cargo target directories. It emits
301 distinct state roots for heights 1–150 and 49900–50050. Both files are
byte-identical; SHA-256:
`9db12288d86f5bcf5d2847174ff203c19ccb02f7dc04a761bd6c061b65ee72ee`.
This is synthetic 777 replay, not an imported live-chain window.

## Initial three-node run (height 150)

`scripts/genesis_888_scratch.py --flat-reward-base-units 2222222222` ran one
producer and two P2P followers to height 150. The explicit amount is a synthetic
control for the post-reduction budget; the owner subsequently selected this exact amount at 21:12Z.

- Common block hash: `732297f8b398ce06a77f5bea2f5ad38340432ae81d5231de8e2551b6f7e56879`.
- Common state root: `3e345048833835dc868267aaeeda9ad6c53a4af28135244a3e317fe927d7fe02`.
- Block 1 contains the eight-gate schedule in GENESIS_888.md, v1 signing,
  recorded faucet and 100,000 WRF allocation, governance key, and 512/512 timing.
- Node pay active at 9; epoch 0 OPEN with `15777777762` scheduled base units.
  This demonstrates accounting activation; no bonded-node payout is claimed.
- API ports 18560–18562 and TCP ports 19010–19012 were loopback-bound;
  QUIC was off, bootstrap peers were loopback, and UPnP construction suppressed.
- Four process lifetimes (including the producer restart) terminated; no
  `launch_api` child remained. Live containers and volumes were untouched.
- Binary SHA-256: `d71aa91082419f78be6588aff63af22ad564af17c4f3b6b02155721bf2aaf5f2`.

Earlier iterations are superseded: they converged but UPnP discovery had not yet
been suppressed, so they do not establish exclusively loopback traffic.
Disposable private keys and databases are excluded from version control.

## Focused checks

- Genesis policy: 7 passed, including authority signature domain controls,
  single allocation, flat-rate arithmetic, checkpoint/snapshot codecs, and timing.
- Legacy authority: 2 passed (plus an isolated child control).
- Faucet binding: 10 passed; chain-binding unit controls: 39 passed.
- Node settlement: 16 passed; node contracts: 38 passed, 1 ignored.
- Emission supply-bound controls: 13 passed; final replay control: 1 passed.
- Loopback discovery guard: 1 passed.
- Python node operator/submit helpers: 14 passed.
- Authority production call-site guard: 2 passed.
- Determinism gate against the baseline at HEAD: 0 NEW, 0 GROWN.
- Markdown render and whitespace checks passed.

The owner selected 2222222222 base units/block at 21:12Z. CI, independent
review, and the real-chain replay below remain required.

## Initial real 777 replay route assessment (historical)

**Initially BLOCKED / NOT EVIDENCED.** The synthetic fixture above is not the required
real-chain replay. Source inspection establishes that the API export contains
tip State and recent block context, not the earlier State needed for a historical
window. No live export was attempted. LEAD received the smallest alternatives:
an offline anchor at H>=48000 with block 1 and H+1..H+200, or one tip export plus
collection of 200 future real blocks. Both binaries must consume identical hashed
inputs and compare state roots at every height. The reviewer accepted either
route; the input route remains pending LEAD.

The hosted determinism disk guard is a separately owned CI environment issue
(core4#3621); it still needs green execution before merge. This PR stays draft.

## Review revision: explicit rate and three nodes through height 1040

The revised policy requires an explicit amount whenever the seconds declaration
is present. Producer/verifier validation occurs before reward arithmetic, and
State admission and storage-envelope decoding reject the missing amount.
Both new integration targets are registered in the CI ratchet.

- Policy integration suite: **8 passed**, including missing-rate controls and
  empty-epoch finalization through height 1040.
- CI integration inventory unit: **1 passed**, 2084 filtered out.
- Determinism gate against ab40f108a: **0 NEW, 0 GROWN**.
- Three actual nodes at **1040**, using the owner-selected `2222222222` units,
  required in block 1; same v1 schedule and 512/512 timing.
- Common block: `b04d2fc5f59b59601e74f6e49878cdcbcda20a9479c078c85536161de8083c97`.
- Common state root: `1caca7588c425913637575ca8db2ff30daddcbd9df5566c7fbabb7fd045e413b`.
- Epoch 0: OPEN at 519; UNISSUED at 520; remains UNISSUED at the inclusive
  challenge deadline 1032, maturity 1033, and final height 1040. Maturity is
  recorded as 1033. Scheduled/unissued budget: `56888888832` units.
- No eligible bonded claimant; payouts remain empty. This proves the empty-epoch
  path across both windows, not a bonded claim payout or fraud adjudication.
- Binary SHA-256: `a26673b5c8c69d8a83be24f0d75b45ce5b822e1ecfe8f3853789c32ea9f3f4c4`.
- The harness honors API 429 Retry-After with bounded retries. The initial attempt
  stopped at a rate limit; the final run completed under unchanged rate limits.
- Four process lifetimes stopped; owned build target removed after evidence.
  No live export or live volume access occurred.

At that revision, the real 777 differential remained blocked on suitable input. The selected
888 reward amount is no longer an owner dependency.

## Producer-auth fresh-boot fixture revision

The non-777 testnet fixture now supplies the valid recorded `0:v1` schedule.
The producer committee count assertion and the no-authority refusal control
remain unchanged. The production schedule guard remains unchanged.

- Before the fixture revision: **0 passed, 1 failed**, with
  `new testnet cut requires WRFCOIN_GENESIS_ACTIVATION_SCHEDULE`.
- After the revision: the entire `producer_auth_persisted_boot_2290` target
  reports **12 passed, 0 failed**.
- Sweep of native-chain test process configurations found no other non-777
  fresh testnet boot missing a schedule. The HSM process fixture uses 777;
  the cut-composition fixture uses its own non-testnet network.

This revision supplies no additional real-chain replay evidence.

## Real 777 follower retry policy audit

The first P2P attempt imported the same verified height-49790 anchor into both
binaries, then both rejected block 49793: `zero-fee weather tx without valid
compute receipt (min_work_factor=24000)`. The clean scratch environment omitted
`WRFCOIN_MIN_WORK_FACTOR`. This is an incomplete replay under the wrong policy,
not an observed difference between the candidate and baseline. No 200-block
result or final export comparison is claimed for that attempt.

Before retrying, direct environment reads in shared-protocol, consensus-engines,
and native-chain were enumerated, including startup inputs reaching validation.
The policy source is the committed node service in
`wrfcoin/infra@19be5ad12310303ae28247fd8e261d50f2827833`,
`testnet/docker-compose.testnet.yml`. Both followers must use the same settings:

| Input | Committed Compose / retry policy |
| --- | --- |
| `WRFCOIN_MIN_WORK_FACTOR` | `${WRFCOIN_MIN_WORK_FACTOR:-0}`; LEAD confirmed live value `0` at 2026-10-19 23:57Z |
| `WRFCOIN_CHAIN_ID`, `WRFCOIN_NETWORK_ID` | `777`, `testnet` |
| `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS` | Existing public `e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00` |
| `WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT` | `1` |
| `WRFCOIN_REQUIRE_PRODUCER_AUTH` | `1` |
| `WRFCOIN_STATE_PRUNING_RETENTION_BLOCKS` | Unset; binary default 100000 |
| `WRFCOIN_ALLOW_NONDEFAULT_RETENTION` | Unset; no nondefault retention acknowledgement |
| `WRFCOIN_GAMIFICATION_OPERATOR_PUBKEYS` | Unset; empty authority allowlist |
| `WRFCOIN_SIGNING_PROTOCOL_VERSION` | Unset; restore the recorded block-1 version |
| `WRFCOIN_PRODUCER_AUTH_REQUIRED` | Unset; retain the on-chain producer-authority requirement |
| `WRFCOIN_MAX_IN_MEMORY_BLOCKS` | Unset; binary default 200 |
| `WRFCOIN_STATE_DIVERGENCE_RECOVERY` | Unset; binary default `PeerAgreement` |
| `WRFCOIN_UPGRADE_NOTICE_FILE`, `WRFCOIN_UPGRADE_NOTICE_PEER_FETCH`, `WRFCOIN_UPGRADE_NOTICE_PEER_URLS` | Unset; no notice file or peer fetch |

Fresh-cut oracle/producer sets, genesis activation/emission/faucet/node-timing
inputs and the node-share rehearsal override remain unset: this is restoration
of verified 777 State and block 1, not a new cut. Explicit scratch settings are
sync-only mode, the verified unprefixed 777 genesis hash, separate local storage,
and loopback API/P2P transport. Signer/HSM/rotation credentials are not loaded.
Authentication bypass is local to the disposable debug API. API/mempool admission,
producer-only credentials, HSM provisioning, test-only environment controls,
logging, and storage encryption do not supply alternate block-validity policy.
The production adapter and transition extractor add no direct environment reads.

HTTP snapshot-ingest authentication and request-size transport are **not
exercised** by the authorized offline importer. It calls the production snapshot
verifier and storage installer after checking the owner-supplied file hash,
State root, snapshot root, canonical round trip and carried chain window.

The failed attempt used candidate `6df53d5a0e784d09ce868a0bdc4448109e9863c3`
and main `ab40f108a254e010024440f2dfd99066b8b17f87`. The baseline retained
main's consensus/state code and used only the approved loopback bind/UPnP
transport patch (SHA-256
`2057c58b7a14dce780dbe028d6ba110528c0ba9665ad079915f924b79405bf8c`).
The identical scratch importer SHA-256 was
`db8e5ab53431ad4675b5e5c9316032b1e68a734e07c72a6778d86c03fbfda2d5`.
Neither helper nor transport patch is part of this PR's evidence revision.

Both imports verified 135115 entries and 10001 carried blocks at height 49790:

- File SHA-256: `b3d7df7e7fcb812fa2e124bb77f6dff791f73fd0e0b9ae25277d87c8f9adcd04`.
- State root: `96f9ad5e0914e9dbeee164bb967cfb379e8cc97412e5c90093fc78192adde751`.
- Snapshot root: `f632cc07afc9c3e4946c71f7b1a36413919955821a8fc430bece9748af4fdd39`.
- Both bad-file-hash controls refused before database creation.

The accepted log for block 49791 matched in both binaries:
`a271efc839f7849f2747ae7c5820ba2674d54bbcd8bf76b1645724618639ba26`.
Both later restored height 49792 from WAL, but shutdown raced that block's
accepted-log emission; this does not establish a complete logged window.
Both then rejected 49793 under the default work-factor policy and stopped.
The candidate/baseline logs contain 4/3 ahead-peer warnings, respectively,
from peer/local heights 49880/49790 through 49883/49792. The unchanged code
labels every taller peer with a different tip `FORK DETECTED`; the reviewer
accepted these as catch-up warnings, requiring actual reorg/divergence and
per-height hash checks instead. This attempt never reached the live tip, so
cessation of those warnings at tip was not established.

Setup errors were retained separately: an initial helper stack buffer was
moved to the heap in both builds; the launcher was kept alive as supervisor;
sync-only mode was explicitly configured; the socket guard was corrected to
allow outbound source binds while refusing non-loopback listeners and all
IP destinations except the designated loopback seed; and the genesis hash
was supplied without the API's `0x` prefix to match the existing handshake.
No block-acceptance result is attributed to these setup attempts.


## Confirmed-policy 200-block P2P replay (2026-10-20)

**Historical checkpoint: window and follower comparison PASS; reader-export
comparison was pending. Superseded by the live-root comparison below.**
The same candidate runtime `6df53d5a0e784d09ce868a0bdc4448109e9863c3` and
baseline `ab40f108a254e010024440f2dfd99066b8b17f87` were independently rebuilt
with `--features p2p`, using the identical importer and approved transport patch
whose hashes are recorded above. An archive comparison confirmed candidate
changes were only the scratch entrypoint/module; baseline changes were only
that entrypoint/module and the approved two transport files. Consensus/state
code was unchanged. Both used the audited policy, including confirmed
`WRFCOIN_MIN_WORK_FACTOR=0`. The earlier failed policy run remains disclosed above.

Both corrected binaries freshly verified and installed the original h49790
anchor, including its exact file hash and both roots, 135115 entries and 10001
carried blocks. Both bad-file-hash controls refused before database creation.

- Both accepted **every height 49791–49990**, exactly 200 contiguous blocks,
  over real P2P from the single loopback seed. All 200 accepted hashes match.
- Each retained `height hash\n` record has SHA-256
  `c0a47202d10f1fdb227022c78cac0d3720b9e33b8272fd9baf506b9c6e6dcebb`.
- No rejected block, actual reorg, divergence observation, local integrity
  failure, noncontiguous acceptance or differing accepted hash was observed.
- Candidate/baseline ordinary taller-peer warnings: **82 / 77**. Both first
  report peer/local heights **49927/49790**; both last report **49990/49989**.
  At the final common tip, counts stayed unchanged over two observations at
  01:40:39Z and 01:40:59Z. No warning line was suppressed or patched.
- Both followers and the live node reported height **49990** at those final
  observations. Both followers then stopped gracefully and persisted shutdown
  checkpoints at exactly **49990**. Offline root readback in both binaries agrees:

| Field | Both followers at H=49990 |
| --- | --- |
| Block hash | `96a46eb22fdc7db6387a4e305302880a5bdebc709a653d92ae9d60f699bd4782` |
| State root | `17897cb26ddefc16af98d7452e0253dff84233c29163f265e6eca70963204c42` |
| Snapshot root | `9680714f9daa78eeeee1838846148260e66e2606e1a78234281b58e04d1b655f` |
| Snapshot entries | 135403 |

The peer-advertised h49990 tip and State root also agree with these values;
that is supporting P2P evidence, **not** the required reader-export comparison.
The prescribed `TO LEAD: need ws export at H` request was posted at
**2026-10-20T01:40:59Z**, requesting **H=49990**. No reader export at that height
has been supplied yet. The hard condition remains incomplete until its State
and snapshot roots are independently compared. No credential was requested or
read, and no additional live export was performed by this lane.

Candidate binary SHA-256:
`8cd9acbf4e6983cf020bf06018819c96179b879e16856e7b09173998e5b73e2b`.
Baseline binary SHA-256:
`ab9b5068402ebf97725ffd3cf8db0cf595e89a0bff238fef4e96e134ad51f312`.

Network-guard controls refused nonloopback connections, connections to other
ports, and wildcard listeners while permitting loopback listeners. The measured
run retained 681/669 refused socket operations respectively; the guard allowed
outbound IP connections only to the designated loopback seed. Seven log-auditor
controls covered matching data, differing hashes, missing heights, rejection,
divergence, actual reorg and ordinary catch-up warnings.

A setup-only rebuild initially omitted the P2P feature; both nodes refused
startup before accepting any block. Those logs were retained separately. Both
binaries were rebuilt with P2P enabled, and both imports and negative controls
were repeated before the measured run. This did not change production source.
Builds were serialized with `-j 8`, owned target directories and over 60 GiB free
before the final builds. All scratch followers stopped; targets, source
archives, databases, private peer keys and the executable socket guard were
removed after root capture. Text harness, patch, logs, per-height records,
provenance hashes and cleanup receipt remain in ignored local evidence.

HTTP snapshot-ingest authentication and request-size transport remain
**unexercised**. This is workstation consensus replay evidence, not CT102,
deployment, hosted-CI or release approval. At that checkpoint the PR remained
draft pending the reader-export comparison and remaining CI/review requirements.


## Replacement condition 4: live-tip root equality — PASS (2026-10-20)

The LEAD's 02:06Z ruling replaced the unavailable historical h49990 export;
the reviewer confirmed the replacement conditions at 02:06Z. A **fresh import
of the original h49790 anchor into both binaries**, including identical file,
State/snapshot-root verification and bad-hash refusal controls, preceded this
run. The earlier 200-block evidence remains supporting evidence.

- Full rerun: **254 contiguous heights 49791–50044**, all accepted hashes equal;
  no rejection, actual reorg, divergence, gap or differing accepted hash.
- Both canonical `height hash\n` records have SHA-256
  `d5956ad061d9f098915e379ba274bba1dc2f6f8617d98d2a7b9e916f9f9fe296`.
- **All ten overlapping heights 50035–50044** match across ws, candidate and
  baseline in **block hash, state root and snapshot root**. Every overlap and
  pairwise shared row was compared; no mismatching row was omitted.
- Catch-up warnings: candidate **22**, baseline **24**; first peer/local
  **50015/49790**, last **50044/50043**. Warnings cease while at tip; none
  followed final-height acceptance. Both graceful shutdown checkpoints: **50044**.
- Comparator controls: ten consecutive rows accepted; missing-height range
  refused as insufficient; block-hash, State-root and snapshot-root mutations
  each refused. Source-boundary and socket-guard controls passed.

Per-height log SHA-256 values at **2026-10-20T02:49:52Z**:

| Log | SHA-256 |
| --- | --- |
| LEAD ws-roots.tsv, immutable captured bytes | `3d9433949861111d2f76b3828f7e7cad95801cc1efe8572d99d6fea59c99ab39` |
| candidate-roots.tsv | `cb5aeb508208a683262ccd772ac46fce21999881a2f5b5b901452ccd277a3536` |
| baseline-roots.tsv | `8eaa64ececbbeed3a73eaea9a1a98d914229d8db326ccbdee999fa3c7bf0f2af` |

The producer log continues independently; its hash above identifies the exact
captured bytes used for comparison, not a later growing-file version. Rows
include UTC, height, block hash, State root, snapshot root and `tip-stable`.
The retained comparison report contains all ten tuples and each source's read
timestamp; the frozen producer log and both follower logs are retained with it.

**Read methods.** Per the LEAD's 02:07Z provenance: on each new ws tip height,
block hash comes from `/chain/block/H`; both roots come from reader-role
`/state/snapshot`; a second read must retain the same height and roots before
recording `tip-stable`. This is a summary read, not a full state export.
Each follower uses reader-role `/state/snapshot`, bracketed by `/network/info`
height reads. A row is recorded only when all heights agree and its accepted
P2P log supplies the canonical block hash. Comparison matches on height and
block hash and requires both roots equal. Snapshot roots were available at
every height, so no additional full export was needed. The lane read only the
LEAD's root log; it did not access live credentials or run a ws snapshot read.

**Runtime provenance and setup.** Same candidate runtime `6df53d5a0e784d09ce868a0bdc4448109e9863c3`,
baseline `ab40f108a254e010024440f2dfd99066b8b17f87`, audited policy including
work factor `0`, identical importer and approved transport patch as above.
One build per binary, with P2P enabled, dev optimization level 1, debug
assertions retained, serialized `-j 8`; free disk before builds 63.90/62.68 GiB.
No production source or consensus policy changed.

Candidate binary SHA-256: `fe86adee617f9cf7b19343e3f49ee7ca6221269bb746269d9a5358adc15dd848`.
Baseline binary SHA-256: `4f85fcb5e123808797346ce718e63dd9de9ee43ff933d8a9005b8d37bc6b4ba2`.

Initial scratch snapshot reads returned 401: the bootstrap no-auth setting
does not grant Reader access. Ephemeral Reader credentials were then generated
only in supervisor memory for the two scratch nodes; never printed or persisted.
An interruption stopped them at a logged 50030. An optional 90-second offline
recovery read timed out and supplies no evidence. Normal startup subsequently
restored **50030 in both**, exactly the last accepted logged height; both then
accepted 50031 onward. Combined logs prove continuity across this disclosed
reader-setup restart. This run began with fresh h49790 imports, not the earlier
run's h49990 database. No extra build or validation waiver was used.

Both followers stopped; owned targets, source archives, databases, peer keys
and executable guard were removed. Text harness/logs, hash manifests and cleanup
receipt retained. Prior failed policy and setup attempts remain disclosed above.
HTTP snapshot-**ingest** authentication/size transport remains unexercised; these
are authenticated summary **reads**. Replay conditions 1–4 are evidenced under
the replacement ruling. Independent review and CI after the LEAD's main refresh
remain separate; no merge, deployment or release approval is claimed.
