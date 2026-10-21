# WRFCoin Platform Constitution

Status: Draft v2 (Issue #380)

---

## Preamble

WRFCoin exists to secure trustworthy environmental information, weather observations, and
forecasting intelligence through open participation, cryptographic integrity, and accountable
governance. This constitution defines the foundational rules that bind protocol operators,
contributors, and governance actors. It enshrines the rights of all participants and establishes
the structure through which the platform evolves.

All protocol actors -- human, AI agent, or autonomous device -- are subject to the same rules
and entitled to the same rights under this constitution.

---

## Article I -- Definitions

1. **Platform** -- The WRFCoin protocol, its consensus rules, smart contracts, and supporting
   infrastructure.
2. **Participant** -- Any entity (human, AI agent, robot, or organization) that interacts with
   the platform by contributing data, staking tokens, voting, or consuming services.
3. **Contributor** -- A participant who submits weather data, forecasts, or other environmental
   payloads to the blockchain via commit-reveal or direct publication.
4. **Validator** -- A participant who produces or attests blocks under the Proof-of-Publishing
   consensus mechanism (`shared-protocol/src/consensus/proof_of_publishing.rs`).
5. **Oracle** -- An authorized entity that attests to off-chain data quality or forecast accuracy,
   registered in the `OracleRegistry` (`shared-protocol/src/oracle/attestation.rs`).
6. **Governor** -- A participant who holds voting power and exercises governance rights through
   on-chain proposals (`shared-protocol/src/governance/proposal.rs`).
7. **Law** -- A codified rule in the on-chain Law Book, enforced deterministically by the state
   machine (`shared-protocol/src/blockchain/state.rs`).
8. **Token (WRF)** -- The native unit of account, denominated in base units
   (1 WRF = 10^8 base units). Emission is controlled by `TokenEmissionManager`
   (`shared-protocol/src/blockchain/token_emission.rs`).
9. **Sanction** -- A penalty applied for protocol violations, as defined in the offense taxonomy
   and sanction matrix (`shared-protocol/src/governance/sanctions.rs`).
10. **Constitutional Amendment** -- A change to this document, requiring a supermajority threshold
    (75%) as defined in `CONSTITUTIONAL_SUPERMAJORITY_PERCENT`
    (`shared-protocol/src/blockchain/state.rs`).

---

## Article II -- Bill of Rights

Every participant is guaranteed the following rights. These rights are constitutional in nature
and may only be modified through the amendment process defined in Article VI.

### Right 1: Right to Contribute

Any participant may submit environmental data, weather observations, or forecasts to the platform
without prior permission, subject only to protocol-defined quality and format requirements.
Contribution is open to all actor classes (Human, AI Agent, Robot) under equal terms as enforced
by the `ActorClassPolicy` (`shared-protocol/src/governance/actor_policy.rs`).

### Right 2: Right to Fair Compensation

Contributors shall receive fair, deterministic compensation for accepted data. Rewards are
calculated by the `TokenEmissionManager` based on role, region scarcity, accuracy, and
reputation. The `AiEmissionController` (`shared-protocol/src/blockchain/ai_emission.rs`)
may adjust emission parameters, but all adjustments are published as governance proposals
with a challenge period allowing human override.

### Right 3: Right to Data Access

All validated on-chain data shall be publicly accessible. Participants may query chain state,
contribution history, and governance records through the API
(`native-chain/src/bin/launch_api/`). Data availability is a protocol-level guarantee.

### Right 4: Right to Privacy

Participants may use privacy-enhancing technologies provided by the platform:
ring signatures (MLSAG), stealth addresses, Pedersen commitments, range proofs, and
payload encryption (`privacy/` crate). The protocol enforces a minimum privacy level
via `PrivacyPolicy` in the consensus engine, but participants may always choose a
higher privacy level. Selective disclosure proofs allow participants to reveal only
the data fields they choose (`SelectiveDisclosureProofPayload` in
`shared-protocol/src/blockchain/transaction.rs`).

### Right 5: Right to Due Process

No participant shall be sanctioned without a defined offense, severity classification, and
deterministic outcome. The sanction matrix (`shared-protocol/src/governance/sanctions.rs`)
defines 16 offense codes across 5 categories (Data, Consensus, Oracle, Governance, Economic),
each with graduated severity tiers. The `CompiledPolicy`
(`shared-protocol/src/governance/policy_engine.rs`) enforces equal liability across all
actor classes. Slashing is executed through `SlashingReason`
(`shared-protocol/src/security/slashing.rs`) with on-chain evidence requirements.

### Right 6: Right to Governance Participation

Any token holder may propose and vote on governance actions. The governance system supports
four vote types (Yes, No, Abstain, Veto) with stake-weighted voting power
(`shared-protocol/src/governance/voting.rs`). Proposals include parameter changes,
protocol upgrades, funding proposals, and text signals
(`shared-protocol/src/governance/proposal.rs`). Governed parameters are enumerated in
`GovernedParameter` (`shared-protocol/src/governance/mod.rs`).

### Right 7: Right to Fork

The protocol source code shall remain open. Any participant or group may fork the protocol
at any time. The canonical chain is defined by consensus rules, not by any single entity.

### Right 8: Right to Interoperability

The platform shall maintain cross-chain bridge interfaces and standardized data formats.
Weather data schemas (`shared-protocol/src/weather/schema.rs`) follow open standards.
Bridge routes are exposed via the API (`native-chain/src/bin/launch_api/routes/bridge.rs`).

### Right 9: Right to Transparency

All governance proposals, voting outcomes, sanction events, emission adjustments, and
constitutional amendments shall be recorded on-chain. The constitution itself is
hash-anchored in governance state (SHA3-256), ensuring tamper-evidence. The `CompiledPolicy`
content hash (BLAKE3) anchors the sanction matrix on-chain.

### Right 10: Right to Exit

Any participant may withdraw their stake, transfer their tokens, and cease participation
at any time, subject only to protocol-defined unbonding periods. No participant shall be
locked into the platform against their will. Shielded withdrawal is supported via the
privacy subsystem.

---

## Article III -- Governance Structure

### Section 1: Legislative Function

Governance proposals serve as the legislative mechanism. Any governor may submit a proposal
of type `ParameterChange`, `ProtocolUpgrade`, `FundingProposal`, or `Text`. Proposals
require a deposit (`GOVERNANCE_DEFAULT_DEPOSIT`) and pass through statuses: Active,
Passed, Executed, Rejected, Failed, or Cancelled. Quorum is defined by
`GOVERNANCE_DEFAULT_QUORUM_PERCENT`.

Constitutional proposals (those modifying this document or protected parameter families)
require a 75% supermajority (`CONSTITUTIONAL_SUPERMAJORITY_PERCENT`).

### Section 2: Executive Function

The `GovernanceImplementor` (`shared-protocol/src/governance/implementation.rs`) provides
automatic execution of passed proposals. `AutomaticImplementation` applies approved
parameter changes to protocol state deterministically, ensuring no human intermediary
is required for execution.

### Section 3: Judicial Function

The sanction system serves as the judicial arm. The `SanctionMatrix` defines outcomes for
all (offense, severity, actor-class) combinations. The `PolicyCompiler`
(`shared-protocol/src/governance/policy_engine.rs`) validates matrix completeness and
range invariants. Enforcement is deterministic: identical offenses produce identical
sanctions regardless of actor identity or class.

---

## Article IV -- Token Economics

1. **Emission**: Block rewards are calculated by `TokenEmissionManager` with decay-based
   emission reduction. Role-based distribution: Analyzer 35%, Collector 30%, Keeper 25%,
   Offsetter 10% (`shared-protocol/src/blockchain/token_emission.rs`).

2. **AI-Governed Adjustment**: The `AiEmissionController` adjusts emission throttle and
   region weights based on network coverage metrics. All adjustments are published as
   proposals with `ai_generated: true` for human override
   (`shared-protocol/src/blockchain/ai_emission.rs`).

3. **Staking**: Accounts have `stake_required` and `stake_locked` balances. Staking
   requirements are governance-controlled via `GovernedParameter::StakeRequired`.

4. **Fee-less Contributions**: Weather data transactions with zero fee are permitted when
   accompanied by a valid `ComputeReceipt`, activated at
   `COMPUTATIONAL_FEE_ACTIVATION_HEIGHT`
   (`shared-protocol/src/consensus/proof_of_publishing.rs`).

5. **Slashing**: Protocol violations result in stake slashing per the sanction matrix.
   Slash amounts are expressed in basis points (10,000 = 100%) for precision.

---

## Article V -- Data Standards

1. **Quality Scoring**: Weather data is validated against `DataQualityPolicy` producing
   scores in bands (High, Acceptable, Borderline, WarningOnly, Low). Quality thresholds
   are governance-controlled parameters.

2. **Commit-Reveal**: Data submissions use a commit-reveal protocol
   (`shared-protocol/src/contributions/commit_reveal.rs`) with governance-controlled
   commit and reveal windows.

3. **CID Requirements**: The Law Book may require Content Identifier (CID) backed
   submissions for certain participant classes (`LAW-001-CID-MANDATORY` in
   `shared-protocol/src/blockchain/state.rs`).

4. **Format Families**: Supported data formats include METAR, extended sensor, SYNOP,
   BUFR metadata, and structured payloads, each with defined inline-size privileges.

5. **Forecast Verification**: Forecast submissions are verified against manifests
   (`ForecastArtifactManifest`) with oracle-attested accuracy metrics, activated at
   `FORECAST_MANIFEST_ACTIVATION_HEIGHT`.

---

## Article VI -- Amendment Process

1. Amendments to this constitution MUST be submitted as constitutional-class governance
   proposals.

2. Constitutional proposals require a 75% supermajority of participating voting power
   to pass.

3. Upon passage, the `GovernanceImplementor` updates the canonical constitution hash
   in governance state. The hash is computed as SHA3-256 over the normalized document
   content.

4. The canonical source path is `docs/governance/constitution.md`, referenced by
   `CONSTITUTION_SOURCE_PATH` in `shared-protocol/src/blockchain/state.rs`.

5. Amendment history is preserved on-chain through sequential proposal records.

6. No amendment may abolish the Bill of Rights (Article II) in its entirety. Individual
   rights may be modified only through the constitutional amendment process.

---

## Appendix -- Initial Laws

The following laws are enacted at genesis as part of the initial Law Book:

| Law ID | Title | Category | Effect |
|--------|-------|----------|--------|
| LAW-001 | CID Mandatory | Data Integrity | Untrusted participants must back submissions with a Content Identifier |
| LAW-002 | METAR Inline Privilege | Data Format | Verified stations and trusted providers may inline METAR-family payloads up to size limits |

Additional laws may be enacted through the standard governance proposal process
(non-constitutional threshold). All laws must remain consistent with this constitution
(Article III, Section 1 of the former draft; now subsumed by Article III above).

---

_Canonical hash algorithm: SHA3-256_
_Source: `docs/governance/constitution.md`_
_Anchored in: `shared-protocol/src/blockchain/state.rs` (`CONSTITUTION_SOURCE_PATH`)_
