# ADR: defer a full encrypted mempool (#2201)

## Decision

Do not implement an encrypted mempool in this cut. Reconsider only after a
multi-operator testnet has measured a material content-based extraction problem
and has a separately approved protocol migration.

## Why

Current admission inspects transaction bytes to validate signatures, apply the
fee floor, account verification cost, reject duplicates, and enforce sender
caps. Encrypting contents removes that information. A fee commitment would
need a new cryptographic admission proof; a deposit/stake scheme shifts spam
cost onto honest low-balance stations; sender limits retain identity leakage
and do not price expensive verification. Any choice changes admission semantics
and therefore requires an activation/version plan.

Threshold decryption also makes progress depend on a quorum being online.
With the current small producer set, loss of one or two operators can turn a
privacy feature into a block-production halt. There is no measured builder or
committee availability to justify that liveness trade today.

Encryption would not hide timing, network sender identity, fee metadata, or
withholding by a proposer. It cannot by itself solve the inclusion-list
censorship case.

## Alternatives and timing

Keep #2199 monitoring. Evaluate an application-level batch auction only if the
AMM has real volume; evaluate a narrowly scoped observation commit/reveal only
with an end-to-end station usability and reward-liveness design. Private
submission endpoints may reduce network exposure but do not provide consensus
censorship resistance. None is authorization to change the current mempool.

## What this does not solve

This decision does not prevent content-based front-running, prove fair ordering,
or protect transactions once they are visible to a proposer.
