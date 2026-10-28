# Bridged station ownership (draft, not activation authority)

Refs wrfcoin/core4-archive#3568 and wrfcoin/core4-archive#3569.

## Security boundary

Every new bridge is off by default (`BRIDGE_CLAIMS_ENABLED=0`). Verification
does not authorize rewards. No bridge observation enters the existing native
signed submission or payout path. Each source needs owner verification before
any later reward activation. Existing Ambient registration is precedent, not
an authorization to add other feeds to its collector.

Public station IDs, location, callsigns and APRS passcodes prove no ownership.
Do not accept a code echoed by the claimant as a challenge response: the server
must independently observe it through a source with authenticated write control.
In particular an APRS status packet is insufficient even if it contains the
correct, fresh code. APRS-IS accepts transmissions using a callsign-derived
passcode; adding a nonce does not repair that sender-authentication boundary.
See [APRS-IS connection specification](https://www.aprs-is.net/Connecting.aspx)
(read 2026-10-28). This is a security inference from the documented login model,
not a claim that APRS-IS authenticates physical hardware.

**Owner ruling, 2026-10-28:** US CWOP/APRS uses
LICENCE-EMAIL proof: send a one-time, single-use, expiring, wallet-bound code to
the licensee email listed in the public amateur licence database (for example
FCC ULS), then accept the entered code. The destination is lookup-derived,
never claimant-supplied. Bind the challenge to the licence-record digest as well
as wallet/source/callsign; a changed address invalidates an outstanding code.
Use only an `EmailSender` interface implemented by `DryRunEmailSender`, and a
`LicenceLookup` interface implemented by recorded fixtures. No live ULS or SMTP
calls. A real email sender requires a later explicit owner go within the zero
budget; this design does not authorize paid email or a live delivery service. Non-US and missing/currently invalid licence records remain reference-only.
APRS status/passcode alone never verifies, even with a fresh echoed nonce.

Limits: stale or reassigned licence email can reach the wrong person; this proof
is US-centric and proves access to the listed licence mailbox, not physical
possession of every transmitter using that callsign. Do not equate mailbox
proof with a device key. No production lookup, real mail delivery, or source
verification is claimed by fixture tests. Every fixture claim remains isolated
and non-earning. Production lookup freshness, licence coverage, disputed mailbox
recovery, and cross-network physical identity remain activation prerequisites.
The previous reference-only-for-all-APRS direction is superseded only by this
bounded fixture licence-email path; the rest of story 1 proceeds.

## Proof adapters

| Source | Evidence required from server-side adapter | Never sufficient |
| --- | --- | --- |
| Ambient | Read-only account grant and exact canonical MAC in its device inventory | Public map, client-supplied MAC |
| WeatherLink v2 | Read-only account authorization and station UUID in its owned inventory | Public station lookup |
| Netatmo | Read-only weather grant and module ID in the authorized account's owned modules | Public weather map |
| WU / WOW | Fresh nonce independently read from an account-controlled field; establish who can write it first | Copied public feed or claimant screenshot |
| CWOP/APRS (US) | Code sent to recorded public licence email, fixture-only | Callsign, passcode, status/comment packet |
| CWOP/APRS (non-US) | None yet; reference-only | Any public packet or claimed mailbox |

Vendor inventory access must distinguish account-owned devices from shared or
public devices. A readable device is not necessarily owned. Adapter fixtures
must include an account that can read a device but cannot control it.

Issue 256-bit random challenges bound to authenticated wallet, normalized source
and device ID. Store only a digest and an expiry; proposed lifetime is ten
minutes. Consume the digest atomically with the pending-to-verified transition.
Use database time and a row lock or conditional update so concurrent verifiers
cannot consume twice. Reject reuse, expiry, wallet/source/device substitution,
missing adapter evidence and any public-ID/passcode-only submission. Expiration
does not release an already verified hardware identity. Issuing a challenge must
not permanently reserve someone else's unverified device.

## Global identity and races

`UNIQUE(source, device_id)` prevents duplicates within a namespace; **it does not
prove one physical device across namespaces**. Maintain a canonical hardware
record with unique authenticated aliases. Bind source identities to it using
vendor-backed serial/MAC evidence, a device-key commitment, or a ratified manual
dispute process. Never take a caller's canonical hardware ID on trust. Unknown
cross-source identity remains pending, with no reward eligibility.

Normalize MACs to twelve hex digits; reject malformed values before normalization.
WeatherLink UUIDs normalize to lowercase. Treat Netatmo IDs according to its
documented identifier format. CWOP callsigns normalize to uppercase and reserve
one base callsign across SSIDs 0–15; absent SSID and `-0` are equivalent. This is
a conservative claim rule, not an assertion that different SSIDs are one device.
Resolve legitimate multiple devices under one callsign before activation.

Use unique constraints and serializable ownership updates, including races with
existing registration. A new bridge-only table that ignores existing Ambient
MAC claims would violate the global invariant. Never auto-transfer or delete an
existing registration when a new claimant presents a vendor credential.

## Lifecycle, disputes and privacy

Pending becomes verified only after independently authenticated source proof and
global identity checks. Verified becomes suspended on token revocation, failed
periodic verification, stale evidence, duplicate-feed hold or stolen-device
freeze. Suspended requires fresh proof to become verified again. Revoked is
terminal; reclaim uses a new audited dispute decision, not challenge replay.

Reverify before accepting each collection batch, with a bounded cache lifetime
chosen during source activation. Timeouts hold new observations rather than
assuming authorization. Revocation removes encrypted credentials immediately;
disconnect deletes them and suspends the claim. Use authenticated encryption
with claim/source/device binding as associated data; never log keys or complete
vendor URLs containing credentials. Keep only evidence hashes and minimal audit
metadata after deletion. Propose 30-day audit retention for owner ratification.

Reuse owner-signed transfer consent from wrfcoin/core4-archive#3345 and
report-stolen freeze from wrfcoin/core4-archive#3347. Recovery is a separate
contract exposed by wrfcoin/backend `server/routes/station-owner-routes.ts`;
those two archive issues are not the recovery specification.
A stronger vendor proof is dispute evidence, not permission to bypass an owner's
signature. Freeze contested claims while the existing recovery process resolves
ownership. No backend balance mutation or automatic clawback is authorized here.

## Duplicate feeds and native precedence

Ingest into an isolated quarantine store. Canonicalize units and observation
timestamps before hashing the measured vector (exclude source names and packet
transport metadata). Detect exact duplicates across sources and native records.
Also compare rolling aligned windows for tolerance-bounded lockstep readings;
proposed window is twelve samples with at least three varying measurements.
Flat weather or a single shared value alone is not evidence of fraud. Flag a
suspected copied feed, hold it, and retain explainable matching evidence for
review; do not revoke ownership automatically.

Native signed records always win when a native and bridge record conflict.
Late native arrival must invalidate any provisional bridge selection. Neither
arrival order nor a bridge's higher sample rate can suppress native data.
Forwarding eligibility must recheck holds immediately before dispatch.

## Probation, vesting and abuse controls

Propose seven days of continuous valid proof and adequate independent coverage
before completing probation; propose thirty days of vesting for any future
bridge allocation. These are draft parameters, not earning promises. All allocations in this proposal are zero. A future revoked claim forfeits held/unvested amounts;
never debit already vested balances without a separately ratified rule.

Limit issuance and verification per wallet and client IP, using the existing
IPv6 `/64` normalization from wrfcoin/backend `server/utils/rate-limit-key.ts`. Persist shared
limits across workers; bound pending challenge retention and attempts. Count
issued, expired, replayed, mismatched and refused challenges; duplicate holds;
vendor revocations; disputes upheld per 1,000 claims; time to resolution; and
source verification age. Metrics use source and reason labels, never raw tokens,
wallets, callsigns or device IDs as metric labels.

## Consensus follow-ups (not implemented here)

1. Reconcile canonical hardware aliases with the existing wrfcoin/core4-archive#3346 commitment,
   uniqueness and stolen-hardware freeze; include replay and migration coverage.
2. Define attested bridge provenance distinct from a device-signed reading. An
   operator signature must never silently upgrade a public packet to native.
3. Gate payout tiers, native precedence, probation, vesting and held-allocation
   forfeiture behind a new non-schedulable `ConsensusFeature` until ratified.
4. Require gate-off state-root parity and producer/applier/WAL parity before
   any activation proposal. No new signed transaction type is introduced here.

## Required implementation evidence

Exercise real storage transactions and mounted routes, not only an in-memory
model: public ID refusal; spoofed callsign with valid passcode and fresh echoed
nonce refusal; expired/replayed challenge; concurrent consume; concurrent claims;
cross-source aliases and existing Ambient collisions; token revocation; shared
inventory refusal; restart persistence; exact and rolling-window duplicates;
late native precedence; and reversible migration round-trip on disposable data.

This note does not claim backend implementation or source verification complete.
The US licence-email ruling permits fixture implementation; non-US remains
reference-only. Global identity evidence must come from authenticated adapters,
and fixture proof must never be treated as live ownership verification.
