# ADR: build EIP-7702-style account delegation as a post-cut rolling upgrade (#2175)

## Decision

Build existing-account delegation: an externally-owned account may adopt smart-account behaviour
by signing an authorization, without changing its address.

Build it under three binding constraints. They are not caveats — each one answers a specific way
this feature could damage the chain, and the decision is only safe with all three.

1. **Feature-gated, activating well after the genesis cut.** The rule ships behind its own
   `ConsensusFeature`, scheduled at a height the cut does not reach. The spend-path change is
   compiled in and **dormant**: pre-activation, block contents and `state_root` are byte-identical
   to a binary without it. This is what the #3090 rolling-upgrade machinery exists for, and it is
   what makes a protocol-invasive change safe to merge before a cut.
2. **#2168 first.** Delegation needs somewhere to delegate *to*. This chain has no per-account
   authorization dispatch (see below), so #2168 must build that dispatch — also feature-gated and
   dormant — before delegation can sit on it. Delegation is not the story that invents the spend
   path's first conditional branch.
3. **Chain-bound authorization, non-negotiable.** See "Chain binding" below.

An earlier revision of this ADR recommended **not** building it. The owner decided to build, with
constraint 1 as the guardrail. That guardrail directly answers the objection, so the reversal is
recorded here as a decision on the merits rather than an override of one: the analysis said "not
in the spend path days before a cut", and a dormant feature is not in the spend path until it
activates.

The findings behind the original recommendation are all still true, and they are why the three
constraints exist. They are kept below rather than deleted, because a reader in six months needs
to know what shaped this.

## Why the constraints are what they are

### There is no authorization dispatch point yet — so #2168 must come first

`Account::code`, `Account::storage` and `Account::is_contract` exist
(`shared-protocol/src/blockchain/state.rs:88-107`). They are hashed into the state-root material
(`is_contract` at `:2310`, `code` at `:2315`) and parsed back at `:8218`, and that is all they do:
the state-root encoder is the **only** reader of `Account::code` in that file.

Which rule authorizes a spend is chosen by the transaction *type* — an ed25519 signature over a
domain-separated preimage for ordinary spends, MLSAG ring signatures for anonymous reward claims
and confidential transfers (`shared-protocol/src/blockchain/transaction.rs:2407-2428`) — never by
the spending account's stored state. There is no per-account authorization dispatch anywhere.

So delegation here is a bigger change than the EIP-7702 analogy suggests. Elsewhere it redirects
an existing dispatch; here something must first *create* one — a conditional branch in the
spending path of every transaction, whose condition is attacker-reachable state. That is #2168's
job, and doing it as its own story, with its own activation height and its own review, is worth
more than the week it costs.

### Account abstraction is still a library, not an account type

`shared-protocol/src/accounts/abstraction.rs` defines `UserOperation`, its domain tag and its
validation, but nothing consumes it: outside its own file its references are a bare re-export
(`native-chain/src/accounts/mod.rs`) and the native/shared parity guards. There is no
`StateTransition` arm, no transaction type and no mempool lane.

This is the second reason #2168 leads. "Delegate to smart-account behaviour" is only meaningful
once smart-account behaviour is something the chain executes.

### Nothing needs migrating, which is why activation can wait

Owner decision **A4**, ratified 2026-09-01 on core4#2976, is *"No carry-forward. Every balance
starts at zero from the next cut."* No genesis allocation, no seeding, no snapshot.

Every account that exists after the cut is created after the cut. So there is no population
waiting on delegation at activation time, and no cost to scheduling the activation comfortably
past the cut rather than close to it. Where an earlier revision read this as "therefore do not
build", it is better read as "therefore there is no schedule pressure" — the feature is for the
users a live network accumulates, not for a migration that does not exist.

## Chain binding is not optional

A delegation authorization changes who may spend from an account. Replayed on another network it
does so on a chain its signer never agreed to — the specific failure mode that has bitten
comparable designs.

The authorization must therefore use the `DomainSeparated` construction at
`SigningProtocolVersion::V2`, under its own tag, bound to the `ChainIdentity { chain_id,
network_id }` **named in the authorization itself** rather than the verifier's ambient
environment. That is the #3094 upgrade-notice pattern, adopted for the same reason: binding to the
verifier's environment makes offline verification dishonest and cross-network replay a
configuration accident. #2713 is the cautionary case — an unset chain identity silently signs the
mainnet domain.

Beyond binding: nonce-based replay protection, an expiry, and an explicit revocation path that
cannot itself be delegated away. A test proving an authorization signed for one network is invalid
on every other is an acceptance criterion of #2175, not a nice-to-have.

## Sequence

1. **#2168** — the authorization dispatch point, feature-gated and dormant.
2. **#2175** — `shared-protocol/src/accounts/delegation.rs`, on top of #2168, same discipline:
   its own `ConsensusFeature`, activation post-cut, gated at every producer, applier and replay
   site, with `determinism_differential` proving roots unchanged pre-activation.

Both stories activate after the genesis cut. Neither is a cut prerequisite, and neither may become
one: if either slips, the cut proceeds without it.

## What this does not solve

Activation does not give existing accounts passkey (#2169), session-key (#2170) or sponsored
(#2171) authorization; those are separate stories that the dispatch point makes reachable, not
things delegation delivers. This ADR does not settle #2168's dispatch design — that is its own
review, and an owner-policy call if it turns out to need one. It does not remove `Account::code` /
`is_contract`, which stay as contract-deployment storage; it records that they carry no
authorization meaning today, so nobody mistakes them for the dispatch point #2168 must build.
