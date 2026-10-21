# Stake / Unstake submission: funds design

Refs #3241, #1763, #3337. Source baseline: core4 main
`9646aa2836f65ab8198cf1abc41f3017d7ead63b`.

The owner’s 2026-10-02 Round 11 instruction requests user stake/unstake through
the node, backend relay and CLI. This supersedes the earlier scope deferral in
#3241 for the requested implementation. The earlier decision remains historical;
this note changes neither an existing activation height nor deployed state.

## Existing funds and authorization

The keyed account owns `balance`, `spendable_balance`, `stake_locked`,
`stake_required` and its account nonce. These are native WRF balances, not a
liquid staking token. All new HTTP amount/nonce fields use canonical decimal
strings; the payload amount is checked against u64 before conversion. One WRF
is 100,000,000 base units (`tools/cli/WrfcoinCLI/src/output.rs:11`). No JSON Number, float conversion or guessed scale.

| Existing operation | State effect | Source at the baseline |
| --- | --- | --- |
| Owner-authorized Stake | Move amount from spendable to locked; total balance unchanged | `state.rs:3632`, extractor `:155` |
| Owner-authorized Unstake | Move amount from locked to spendable; total balance unchanged | `state.rs:3648`, extractor `:171` |
| Reward and contribution/gamification/service settlement | Fill `stake_required - stake_locked` first; remainder spendable | `state.rs:3699`, `:3764`, `:3980`, `:4055`, `:4096`, `:4229` |
| Slash | Deduct from the account’s funds according to the existing slash transition | `state.rs:5013` |

Stopping contribution does not itself release locked stake. The existing
release transition allows release up to all currently locked stake; it carries
no cooldown or required-stake floor. Preserve those amount rules in this
submission story. Unstaking below the requirement can lower economic eligibility
and later rewards can refill the deficit. No new collateral policy, receipt,
custodian, derivative token or supply issuance is introduced.

`StakeAuthPayload` and the runtime chain/network domain bind the account, amount,
action and transaction nonce. The node verifies that the signer derives to the
account, and uses the state-aware authorization view at inclusion height.
References: `transaction.rs` stake authorization methods; extractor `:155–190`.
AA declaration/session authorization must retain the existing state-view checks;
the CLI initially supports a directly owned ed25519 account.

## Nonce prerequisite

The executed preflight below confirms a persistent replay gap: the extractor
emits only the balance-split transitions above,
which do not consume a nonce. The state protocol sink explicitly treats
Stake/Unstake as no-ops (`state.rs:7461`), and the mempool removes its
`action:account:amount:nonce` replay key on confirmation (`pool.rs:713`).
An HTTP TTL or a backend database row cannot prove persistent replay safety.
The exposure already exists on main through signed peer/gossip admission;
no HTTP route is required. `pool.rs` admits correctly authorized Stake/Unstake,
and `p2p_events.rs:1924` sends inbound gossip to that admission pipeline. A
funded account can therefore exercise the producer/peer divergence described
below through existing gossip. The local preflight proves admission and state
effects; no live node was probed.

Preflight must apply an actual extracted signed transaction, remove its confirmed
pool entry, then try a rewrapped copy with a distinct transaction id and the same
authorization. Repeat after real storage/reload. The Transfer nonce path is a
source reference; this preflight does not execute a Transfer comparison.

Implement `stake_submission_nonce` as a **NEW,
UNSCHEDULED** consensus feature before the routes can accept writes. Do not
alter existing activation heights or reinterpret historical staking blocks.
Coordinate the shared state, transaction, extractor and feature-registry hunks
with their current owners before editing them.

Proposed active semantics: authenticate the account under the existing state
view; require the transaction nonce to equal the account’s current nonce;
validate the lock/release amount; apply the split and commit nonce+1 atomically.
Overflow, insufficient funds, forged authorization and stale nonce change
nothing. Stake, Unstake and Transfer share the account nonce. The existing
protocol dispatch transition lacks inclusion timestamp. Append a
`ProcessStakeTransaction { tx, block_height, block_timestamp }` transition at
the enum tail, preserving all deployed positional discriminants; coordinate
its landing order with other pending tail additions. This adds no state section,
checkpoint format or transaction-type allocation. Producer, peer applier
and WAL replay must derive identical transitions and roots. The actual producer
currently omits staking in `governance_transitions_for_block`, while the peer
extractor applies it. The same feature must route both through the shared
protocol sink and suppress the peer's legacy split transitions when active.

Feature-off historical transactions, roots, serialized state and checkpoints
must remain byte-identical. Deployment requires a reviewed B4 image and a later
owner-approved schedule extension. A route must inspect the effective schedule
for the next block, including any authenticated on-chain extension; a boot-only
schedule lookup is insufficient. Until active, return an explicit unavailable
response before accepting or forwarding a transaction. No activation is part
of this work.

## Node contract

- `POST /transactions/stake` and `POST /transactions/unstake`, Contributor role.
  Body: account, amount, nonce, auth_pubkey, auth_signature; no private key.
- Existing `GET /transactions/stake/{address}` and `/transactions/stake/metrics`
  remain state-backed Reader routes with decimal monetary strings.
- Validate bounded shape and effective feature admission, then account signature,
  exact account nonce/pending contention, balance and configured fee admission.
  Malformed, forged or inactive requests do not reserve a nonce or replay key.
- Preserve the same configured mempool size, sender quota, expiry, fee floor,
  gossip and validation pipeline used by transfers. The protocol enqueue helper
  builds fee=0 today (`helpers.rs:1120`, `Transaction::new`); this story offers
  no caller-controlled fee because the existing stake payload does not bind it.
  A positive configured minimum can refuse that transaction; report the refusal.
- At most one pending balance-moving operation for an account nonce across
  Stake/Unstake/Transfer. Never overcommit the spendable/locked balance through
  concurrent admission. A successful response contains the actual tx id and
  means queued, not mined or settled. A stale inclusion fails without mutation.

## Backend relay and CLI

Use a dedicated mounted backend staking router and the canonical bounded
`shared/core4-route-client` transport. Bind the signed account to the caller’s
authenticated wallet, keep Contributor authority distinct from unrelated
Operator operations, forward decimal strings verbatim, and preserve upstream
status/refusal/tx id. No server signing, synthetic success, custodial balance,
auto nonce substitution or retry of a funds POST. A timeout means submission
state unknown; check the returned/local id when available before any resubmit.

CLI `stake status`, `stake lock` and `stake unlock` use the same base-unit
contract and canonical stake authorization function. Read the account nonce
once, obtain the existing wallet’s key locally, sign the intended action and
submit once. Private keys and mnemonics never enter argv, HTTP, logs or error
messages. Preview and dry-run must not POST. An accepted response prints the
tx id and queued status; status reads actual locked/required/spendable state.
Nonce conflict requires a new explicit user operation, not a hidden re-sign.

The app/website UI belongs to SOL-J/SOL-D after the route contract is reviewed.
The liquid staking dashboard may show these native stake balances, but must
not describe them as minted liquid tokens or claim an unwired redemption path.

## Validation and delivery

0. Interim security mitigation first: locally refuse Stake/Unstake admission
   and relay, as policy only, until the safe submission feature ships. Retain
   historical block verification/application and state semantics; do not change
   any existing activation. Prove signed peer admission refusal and no relay,
   not merely an HTTP refusal. This takes priority over new submission wiring.
1. Publish this funds design and actual preflight findings first.
2. The small consensus prerequisite owns feature-off historical
   goldens and active signature/nonce/amount/overflow controls, competing same
   nonce across transaction types, actual producer/peer/WAL/storage parity and
   effective schedule-extension activation. Mutation must restore replay.
3. Separate node route PR: mounted production-route RED on unmodified main,
   signed lock/unlock mined and read back, forged key/action/amount/domain/nonce,
   insufficient funds, unauthenticated/Reader denial, simultaneous submissions,
   gossip admission, refused-write atomicity and receipt controls.
4. Separate backend relay and CLI PRs with real local HTTP boundaries, large
   decimal amounts, account binding, upstream refusals/timeout, no-retry and
   no-private-key-output controls. Backend canonical full suite/maxWorkers=2;
   Rust jobs=3 and one heavy job at a time, own targets and memory gate.

No live testnet, deployment, paid service or schedule mutation is authorized.
The implementation queue is interim admission/relay refusal → node prerequisite
→ node route → backend relay → CLI, with each small PR carrying exact-head local
evidence and CI step counts. The interim refusal remains until active admission
can prove authoritative feature context, signature, funds and persistent nonce.

## Executed preflight

Two local tests appended temporarily to the compiled production launch API test
module fail on unmodified baseline production code. No live node was contacted.
Command: `cargo test --offline --locked -p wrfcoin-native-chain --features
full-mining,p2p,hsm,upgrade-notice-fetch --bin launch_api
stake_submission_preflight -- --nocapture --test-threads=2`, jobs=3, own target.

- Persistent replay: actual mempool refuses a pending rewrapped copy, then
  admits it after confirmation removes the replay key. The compiled extractor
  applies the authorization twice; nonce remains zero. Stake locks 2,000 twice;
  Unstake releases 2,000 twice from a 4,000 fixture lock. A real redb genesis
  anchor codec/reopen retains nonce zero, and a fresh mempool admits the replay.
  This is a storage-anchor round trip, not a mined checkpoint/WAL/node boot test.
- Producer/applier: actual `ProofOfPublishing::create_next_block` includes the
  signed Stake. Producer locked balance is zero; the compiled peer extractor
  locks 2,000. Their state roots differ, and both nonces remain zero. An empty
  block producer/applier negative control passes before the failing Stake.

Both tests execute, then fail their safety assertions (0 passed, 2 failed).
Proof source and stdout are retained in the SOL-G r20 evidence packet outside
`target/`; temporary shared-file edits are removed before this design PR.
No end-to-end readiness or replay-safety claim is made. The next implementation
is the unscheduled consensus prerequisite, followed by mounted node, relay and
CLI submission. The demonstrated replay and producer/applier disagreement are
already reachable through main's signed gossip admission. An HTTP submission
route would add another entrypoint, so the interim admission/relay refusal is
the immediate security priority.
