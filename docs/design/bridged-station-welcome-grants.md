# Geobound welcome grants (draft)

Refs wrfcoin/core4-archive#3571 and wrfcoin/core4-archive#3568.
Prerequisites: ownership design https://github.com/wrfcoin/core4/pull/51 and
trust-tier design https://github.com/wrfcoin/core4/pull/63. No grant is implemented,
funded, authorized or paid by this document. Testnet tokens have no promised value.

## Eligibility and identity

A source must have shipped ownership proof and passed owner verification for
that source. Fixture verification and dry-run licence-email delivery do not
qualify. A claim must be verified, undisputed, freshly reverified and through
probation; no copied-feed hold or stolen-device freeze may remain.

Use the chain's paid approximately 2-degree cell and
`GET /network/scarcity` (`desert_bonus_percent`) from wrfcoin/core4#46. Sparse
eligibility follows the authoritative `desert_multiplier` rule. Never derive a
second grid/density formula. Pin chain/genesis, height and cell to the eligibility
snapshot; a stale/unavailable snapshot means no grant. Bind the station location
to independently reviewed installation evidence, not an editable claim field.
Moving a station never resets its lifetime grant eligibility.

One lifetime grant per canonical hardware identity and all authenticated source
aliases, per `(source,device_id)`, and per owner wallet. Licence-email proves
control of a listed US mailbox, not uniqueness of every physical device, so an
APRS grant additionally requires resolved physical aliases. Non-US APRS has no
proof path and is ineligible. No grant follows from a public identifier.

Per-person limits require an approved privacy-preserving adjudication procedure;
multiple wallets do not establish multiple people. Until that procedure exists,
manual cluster review is an activation prerequisite. Normalize IPv6 addresses to
/64 for rate limiting, bound requests per IP/wallet, and flag neighbouring
same-operator claims. Shared networks alone are not proof of fraud. Neither IP
nor exact coordinates appear in the public audit list.

## Reuse the bond-grant pacing pattern, not its payment semantics

Source: wrfcoin/infra `testnet/services/faucet/BOND_GRANT.md` at
`5f1ca0e985988b65936e231e99bb22094dbebb70`. Its self-serve controls include
identity possession, probation, atomic identity/count/spacing reservations,
six-hour spacing, two grants per UTC day, pending-outcome retention, and a
bounded public audit list. Those are precedent, not authorization to reuse its
node-bond amount or node eligibility rules for weather stations.

Propose preserving the six-hour global spacing and two-per-UTC-day count as
initial upper bounds, plus one grant per paid cell per active consensus epoch.
Owner must ratify those limits, the grant amount, the period definition and all
identity rules. Apply per-cell and per-epoch amount caps in the same atomic
reservation as lifetime marks, wallet/IP limits, daily count and global spacing.
Define the epoch using the chain's active epoch schedule; invent no new cadence.
Concurrent workers must not reserve the same identity or exceed any budget.
Unknown transfer outcomes retain all reservations until reconciled by transaction
identity. A definitive non-admission may release only its own reservation.

## Funding, probation and hold

Proposed source is a separately allocated, finite existing faucet pool funded by
owner-authorized transfers, with a minimum balance floor. There is no minting or
premine authorization. Approved allocation is zero until the owner names the
pool and caps. Refuse a reservation if the available pool cannot cover it.

The ownership design proposes seven days of probation and thirty-day vesting;
these remain unratified station parameters. A grant initially creates a held
allocation, not a transferable payout. Recheck ownership, location, freshness,
duplicate-feed and dispute state before every vesting release. On revocation,
forfeit remaining held/unvested amounts back to the defined pool and preserve
lifetime identity marks. Do not debit already vested funds under this proposal.

The bond-grant precedent explicitly has no clawback and pays an owner account
immediately. It therefore cannot implement this hold by being called unchanged.
A separately reviewed escrow/hold mechanism is required. Any consensus-visible
vesting, forfeiture or payout change requires a new gated consensus feature,
state-root gate-off parity and producer/applier/WAL parity before activation.

## Public audit and acceptance cases

Publish a bounded, paginated list with claim alias, coarse paid cell, eligibility
snapshot, held/released/forfeited amounts, decision reason and transaction hash
where one exists. Publish aggregate daily/cell/epoch budget use. Exclude personal
names, raw addresses, email, IP, device serials and precise coordinates. A held
allocation must not be presented as paid; unknown outcomes remain pending.

Implementation tests must cover:

- Eligible sparse-cell claim after probation, and dense/stale-snapshot refusal.
- Same device through two networks, multiple wallets and geographic clusters.
- Concurrent identity/cell/epoch/day cap and spacing reservations.
- Revocation before any release and after partial vesting; only unvested funds
  return to the pool, with no second lifetime grant.
- Unknown transfer outcome, restart/reconciliation, and depleted pool refusal.
- Changed location and non-US/unverified APRS; no eligibility reset or bypass.

This document supplies acceptance cases, not executable test evidence. Owner
ratification is required for funding, amounts/caps, location proof, person-level
adjudication, probation/vesting, privacy policy and any eventual activation.
