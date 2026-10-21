# WRFCoin Constitutional Enforceability Matrix

Issue #394 -- Maps each Bill of Rights entry to protocol-level enforcement controls.

---

## Legend

| Enforcement Level | Meaning |
|-------------------|---------|
| **Machine** | Enforced deterministically by consensus/state-machine code; violations are rejected or penalized automatically |
| **Semi-machine** | Protocol provides tooling (APIs, data structures) but enforcement requires off-chain coordination or governance action |
| **Policy** | Enforced through governance norms, social consensus, or legal/licensing terms; no automatic on-chain enforcement |

---

## Matrix

| # | Right | Control | Enforcement Level | Owner Module(s) | Gap / Follow-up |
|---|-------|---------|-------------------|-----------------|-----------------|
| 1 | **Right to Contribute** | Open commit-reveal: any address can submit `ContributionCommit` / `ContributionReveal` transactions. `ActorClassPolicy` enforces equal treatment across Human, AI Agent, Robot classes. No allowlist required for basic contribution. | **Machine** | `shared-protocol/src/contributions/commit_reveal.rs`, `shared-protocol/src/governance/actor_policy.rs`, `shared-protocol/src/blockchain/state.rs` | Rate limiting exists at the API layer (`native-chain/src/bin/launch_api/`) but is not constitutionally bounded; governance should define maximum rate-limit parameters to prevent de facto contribution denial. |
| 2 | **Right to Fair Compensation** | `TokenEmissionManager` computes deterministic rewards per role (Analyzer 35%, Collector 30%, Keeper 25%, Offsetter 10%), region scarcity, accuracy, and reputation. `AiEmissionController` publishes adjustments as governance proposals with `ai_generated: true` and challenge period. Quality bands (`DataQualityPolicy`) gate reward multipliers. | **Machine** | `shared-protocol/src/blockchain/token_emission.rs`, `shared-protocol/src/blockchain/ai_emission.rs`, `shared-protocol/src/consensus/proof_of_publishing.rs` | AI emission adjustments use floating-point (`f64`) region weights and throttle -- consider migrating to fixed-point for full determinism (tracked by f64-to-u64 migration pattern in PR #505). |
| 3 | **Right to Data Access** | All validated blocks and transactions are stored on-chain and queryable via REST API routes: `/chain/*`, `/transactions/*`, `/governance/*`. Snapshot sync (`snapshot_sync.rs`) enables new nodes to bootstrap full state. | **Semi-machine** | `native-chain/src/bin/launch_api/routes/chain.rs`, `native-chain/src/bin/launch_api/routes/transactions.rs`, `native-chain/src/bin/launch_api/routes/governance.rs`, `shared-protocol/src/blockchain/snapshot_sync.rs` | No protocol-level guarantee that API nodes remain publicly accessible. Data availability depends on node operators running the API. Consider a minimum-node-count governance parameter or archival-node incentive. |
| 4 | **Right to Privacy** | `PrivacyPolicy` in consensus engine enforces `min_privacy_level` per block height. Privacy crate provides: Pedersen commitments, MLSAG ring signatures, stealth addresses, range proofs, ChaCha20-Poly1305 payload encryption, key-image double-spend prevention. `SelectiveDisclosureProofPayload` allows field-level disclosure control. Privacy API routes require authentication (PR #503). | **Machine** | `privacy/src/` (all modules), `shared-protocol/src/consensus/proof_of_publishing.rs` (`PrivacyPolicy`), `shared-protocol/src/blockchain/transaction.rs` (`SelectiveDisclosureProofPayload`), `native-chain/src/bin/launch_api/routes/privacy.rs` | Private key material replaced with `key_id` registry (PR #503), but key rotation is not yet automated. Wallet encryption uses argon2+chacha20 (PR #504) -- needs periodic re-keying policy. |
| 5 | **Right to Due Process** | `SanctionMatrix` defines 16 `OffenseCode` variants across 5 categories with graduated `SeverityTier` levels. `CompiledPolicy` evaluates (offense, severity, actor_class) deterministically. `PolicyCompiler` validates matrix completeness and range invariants. BLAKE3 content hash anchors policy on-chain. `SlashingReason` triggers slashing through `shared-protocol/src/security/slashing.rs`. Equal liability: actor class does NOT change outcomes. | **Machine** | `shared-protocol/src/governance/sanctions.rs`, `shared-protocol/src/governance/policy_engine.rs`, `shared-protocol/src/governance/actor_policy.rs`, `shared-protocol/src/security/slashing.rs` | No on-chain appeals mechanism. Once a sanction is applied, there is no protocol-defined process for reversal. Recommend adding a `SanctionAppeal` proposal type (follow-up issue needed). |
| 6 | **Right to Governance Participation** | `VotingSystem` trait with `TokenBasedVoting` implementation. `VotingPower` derived from staked balance. 4 vote types: Yes, No, Abstain, Veto. `Proposal` supports ParameterChange, ProtocolUpgrade, FundingProposal, Text. `GovernedParameter` enum lists all DAO-controlled parameters (oracle set, quorum, stake required, emission rate, quality thresholds, etc.). Governance API routes exposed. | **Machine** | `shared-protocol/src/governance/voting.rs`, `shared-protocol/src/governance/proposal.rs`, `shared-protocol/src/governance/mod.rs` (`GovernedParameter`), `native-chain/src/bin/launch_api/routes/governance.rs` | Delegation/liquid democracy not implemented -- voting requires direct participation. No vote-weight cap, so whale concentration risk exists. Consider governance parameter for max voting power percentage. |
| 7 | **Right to Fork** | Source code is published under open license. Consensus rules are fully defined in `proof_of_publishing.rs`. Chain state is exportable via snapshot sync. No code obfuscation or proprietary dependencies in the consensus path. | **Policy** | `shared-protocol/src/consensus/proof_of_publishing.rs`, `shared-protocol/src/blockchain/snapshot_sync.rs` | Fork right is license-dependent, not protocol-enforced. Ensure license file remains permissive. No on-chain mechanism prevents a governance majority from proposing license changes (would require constitutional protection). |
| 8 | **Right to Interoperability** | Bridge routes (`/bridge/*`) exposed via API. Weather data schemas support standard formats (METAR, SYNOP, BUFR metadata, structured). `WeatherPayloadParseOptions` in state controls format acceptance. Oracle attestation framework supports external data sources. | **Semi-machine** | `native-chain/src/bin/launch_api/routes/bridge.rs`, `shared-protocol/src/weather/schema.rs`, `shared-protocol/src/blockchain/state.rs` (`WeatherPayloadParseOptions`), `shared-protocol/src/oracle/attestation.rs` | Bridge implementations are API-level only in core4; actual cross-chain bridge contracts live in `wrfcoin/smart-contracts`. No protocol-level guarantee of bridge liveness. Interoperability standards are not governance-controlled parameters yet. |
| 9 | **Right to Transparency** | Constitution hash (SHA3-256) anchored in governance state via `CONSTITUTION_SOURCE_PATH`. Sanction matrix hash (BLAKE3) anchored via `CompiledPolicy::content_hash()`. All proposals and votes recorded on-chain. Governance state transitions are deterministic and auditable in `shared-protocol/src/blockchain/state.rs`. Quality policy events logged via `QualityPolicyEventRecord`. | **Machine** | `shared-protocol/src/blockchain/state.rs`, `shared-protocol/src/governance/policy_engine.rs`, `shared-protocol/src/governance/proposal.rs` | Emission adjustment rationale strings (`EmissionAdjustment::rationale`) are stored but not independently verifiable. Off-chain governance discussions are not anchored. Consider CID-linking forum discussions to on-chain proposals. |
| 10 | **Right to Exit** | Token transfers are permissionless. Shielded withdrawal supported via `ShieldedWithdrawalPayload`. Wallet private keys encrypted at rest with argon2+chacha20 (PR #504). No lock-in beyond protocol-defined unbonding/staking periods. Stake unlock governed by `stake_required` / `stake_locked` on `Account`. | **Semi-machine** | `shared-protocol/src/blockchain/state.rs` (`Account`), `shared-protocol/src/blockchain/transaction.rs` (`ShieldedWithdrawalPayload`), `native-chain/src/bin/launch_api/routes/stake.rs` | Unbonding period duration is not yet a `GovernedParameter` -- currently hardcoded. Should be added to `GovernedParameter` enum so governance can adjust it. No data export/portability tool for contributor history (would strengthen exit right). |

---

## Summary of Gaps

| # | Gap | Severity | Suggested Follow-up |
|---|-----|----------|---------------------|
| 1 | API rate limits not constitutionally bounded | Medium | Add `MaxRateLimit` to `GovernedParameter` |
| 2 | AI emission uses f64 for region weights | Low | Continue f64-to-fixed-point migration |
| 3 | No minimum public API node guarantee | Medium | Archival-node incentive or governance parameter |
| 4 | Key rotation not automated | Low | Implement periodic re-keying in key registry |
| 5 | No on-chain appeals for sanctions | High | Add `SanctionAppeal` proposal type |
| 6 | No delegation or vote-weight cap | Medium | Implement liquid delegation and/or max-power cap |
| 7 | Fork right is license-dependent only | Low | Constitutionally protect license terms |
| 8 | Bridge liveness not protocol-guaranteed | Medium | Cross-chain health as governance parameter |
| 9 | AI rationale strings not independently verifiable | Low | CID-link rationale evidence |
| 10 | Unbonding period not governance-controlled | Medium | Add to `GovernedParameter` enum |

---

_See also: [Platform Constitution](constitution.md) | [Governance README](README.md)_
