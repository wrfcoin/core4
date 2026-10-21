# ADR: defer consensus inclusion lists (#2200)

## Decision

Do not add fork-choice inclusion lists in the fresh-genesis cut. Revisit after
the public testnet has independent producers and #2199 supplies evidence that
sustained censorship is a real operational problem.

## Cost and breakage

An inclusion list has to be signed, propagated, retained across forks, and
bound to the next block. Making an omission invalid changes block validity and
fork choice. Every node must deterministically reproduce every exclusion reason
against the same pre-state, including nonce, balance, gas/size, expiry and
dependency interactions. It also requires activation-height compatibility,
storage/WAL and IBD migration, P2P limits, list-size/gas anti-DoS bounds, and
new adversarial multi-node tests.

## Timing

The minimum trigger is measured, cross-node corroborated censorship plus a
written activation and rollback/relaunch plan. A public testnet carries no real
monetary value or production bridges (#2158), so that evidence should precede
this consensus cost.

## What this does not solve

It does not hide order flow, force inclusion during a multi-proposer collusion,
or solve application-level sandwiching. It is not an advisory monitoring rule:
without invalid-block semantics it provides no enforcement, and with them it is
consensus work deliberately deferred here.
