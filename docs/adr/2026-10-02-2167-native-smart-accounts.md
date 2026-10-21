# Native smart accounts as a transaction type

Refs #2167; related #3206, #3205, #2170, #2171 and #2172.

Status: proposed for review before implementation. Owner ruling, 2026-10-02:
account abstraction uses a native transaction type; recovery revokes the old key.
This note defines the proposed first implementation for B4. It changes no runtime
code, genesis, feature schedule, account balances or existing activation behavior.

## Decision and existing foundations

Append `TransactionType::NativeSmartAccount` after SOL-E's `SlashEvidenceReport`.
Carry a typed, client-signed payload through the ordinary transaction pool and
native block pipeline. Never submit these actions to `add_user_operation` or
drain `user_operations_for_block` into native blocks. Keep those existing library
interfaces dormant; deleting or exposing that lane is separate work.

Source baseline: core4 main `016d911b70fb013a121b7f0bf95cbad892adc9de`.

The actual enum tail is StationRegistration = 34, FeatureScheduleExtension = 35
(36 variants). The append-only table in
`shared-protocol/src/blockchain/wire_layout_fixtures.rs` still
has 35 entries and omits FeatureScheduleExtension; its literal count is not an
inventory of the current enum. Intended order: FeatureScheduleExtension = 35,
SlashEvidenceReport = 36 (SOL-E), NativeSmartAccount = 37 (SOL-F), final count 38.
SOL-E's implementation PR adds the missing index-35 row and its index-36 row,
with count 37. SOL-F's implementation PR rebases on that head, appends index 37
and updates the count to 38. Each PR pins all legacy indices and old block bytes;
new golden fixtures include the new transaction's four-byte bincode discriminant.

If SOL-E is delayed, SOL-F holds its enum/code publication. Only an explicit
LEAD integration-order amendment may permit SOL-F first: NativeSmartAccount
would then be 36/count 37 and SOL-E would append SlashEvidenceReport at 37/count
38. The first PR carries the missing FeatureScheduleExtension row; the second
updates the complete table/count and regenerates only its unpublished new-type
vectors. Never renumber a merged variant. Publish the agreed full head and
indices in COORDINATION before another lane builds on them.

When native_smart_accounts is off or unscheduled, reject NativeSmartAccount at
every carrying height in admission, producer, verify-block, apply and replay;
no new-type transaction can enter a valid legacy block. Exhaustive match arms in
verify_cost.rs, mempool/pool.rs and every routing site explicitly handle the new
type without altering pricing, selection, encoding or behavior of existing types.

| Existing source | Confirmed behavior and reuse boundary |
| --- | --- |
| `shared-protocol/src/accounts/abstraction.rs` | UserOperation, nonce checking and metered validation exist. Its opaque calldata hash is insufficient to execute a native batch. Reuse reviewed concepts, without its in-memory nonce cache as consensus authority. |
| `shared-protocol/src/accounts/authorization.rs` | AccountAuthorizationDispatch selects rules independently of the UserOperation pool. `check_declaration` refuses SmartAccount and Delegated rules; pre-block callers use EmptyAuthorizationView. These are current restrictions, not proof of native AA support. |
| `shared-protocol/src/accounts/session_keys.rs` | Height windows, deny-by-default method/target scopes, per-use/cumulative value bounds, revocation and registration/use counters exist. Use currently records authorization usage rather than performing a transfer. |
| `shared-protocol/src/accounts/recovery.rs` | Guardian threshold, approvals, request nonce, policy change delay, cancellation and recovery timelock exist. Execute returns the verified new public key to State. |
| `shared-protocol/src/blockchain/state.rs` | ApplyAccountRecovery invokes `move_recovered_account`, which moves financial and recovery state to a public-key-derived destination. It leaves the old session-key map behind. Removing balances from an old address alone does not establish permanent key revocation. |
| `shared-protocol/src/accounts/paymaster.rs` | Deposits, delayed withdrawals, epoch quotas and failure throttling exist. SponsorOperation trusts caller-provided cost/outcome and embeds no signature verification; native admission/apply must supply authenticated, recomputed facts. |
| `shared-protocol/src/crypto/chain_binding.rs` | Existing social_recovery, session_keys and paymaster gates retain their current meaning. They are not permission to expose a new wire path. |
| `native-chain/src/bin/launch_api/transition_extractor.rs` | Producer, applier and WAL require a single transition source. The documented transfer double-application bug is a control case for AA routing. |

Implement new behavior behind `ConsensusFeature::NativeSmartAccounts`, name
`native_smart_accounts`, with no activation entry and `schedulable(...) == false`.
Do not replace the old modules' behavior at their existing heights. Native
accounts use new epoch-bound authorization wrappers and deterministic helpers;
legacy transitions retain their previous bytes and behavior when the gate is off.
Add NativeSmartAccounts to chain_binding.rs's ALL/name inventory but keep it out
of S4_SCHEDULABLE. Extend schedulable_features_are_pinned_3277 with a direct
NativeSmartAccounts == false assertion; a mutation making it schedulable must fail.

## Account identity and authorization

Use a stable smart-account address and a durable controller record, rather than
moving a smart account's funds to each new controller's key-derived address.
Creation derives that address from a domain-separated, explicitly chain-bound
encoding of the creator public key and creator transaction nonce. Use the existing
protocol hash and Address conversion convention, with golden bytes. Creation
requires creator possession, refuses an existing controller or conflicting
destination authority/code, and debits its fee from the creator or an authorized
sponsor. Preserve a balance-only destination created by incoming credits: sending
dust to a predictable address must not veto creation or discard that balance.
Derivation/collision checks and the exact eligible destination shape get golden
and adversarial tests. Adoption of an existing externally owned account is deferred.

Add a sorted `native_smart_accounts` state map containing the current ed25519
owner public key, checked `owner_epoch`, recovery policy/request state and native
session grants. Financial values remain in `State::accounts[stable_address]`.
The existing Account nonce is authoritative for owner/session actions; do not
maintain a second advancing account nonce in a UserOperation cache.

Native authorization reads this controller from the sequential state at inclusion.
It never infers control from a key whose hash equals the address. Native sessions
are owned by `(stable_address, owner_epoch)`. No fallback to SignerIsSender,
authorization-program storage, expired delegation or a former owner is permitted
for a registered smart account. Initially only ed25519 is supported; arbitrary
account programs, passkey integration and EIP-7702 adoption require later designs.

Before making accounts reachable, audit every debit/privileged-control path,
including transparent/privacy transfers, UTXO spending, stake/unstake, governance,
paymaster withdrawal and authorization declarations. Existing transaction types
cannot independently debit or change a registered smart account's authority.
They must reject those actions while native_smart_accounts is active; ordinary
credits are allowed. Each future callable operation needs explicit native dispatch.
The state-backed guard must also run in block verification and direct State APIs;
an HTTP-only guard or today's EmptyAuthorizationView is insufficient.

## Transaction and signed bytes

The outer Transaction retains its current fields. `data` carries a strictly
decoded NativeSmartAccountPayload; outer nonce must equal payload nonce, outer
fee must equal the recomputed native fee, and hash/id must match the canonical
transaction. Reject unrelated outer inputs/outputs/operations that could cause
a second debit or a UTXO authorization bypass. Strictly parse bounded data before
allocating nested collections; reject unknown fields, tags and noncanonical keys.

The version-1 payload contains:

- Explicit ChainIdentity and durable genesis hash; schema version 1.
- Target smart-account address; authorizing actor; actor nonce; target owner epoch.
- Inclusive valid_from/valid_until block heights and a total fee cap in base units.
- One typed action and an owner, session or external-actor authorization proof.
- Optional sponsor terms and a separate sponsor authorization signature.

Actions are Create, Batch, RegisterSession, RevokeSession, SetRecoveryPolicy,
ApproveRecovery, CancelRecovery, ExecuteRecovery, and sponsor deposit/request
withdrawal/withdrawal. External actor signatures prove creator, guardian, executor
or sponsor identity for the corresponding lifecycle action; they cannot authorize
a batch from the target. Owner-only management actions require the current owner.
Session authorization permits Batch only. No session may create grants, change
recovery policy, initiate recovery or withdraw a paymaster deposit.

Batch v1 contains 1..16 typed `Transfer { to, amount_base_units }` calls. There
is no caller-provided method string, from-address, arbitrary calldata, nested
batch, VM callback or recursive call. Derive the method tag, target and total
spend from the decoded operation. Weather submissions and other application
actions can be added with their own consensus review; they are not implicitly
enabled by a session scope listing their name.

All signed subjects implement the existing DomainSeparated V2 construction
with explicit identity/version; never an environment-derived signing fallback.
Proposed tags: `wrfcoin.native_smart_account.tx.v1`,
`wrfcoin.native_smart_account.sponsor.v1`,
`wrfcoin.native_smart_account.session_possession.v1`, and
`wrfcoin.native_smart_account.address.v1`.

Use stable action tags and constant-width little-endian integers, explicit option
markers, length-prefixed fields and sorted/deduplicated allowlists. Canonical
encoding is independent of serde JSON and bincode layout. Account signing binds
every payload field including sponsor identity/nonce/cap/window; signature bytes
are excluded. The sponsor signs the digest of that unsigned subject under its
own domain, plus its terms, so no signature depends on another signature.
Session registration additionally proves possession of the granted key.

Recovery approvals bind stable account, old owner epoch, request nonce, proposed
new owner key and policy generation. Register, revoke and cancel bind their
exact object and counters through the same native envelope; do not accept legacy
messages that omit the native epoch. Inventory every new signed subject in
domain_registry/domain_impls and signed_type_inventory, with full preimage,
digest and signature golden vectors; wrappers need explicit reasoned exclusions.

## Validation order and nonce model

The same pure validator runs at admission, producer selection, block validation,
State application and replay. Admission uses the current view as an advisory
check; block validity uses the sequential prefix state at the carrying height.
All later sites recheck signatures, epochs, quotas, fee coverage and scopes.

1. Bounds/decode: cap bytes, nesting, collections and signature work; strictly
   decode action shape and require the native feature, identity/genesis/version,
   height window and consistent outer fields/hash. Pre-signature computation is
   limited to bounded parsing, canonical hashing and cheap work counts; it does
   not calculate/debit a fee, compare account nonces or reserve anything.
2. Signatures: read the role's authoritative key and verify every required
   owner/session, external-actor, possession, guardian and sponsor proof against
   the complete canonical subject. No successful account proof substitutes for
   a missing sponsor proof. No failed proof mutates state or consumes a nonce.
3. Nonce/epoch equality: explicitly compare actor nonce with the durable expected
   Account nonce, outer nonce with payload nonce, target epoch/policy generation
   with the current record, and sponsor/request/grant counters with their expected
   values. Check increments for exhaustion. Then check session revocation, scopes,
   height validity, use cap and aggregate/cumulative spend limits against state.
4. Fee/cap/coverage/source: compute fee from authenticated action/signature counts,
   compare outer fee and every signed cap/window, choose exactly one payer, and
   check spendable balance/deposit, unstaking, quota and throttle state. Validate
   total fee plus aggregate batch value for the target before executing any call.
5. Staged execution: reserve fee/counters on disposable state, perform the bounded
   action and derive a receipt. Publish the defined success or execution-failure
   commit atomically. All failures in steps 1-4 are unchargeable validation
   rejection with no nonce, quota, fee, grant or account-record mutation. Only an
   authenticated action that passed all validation may pay for execution failure.

Owner and session actions consume the same target Account nonce in ordered
execution. Creation and external lifecycle actions instead consume the signing
external actor's Account nonce; a guardian does not need the unavailable owner's
nonce to approve recovery. Target owner_epoch still binds those actions. A
recovery request has a separate monotonically advancing request nonce; guardian
approvals deduplicate by guardian and request. Sponsor offers have a durable
per-paymaster nonce, including across withdrawal/redeposit. Every nonce/epoch
increment is checked; exhaustion refuses atomically, never wraps or resets.

For a sponsored creator, guardian or executor with no financial Account entry,
read expected nonce 0 without mutation. After all proofs, counter checks and
fee coverage pass, create a default zero-balance Account on staged state and
commit its checked nonce increment with the included action. Retain that record
even at zero balance: pruning or failed calls cannot reset replay protection.
Invalid signatures/fee coverage create no account; unsponsored unfunded actors
are refused. Do not reuse check_and_reserve_nonce's absent-account None result
as native authorization. Test first sponsored action, absent-account replay,
invalid-action noncreation, snapshot/WAL persistence and zero-balance retention.

Session proofs bind (target, owner_epoch, session public key, target nonce), plus
the complete action/terms. Neither a nonce reused in a later epoch nor an old
registration/use signature can revive authority after recovery or revoke/regrant.

An authorized included action consumes its actor nonce and sponsor nonce on
success or execution failure. Rejected validation consumes neither. Queued
transactions are revalidated after prior batch, revoke and recovery actions.
Recovery leaves the target account nonce monotonic and advances owner_epoch,
making old queued owner and session signatures invalid even if their nonce is
otherwise current. Transport retries return the same transaction id; they cannot
reserve a victim's nonce before signatures pass or substitute a local replay TTL
for durable protection.

## Fees, atomic batches and sponsor limits

Proposed tariff for review: verification fee uses the existing CALIBRATED
VerifyPricing counts at one base unit per verification unit, plus 100 base units
per transfer call (the ed25519 anchor, a conservative proposed execution charge,
not a measured transfer cost). Management actions pay verification cost. Count
owner/session, guardian, possession and sponsor proofs actually required. Fee
calculation and equality are consensus constants, never node environment values;
the new pricing applies only under the new feature. Any calibration adjustment
after activation requires a separately gated policy change.

Actual fee is computable before bounded execution. Require actual_fee <= the
account cap and, if sponsored, <= the signed sponsor cap. Reject zero-fee native
actions, insufficient coverage and overflow. For both owner- and session-authorized
actions, unsponsored fees debit the target smart account's spendable balance;
the session credential has no independent fee balance. External lifecycle actors
pay from their own account when unsponsored. Sponsored fees debit a real deposit.
For unsponsored batches, checked fee + aggregate outgoing value must fit the
target spendable balance; for sponsored batches the target must cover aggregate
value and the sponsor must cover fee. Insufficiency is step-4 validation rejection,
not a paid execution failure. Incoming/self-transfer credits cannot fund this
prevalidation bound. A session's per-use/cumulative spend caps include the target's
fee when unsponsored; sponsorship cannot increase its transferred-value allowance.
Batch transfer value always comes from the target, never from the sponsor. Burn exactly
one fee via the existing authorized-burning supply accounting; no producer fee
credit and no additional outer transaction fee debit. Assert conservation across
accounts, stake, deposits and burned supply, with checked balance arithmetic.

Reserve the fee before calls on staged state. If a later call fails, revert all
call effects and emitted call events; retain only the fee, actor/sponsor nonce,
sponsor outcome counters and a bounded failure receipt. Successful batches also
commit aggregate session spend/usage counters once per batch. Failed batches
consume session use count and any target-paid fee allowance, but no transferred
value allowance. Invalid signatures,
revoked sessions and over-cap requests are validation rejection, not chargeable
execution failure. Receipts include id, success/error code, call outcome summary
and charged fee; roots and outcome facts must be reproduced from signed inputs.

Retain existing proposed paymaster epoch policy initially: 7,200 blocks,
10,000 sponsorships per sponsor, 200 per sender, throttle at >=20 samples and
>=20% failures, duration 7,200 blocks. These are existing placeholder defaults,
not proof of calibrated abuse protection. Add the following bounded native rules:

| Resource | Proposed v1 ceiling |
| --- | --- |
| Native payload bytes | 16 KiB; independently bounded HTTP envelope |
| Calls per batch | 16; no recursion |
| Guardian set / approvals | 16 distinct guardians; one live request per target |
| Live session grants | 64 per target, matching the existing module ceiling |
| Method and target scope entries | 16 methods and 64 targets, sorted and unique |
| Sponsor sender counters | 1,024 distinct senders per epoch; refuse a new sender at capacity |
| Verification work | Count all native proofs in the existing transaction/block budgets |

Apply quotas to all included attempts, including execution failures. Reset epoch
state deterministically by height and bound the retained per-sender map; signed
fee caps alone do not bound its growth. Mempool capacity and validation budgets
also cover native transactions. Invalid submissions burn no on-chain deposit,
so HTTP body/rate limits and pre-signature resource bounds remain necessary.

## Recovery revokes authority atomically

Use the established guardian and height-based timelock concepts, with new native
signing domains bound to stable address and owner epoch. Require a threshold of
distinct authorized guardians; pin policy-change delays, cancellation and
cooldown semantics from the existing recovery machine. No wall-clock input.

On ExecuteRecovery, validate the full request and destination key first. Then,
in one staged state commit: set the current owner to the verified new key,
increment owner_epoch, revoke all prior native session grants, clear active
request/pending old-epoch policy work and start the cooldown. Keep the financial
account and recovery policy at the stable address. Preserve registration and
request counters or epoch-qualified tombstones; never delete a wrapper in a way
that makes an old grant/approval replayable. Refuse a new key equal to the current
owner key. Explicitly retain only the approved guardian policy for future recovery.

The previous key has no authority after that transition: owner batches, session
registration, policy changes and cancellation all consult the new owner/epoch.
Old sessions cannot spend or install themselves again. Recovery must also reject
the old key through ordinary transfer/stake/UTXO/declaration paths, including
after fresh funds arrive. Merely asserting that an old balance is zero is not
the required regression. Legacy recovery's missing session invalidation gets a
RED test; any change to its existing activated behavior stays behind the new
feature and receives a compatibility review before implementation.

## Native transaction pipeline

Use one shared native state-application function and exactly one protocol routing
path, consumed by producer, transition_extractor, P2P application and WAL replay.
Do not emit a direct AA transition and an additional governance transition for
the same transaction. Never accept producer-supplied cost, outcome or transition
metadata without deriving and checking it against signed transactions. Include
new maps/receipts only under the active native gate as specified below.
Native-chain re-exports shared types rather than duplicating crypto or state logic.

## State-root and persistence compatibility

The legacy root encoder is positional. Appending a domain marker, empty-map
count or default flag unconditionally changes every root. Require an explicit
root context: canonical carrying/checkpoint/snapshot height and the durable
effective feature schedule. The native-aware encoder emits the exact legacy
material first, then emits a native domain/version and sorted controller,
paymaster and bounded receipt sections ONLY if native_smart_accounts is active
in that context. When off, emit no native marker, count, flag or bytes, even for
empty maps. Keep the legacy root domain tag and ordering byte-identical. An
inactive state containing native records is invalid; ignoring such records in
hashing is not a way to make it valid. Active empty native sections have a pinned
encoding, so the first activation's root change is explicit and testable.

Native field names are native_smart_accounts (controllers, sessions and recovery),
native_paymasters (deposit, durable sponsor nonce and bounded epoch counters),
and native_account_receipts (latest 16 receipts per target in inclusion order).
Receipt retention never deletes controller/actor/sponsor replay counters. Pin
each native section's version, field order and sorted map encoding in goldens.

State::state_root_material currently has no height argument. Plumbing a native
root context through producer, verifier, direct apply, replay_verify, WAL and
checkpoint/snapshot import is part of implementation, not an assumed helper.
Use carrying block height for post-block roots and source checkpoint/snapshot
height for imports; never tx.timestamp, wall time, tip guesses or ambient node
configuration. Historical replay uses its historical effective schedule. A
legacy no-context entry point cannot apply/import native-active state; it must
retain feature-off behavior or require the explicit context. Root contribution
ordering is coordinated after SOL-E's new section, with preexisting bytes intact.

B3 at this baseline writes checkpoint format 10. bincode(State) is positional:
serde(default) cannot load a format-10 body into a struct with appended fields.
Coordinate with SOL-E/storage owners to freeze the complete predecessor layout,
including changed nested types, and register each consecutive migration to the
new checkpoint format. Decode B3 through the existing frozen-layout migration
chain; initialize all new native maps/counters empty, preserve every legacy field
and nonce, and verify the feature-off root equals the B3 committed root before
writing migrated bytes. No write occurs until all boot/WAL assertions pass.
Do not allocate a version independently of SOL-E. Golden B3 input bytes remain
immutable; test real B3 read/migration/replay and retain the original on failure.

An off-feature writer must select the frozen predecessor checkpoint layout and
envelope, omitting all native fields, so its B3-format output matches the old
writer byte-for-byte. If SOL-E is independently active, use the coordinated
SOL-E predecessor layout; AA adds no bytes while its own gate is off. Active
AA writes the coordinated new layout/version and must load with AA-aware code.
A codec version number changing during decode is not feature-off root parity.

The v2 keyed snapshot exporter currently emits every serialized field, including
empty maps, and its importer requires every field. Add explicit native-field
omission while off. On B3 snapshot import, default only the specifically new,
absent native map fields to empty under the off-feature context; retain refusal
for missing legacy fields, unknown fields and injected off-feature native data.
Once active, require the native fields rather than silently defaulting lost
authority state. Test exported entry bytes as well as root bytes; serde defaults
alone cannot satisfy the current importer's missing-field checks.

Pin both-feature-off equivalence against B3 and AA-off equivalence against the
coordinated SOL-E predecessor, at legacy heights and H0-1. Add H0/H0+1 synthetic
activation fixtures, unknown/new tx rejection, nonempty inactive-state refusal,
checkpoint/snapshot byte comparisons and parity through producer/P2P/WAL/replay.
These are implementation acceptance gates; this docs revision runs no native code.

## Account API and CLI

Proposed launch_api surfaces: Reader-classified GET /accounts/{address},
GET /accounts/{address}/sessions and GET /accounts/{address}/recovery; POST
/transactions/accounts accepts only a complete client-signed native transaction.
The submit route follows the existing client-signed admission classification;
no Reader token is spending authority and no server-held key signs user actions.
Reads project bounded public records and expose no secret credential material.
Mount the routes, update RPC_SURFACE and exercise the real application policy.
The feature-off submit response explicitly reports inactive status; HTTP success
reports admission, while status/receipt reports inclusion and action outcome.

Add `wrfcoin account create|show|batch|session register|session revoke|recovery
policy|recovery approve|recovery cancel|recovery execute|sponsor` in
tools/cli/WrfcoinCLI. The CLI builds the same typed payload, derives canonical
bytes locally, signs through the existing key interface and submits signed
transactions. Bind fetched chain identity and genesis explicitly; never silently
sign under a default identity. Support offline serialization/signature verification.
No private key in argv, request bodies, logs or diagnostics. Reader credentials
authorize reads only. Backend meta-transaction routes and SDK work are separate.

## Threat model and review gates

| Attack or regression | Required evidence before enabling reachability |
| --- | --- |
| Cross-chain, cross-action or modified batch replay | Independent full preimage/digest/signature vectors; tamper every field, domain, identity/genesis and action tag. |
| Stale owner or session after recovery | Main RED for retained legacy sessions; production-path native recovery with new owner success and old owner/session rejection, queued replay and fresh credits. |
| Authorization bypass through another transaction type | Adversarial tests for every registered-account debit/control path; mutations bypassing the state-backed guard must fail. |
| Revocation, empty scopes, expiry or cumulative-value bypass | Height-boundary, second-call aggregate-spend, grant generation, revoke/regrant and stale-registration controls; no self-escalation. |
| Sponsor theft or free failure spam | Unsigned/altered sponsorship, cap overflow, nonce replay, exhausted deposit, withdraw race, quota limit+1 and failure-throttle controls; deterministic failed-batch fee conservation. |
| Partial batch or double application | Fail the last call; compare receipts, balances, nonce and supply; mutate rollback and extractor routing independently. |
| Validation CPU or memory abuse | Wire/collection limit+1, unknown method/tag, duplicate guardians/scopes, excessive signatures and bounded sender-map tests before expensive work. |
| Consensus or storage fork | Full native-chain set, determinism_differential, snapshot roundtrip/migration and producer == P2P == WAL == replay_verify parity, including failed batches and recovery. |
| Feature-off behavior change | Pre/post byte comparison of legacy transactions, block bytes, state-root material and checkpoints at existing heights; zero native schedule entries. |
| Unmounted or overprivileged product surface | Mounted Reader tests, client-signed refusal/admission/inclusion, CLI offline vectors and sanitized key/credential transport tests. |
| Compromised owner repeatedly cancels recovery | Only the current owner may CancelRecovery before Execute commits; guardians and sessions cannot cancel. Test cancel/reinitiate at cooldown boundaries and stale-epoch cancellation after Execute. Cooldown limits guardian initiation grief; it does NOT prevent a compromised online owner from vetoing every attempt. This lost-key recovery design accepts that liveness risk, requiring explicit security/owner acceptance before activation; no claim of guaranteed rescue from an actively malicious controller. |
| Sponsor sender-map exhaustion | A 1,025th distinct sender is refused without evicting existing counters or consuming nonce/deposit. Only sponsor-signed included attempts allocate entries, with paid fees and sponsor-controlled enrollment offers. Test 1,024 sybils, refusal, same-sender continued service and epoch reset. A sponsor that authorizes 1,024 hostile senders can lock new legitimate senders out for the epoch; accepted bounded-capacity risk, not an on-chain sybil-identity guarantee. |
| Guardian collusion/compromise | Fewer than threshold distinct guardian keys cannot rotate control; duplicates, stale policy/epoch, forged approvals and premature execution fail. Test a threshold coalition's permitted recovery explicitly: threshold compromise can take control after timelock, a trust assumption. Cancellation protects an available honest owner but not a lost key; guardian selection and independent custody are activation review obligations. |
| Shared owner/session nonce contention | Pin two signed actions at one target nonce: only the first included consumes it; later ones fail without fees. Bounded session windows/use caps contain delegated activity; clients refresh and sign current nonces, and revocation invalidates queued session work. A hostile session can race queued owner management; v1 guarantees no consensus owner-priority or starvation bound. Guardian/executor recovery uses external nonces and remains independent. Separate control nonce lanes are deferred if this accepted availability tradeoff is unacceptable at activation review. |
| Advisory mempool balance overcommit | Admit two individually valid transactions, then validate selection and verify-block against sequential prefix state with fees/value/deposit reservations. The later overcommitting transaction is omitted/rejected before execution and consumes no nonce/fee; block application and replay agree. Pending admission never promises inclusion or reserves consensus balances. Bound mempool size; no batch may fund prevalidation from its own future credits. |
| Unfunded external-actor nonce bypass | Sponsored absent creators/guardians get a zero-balance Account nonce record only after validation on staged state; it survives failed execution, zero-balance retention, checkpoint and WAL. Replay fails and malformed/unauthenticated requests create no durable account. Mutating absent-account nonce handling to None must fail. |

Implementation order: reviewed design; coordinated enum/types/golden inventory;
feature-gated shared validator/apply and state codec; recovery revocation;
session spend enforcement; sponsor accounting/bounds; mounted API and CLI; full
native/parity evidence. Each behavior receives main RED, GREEN and a caught
mutation. Where a native feature does not exist on main, missing support is RED,
not an invented historical execution failure. Docs-only validation makes no
runtime security or Rust suite claim; any named docs CI evidence is reported
separately against the exact PR head.

Tradeoffs: stable accounts avoid address-changing recovery and accidental old-key
fallback but require a new controller/state codec. Typed transfer batches limit
initial application coverage in exchange for bounded deterministic execution.
Failed execution pays its fee and consumes a nonce while rolling back value
transfers; clients must distinguish admission, validation and execution outcome.
Sponsor defaults and the proposed execution tariff need review/calibration before
activation. The design leaves arbitrary programs and account adoption for later.

Deployment: code requires a future B4 node image and versioned storage support;
app surfaces may ship in r20 but report inactive until authorized activation.
No new environment fee/schedule knobs, no genesis edits, no live state migration,
no feature scheduling and no release pin changes in this work. A later owner
decision, security review, supported node release and schedule extension are
required before any activation.
