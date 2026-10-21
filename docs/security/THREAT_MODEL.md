# WRFCoin Threat Model

**Status:** Normative companion to the protocol specification,
[`PROTOCOL_SPEC.md`](./PROTOCOL_SPEC.md) in this directory (merged via PR #2221;
both files are on `main` as of 2026-07-22, so the link this header previously
withheld against a dangling-link risk is now live).
**Derived from:** `wrfcoin/core4` commit `798b33367` (2026-07-21).
**Audience:** external auditors scoping an engagement, and engineers reasoning about
what a *malicious* actor can attempt — not what a stable node does under normal load.

The correct security standard for this system is **not** "the node seems stable under
normal usage." It is: *we understand precisely what an actively malicious peer,
validator, RPC client, dependency maintainer, release-key thief, sensor operator, and
network-level adversary can attempt, and what stops each one today.* This document is
that understanding written down.

Where a defence named here does not actually exist yet, it is stated as **NOT DEFENDED**
and cross-referenced to the divergence in PROTOCOL_SPEC.md and its tracking issue. A
threat model that lists only the defences that work is a marketing document; this one
lists the gaps because the gaps are what the audit must scope.

Secrets, key material, and tokens are referenced by name only. None appear in this file.

---

## Adversary classes

Each class states **capabilities** (what the attacker controls), **goals** (what they
want), **assumed budget**, and **what stops them today** (with the honest answer where
that is "nothing yet").

### A1 — Malicious peer

- **Capabilities:** speaks the gossipsub protocol; can connect to the P2P port and send
  arbitrary `WrfcoinMessage` payloads (`p2p/src/message.rs`): blocks, transactions,
  weather data, finality votes/certs, DA challenges, genesis handshakes.
- **Goals:** get an invalid block or transaction accepted; fork honest nodes; poison the
  mempool; exhaust node resources.
- **Assumed budget:** a few commodity hosts; no stake; no operator token.
- **What stops them today:**
  - Blocks: full content validation (`verify_block`) + fork choice; invalid content is
    rejected. **But** blocks carry no producer signature, so a peer *may* author
    content-valid blocks and reorg honest nodes up to `MAX_REORG_DEPTH = 100` — **NOT
    DEFENDED** at the identity layer (PROTOCOL_SPEC D3, #2220), and the finality
    backstop that would bound this is inert (D1, #1501).
  - Transactions: full admission validation (`pool.rs`) — signatures, nonces, replay
    keys, double-spend, size caps. Forged transfers/stakes/oracle payloads are rejected
    fail-closed.
  - Resource exhaustion: per-peer reputation scoring with penalty/graylist
    (`native-chain/src/bin/launch_api/peer_reputation.rs`), permissionless-submit rate
    limiting with a bounded replay map (#2077/#2079), mempool capacity + fee-ordered
    eviction — enforced at admission (`mempool_max_txs_per_sender`,
    `shared-protocol/src/mempool/pool.rs:322`; `mempool_max_size_bytes`, `:136`).
  - Block size: **NOT DEFENDED** (#2700). This entry previously claimed
    `MAX_BLOCK_SIZE = 32 MiB` as a mitigation. That constant is referenced by **zero**
    lines of code outside its own declaration
    (`shared-protocol/src/common/constants.rs:143`), and the workspace's only
    block-size comparison (`consensus-engines/src/security/attack_prevention.rs:279`)
    sits in `AttackPrevention::validate_block`, which `native-chain` never calls.
    A deliberately-RED test in #2701 exercises this, but the claim does not rest on it:
    #2701 is an unmerged draft whose red no gate yet runs (#2711 wires that), so the
    supporting evidence here is the direct one — on `main` `1cf2873e2` each of the three
    constants appears in exactly one `.rs` file, its own declaration, which a single
    `rg MAX_BLOCK_SIZE` reproduces. The practical bound on an inbound
    block today is the gossip `max_transmit_size` of 1 MiB (`p2p/src/config.rs:444` →
    `p2p/src/behaviour.rs:187`) — a transport constant doing protocol duty by accident,
    not a declared ceiling. `MAX_TRANSACTION_DATA_SIZE` and `MEMPOOL_CAPACITY` are dead
    in the same way; the mempool bounds above are real because they come from config
    fields, not from the constant that shares the name. *(Every line reference in this
    entry is pinned to `main` `1cf2873e2` and was re-verified there on 2026-08-30; check
    the symbol, not the line, if the tree has moved since.)*
  - Malformed bytes: every `bincode::deserialize` on the P2P path is fallible and logs +
    `continue`s (`p2p_events.rs`); a decode failure never panics.

### A2 — Malicious validator (block producer)

There is no on-chain validator set for block production (PROTOCOL_SPEC §4), so this class
is really *"whoever can get blocks accepted."*

- **Single malicious producer:**
  - **Capabilities:** holds an `Operator` token or reaches the P2P port; chooses block
    contents and ordering.
  - **Goals:** censor transactions, extract MEV via ordering, mint beyond schedule,
    rewrite recent history.
  - **What stops them today:** minting is bounded by `verify_block` (coinbase ≤
    `adjusted_block_reward`, INV-ECON-2; per-block ≤ `MAX_BLOCK_EMISSION`; supply ≤
    `MAX_SUPPLY`) and validated identically by every syncing peer, so an
    over-minting block is rejected network-wide. Ordering/censorship is **NOT
    DEFENDED** (no proposer rotation, no inclusion lists). History rewrite is bounded
    only by `MAX_REORG_DEPTH` because finality is inert (D1).
- **Colluding validators below threshold:**
  - The 2/3-stake finality threshold (`bft_finality.rs`) is the intended defence: <1/3
    Byzantine stake cannot finalize a conflicting block, and equivocation is detected and
    slashable. **This is entirely inert on the running network** (D1, #1501): no votes
    are cast, so "below threshold" has no operational meaning today.
- **Assumed budget:** for the intended (wired) design, < 1/3 of total staked weight.

### A3 — Malicious RPC client

- **Capabilities:** reaches the HTTP API; may hold a `Reader`, `Contributor`, `Oracle`,
  or `Operator` token, or none.
- **Goals:** privilege escalation across the role lattice; mutate state it should not;
  read data it should not; DoS via expensive queries.
- **What stops them today:** the role lattice
  (`shared-protocol/src/security/api_auth.rs`): Operator ⊇ {Contributor, Oracle, Reader};
  Contributor/Oracle ⊇ Reader; Reader read-only. `require_role` guards every mutating
  route. The node fail-fasts at boot with no tokens (INV-PRIV-2); the
  `WRFCOIN_ALLOW_NO_AUTH=1` escape hatch exists in **debug builds only** — the check is
  `#[cfg(debug_assertions)]`-gated in `collect_token_store` (`launch_api/main.rs`), so
  release binaries ignore the variable and always reject an empty token store (enforced
  by a release-only regression test). A release-build attacker cannot use this path.
  Owner-scoped websocket channels check identity claims, not just role.
  - **Gaps:** authorization is a single bearer token per role — no multi-sig on any
    privileged path (D4). Governance proposer identity is an unverified
    `X-Proposer-Address` header (D5). The three state-forking admin APIs
    (NFT/oracle/debug-seed) are Operator-gated *and* flag-gated off by default because a
    single call forks the state root (#1764/#1775/#1811).

### A4 — Malicious dependency maintainer

- **Capabilities:** controls an upstream Rust crate or npm package in the build graph;
  can ship a malicious version.
- **Goals:** exfiltrate keys, backdoor consensus, tamper with builds.
- **What stops them today:** `supply-chain-security.yml` and `sbom-generate.yml` generate
  an SBOM and scan the dependency graph; `docker-security-scan.yml` scans images.
  `release-signing.yml` GPG-signs the release binaries and their SHA-256 checksums.
  **Gaps:** scanning is detective, not preventive; there is no vendored-dependency
  pin-and-review gate; the release build is not independently rebuilt-and-compared
  (see A5); and the Dependabot backlog on the default
  branch (231 open alerts as of 2026-07-22 — an unstable count; read it as
  "hundreds", not as a fixed number) means the signal-to-noise for a genuinely
  malicious injection is poor. Treat this as partially defended.

### A5 — Release-signing-key thief

- **Capabilities:** has stolen the GitHub release-signing credentials / signing keys used
  by `release-signing.yml`.
- **Goals:** ship a backdoored binary that operators install as an authentic release.
- **What stops them today — stated as actually implemented in `release-signing.yml`, not as
  intended:** the workflow GPG-detached-signs each release binary and a `CHECKSUMS.txt` of
  their SHA-256 sums (`gpg --armor --detach-sign`). An operator who has the project's public
  GPG key can verify a downloaded binary was signed by the release key. **That is the whole
  of the control.** The workflow does **not** use cosign/sigstore, does **not** produce a
  attestation, and does **not** rebuild-and-compare. The prior unsigned SLSA-shaped
  `PROVENANCE.json`, unused OIDC permission, and self-asserted `"reproducible": true` claim
  were removed in #2263 rather than presented as assurance. **Consequence for this adversary:**
  a thief of the GPG release key can produce a fully signed backdoored release, and nothing else in the
  pipeline detects it — there is no cosign transparency-log entry, no independent-build
  comparison, and nothing forces operators to reproduce before installing. **Weakly
  defended**, with the boundary now stated honestly as GPG authentication only.

### A6 — Malicious sensor / station operator

- **Capabilities:** runs a registered weather station / oracle; submits commits, reveals,
  attestations, and calibration data.
- **Goals:** earn rewards for fabricated or replayed weather data; corrupt the data set;
  grief honest stations via collusion.
- **What stops them today:** commit-reveal binds a station-authorized ed25519 signature
  (v2 domain, chain-bound, #1848/#2034); reveals must match a pending commit inside the
  window; oracle attestations need `oracle_set` membership, a per-payload nonce
  (`spent_oracle_nonces`, replay-proof), and a 2/3+1 quorum
  (`compute_oracle_quorum`); data-quality range checks, spatial-consistency scoring, and
  collusion detection (#808) slash fabricated data (`shared-protocol/src/security/slashing.rs`, bounded by a
  per-block circuit breaker). Reward claims use MLSAG ring signatures (min ring 7) with
  key-image dedup. **Gap:** below 3 oracles the quorum collapses to trusting ≤ 2 keys —
  the system is explicitly centralized there (PROTOCOL_SPEC §2).

### A7 — Network-level adversary

- **Capabilities:** controls or degrades network paths — can partition, delay, drop, or
  eclipse a node (control all of a victim's peer connections).
- **Goals:** split the network into divergent chains; isolate a victim onto a false chain;
  stall liveness.
- **What stops them today:** partition policy (`shared-protocol/src/consensus/partition_policy.rs`)
  puts a minority-partition node into a read-only mempool mode (#1217) so it does no work
  it cannot finalize; `NODE_SYNC_STALE_AFTER_SECS = 600` surfaces staleness on
  `/readyz`/`/node/sync/status`; genesis-handshake disconnect stops cross-genesis peering
  (INV-GEN-1). **Gaps:** eclipse resistance depends on peer diversity (`MAX_PEERS = 50`,
  DHT) with no formal guarantee; and because finality is inert (D1), a partition healing
  after > 100 blocks of divergence cannot auto-reconcile via the reorg guard and needs
  operator intervention.

## Trust boundaries

Every crossing where untrusted input enters a trusted context, the interface that
receives it, and what is validated there. All external input is untrusted (`AGENTS.md`).

| Boundary | Crossing interface | Validated at the crossing |
|---|---|---|
| Public P2P → node | `p2p/src/message.rs` (`WrfcoinMessage`), consumed by `native-chain/src/bin/launch_api/p2p_events.rs` | fallible bincode decode (no panic); blocks → `verify_block` + fork choice; txs → full mempool admission; finality votes → ed25519 verify + validator-set membership (when gadget attached); genesis handshake → hash-match-or-disconnect |
| Public RPC → node | `shared-protocol/src/security/api_auth.rs` role lattice + `require_role` in `native-chain/src/bin/launch_api/routes/*.rs` | bearer-token → role; per-route `require_role`; nonce + timestamp + replay + rate limit on the permissionless lane; owner-scoped identity checks on private WS channels |
| Sensor/oracle ingest → chain | `sensor-attestation/src/lib.rs`, `oracle-packet/src/lib.rs`, commit/reveal + `OracleAttestation` paths in `shared-protocol/src/blockchain/transaction.rs` | device/station ed25519 signatures over chain-bound v2 domains; per-payload nonce replay sets; quorum; data-quality + spatial-consistency scoring; window arithmetic |
| Bridge / cross-chain → chain | `shared-protocol/src/cross_chain/` (`adapters.rs`, `relay_queue.rs`, `types.rs`); operator bridge-safety routes | operator-gated halt/resume/caps/challenge flow; relay-queue status tracking; exposure caps |
| Build pipeline → release artifact | `.github/workflows/release-signing.yml` (+ `supply-chain-security.yml`, `sbom-generate.yml`, `docker-security-scan.yml`) | GPG detached signatures on binaries + SHA-256 `CHECKSUMS.txt`; SBOM; dependency + image scan. **No cosign, signed provenance, transparency log, or independently verified reproducibility; #2263 removed the former unsigned provenance artifact and misleading claim.** |
| Operator env → node | process environment read in `native-chain/src/bin/launch_api/main.rs`/`helpers.rs` | token presence fail-fast (INV-PRIV-2); admin-API flags default off; CORS origin policy in production |

## Privileged capabilities

Mirrors PROTOCOL_SPEC.md §10, adding — for each capability — the authorizing key, whether
it is reachable from a public interface, and what an attacker gains by holding it.

| Capability | Authorizing key | Public-reachable? | Attacker gain if held |
|---|---|---|---|
| Produce blocks (`/blocks/mine`) | `WRFCOIN_OPERATOR_TOKEN` | Yes (HTTP), and gossip needs no key at all | Full control of chain extension: censor, order, reorg ≤ 100, mint up to the schedule ceiling |
| Purge/clear mempool | Operator token | Yes (HTTP) | Censor pending transactions network-locally |
| Bridge halt/resume/caps/challenge | Operator token | Yes (HTTP) | Freeze or unfreeze cross-chain value flow; adjust exposure caps |
| Set partition mode | Operator token | Yes (HTTP) | Force a node read-only (self-DoS) or clear a legitimate partition latch |
| Governance proposal lifecycle | Operator token (+ unauthenticated proposer header, D5) | Yes (HTTP) | Drive governance parameter changes; proposer attribution is spoofable |
| Mutate oracle set / config | Operator token **and** `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1` | Flag off by default | Admit/evict oracles — and fork this node's state root (#1775) |
| Register/revoke signing-key epochs | Operator token (+ admin-API flag family) | Flag off by default | Rotate protocol-role keys out of band; state-root fork risk |
| Seed balances (`/debug/seed-account`) | Operator token **and** `WRFCOIN_ENABLE_DEBUG_SEED_API=1` | Flag off by default | Mint arbitrary balance/stake from nothing; forks state root (#1811) |
| NFT mint/transfer | Operator token **and** `WRFCOIN_ENABLE_NFT_MUTATION_API=1` | Flag off by default | Mutate NFT state; forks state root (#1764) |
| NFT verify | **Contributor** token **and** `WRFCOIN_ENABLE_NFT_MUTATION_API=1` (`verify_nft`, `routes/nft.rs`) | Flag off by default | Mutate NFT verification state; forks state root (#1764) — Contributor, not Operator |
| Disable all auth (**debug builds only** — compiled out of release binaries, which reject an empty token store unconditionally) | `WRFCOIN_ALLOW_NO_AUTH=1` + a debug-profile binary | Env only, debug builds | Turns every Operator route into an open endpoint on a dev node; **no release attack surface** |
| Trusted snapshot signers | `WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS` | Env only | Have the node accept attacker-signed state snapshots during sync |
| Sign releases | GitHub repo secrets / signing keys | No (CI) | Ship a signed backdoored binary (A5) |
| Oracle attestations | `WRFCOIN_ORACLE_TOKEN` + oracle-set membership | Yes (HTTP) | Contribute toward attestation quorum |
| Contribution commit/reveal | `WRFCOIN_CONTRIBUTOR_TOKEN` | Yes (HTTP) | Submit weather contributions for reward |

Key observations for the auditor:

1. Every HTTP privileged capability rests on a **single bearer token per role** — no
   multi-sig, no threshold, no hardware second factor at the API layer (D4). Compromise
   of `WRFCOIN_OPERATOR_TOKEN` is game-over for a single node.
2. The most dangerous state-mutating routes (oracle/signing-key/debug-seed/NFT) are
   Operator-gated *and* disabled by default precisely because they fork the consensus
   state root; their safety depends on the flags staying off in multi-node deployments,
   which is an operational control, not a cryptographic one (INV-PRIV-1).
3. Block production needs no key over gossip at all (D3, #2220) — the token gate only
   covers the HTTP `/blocks/mine` route.

## Assets

What is worth stealing, at testnet versus mainnet, and why.

| Asset | Testnet value | Mainnet value | Rationale |
|---|---|---|---|
| WRF balances | **None — declared valueless** | High | Testnet balances have no exchange value and exist to exercise the earn loop; they are explicitly not an asset to defend. |
| `WRFCOIN_OPERATOR_TOKEN` | Medium | Critical | Controls block production and every privileged route; on mainnet this is the crown jewel. |
| Oracle / signing keys | Low–Medium | High | Fabricate accepted weather data (subject to quorum); rotate protocol keys. |
| Release-signing credentials | Medium | Critical | Ship trusted backdoored binaries to every operator (A5). |
| Weather data integrity | Medium | High | The product *is* trustworthy weather data; fabrication corrupts the core value proposition even where coins are valueless. |
| Node availability / liveness | Medium | High | A halted chain (as happened for 28h on 2026-07-08 when commit-auth broke, see project history) stops the earn loop and erodes trust. |

Testnet balances being valueless is the single most important scope-limiter: it removes
theft-of-funds from the testnet threat surface and lets the audit concentrate on
integrity (weather data, state-root agreement) and availability.

## Out of scope

Stated explicitly so the external-audit scope is unambiguous. On the **current testnet**,
the following are **not defended** and MUST NOT be treated as in-scope failures:

1. **≥ 1/3 Byzantine stake collusion** — the finality gadget that would bound this is
   inert (D1, #1501); on testnet there is no staking-secured finality at all.
2. **A dishonest single operator** — chain safety today explicitly assumes the one
   block-producing operator and its `Operator` token are honest (PROTOCOL_SPEC §9.3).
   Operator misbehaviour is an accepted risk, not a defended-against threat.
3. **Theft of testnet WRF** — balances are valueless by declaration.
4. **Block-producer identity spoofing over P2P** — blocks are unsigned (D3, #2220);
   production permissioning is HTTP-token-only.
5. **Eclipse of a node with no diverse peers** — peer diversity is best-effort
   (`MAX_PEERS`, DHT), not guaranteed.
6. **MEV / transaction-ordering fairness** — no proposer rotation or inclusion lists
   exist; ordering is at the producer's discretion.
7. **The six unwired consensus engines** (D6) as *L1 consensus* attack surface — they are
   unreachable from the node binary (they remain supply-chain surface, A4).

Everything **in scope** for the current testnet is, by elimination: transaction-layer
authorization and replay protection, state-root determinism and producer/applier parity,
mempool admission correctness, oracle/weather-data integrity and quorum, the role-based
API authorization lattice, resource-exhaustion resistance, and the supply-chain/release
pipeline.

## Mapping to protocol invariants

Each adversary class is bounded (where it is bounded at all) by named invariants from
PROTOCOL_SPEC.md:

- A1 malicious peer → INV-BLOCK-1/2/3, INV-TX-1/2, INV-CONS-1/2/3/4 (bound); identity
  layer **unbounded** (D3).
- A2 malicious validator → INV-ECON-1/2, INV-RWD-2 (minting bounded); INV-FIN-2 (would
  bound reorg — inert, D1).
- A3 malicious RPC client → INV-PRIV-1/2 (authorization + fail-fast).
- A6 malicious sensor → INV-TX-1/2, oracle quorum (§2), INV-SLASH-1.
- A7 network adversary → INV-GEN-1, INV-BLOCK-3/4 (INV-BLOCK-4 vacuous under D1).

---

*Maintenance rule: any PR that adds a privileged route, changes the role lattice, adds a
trust boundary, or wires the finality gadget MUST update this document in the same change
set — the out-of-scope list in particular gates the external-audit RFP.*
