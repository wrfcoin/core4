//! Offense taxonomy and equal-liability sanction matrix.
//!
//! This module defines the canonical offense codes, severity tiers, actor classes,
//! and sanction outcomes for the WRFCoin governance system. The matrix enforces
//! equal liability across all actor classes (Human, AI Agent, Robot).
//!
//! # Design decisions
//!
//! * **Integer basis points (`u16`)** for all slash values. `10_000 = 100%`. No floats.
//! * **`BTreeMap`** for deterministic iteration order (needed for content hashing).
//! * **`blake3`** for content hashing (already a workspace dependency).
//! * **Actor class does NOT change outcomes** — equal liability by design.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::security::slashing::SlashingReason;

// ---------------------------------------------------------------------------
// Offense code
// ---------------------------------------------------------------------------

/// Offense code identifying a specific type of protocol violation.
/// Each variant maps to a canonical code string (e.g., `"DATA-001"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OffenseCode {
    // Data integrity
    DataInvalidReveal,
    DataLowQuality,
    DataFabrication,
    DataPhysicalRangeViolation,
    // Consensus
    ConsensusDoubleSign,
    ConsensusSurroundVote,
    ConsensusDowntime,
    ConsensusInvalidBlock,
    // Oracle
    OracleAttestationFraud,
    OracleUnregistered,
    OracleDoubleAttestation,
    // Governance
    GovernanceVoteManipulation,
    GovernanceBribery,
    // Operational
    OperationalRateLimitAbuse,
    OperationalSpam,
    // Consensus (ZK)
    ConsensusInvalidZKProof,
    // Economic
    EconomicWashTrading,
    EconomicMarketManipulation,
}

impl OffenseCode {
    /// All 18 offense codes in canonical order.
    pub fn all() -> &'static [OffenseCode] {
        use OffenseCode::*;
        &[
            DataInvalidReveal,
            DataLowQuality,
            DataFabrication,
            DataPhysicalRangeViolation,
            ConsensusDoubleSign,
            ConsensusSurroundVote,
            ConsensusDowntime,
            ConsensusInvalidBlock,
            ConsensusInvalidZKProof,
            OracleAttestationFraud,
            OracleUnregistered,
            OracleDoubleAttestation,
            GovernanceVoteManipulation,
            GovernanceBribery,
            OperationalRateLimitAbuse,
            OperationalSpam,
            EconomicWashTrading,
            EconomicMarketManipulation,
        ]
    }

    /// Canonical short code string (e.g. `"DATA-001"`).
    pub fn code_str(&self) -> &'static str {
        match self {
            Self::DataInvalidReveal => "DATA-001",
            Self::DataLowQuality => "DATA-002",
            Self::DataFabrication => "DATA-003",
            Self::DataPhysicalRangeViolation => "DATA-004",
            Self::ConsensusDoubleSign => "CONS-001",
            Self::ConsensusSurroundVote => "CONS-002",
            Self::ConsensusDowntime => "CONS-003",
            Self::ConsensusInvalidBlock => "CONS-004",
            Self::ConsensusInvalidZKProof => "CONS-005",
            Self::OracleAttestationFraud => "ORAC-001",
            Self::OracleUnregistered => "ORAC-002",
            Self::OracleDoubleAttestation => "ORAC-003",
            Self::GovernanceVoteManipulation => "GOVN-001",
            Self::GovernanceBribery => "GOVN-002",
            Self::OperationalRateLimitAbuse => "OPER-001",
            Self::OperationalSpam => "OPER-002",
            Self::EconomicWashTrading => "ECON-001",
            Self::EconomicMarketManipulation => "ECON-002",
        }
    }

    /// Category that this offense belongs to.
    pub fn category(&self) -> OffenseCategory {
        match self {
            Self::DataInvalidReveal
            | Self::DataLowQuality
            | Self::DataFabrication
            | Self::DataPhysicalRangeViolation => OffenseCategory::DataIntegrity,
            Self::ConsensusDoubleSign
            | Self::ConsensusSurroundVote
            | Self::ConsensusDowntime
            | Self::ConsensusInvalidBlock
            | Self::ConsensusInvalidZKProof => OffenseCategory::Consensus,
            Self::OracleAttestationFraud
            | Self::OracleUnregistered
            | Self::OracleDoubleAttestation => OffenseCategory::Oracle,
            Self::GovernanceVoteManipulation | Self::GovernanceBribery => {
                OffenseCategory::Governance
            },
            Self::OperationalRateLimitAbuse | Self::OperationalSpam => {
                OffenseCategory::Operational
            },
            Self::EconomicWashTrading | Self::EconomicMarketManipulation => {
                OffenseCategory::Economic
            },
        }
    }

    /// Plain-language description following the law-book style guide.
    ///
    /// Pattern: `<Who> must <required behavior>. <Consequence or condition>.`
    pub fn plain_language_description(&self) -> &'static str {
        match self {
            Self::DataInvalidReveal => {
                "All participants must submit valid reveals matching their commits. \
                 Invalid reveals undermine round integrity."
            },
            Self::DataLowQuality => {
                "All participants must submit weather data meeting minimum quality thresholds. \
                 Repeated low-quality submissions trigger escalating sanctions."
            },
            Self::DataFabrication => {
                "All participants must submit authentic weather observations. \
                 Fabricated or forged data results in severe sanctions."
            },
            Self::DataPhysicalRangeViolation => {
                "All oracles must submit weather readings within physically plausible ranges. \
                 Values outside hard bounds indicate fabricated or malfunctioning sensor data."
            },
            Self::ConsensusDoubleSign => {
                "Validators must not sign conflicting blocks at the same height. \
                 Double signing threatens chain finality."
            },
            Self::ConsensusSurroundVote => {
                "Validators must not cast votes that surround or are surrounded by their prior votes. \
                 Surround voting enables long-range attacks."
            },
            Self::ConsensusDowntime => {
                "Validators must maintain minimum uptime requirements. \
                 Extended downtime degrades network liveness."
            },
            Self::ConsensusInvalidBlock => {
                "Validators must only propose blocks that pass all validity checks. \
                 Invalid block proposals waste network resources."
            },
            Self::ConsensusInvalidZKProof => {
                "All participants must submit valid zero-knowledge proofs. \
                 Invalid ZK proofs indicate forged credentials or tampered data."
            },
            Self::OracleAttestationFraud => {
                "Oracles must submit truthful attestations with valid signatures. \
                 Fraudulent attestations corrupt data integrity."
            },
            Self::OracleUnregistered => {
                "Oracles must be registered in the active oracle set before submitting attestations. \
                 Unregistered oracle activity is rejected."
            },
            Self::OracleDoubleAttestation => {
                "Oracles must submit at most one attestation per commit round. \
                 Double attestations can skew consensus outcomes."
            },
            Self::GovernanceVoteManipulation => {
                "All participants must cast votes reflecting genuine intent. \
                 Vote manipulation undermines democratic governance."
            },
            Self::GovernanceBribery => {
                "All participants must not offer or accept inducements for voting. \
                 Bribery corrupts governance outcomes."
            },
            Self::OperationalRateLimitAbuse => {
                "All participants must respect published rate limits. \
                 Exceeding rate limits degrades service for other participants."
            },
            Self::OperationalSpam => {
                "All participants must not flood the network with low-value or duplicate transactions. \
                 Spam consumes shared resources."
            },
            Self::EconomicWashTrading => {
                "All participants must not execute self-trades to inflate volume or manipulate metrics. \
                 Wash trading distorts market signals."
            },
            Self::EconomicMarketManipulation => {
                "All participants must not engage in coordinated price manipulation or front-running. \
                 Market manipulation harms all traders."
            },
        }
    }

    /// Default severity tier for this offense.
    pub fn default_severity(&self) -> SeverityTier {
        match self {
            Self::DataInvalidReveal => SeverityTier::Major,
            Self::DataLowQuality => SeverityTier::Moderate,
            Self::DataFabrication => SeverityTier::Critical,
            Self::DataPhysicalRangeViolation => SeverityTier::Moderate,
            Self::ConsensusDoubleSign => SeverityTier::Critical,
            Self::ConsensusSurroundVote => SeverityTier::Critical,
            Self::ConsensusDowntime => SeverityTier::Minor,
            Self::ConsensusInvalidBlock => SeverityTier::Major,
            Self::ConsensusInvalidZKProof => SeverityTier::Critical,
            Self::OracleAttestationFraud => SeverityTier::Critical,
            Self::OracleUnregistered => SeverityTier::Major,
            Self::OracleDoubleAttestation => SeverityTier::Major,
            Self::GovernanceVoteManipulation => SeverityTier::Critical,
            Self::GovernanceBribery => SeverityTier::Critical,
            Self::OperationalRateLimitAbuse => SeverityTier::Minor,
            Self::OperationalSpam => SeverityTier::Moderate,
            Self::EconomicWashTrading => SeverityTier::Major,
            Self::EconomicMarketManipulation => SeverityTier::Critical,
        }
    }
}

// ---------------------------------------------------------------------------
// Backward compatibility: From<SlashingReason>
// ---------------------------------------------------------------------------

impl From<SlashingReason> for OffenseCode {
    fn from(reason: SlashingReason) -> Self {
        match reason {
            SlashingReason::InvalidReveal => OffenseCode::DataInvalidReveal,
            SlashingReason::OracleAttestationFraud => OffenseCode::OracleAttestationFraud,
            SlashingReason::RepeatedLowQualityData => OffenseCode::DataLowQuality,
            SlashingReason::InvalidZKProof => OffenseCode::ConsensusInvalidZKProof,
            SlashingReason::DataQualityViolation => OffenseCode::DataPhysicalRangeViolation,
        }
    }
}

// ---------------------------------------------------------------------------
// Offense category
// ---------------------------------------------------------------------------

/// Category grouping for offense codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OffenseCategory {
    DataIntegrity,
    Consensus,
    Oracle,
    Governance,
    Operational,
    Economic,
}

// ---------------------------------------------------------------------------
// Actor class
// ---------------------------------------------------------------------------

/// Actor class subject to sanctions. Equal liability means the sanction outcome
/// is identical regardless of actor class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActorClass {
    Human,
    AiAgent,
    Robot,
}

impl ActorClass {
    /// All actor classes.
    pub fn all() -> &'static [ActorClass] {
        &[ActorClass::Human, ActorClass::AiAgent, ActorClass::Robot]
    }
}

// ---------------------------------------------------------------------------
// Severity tier
// ---------------------------------------------------------------------------

/// Severity tier for offenses. Determines the sanction outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SeverityTier {
    Minor,
    Moderate,
    Major,
    Critical,
}

impl SeverityTier {
    /// All severity tiers in ascending order.
    pub fn all() -> &'static [SeverityTier] {
        &[
            SeverityTier::Minor,
            SeverityTier::Moderate,
            SeverityTier::Major,
            SeverityTier::Critical,
        ]
    }
}

// ---------------------------------------------------------------------------
// Sanction outcome
// ---------------------------------------------------------------------------

/// Sanction outcome using integer basis points for deterministic hashing.
/// 10 000 bps = 100%.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanctionOutcome {
    /// Minimum slash as basis points of stake (inclusive).
    pub slash_bps_min: u16,
    /// Maximum slash as basis points of stake (inclusive).
    pub slash_bps_max: u16,
    /// Number of blocks the actor is suspended for.
    pub suspension_duration_blocks: u64,
    /// Whether the actor may appeal this sanction.
    pub appeal_eligible: bool,
    /// Cooldown blocks before the actor can resume normal participation.
    pub cooldown_blocks: u64,
}

// ---------------------------------------------------------------------------
// Sanction matrix
// ---------------------------------------------------------------------------

/// Deterministic, versioned sanction matrix.
///
/// Uses `BTreeMap` so iteration order is sorted by `(OffenseCode, SeverityTier)`,
/// which guarantees deterministic serialization for content hashing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanctionMatrix {
    entries: BTreeMap<(OffenseCode, SeverityTier), SanctionOutcome>,
    version: u32,
}

impl SanctionMatrix {
    /// Build the v1 sanction matrix with all 18 offenses x 4 severities = 72 entries.
    ///
    /// Values here are the canonical source of truth and MUST match
    /// `docs/governance/offense-taxonomy.toml`.
    #[allow(clippy::too_many_lines)]
    pub fn v1() -> Self {
        let mut entries = BTreeMap::new();

        // Helper to insert all four severity tiers for one offense.
        macro_rules! offense {
            ($code:expr, [
                ($mn_min:expr, $mn_max:expr, $mn_susp:expr, $mn_appeal:expr, $mn_cool:expr),
                ($mo_min:expr, $mo_max:expr, $mo_susp:expr, $mo_appeal:expr, $mo_cool:expr),
                ($ma_min:expr, $ma_max:expr, $ma_susp:expr, $ma_appeal:expr, $ma_cool:expr),
                ($cr_min:expr, $cr_max:expr, $cr_susp:expr, $cr_appeal:expr, $cr_cool:expr)
            ]) => {
                entries.insert(
                    ($code, SeverityTier::Minor),
                    SanctionOutcome {
                        slash_bps_min: $mn_min,
                        slash_bps_max: $mn_max,
                        suspension_duration_blocks: $mn_susp,
                        appeal_eligible: $mn_appeal,
                        cooldown_blocks: $mn_cool,
                    },
                );
                entries.insert(
                    ($code, SeverityTier::Moderate),
                    SanctionOutcome {
                        slash_bps_min: $mo_min,
                        slash_bps_max: $mo_max,
                        suspension_duration_blocks: $mo_susp,
                        appeal_eligible: $mo_appeal,
                        cooldown_blocks: $mo_cool,
                    },
                );
                entries.insert(
                    ($code, SeverityTier::Major),
                    SanctionOutcome {
                        slash_bps_min: $ma_min,
                        slash_bps_max: $ma_max,
                        suspension_duration_blocks: $ma_susp,
                        appeal_eligible: $ma_appeal,
                        cooldown_blocks: $ma_cool,
                    },
                );
                entries.insert(
                    ($code, SeverityTier::Critical),
                    SanctionOutcome {
                        slash_bps_min: $cr_min,
                        slash_bps_max: $cr_max,
                        suspension_duration_blocks: $cr_susp,
                        appeal_eligible: $cr_appeal,
                        cooldown_blocks: $cr_cool,
                    },
                );
            };
        }

        use OffenseCode::*;

        // --- Data integrity ---
        offense!(DataInvalidReveal, [
            (50, 100, 0, true, 100),
            (100, 300, 1000, true, 500),
            (300, 500, 5000, true, 2000),
            (500, 1000, 50000, false, 10000)
        ]);

        offense!(DataLowQuality, [
            (25, 50, 0, true, 100),
            (50, 150, 1000, true, 500),
            (150, 300, 5000, true, 2000),
            (300, 600, 25000, false, 10000)
        ]);

        offense!(DataFabrication, [
            (100, 200, 0, true, 200),
            (200, 500, 5000, true, 1000),
            (500, 1000, 25000, true, 5000),
            (1000, 2000, 100000, false, 50000)
        ]);

        // DATA-004: DataPhysicalRangeViolation
        // Calibrated between DataLowQuality (soft) and DataFabrication (hard)
        offense!(DataPhysicalRangeViolation, [
            (50,  100,      0, true,   200),
            (100, 300,   2000, true,  1000),
            (300, 600,  10000, true,  5000),
            (600, 1200, 50000, false, 25000)
        ]);

        // --- Consensus ---
        offense!(ConsensusDoubleSign, [
            (100, 300, 0, true, 500),
            (300, 500, 5000, true, 2000),
            (500, 1000, 25000, true, 10000),
            (1000, 2000, 100000, false, 50000)
        ]);

        offense!(ConsensusSurroundVote, [
            (100, 300, 0, true, 500),
            (300, 600, 5000, true, 2000),
            (600, 1200, 25000, true, 10000),
            (1200, 2000, 100000, false, 50000)
        ]);

        offense!(ConsensusDowntime, [
            (10, 25, 0, true, 100),
            (25, 75, 1000, true, 500),
            (75, 200, 5000, true, 2000),
            (200, 500, 25000, false, 10000)
        ]);

        offense!(ConsensusInvalidBlock, [
            (50, 100, 0, true, 200),
            (100, 300, 2000, true, 1000),
            (300, 600, 10000, true, 5000),
            (600, 1200, 50000, false, 25000)
        ]);

        offense!(ConsensusInvalidZKProof, [
            (100, 300, 0, true, 500),
            (300, 600, 5000, true, 2000),
            (600, 1200, 25000, true, 10000),
            (1200, 2000, 100000, false, 50000)
        ]);

        // --- Oracle ---
        offense!(OracleAttestationFraud, [
            (100, 200, 0, true, 300),
            (200, 500, 5000, true, 1500),
            (500, 1000, 25000, true, 7000),
            (1000, 1500, 100000, false, 50000)
        ]);

        offense!(OracleUnregistered, [
            (50, 100, 0, true, 200),
            (100, 300, 2000, true, 1000),
            (300, 500, 10000, true, 5000),
            (500, 1000, 50000, false, 25000)
        ]);

        offense!(OracleDoubleAttestation, [
            (75, 150, 0, true, 200),
            (150, 400, 3000, true, 1000),
            (400, 800, 15000, true, 5000),
            (800, 1500, 75000, false, 30000)
        ]);

        // --- Governance ---
        offense!(GovernanceVoteManipulation, [
            (100, 200, 0, true, 500),
            (200, 500, 5000, true, 2000),
            (500, 1000, 25000, true, 10000),
            (1000, 2000, 100000, false, 50000)
        ]);

        offense!(GovernanceBribery, [
            (150, 300, 0, true, 500),
            (300, 600, 5000, true, 2000),
            (600, 1200, 25000, true, 10000),
            (1200, 2000, 100000, false, 50000)
        ]);

        // --- Operational ---
        offense!(OperationalRateLimitAbuse, [
            (10, 25, 0, true, 100),
            (25, 75, 1000, true, 500),
            (75, 200, 5000, true, 2000),
            (200, 500, 25000, false, 10000)
        ]);

        offense!(OperationalSpam, [
            (15, 40, 0, true, 100),
            (40, 100, 2000, true, 500),
            (100, 250, 10000, true, 2000),
            (250, 500, 50000, false, 10000)
        ]);

        // --- Economic ---
        offense!(EconomicWashTrading, [
            (50, 150, 0, true, 300),
            (150, 400, 3000, true, 1500),
            (400, 800, 15000, true, 5000),
            (800, 1500, 75000, false, 30000)
        ]);

        offense!(EconomicMarketManipulation, [
            (100, 200, 0, true, 500),
            (200, 500, 5000, true, 2000),
            (500, 1000, 25000, true, 10000),
            (1000, 2000, 100000, false, 50000)
        ]);

        Self {
            entries,
            version: 1,
        }
    }

    /// Look up the sanction outcome for a given offense, severity, and actor class.
    ///
    /// The `_actor_class` parameter is accepted for API completeness but is
    /// intentionally ignored: equal liability means all actor classes receive the
    /// same outcome.
    pub fn lookup(
        &self,
        offense: OffenseCode,
        severity: SeverityTier,
        _actor_class: ActorClass,
    ) -> Option<&SanctionOutcome> {
        self.entries.get(&(offense, severity))
    }

    /// BLAKE3 content hash of the canonical JSON serialization of the matrix.
    ///
    /// Because `entries` is a `BTreeMap`, iteration order is deterministic.
    /// The map is serialized as an array of `(key, value)` pairs to avoid
    /// JSON's string-key-only limitation on object maps.
    pub fn content_hash(&self) -> [u8; 32] {
        // Build a canonical representation: sorted array of entries + version.
        let canonical: Vec<_> = self
            .entries
            .iter()
            .map(|((offense, severity), outcome)| {
                serde_json::json!({
                    "offense": offense,
                    "severity": severity,
                    "outcome": outcome,
                })
            })
            .collect();

        let payload = serde_json::json!({
            "version": self.version,
            "entries": canonical,
        });

        let json = serde_json::to_vec(&payload)
            .expect("Canonical sanction matrix payload must be JSON-serializable");
        *blake3::hash(&json).as_bytes()
    }

    /// Hex-encoded BLAKE3 content hash.
    pub fn content_hash_hex(&self) -> String {
        hex::encode(self.content_hash())
    }

    /// Schema version of this matrix.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Number of entries in the matrix.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the matrix is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
impl SanctionMatrix {
    /// Remove an entry from the matrix (test-only).
    pub(crate) fn remove_entry(&mut self, offense: OffenseCode, severity: SeverityTier) {
        self.entries.remove(&(offense, severity));
    }

    /// Mutate an existing entry in-place (test-only).
    pub(crate) fn mutate_entry<F>(&mut self, offense: OffenseCode, severity: SeverityTier, f: F)
    where
        F: FnOnce(&mut SanctionOutcome),
    {
        if let Some(entry) = self.entries.get_mut(&(offense, severity)) {
            f(entry);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_matrix_covers_all_combinations() {
        let matrix = SanctionMatrix::v1();
        assert_eq!(
            matrix.len(),
            18 * 4,
            "v1 matrix must contain exactly 18 offenses x 4 severities = 72 entries"
        );

        for offense in OffenseCode::all() {
            for severity in SeverityTier::all() {
                assert!(
                    matrix
                        .lookup(*offense, *severity, ActorClass::Human)
                        .is_some(),
                    "missing entry for ({:?}, {:?})",
                    offense,
                    severity,
                );
            }
        }
    }

    #[test]
    fn test_lookup_is_actor_class_agnostic() {
        let matrix = SanctionMatrix::v1();

        for offense in OffenseCode::all() {
            for severity in SeverityTier::all() {
                let human = matrix.lookup(*offense, *severity, ActorClass::Human).unwrap();
                let ai = matrix
                    .lookup(*offense, *severity, ActorClass::AiAgent)
                    .unwrap();
                let robot = matrix.lookup(*offense, *severity, ActorClass::Robot).unwrap();

                assert_eq!(
                    human, ai,
                    "Human and AiAgent outcomes must match for ({:?}, {:?})",
                    offense, severity,
                );
                assert_eq!(
                    human, robot,
                    "Human and Robot outcomes must match for ({:?}, {:?})",
                    offense, severity,
                );
            }
        }
    }

    #[test]
    fn test_content_hash_is_deterministic() {
        let matrix_a = SanctionMatrix::v1();
        let matrix_b = SanctionMatrix::v1();

        assert_eq!(
            matrix_a.content_hash(),
            matrix_b.content_hash(),
            "Two independently constructed v1 matrices must produce the same content hash"
        );

        assert_eq!(
            matrix_a.content_hash_hex(),
            matrix_b.content_hash_hex(),
            "Hex-encoded hashes must also match"
        );
    }

    #[test]
    fn test_content_hash_changes_on_mutation() {
        let matrix_a = SanctionMatrix::v1();
        let mut matrix_b = SanctionMatrix::v1();

        // Mutate one entry.
        let key = (OffenseCode::DataInvalidReveal, SeverityTier::Minor);
        let entry = matrix_b.entries.get_mut(&key).unwrap();
        entry.slash_bps_min += 1;

        assert_ne!(
            matrix_a.content_hash(),
            matrix_b.content_hash(),
            "Mutating any entry must change the content hash"
        );
    }

    #[test]
    fn test_plain_language_follows_style_guide() {
        // Style guide pattern: `<Who> must <behavior>. <Consequence>.`
        // At minimum: contains "must" (or "must not") and has two sentences.
        for offense in OffenseCode::all() {
            let desc = offense.plain_language_description();

            assert!(
                desc.contains("must"),
                "Description for {:?} must contain the word 'must': {desc}",
                offense,
            );

            let sentence_count = desc.matches(". ").count() + if desc.ends_with('.') { 1 } else { 0 };
            assert!(
                sentence_count >= 2,
                "Description for {:?} must have at least 2 sentences: {desc}",
                offense,
            );
        }
    }

    #[test]
    fn test_slash_bps_ranges_valid() {
        let matrix = SanctionMatrix::v1();

        for offense in OffenseCode::all() {
            for severity in SeverityTier::all() {
                let outcome = matrix.lookup(*offense, *severity, ActorClass::Human).unwrap();

                assert!(
                    outcome.slash_bps_min <= outcome.slash_bps_max,
                    "slash_bps_min ({}) must be <= slash_bps_max ({}) for ({:?}, {:?})",
                    outcome.slash_bps_min,
                    outcome.slash_bps_max,
                    offense,
                    severity,
                );
                assert!(
                    outcome.slash_bps_max <= 10_000,
                    "slash_bps_max ({}) must be <= 10000 for ({:?}, {:?})",
                    outcome.slash_bps_max,
                    offense,
                    severity,
                );
            }
        }
    }

    #[test]
    fn test_from_slashing_reason() {
        assert_eq!(
            OffenseCode::from(SlashingReason::InvalidReveal),
            OffenseCode::DataInvalidReveal,
        );
        assert_eq!(
            OffenseCode::from(SlashingReason::OracleAttestationFraud),
            OffenseCode::OracleAttestationFraud,
        );
        assert_eq!(
            OffenseCode::from(SlashingReason::RepeatedLowQualityData),
            OffenseCode::DataLowQuality,
        );
    }

    #[test]
    fn data_physical_range_violation_in_all_offense_list() {
        assert!(OffenseCode::all().contains(&OffenseCode::DataPhysicalRangeViolation));
    }

    #[test]
    fn from_slashing_reason_data_quality_maps_to_physical_range_violation() {
        assert_eq!(
            OffenseCode::from(SlashingReason::DataQualityViolation),
            OffenseCode::DataPhysicalRangeViolation
        );
    }

    #[test]
    fn test_offense_code_str_uniqueness() {
        let mut seen = HashSet::new();
        for offense in OffenseCode::all() {
            let code = offense.code_str();
            assert!(
                seen.insert(code),
                "Duplicate code_str found: {code} (offense {:?})",
                offense,
            );
        }
        assert_eq!(seen.len(), 18, "Must have exactly 18 unique code strings");
    }
}
