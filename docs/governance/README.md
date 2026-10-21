# WRFCoin Governance Documentation

This directory contains the governance framework documents for the WRFCoin platform.

## Documents

| Document | Description | Issue |
|----------|-------------|-------|
| [constitution.md](constitution.md) | Platform Constitution -- Preamble, definitions, Bill of Rights (10 rights), governance structure, token economics, data standards, amendment process, and initial laws | #380 |
| [enforceability-matrix.md](enforceability-matrix.md) | Enforceability Matrix -- Maps each constitutional right to protocol controls, enforcement level (machine/semi-machine/policy), owner modules, and identified gaps | #394 |
| [constitution-hash-anchoring.md](constitution-hash-anchoring.md) | Technical specification for SHA3-256 hash anchoring of the constitution in governance state |
| [law-book-plain-language-style-guide.md](law-book-plain-language-style-guide.md) | Style guide for drafting Law Book entries |
| [offense-taxonomy.toml](offense-taxonomy.toml) | Machine-readable offense taxonomy (mirrors `sanctions.rs`) |
| [governance_expansion_plan.md](governance_expansion_plan.md) | Expansion roadmap for the governance system |
| [implementation_guide.md](implementation_guide.md) | Implementation guide for governance features |
| [makerdao-comparison-analysis.md](makerdao-comparison-analysis.md) | Comparative analysis with MakerDAO governance |

## Key Code References

| Module | Path | Role |
|--------|------|------|
| Governance core | `shared-protocol/src/governance/` | Proposals, voting, sanctions, policy engine |
| Consensus | `shared-protocol/src/consensus/proof_of_publishing.rs` | Block validation, reward/slashing, privacy enforcement |
| State machine | `shared-protocol/src/blockchain/state.rs` | Law Book, constitution hash, governance parameters |
| Token emission | `shared-protocol/src/blockchain/token_emission.rs` | Reward calculation |
| AI emission | `shared-protocol/src/blockchain/ai_emission.rs` | Coverage-based emission adjustment |
| Privacy | `privacy/` | Ring signatures, stealth addresses, Pedersen commitments |
| Slashing | `shared-protocol/src/security/slashing.rs` | Slashing reason types and execution |
| API routes | `native-chain/src/bin/launch_api/routes/governance.rs` | Governance REST endpoints |
