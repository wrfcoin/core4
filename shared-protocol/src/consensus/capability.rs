//! Contributor capability scoring (#735).
//!
//! Rewards contributor nodes with richer hardware (RTL-SDR, Zigbee, LoRaWAN,
//! mesh participation) via a bounded reward multiplier that sits alongside
//! the existing data-desert and trust multipliers in
//! [`crate::consensus::proof_of_publishing::ProofOfPublishing::apply_contribution_rewards`].
//!
//! ## Design principles (from the #735 spike)
//!
//! 1. **Evidence is consensus-visible only.** Every validator must compute
//!    the same multiplier on replay. Node-local telemetry (hostapd status,
//!    batman-adv peer count, LTE link state) is explicitly **not** an input.
//!    The evidence schema keys on aggregate counts of observations that
//!    have already been committed to chain state, broken down by
//!    [`meteo_types::RadioSourceType`].
//! 2. **Integer-only arithmetic.** All scoring is in basis points
//!    (`10_000 = 1.0×`) with `u32` / `u128` math. No `f64` on the reward
//!    path. Mirrors the pattern used by [`super::hyper_local`] and
//!    [`super::partition_policy`] for deterministic consensus.
//! 3. **Disabled by default.** The default [`CapabilityMultiplierPolicy`]
//!    ships with `activation_height = u64::MAX` and `max_multiplier_bps =
//!    10_000`, so until governance explicitly activates the system, every
//!    contributor gets the baseline `1.0×` multiplier and the reward math
//!    is unchanged. Two independent kill-switches (activation gate + max
//!    clamp) give governance a rollback path without a code change.
//! 4. **Bounded amplification.** The combined reward multiplier
//!    (`base × trust × desert × capability`) must still respect
//!    [`MAX_COMBINED_MULTIPLIER_BPS`]. See
//!    [`apply_capability_with_ceiling`].
//!
//! ## Evidence schema
//!
//! Evidence is a per-contributor aggregate of recent observations,
//! derivable from chain state alone. The accumulation path that populates
//! [`ContributorCapabilityEvidence`] lives in a follow-up PR — this module
//! provides the types, the classification, and the multiplier function so
//! the integration point is already present and the default (empty)
//! evidence always produces a baseline multiplier.
//!
//! ## Bands
//!
//! | Band | Evidence required (with defaults) |
//! |---|---|
//! | [`CapabilityBand::None`] | Not enough SDR observations to qualify. |
//! | [`CapabilityBand::Tier1`] | ≥ `tier1_min_sdr_obs` SDR observations. |
//! | [`CapabilityBand::Tier2Partial`] | Tier 1 + any Zigbee or LoRaWAN above the partial threshold. |
//! | [`CapabilityBand::Tier2Full`] | Tier 1 + Zigbee **and** LoRaWAN above the full threshold. |
//! | [`CapabilityBand::Tier3`] | Tier 2 full + mesh-relayed observations above the mesh threshold. |
//!
//! Thresholds and per-band multipliers are held in
//! [`CapabilityMultiplierPolicy`] so governance can tune them without a
//! code release. The operator-facing view of their own node sits in
//! [`NodeCapabilityProfile`].
//!
//! Refs #735, companions #1230 / #1231 / #1232 / #1233.

use serde::{Deserialize, Serialize};

use super::proof_of_publishing::MAX_COMBINED_MULTIPLIER_BPS;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Baseline multiplier: 10_000 bps = 1.0× (no reward change).
pub const BASELINE_MULTIPLIER_BPS: u32 = 10_000;

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

/// Consensus-visible evidence of a contributor's recent hardware activity.
///
/// Every field is an aggregate over a sliding window of recent blocks,
/// computed deterministically from committed observations. Two validators
/// replaying the same chain must produce the same evidence struct for the
/// same contributor — this is the core correctness requirement.
///
/// `window_blocks` is included so tests and audits can confirm the sliding
/// window used to produce the counts matches the policy's expected window.
/// The evidence accumulation path (follow-up PR) is responsible for keeping
/// `window_blocks` consistent with
/// [`CapabilityMultiplierPolicy::evidence_window_blocks`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributorCapabilityEvidence {
    /// Observations with `RadioSourceType::Sdr` in the sliding window.
    pub sdr_observations_in_window: u32,

    /// Observations with `RadioSourceType::Zigbee` in the sliding window.
    pub zigbee_observations_in_window: u32,

    /// Observations with `RadioSourceType::LoRa` in the sliding window.
    pub lorawan_observations_in_window: u32,

    /// Observations relayed through this contributor from a mesh peer in
    /// the sliding window.
    pub mesh_relayed_observations_in_window: u32,

    /// Count of distinct Zigbee sensor identifiers seen in the window.
    /// Helps distinguish "one sensor reporting a lot" from "a real mesh."
    pub distinct_zigbee_sensors: u32,

    /// Count of distinct LoRaWAN device identifiers seen in the window.
    pub distinct_lorawan_sensors: u32,

    /// The sliding window size (in blocks) that produced these counts.
    /// Used for audit / sanity checks against the active policy.
    pub window_blocks: u64,
}

impl ContributorCapabilityEvidence {
    /// Build an empty evidence record for a given window size.
    ///
    /// Convenience for the accumulation path to produce a zeroed struct
    /// that still carries its policy window for downstream validation.
    pub fn empty(window_blocks: u64) -> Self {
        Self { window_blocks, ..Self::default() }
    }

    /// True if this contributor has **no** capability evidence at all.
    ///
    /// Used by the reward path to skip the classification work entirely
    /// when there is nothing to classify — a small but useful optimization
    /// because the empty case is by far the most common one on launch day.
    pub fn is_empty(&self) -> bool {
        self.sdr_observations_in_window == 0
            && self.zigbee_observations_in_window == 0
            && self.lorawan_observations_in_window == 0
            && self.mesh_relayed_observations_in_window == 0
    }
}

// ---------------------------------------------------------------------------
// Band
// ---------------------------------------------------------------------------

/// Discrete capability band a contributor falls into for reward scoring.
///
/// The ordering (`None < Tier1 < Tier2Partial < Tier2Full < Tier3`) is
/// deliberate: higher bands must always produce a multiplier ≥ the lower
/// band. [`CapabilityMultiplierPolicy::validate`] enforces monotonicity at
/// config load time so a misconfigured policy cannot silently invert the
/// incentive order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityBand {
    /// Baseline: insufficient evidence of any tier-1 capability.
    None,
    /// RTL-SDR active — passive RF reception.
    Tier1,
    /// RTL-SDR + at least one active sensor mesh (Zigbee or LoRaWAN).
    Tier2Partial,
    /// RTL-SDR + both Zigbee and LoRaWAN active sensor meshes.
    Tier2Full,
    /// Tier 2 full + mesh-relayed observations (acting as a mesh peer).
    Tier3,
}

impl CapabilityBand {
    /// A short lowercase label for logs and metrics.
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Tier1 => "tier1",
            Self::Tier2Partial => "tier2_partial",
            Self::Tier2Full => "tier2_full",
            Self::Tier3 => "tier3",
        }
    }
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Governance-controlled thresholds and per-band multipliers.
///
/// Every numeric field is deterministic and integer-only. Default values
/// correspond to the "disabled" state — the system ships with an
/// unreachable `activation_height` and a max clamp of `1.0×`, so enabling
/// the feature requires an explicit DAO proposal that updates **both**
/// `activation_height` and `max_multiplier_bps` in addition to any band
/// thresholds and multipliers.
///
/// The disabled default is intentional: shipping a non-trivial multiplier
/// on the reward path without evidence-accumulation support (which lands
/// in a follow-up PR) could credit nodes with phantom capabilities. Two
/// independent gates mean governance cannot accidentally activate the
/// feature by flipping a single knob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityMultiplierPolicy {
    /// Block height at or above which the capability multiplier applies.
    /// Below this height every contributor gets the baseline `1.0×`.
    ///
    /// Defaults to `u64::MAX` (disabled).
    pub activation_height: u64,

    /// How many blocks of history the evidence aggregation looks back
    /// over. Informs the follow-up accumulation path and is echoed into
    /// [`ContributorCapabilityEvidence::window_blocks`] so audit code can
    /// verify consistency.
    pub evidence_window_blocks: u64,

    // ---- tier thresholds (observation counts within the window) ----
    /// Minimum SDR observations required for `Tier1` and above.
    pub tier1_min_sdr_obs: u32,

    /// Minimum Zigbee **or** LoRaWAN observations for `Tier2Partial`.
    pub tier2_partial_min_obs: u32,

    /// Minimum Zigbee observations for `Tier2Full`.
    pub tier2_full_min_zigbee_obs: u32,

    /// Minimum LoRaWAN observations for `Tier2Full`.
    pub tier2_full_min_lorawan_obs: u32,

    /// Minimum mesh-relayed observations for `Tier3`.
    pub tier3_min_mesh_relayed: u32,

    // ---- per-band multipliers (basis points, 10_000 = 1.0×) ----
    /// Multiplier for the `None` band. Must be `10_000` (disabled
    /// contributors cannot be punished by capability scoring).
    pub none_bps: u32,

    /// Multiplier for `Tier1`.
    pub tier1_bps: u32,

    /// Multiplier for `Tier2Partial`.
    pub tier2_partial_bps: u32,

    /// Multiplier for `Tier2Full`.
    pub tier2_full_bps: u32,

    /// Multiplier for `Tier3`.
    pub tier3_bps: u32,

    /// Overall ceiling on the capability multiplier. Acts as a governance
    /// kill-switch: setting this to `10_000` instantly forces every
    /// contributor back to the baseline, regardless of band thresholds.
    ///
    /// Defaults to `10_000` (disabled).
    pub max_multiplier_bps: u32,
}

impl Default for CapabilityMultiplierPolicy {
    fn default() -> Self {
        Self {
            // Disabled by default. DAO must update both activation_height
            // and max_multiplier_bps to activate.
            activation_height: u64::MAX,
            // ~1 day at 30 s blocks.
            evidence_window_blocks: 2_880,

            tier1_min_sdr_obs: 10,
            tier2_partial_min_obs: 5,
            tier2_full_min_zigbee_obs: 10,
            tier2_full_min_lorawan_obs: 10,
            tier3_min_mesh_relayed: 5,

            // Baseline / suggested values from the hardware roadmap.
            // These are only meaningful once `max_multiplier_bps` is lifted.
            none_bps: BASELINE_MULTIPLIER_BPS,
            tier1_bps: 11_500,
            tier2_partial_bps: 13_000,
            tier2_full_bps: 15_000,
            tier3_bps: 17_500,

            // DISABLED. Governance must lift this before the band
            // multipliers above have any effect.
            max_multiplier_bps: BASELINE_MULTIPLIER_BPS,
        }
    }
}

impl CapabilityMultiplierPolicy {
    /// Is the policy effectively disabled?
    ///
    /// True when either kill-switch is engaged: activation height
    /// unreachable, or max clamp at or below baseline.
    pub fn is_disabled(&self) -> bool {
        self.activation_height == u64::MAX || self.max_multiplier_bps <= BASELINE_MULTIPLIER_BPS
    }

    /// Validate that band multipliers are non-decreasing.
    ///
    /// A policy that offered a higher reward for a lower capability band
    /// would invert operator incentives (buy less hardware, earn more),
    /// so the load-time check rejects such configurations outright.
    ///
    /// Returns a descriptive error string on the first violation, or
    /// `Ok(())` if every band satisfies `higher ≥ lower`.
    pub fn validate(&self) -> Result<(), String> {
        if self.none_bps != BASELINE_MULTIPLIER_BPS {
            return Err(format!(
                "none_bps must equal {BASELINE_MULTIPLIER_BPS} (baseline), got {}",
                self.none_bps
            ));
        }
        let bands = [
            ("tier1_bps", self.tier1_bps),
            ("tier2_partial_bps", self.tier2_partial_bps),
            ("tier2_full_bps", self.tier2_full_bps),
            ("tier3_bps", self.tier3_bps),
        ];
        let mut prev = self.none_bps;
        for (name, value) in bands {
            if value < prev {
                return Err(format!(
                    "capability band multipliers must be non-decreasing: {name}={value} < prev={prev}",
                ));
            }
            prev = value;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Classification + scoring
// ---------------------------------------------------------------------------

/// Map evidence to a [`CapabilityBand`] under a given policy.
///
/// Band selection is **monotonic in evidence**: adding more observations
/// can only move a contributor up (or leave them where they are). This is
/// a correctness property for the reward path — incidentally accumulating
/// evidence must never downgrade a contributor.
pub fn classify_capability_band(
    evidence: &ContributorCapabilityEvidence,
    policy: &CapabilityMultiplierPolicy,
) -> CapabilityBand {
    let has_sdr = evidence.sdr_observations_in_window >= policy.tier1_min_sdr_obs;
    if !has_sdr {
        return CapabilityBand::None;
    }

    let has_zigbee_full =
        evidence.zigbee_observations_in_window >= policy.tier2_full_min_zigbee_obs;
    let has_lorawan_full =
        evidence.lorawan_observations_in_window >= policy.tier2_full_min_lorawan_obs;
    let has_any_tier2 = evidence.zigbee_observations_in_window >= policy.tier2_partial_min_obs
        || evidence.lorawan_observations_in_window >= policy.tier2_partial_min_obs;
    let has_mesh =
        evidence.mesh_relayed_observations_in_window >= policy.tier3_min_mesh_relayed;

    if has_zigbee_full && has_lorawan_full {
        if has_mesh {
            CapabilityBand::Tier3
        } else {
            CapabilityBand::Tier2Full
        }
    } else if has_any_tier2 {
        CapabilityBand::Tier2Partial
    } else {
        CapabilityBand::Tier1
    }
}

/// Look up the raw per-band multiplier (before policy-level clamp).
pub fn multiplier_for_band(band: CapabilityBand, policy: &CapabilityMultiplierPolicy) -> u32 {
    match band {
        CapabilityBand::None => policy.none_bps,
        CapabilityBand::Tier1 => policy.tier1_bps,
        CapabilityBand::Tier2Partial => policy.tier2_partial_bps,
        CapabilityBand::Tier2Full => policy.tier2_full_bps,
        CapabilityBand::Tier3 => policy.tier3_bps,
    }
}

/// Compute the final capability reward multiplier in basis points.
///
/// Applies, in order:
///
/// 1. **Activation gate** — returns [`BASELINE_MULTIPLIER_BPS`] if
///    `current_height < policy.activation_height`.
/// 2. **Max clamp gate** — returns [`BASELINE_MULTIPLIER_BPS`] if
///    `policy.max_multiplier_bps <= BASELINE_MULTIPLIER_BPS`. This is the
///    governance kill-switch and the default-disabled state.
/// 3. **Empty evidence short-circuit** — zero evidence → baseline. Useful
///    optimization for the common "no capability data yet" case.
/// 4. **Band classification + lookup** — produces the raw per-band bps.
/// 5. **Final clamp** — the raw value is clamped to
///    `[BASELINE_MULTIPLIER_BPS, policy.max_multiplier_bps]` so a
///    contributor is never punished below baseline and never exceeds the
///    governance ceiling.
///
/// This function is pure: given the same `(evidence, policy, height)` it
/// returns the same bps value on every validator, which is the
/// determinism requirement for the reward path.
pub fn capability_multiplier_bps(
    evidence: &ContributorCapabilityEvidence,
    policy: &CapabilityMultiplierPolicy,
    current_height: u64,
) -> u32 {
    if current_height < policy.activation_height {
        return BASELINE_MULTIPLIER_BPS;
    }
    if policy.max_multiplier_bps <= BASELINE_MULTIPLIER_BPS {
        return BASELINE_MULTIPLIER_BPS;
    }
    if evidence.is_empty() {
        return BASELINE_MULTIPLIER_BPS;
    }

    let band = classify_capability_band(evidence, policy);
    let raw = multiplier_for_band(band, policy);
    raw.clamp(BASELINE_MULTIPLIER_BPS, policy.max_multiplier_bps)
}

// ---------------------------------------------------------------------------
// Reward composition
// ---------------------------------------------------------------------------

/// Apply the capability multiplier to an already-composed reward while
/// preserving the `MAX_COMBINED_MULTIPLIER_BPS` ceiling.
///
/// The existing reward path in
/// [`super::proof_of_publishing::ProofOfPublishing::apply_contribution_rewards`]
/// produces `reward_after_other_multipliers` by composing the base reward
/// with the trust, quality, and data-desert multipliers under the shared
/// 2.0× cap from [`MAX_COMBINED_MULTIPLIER_BPS`]. This helper then scales
/// that result by the capability multiplier, re-clamping against the
/// *original* `base_reward` so the final amplification cannot exceed
/// 2.0× regardless of how the capability bps compound with the prior
/// multipliers.
///
/// ## Why preserve the cap here
///
/// Without the re-clamp, a contributor in a data-desert zone who also
/// qualifies for the highest capability band would receive
/// `2.0× * 1.75× = 3.5×` the base reward, blowing past the invariant
/// that any single block cannot produce more than `2.0× base` for any
/// single contributor. The block-level emission budget depends on that
/// invariant holding, so we preserve it here rather than spread the fix
/// across the reward callsite.
///
/// ## Inputs
/// - `reward_after_other_multipliers`: the reward value *after* the
///   existing compose step that already clamped to the 2.0× ceiling.
/// - `base_reward`: the pre-multiplier base (used to compute the ceiling).
/// - `capability_bps`: the capability multiplier in basis points.
///
/// Returns the final reward, guaranteed `≤ base_reward * 2.0×`.
pub fn apply_capability_with_ceiling(
    reward_after_other_multipliers: u128,
    base_reward: u128,
    capability_bps: u32,
) -> u128 {
    // When capability is baseline, short-circuit to avoid a no-op
    // multiply/divide round-trip.
    if capability_bps == BASELINE_MULTIPLIER_BPS {
        return reward_after_other_multipliers;
    }

    // Absolute ceiling: never exceed 2.0× of the original base reward.
    let ceiling = base_reward
        .saturating_mul(MAX_COMBINED_MULTIPLIER_BPS)
        .saturating_div(BASELINE_MULTIPLIER_BPS as u128);

    let boosted = reward_after_other_multipliers
        .saturating_mul(capability_bps as u128)
        .saturating_div(BASELINE_MULTIPLIER_BPS as u128);

    boosted.min(ceiling)
}

// ---------------------------------------------------------------------------
// Operator-facing profile (non-consensus)
// ---------------------------------------------------------------------------

/// Operator-facing snapshot of a single node's capability status.
///
/// This struct is a **derived view** intended for node dashboards and
/// admin APIs. It is not consensus-critical and does not participate in
/// state-root computation. Fields mirror the evidence plus the classified
/// band so operators can see what tier they currently qualify for under
/// the active policy.
///
/// The `telemetry_active` fields capture live hostapd / batman-adv / LTE
/// status for the operator's own reference only — they are deliberately
/// absent from [`ContributorCapabilityEvidence`] because they cannot be
/// independently verified by other validators.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCapabilityProfile {
    /// Evidence derived from committed chain state.
    pub evidence: ContributorCapabilityEvidence,
    /// Classified band under the currently active policy.
    pub band: CapabilityBand,
    /// Final capability multiplier in basis points (post-clamp).
    pub multiplier_bps: u32,
    /// True if hostapd reports a live WiFi AP (issue #739).
    pub telemetry_ap_active: bool,
    /// True if a WAN-failover LTE link is live (issue #731).
    pub telemetry_lte_active: bool,
    /// Count of live mesh peers reported by batman-adv (issue #733).
    pub telemetry_mesh_peer_count: u32,
}

impl Default for CapabilityBand {
    fn default() -> Self {
        Self::None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// An enabled policy (for tests that want to exercise the scoring
    /// path past the governance gates). Uses the default thresholds and
    /// multipliers but lifts the activation and clamp.
    fn enabled_policy() -> CapabilityMultiplierPolicy {
        CapabilityMultiplierPolicy {
            activation_height: 0,
            max_multiplier_bps: 20_000, // 2.0× ceiling matches MAX_COMBINED
            ..CapabilityMultiplierPolicy::default()
        }
    }

    fn full_tier3_evidence() -> ContributorCapabilityEvidence {
        ContributorCapabilityEvidence {
            sdr_observations_in_window: 100,
            zigbee_observations_in_window: 50,
            lorawan_observations_in_window: 50,
            mesh_relayed_observations_in_window: 20,
            distinct_zigbee_sensors: 5,
            distinct_lorawan_sensors: 3,
            window_blocks: 2_880,
        }
    }

    // ── Default policy is disabled ───────────────────────────────────

    #[test]
    fn default_policy_is_disabled() {
        let policy = CapabilityMultiplierPolicy::default();
        assert!(policy.is_disabled());
        assert_eq!(policy.activation_height, u64::MAX);
        assert_eq!(policy.max_multiplier_bps, BASELINE_MULTIPLIER_BPS);
    }

    #[test]
    fn default_policy_returns_baseline_for_every_evidence() {
        let policy = CapabilityMultiplierPolicy::default();
        // Even maximum tier-3 evidence should be baseline under default.
        let evidence = full_tier3_evidence();
        assert_eq!(
            capability_multiplier_bps(&evidence, &policy, 1_000_000),
            BASELINE_MULTIPLIER_BPS,
            "default policy must be disabled for all evidence"
        );
    }

    #[test]
    fn default_policy_baseline_for_empty_evidence() {
        let policy = CapabilityMultiplierPolicy::default();
        let evidence = ContributorCapabilityEvidence::default();
        assert_eq!(
            capability_multiplier_bps(&evidence, &policy, 0),
            BASELINE_MULTIPLIER_BPS
        );
    }

    // ── Activation height gate ────────────────────────────────────────

    #[test]
    fn activation_height_gate_returns_baseline_below_threshold() {
        let mut policy = enabled_policy();
        policy.activation_height = 100;
        let evidence = full_tier3_evidence();

        assert_eq!(
            capability_multiplier_bps(&evidence, &policy, 99),
            BASELINE_MULTIPLIER_BPS,
            "below activation → baseline"
        );
        assert!(
            capability_multiplier_bps(&evidence, &policy, 100) > BASELINE_MULTIPLIER_BPS,
            "at activation → boosted"
        );
    }

    // ── Max clamp kill switch ─────────────────────────────────────────

    #[test]
    fn max_clamp_kill_switch_returns_baseline() {
        let mut policy = enabled_policy();
        policy.max_multiplier_bps = BASELINE_MULTIPLIER_BPS; // kill
        let evidence = full_tier3_evidence();
        assert_eq!(
            capability_multiplier_bps(&evidence, &policy, 1_000),
            BASELINE_MULTIPLIER_BPS
        );
    }

    #[test]
    fn max_clamp_below_band_caps_multiplier() {
        let mut policy = enabled_policy();
        // Cap below the tier3 band, at tier2_partial level.
        policy.max_multiplier_bps = 13_000;
        let evidence = full_tier3_evidence();
        assert_eq!(
            capability_multiplier_bps(&evidence, &policy, 1_000),
            13_000,
            "max clamp enforces ceiling on band multiplier"
        );
    }

    // ── Band classification boundaries ────────────────────────────────

    #[test]
    fn empty_evidence_is_none_band() {
        let policy = enabled_policy();
        let evidence = ContributorCapabilityEvidence::empty(2_880);
        assert_eq!(classify_capability_band(&evidence, &policy), CapabilityBand::None);
    }

    #[test]
    fn insufficient_sdr_observations_is_none() {
        let policy = enabled_policy();
        let evidence = ContributorCapabilityEvidence {
            sdr_observations_in_window: 9, // threshold is 10
            ..Default::default()
        };
        assert_eq!(classify_capability_band(&evidence, &policy), CapabilityBand::None);
    }

    #[test]
    fn sdr_threshold_exactly_met_is_tier1() {
        let policy = enabled_policy();
        let evidence = ContributorCapabilityEvidence {
            sdr_observations_in_window: 10,
            ..Default::default()
        };
        assert_eq!(classify_capability_band(&evidence, &policy), CapabilityBand::Tier1);
    }

    #[test]
    fn sdr_plus_partial_zigbee_is_tier2_partial() {
        let policy = enabled_policy();
        let evidence = ContributorCapabilityEvidence {
            sdr_observations_in_window: 10,
            zigbee_observations_in_window: 5, // partial threshold
            ..Default::default()
        };
        assert_eq!(
            classify_capability_band(&evidence, &policy),
            CapabilityBand::Tier2Partial
        );
    }

    #[test]
    fn sdr_plus_partial_lorawan_is_tier2_partial() {
        let policy = enabled_policy();
        let evidence = ContributorCapabilityEvidence {
            sdr_observations_in_window: 10,
            lorawan_observations_in_window: 5,
            ..Default::default()
        };
        assert_eq!(
            classify_capability_band(&evidence, &policy),
            CapabilityBand::Tier2Partial
        );
    }

    #[test]
    fn sdr_plus_full_zigbee_alone_is_still_tier2_partial() {
        let policy = enabled_policy();
        let evidence = ContributorCapabilityEvidence {
            sdr_observations_in_window: 10,
            zigbee_observations_in_window: 100, // way above full threshold
            lorawan_observations_in_window: 0,  // but no lorawan
            ..Default::default()
        };
        assert_eq!(
            classify_capability_band(&evidence, &policy),
            CapabilityBand::Tier2Partial,
            "full requires BOTH sensor meshes — single-mesh saturation stays partial"
        );
    }

    #[test]
    fn sdr_plus_both_meshes_full_is_tier2_full() {
        let policy = enabled_policy();
        let evidence = ContributorCapabilityEvidence {
            sdr_observations_in_window: 10,
            zigbee_observations_in_window: 10,
            lorawan_observations_in_window: 10,
            ..Default::default()
        };
        assert_eq!(
            classify_capability_band(&evidence, &policy),
            CapabilityBand::Tier2Full
        );
    }

    #[test]
    fn tier2_full_plus_mesh_relayed_is_tier3() {
        let policy = enabled_policy();
        let evidence = ContributorCapabilityEvidence {
            sdr_observations_in_window: 10,
            zigbee_observations_in_window: 10,
            lorawan_observations_in_window: 10,
            mesh_relayed_observations_in_window: 5,
            ..Default::default()
        };
        assert_eq!(classify_capability_band(&evidence, &policy), CapabilityBand::Tier3);
    }

    #[test]
    fn tier3_requires_tier2_full_plus_mesh() {
        let policy = enabled_policy();
        // Mesh without tier2_full falls back to tier2_partial.
        let evidence = ContributorCapabilityEvidence {
            sdr_observations_in_window: 10,
            zigbee_observations_in_window: 5, // partial only
            mesh_relayed_observations_in_window: 100,
            ..Default::default()
        };
        assert_eq!(
            classify_capability_band(&evidence, &policy),
            CapabilityBand::Tier2Partial,
            "mesh alone does not promote past tier2_full gate"
        );
    }

    // ── Monotonicity: more evidence never downgrades ──────────────────

    #[test]
    fn adding_evidence_never_downgrades_band() {
        let policy = enabled_policy();

        let evidences = [
            ContributorCapabilityEvidence::default(),
            ContributorCapabilityEvidence {
                sdr_observations_in_window: 10,
                ..Default::default()
            },
            ContributorCapabilityEvidence {
                sdr_observations_in_window: 10,
                zigbee_observations_in_window: 5,
                ..Default::default()
            },
            ContributorCapabilityEvidence {
                sdr_observations_in_window: 10,
                zigbee_observations_in_window: 10,
                lorawan_observations_in_window: 10,
                ..Default::default()
            },
            ContributorCapabilityEvidence {
                sdr_observations_in_window: 10,
                zigbee_observations_in_window: 10,
                lorawan_observations_in_window: 10,
                mesh_relayed_observations_in_window: 5,
                ..Default::default()
            },
        ];

        let mut prev = CapabilityBand::None;
        for evidence in &evidences {
            let band = classify_capability_band(evidence, &policy);
            assert!(band >= prev, "band must not downgrade, went {prev:?} → {band:?}");
            prev = band;
        }
    }

    #[test]
    fn multiplier_is_monotonic_in_band_order() {
        let policy = enabled_policy();
        let m_none = multiplier_for_band(CapabilityBand::None, &policy);
        let m_t1 = multiplier_for_band(CapabilityBand::Tier1, &policy);
        let m_t2p = multiplier_for_band(CapabilityBand::Tier2Partial, &policy);
        let m_t2f = multiplier_for_band(CapabilityBand::Tier2Full, &policy);
        let m_t3 = multiplier_for_band(CapabilityBand::Tier3, &policy);
        assert!(m_none <= m_t1);
        assert!(m_t1 <= m_t2p);
        assert!(m_t2p <= m_t2f);
        assert!(m_t2f <= m_t3);
    }

    // ── Validation ────────────────────────────────────────────────────

    #[test]
    fn validate_accepts_default_policy() {
        assert!(CapabilityMultiplierPolicy::default().validate().is_ok());
    }

    #[test]
    fn validate_rejects_non_baseline_none_bps() {
        let mut policy = CapabilityMultiplierPolicy::default();
        policy.none_bps = 9_500; // punishes uncapable nodes
        assert!(policy.validate().is_err());
    }

    #[test]
    fn validate_rejects_inverted_band_order() {
        let mut policy = enabled_policy();
        policy.tier2_full_bps = 10_500;
        policy.tier2_partial_bps = 12_000; // higher than tier2_full
        let err = policy.validate().expect_err("should reject");
        assert!(err.contains("tier2_full_bps"));
    }

    #[test]
    fn validate_accepts_enabled_policy() {
        assert!(enabled_policy().validate().is_ok());
    }

    // ── Reward composition ceiling ────────────────────────────────────

    #[test]
    fn baseline_capability_is_noop_on_reward() {
        let base = 1_000_000u128;
        let after_desert = 1_500_000u128; // 1.5× base from desert
        let result = apply_capability_with_ceiling(after_desert, base, BASELINE_MULTIPLIER_BPS);
        assert_eq!(result, after_desert, "baseline bps must be a no-op");
    }

    #[test]
    fn capability_boosts_within_ceiling() {
        let base = 1_000_000u128;
        let after_desert = 1_200_000u128; // 1.2× from desert
        // capability = 1.5× → 1.2 * 1.5 = 1.8× total, under 2.0× cap
        let result = apply_capability_with_ceiling(after_desert, base, 15_000);
        assert_eq!(result, 1_800_000);
    }

    #[test]
    fn capability_clamped_by_two_x_ceiling_against_base() {
        let base = 1_000_000u128;
        let after_desert = 2_000_000u128; // already at 2.0× from desert
        // capability = 1.5× → raw would be 3.0×, must clamp to 2.0×
        let result = apply_capability_with_ceiling(after_desert, base, 15_000);
        assert_eq!(result, 2_000_000, "combined amplification must respect 2.0× cap");
    }

    #[test]
    fn capability_ceiling_not_affected_by_composed_prior_math() {
        // Ceiling must be computed against base, not against the already-
        // composed reward. Confirm by using a pathological input where the
        // composed reward is smaller than base (shouldn't happen in practice
        // but tests the math).
        let base = 1_000_000u128;
        let after_desert = 500_000u128; // below base (hypothetical)
        let result = apply_capability_with_ceiling(after_desert, base, 15_000);
        // 500_000 * 1.5 = 750_000, well under 2_000_000 cap
        assert_eq!(result, 750_000);
    }

    #[test]
    fn capability_saturates_on_huge_inputs() {
        // Stress the u128 math with values near the top of the range.
        let base = u128::MAX / 4;
        let after_desert = u128::MAX / 3;
        // Should not panic and should produce a bounded result.
        let result = apply_capability_with_ceiling(after_desert, base, 20_000);
        let ceiling = base.saturating_mul(MAX_COMBINED_MULTIPLIER_BPS)
            / (BASELINE_MULTIPLIER_BPS as u128);
        assert!(result <= ceiling);
    }

    // ── Serde round-trip ──────────────────────────────────────────────

    #[test]
    fn policy_roundtrips_through_json() {
        let policy = enabled_policy();
        let json = serde_json::to_string(&policy).unwrap();
        let recovered: CapabilityMultiplierPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.activation_height, policy.activation_height);
        assert_eq!(recovered.tier3_bps, policy.tier3_bps);
        assert_eq!(recovered.max_multiplier_bps, policy.max_multiplier_bps);
    }

    #[test]
    fn evidence_roundtrips_through_json() {
        let evidence = full_tier3_evidence();
        let json = serde_json::to_string(&evidence).unwrap();
        let recovered: ContributorCapabilityEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, evidence);
    }

    #[test]
    fn band_and_profile_roundtrip_through_json() {
        let profile = NodeCapabilityProfile {
            evidence: full_tier3_evidence(),
            band: CapabilityBand::Tier3,
            multiplier_bps: 17_500,
            telemetry_ap_active: true,
            telemetry_lte_active: false,
            telemetry_mesh_peer_count: 4,
        };
        let json = serde_json::to_string(&profile).unwrap();
        let recovered: NodeCapabilityProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, profile);
    }

    // ── Determinism: pure function ────────────────────────────────────

    #[test]
    fn multiplier_is_deterministic_across_repeat_calls() {
        let policy = enabled_policy();
        let evidence = full_tier3_evidence();
        let first = capability_multiplier_bps(&evidence, &policy, 1_000);
        for _ in 0..1_000 {
            assert_eq!(capability_multiplier_bps(&evidence, &policy, 1_000), first);
        }
    }
}
