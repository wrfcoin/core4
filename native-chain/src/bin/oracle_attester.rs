//! Oracle attester service (#2464) — poll, validate mechanically, sign, submit.
//!
//! The daemon half of the attestation path. Composes three pieces that already
//! exist in `shared-protocol` and adds only the plumbing between a node and a
//! signer:
//!
//! - discovery via `GET /transactions/oracle/pending` (the #2464 read
//!   surface: full reveal preimages plus attestation state);
//! - decisions via [`MechanicalPolicy`] (physics ranges, freshness, reference
//!   bands — nothing from #724's human-trust territory);
//! - signing via [`Attester`], which resolves the production oracle signer
//!   role (#2472) and enforces the #2464 custody rule.
//!
//! # The identity seam — a config change, not a redesign
//!
//! Which oracle identity this process is depends only on how the signer role
//! resolves, and that resolution is entirely environmental:
//!
//! - **Test key (today):** debug build with
//!   `WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY=<hex>` or
//!   `WRFCOIN_ORACLE_SIGNER_KEY_FILE=<path>` (mode 0600).
//! - **Real identity (after re-keying ceremony):**
//!   `WRFCOIN_HSM_MODULE_PATH=<softhsm2/ykcs11 .so>` plus
//!   `WRFCOIN_HSM_ORACLE_KEY_LABEL=<label>` — the PKCS#11 path, which takes
//!   priority over any software key and refuses to downgrade if the token is
//!   unavailable. No code here changes.
//!
//! Two deployments (oracle 1's service key in SoftHSM on node container, oracle 2's on
//! the workstation) are two instances of this binary with different env files.
//! Oracle 3 stays cold; quorum 2-of-3 is reachable without it.
//!
//! # Chain identity is explicit, never defaulted
//!
//! `WRFCOIN_CHAIN_ID` and `WRFCOIN_NETWORK_ID` are **required**. An unset
//! chain identity silently signs the mainnet domain (#2713), and a value that
//! disagrees with the node surfaces only as opaque signature rejections
//! (#2744). So startup fails fast when they are unset, and the first poll
//! cross-checks them against the identity the node echoes in the pending
//! response — a mismatch is a refusal to run, with both values in the error.
//!
//! # The timing rule and what enforces it
//!
//! `process_contribution_rewards` evaluates commit ids revealed in the
//! current block, so the prompt payment path needs the attestation mined
//! **before or in** the reveal's block. Two mechanisms cover it:
//!
//! 1. This daemon polls the mempool lane of the read surface at
//!    `WRFCOIN_ATTESTER_POLL_SECS` (default 5s, well under block cadence) and
//!    submits immediately, so attestation and reveal normally mine together.
//! 2. If the window is missed anyway (attester down, mempool race), the
//!    chain's deferred late-quorum settlement (#2508) pays once quorum
//!    arrives — the read surface's `"block"` lane keeps serving the reveal
//!    preimage precisely so a late attester can still act.
//!
//! # Environment
//!
//! | var | required | meaning |
//! |---|---|---|
//! | `WRFCOIN_ATTESTER_NODE_URL` | yes | node base URL, e.g. `http://127.0.0.1:8556` |
//! | `WRFCOIN_ATTESTER_API_TOKEN` | yes | bearer token with the Oracle role |
//! | `WRFCOIN_CHAIN_ID` / `WRFCOIN_NETWORK_ID` | yes | signing domain (see above) |
//! | `WRFCOIN_ATTESTER_FORBID_GOVERNANCE_PUBKEY` | yes | one or more comma-separated governance public key hex values |
//! | `WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY` | on attester hosts | one or more comma-separated snapshot public key hex values |
//! | `WRFCOIN_ATTESTER_FORBID_PRODUCER_PUBKEY` | on attester hosts | one or more comma-separated producer public key hex values |
//! | oracle signer vars (#2472) | yes | see the identity seam above |
//! | `WRFCOIN_ATTESTER_POLL_SECS` | no (5) | poll interval |
//! | `WRFCOIN_ATTESTER_MAX_AGE_SECS` | no (3600) | freshness window |
//! | `WRFCOIN_ATTESTER_ONCE` | no | `1` = one poll cycle, then exit (smoke tests) |
//! | `WRFCOIN_ATTESTER_NWS_STATION` | no | local reference station id (e.g. `KDLH`); unset = loud PHYSICS-ONLY mode with capped confidence |
//! | `WRFCOIN_ATTESTER_NWS_BASE_URL` | no (`https://api.weather.gov`) | NWS API base, overridable for tests/mirrors |
//! | `WRFCOIN_ATTESTER_REFERENCE_MAX_PAIR_SECS` | no (1800) | max reading↔reference time gap for a valid pair |
//! | `WRFCOIN_ATTESTER_REFERENCE_MODE` | no (`fixed`) | #3240 reference selection: `fixed` = every commit vs `WRFCOIN_ATTESTER_NWS_STATION` (pre-#3240 behaviour); `shadow` = submit exactly the `fixed` verdict, also compute and log the per-station one; `per_station` = pair each commit with the NWS station nearest its registered location. Unknown value = refuse to start. Flipping to `per_station` changes rewards and is owner-gated (see `docs/oracle-attester-deployment.md`) |
//! | `WRFCOIN_ATTESTER_MAX_REFERENCE_KM` | in `shadow`/`per_station` | max station↔reference distance; beyond it the commit is PHYSICS-ONLY. No default: the `nws_station` bands were calibrated at ~3 km, so the radius is an owner reward-policy choice. Ignored (with a warning) in `fixed` |
//! | `WRFCOIN_ATTESTER_REFERENCE_LOOKUP_TIMEOUT_SECS` | no (5) | per-request timeout for NWS point/station lookups and per-station observation fetches |
//! | `WRFCOIN_ATTESTER_MAX_REFERENCE_STATIONS_PER_CYCLE` | no (8) | bound on distinct NWS stations whose observations are fetched per poll cycle; point/station lookups are bounded at twice this |
//!
//! # Per-commit reference selection (#3240) is service-side, not consensus
//!
//! Which NWS station a reading is compared against is decided here and
//! nowhere else. The chain receives only the signed attestation — `commit_id`,
//! `oracle_id`, `approved`, `confidence`, `timestamp`, `nonce` — and never the
//! reference station, the distance or the residuals. The station location comes
//! from the node's pending surface (`station_location`, registry-sourced and
//! coarsened to 0.01°), never from the contributor-controlled reveal `data`,
//! which would let a contributor pick their own reference. Fallback order, each
//! step named in the verdict reasons and logged: nearest station within
//! `WRFCOIN_ATTESTER_MAX_REFERENCE_KM` → the configured
//! `WRFCOIN_ATTESTER_NWS_STATION` (for a station with no registry location
//! unconditionally, as `fixed` would; otherwise only if it is inside the same
//! radius) → PHYSICS-ONLY with the capped confidence.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use wrfcoin_shared_protocol::crypto::domain_signing::{
    configured_chain_id, configured_network_id, oracle_governance_pubkeys,
};
use wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature;
use wrfcoin_shared_protocol::crypto::signature::PublicKey;
use wrfcoin_shared_protocol::oracle::attester::{
    Attester, AttestationSubmission, ForbiddenRoleKeys, PendingObservation,
};
use wrfcoin_shared_protocol::oracle::mechanical::{MechanicalPolicy, MechanicalVerdict};
use wrfcoin_shared_protocol::weather::reference::ReferenceComparison;
use wrfcoin_shared_protocol::weather::station_reference::{
    DEFAULT_MAX_PAIR_DELTA_SECS, PressurePairing, SeaLevelPressureDecision,
    SeaLevelPressureOutcome, StationReferenceObservation,
    SeaLevelPressureClaim, pair_station_reference_sea_level,
};
use wrfcoin_shared_protocol::weather::validation::weather_observation_from_json;

/// How long a locally-submitted commit id suppresses re-submission while the
/// attestation waits to mine. After this, chain state (`attesting_oracles`)
/// is the deduplicator of record.
const RESUBMIT_SUPPRESSION: Duration = Duration::from_secs(300);

#[derive(Debug, serde::Deserialize)]
struct PendingResponse {
    chain_id: String,
    network_id: String,
    height: u64,
    oracle_set: Vec<String>,
    effective_quorum: usize,
    observations: Vec<PendingEntry>,
    /// Quorum-complete entries awaiting mining. Previously not even
    /// deserialized, so a fully-attested backlog reported as `pending=0` and
    /// looked identical to an idle chain during the 2026-08-28 quorum outage
    /// (#2833). Only the count is surfaced; entries need no re-attestation.
    #[serde(default)]
    ready_to_mine: Vec<serde_json::Value>,
    #[serde(default)]
    unrecoverable: Vec<String>,
    /// Consensus features in force at `height + 1`, as the node's genesis
    /// schedule resolves them (#3131). Absent on older nodes → empty → the
    /// pre-activation reward-tier rule.
    #[serde(default)]
    active_features: Vec<String>,
}

/// Whether the node reports the #3131 gross-outlier guard in force for the
/// next block. The node, not this process, decides: every honest attester then
/// changes rules at the one height the chain scheduled.
fn gross_outlier_guard_active(active_features: &[String]) -> bool {
    active_features
        .iter()
        .any(|name| name == ConsensusFeature::RewardGrossOutlierGuard.name())
}

/// Whether the node reports the #3285 sea-level pressure pairing in force for
/// the next block. Same rule as [`gross_outlier_guard_active`]: the chain's
/// schedule decides, never this process's configuration.
fn sea_level_pressure_active(active_features: &[String]) -> bool {
    active_features
        .iter()
        .any(|name| name == ConsensusFeature::ReferenceSeaLevelPressure.name())
}

/// The policy this cycle evaluates under: `policy` with its feature-gated
/// bits (#3131 guard, #3285 sea-level pressure) set from the node's
/// `active_features`. `None` when `policy` already matches, so the common
/// cycle clones nothing.
fn cycle_policy_override(
    policy: &MechanicalPolicy,
    active_features: &[String],
) -> Option<MechanicalPolicy> {
    let guard_active = gross_outlier_guard_active(active_features);
    let sea_level_active = sea_level_pressure_active(active_features);
    if guard_active == policy.gross_outlier_guard && sea_level_active == policy.sea_level_pressure
    {
        return None;
    }
    Some(MechanicalPolicy {
        gross_outlier_guard: guard_active,
        sea_level_pressure: sea_level_active,
        ..policy.clone()
    })
}

/// One pending entry, evaluated: the reveal preimage to sign, the reference
/// comparison (if any), the verdict, and how pressure was paired.
struct EntryEvaluation {
    observation: PendingObservation,
    comparison: Option<ReferenceComparison>,
    verdict: MechanicalVerdict,
    sea_level: SeaLevelPressureDecision,
}

impl EntryEvaluation {
    /// The tier as the shadow log reports it (#3240 review N4). A verdict
    /// with no scored reference comparison is PHYSICS-ONLY — the policy's
    /// own `checked` predicate — and printing the fold's default `good`
    /// next to its capped confidence would mislead the shadow review.
    fn tier_label(&self) -> String {
        if self.comparison.as_ref().is_some_and(|c| !c.residuals.is_empty()) {
            self.verdict.tier.to_string()
        } else {
            "PHYSICS-ONLY".to_string()
        }
    }
}

/// Evaluate one pending entry under `policy` — the pure part of the cycle.
///
/// Local-sensor reference comparison (#2746 ruling): pair the
/// revealed reading with the nearest NWS observation and let the band
/// machinery tier it. `None` — nothing paired, fetch failed, or no station
/// configured — produces a visibly capped physics-only attestation, never a
/// silent full-confidence one. With `policy.sea_level_pressure` (#3285) the
/// pressure variable pairs the reveal's `pressure_sea_level_hpa`, read from
/// the raw `data` JSON (never parsed into `WeatherObservation`), once it
/// recomputes from `pressure_hpa` and the station's registered elevation; a
/// mismatch or an unverifiable claim is added to the verdict's audit trail.
fn evaluate_entry(
    entry: &PendingEntry,
    policy: &MechanicalPolicy,
    references: &[StationReferenceObservation],
    max_pair_secs: i64,
    now: i64,
) -> EntryEvaluation {
    let observation = PendingObservation {
        commit_id: entry.commit_id.clone(),
        station_id: entry.station_id.clone(),
        nonce: entry.nonce.clone(),
        timestamp: entry.timestamp,
        data: entry.data.clone(),
        contribution_type: entry.contribution_type.clone(),
    };
    let mut sea_level = SeaLevelPressureDecision {
        outcome: SeaLevelPressureOutcome::Inactive,
        pairing: PressurePairing::Raw,
    };
    let comparison = weather_observation_from_json(&observation.data).and_then(|parsed| {
        let (comparison, decision) = pair_station_reference_sea_level(
            &parsed,
            SeaLevelPressureClaim::from_reveal(&observation.data),
            entry.elevation_meters,
            policy.sea_level_pressure,
            observation.timestamp,
            references,
            max_pair_secs,
        );
        sea_level = decision;
        comparison
    });
    // #3285 owner ruling: a published, checkable pressure_hpa with no
    // trustworthy sea-level value (station with a registered elevation) is
    // scored as an unverified Outlier variable, never simply dropped.
    let unverified: &[&str] = if sea_level.pairing.unverified_variables() > 0 {
        &["pressure_hpa"]
    } else {
        &[]
    };
    let mut verdict =
        policy.evaluate_with_unverified(&observation, comparison.as_ref(), now, unverified);
    if let Some(reason) = sea_level.audit_reason() {
        verdict.reasons.push(reason);
    }
    EntryEvaluation { observation, comparison, verdict, sea_level }
}

/// Wire shape of one pending observation. The preimage fields feed
/// [`PendingObservation`]; the rest is skip logic.
#[derive(Debug, serde::Deserialize)]
struct PendingEntry {
    commit_id: String,
    station_id: String,
    timestamp: i64,
    nonce: String,
    contribution_type: String,
    data: serde_json::Value,
    source: String,
    #[serde(default)]
    attesting_oracles: Vec<String>,
    has_quorum: bool,
    /// The station's registered elevation (#3285), for the sea-level
    /// pressure recompute. Absent on older nodes or unregistered stations →
    /// `None` → the claim is not trusted and pressure pairs raw.
    #[serde(default)]
    // determinism-ok: attester-side wire input, never consensus-executed
    elevation_meters: Option<f64>,
    /// #3240: registry location, coarsened by the node. Absent on older nodes
    /// and for unregistered stations.
    #[serde(default)]
    station_location: Option<ReferenceLocation>,
}

/// A coarse station location as served on the pending surface (#3240).
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub(crate) struct ReferenceLocation {
    // determinism-ok: service-side reference lookup input; never consensus
    latitude: f64,
    // determinism-ok: as above
    longitude: f64,
}

impl ReferenceLocation {
    /// The attester re-coarsens to 0.01° itself, so a node that served exact
    /// coordinates still never has them forwarded to api.weather.gov.
    fn coarse(self) -> Option<Self> {
        // determinism-ok: service-side rounding for an NWS lookup key; never consensus
        let round = |v: f64| (v * 100.0).round() / 100.0;
        let valid = self.latitude.is_finite()
            && self.longitude.is_finite()
            // determinism-ok: service-side coordinate range check; never consensus
            && (-90.0..=90.0).contains(&self.latitude)
            // determinism-ok: as above
            && (-180.0..=180.0).contains(&self.longitude);
        valid.then(|| Self {
            latitude: round(self.latitude),
            longitude: round(self.longitude),
        })
    }

    fn point_key(self) -> String {
        format!("{:.2},{:.2}", self.latitude, self.longitude)
    }
}

/// How this process chooses the reference station for a commit (#3240).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReferenceMode {
    /// Every commit vs the configured station — exactly the pre-#3240 path.
    Fixed,
    /// Submit the `Fixed` verdict; compute and log the per-station one.
    Shadow,
    /// Submit the per-station verdict.
    PerStation,
}

impl ReferenceMode {
    fn parse(raw: Option<&str>) -> Result<Self> {
        match raw.map(str::trim).filter(|v| !v.is_empty()) {
            None | Some("fixed") => Ok(Self::Fixed),
            Some("shadow") => Ok(Self::Shadow),
            Some("per_station") => Ok(Self::PerStation),
            Some(other) => bail!(
                "WRFCOIN_ATTESTER_REFERENCE_MODE={other:?} is not one of fixed | shadow | \
                 per_station. Refusing to start rather than guess a reward-affecting mode."
            ),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Shadow => "shadow",
            Self::PerStation => "per_station",
        }
    }
}

pub(crate) struct Config {
    node_url: String,
    api_token: String,
    chain_id: String,
    network_id: String,
    poll_interval: Duration,
    once: bool,
    /// NWS station id for local-sensor reference comparison (e.g. `KDLH` for
    /// both Duluth PWS deployments). `None` = PHYSICS-ONLY MODE: every
    /// attestation's confidence is capped and the mode is logged loudly.
    reference_station: Option<String>,
    /// Base URL for the NWS API; overridable so tests and air-gapped
    /// deployments can point at a mirror.
    reference_base_url: String,
    /// Max |Δt| between a reading and its paired reference observation.
    reference_max_pair_secs: i64,
    /// #3240 reference selection mode (default `Fixed`).
    reference_mode: ReferenceMode,
    /// Max station↔reference distance in km; required outside `Fixed`.
    // determinism-ok: service-side reference radius; never consensus
    max_reference_km: Option<f64>,
    /// Per-request timeout for NWS lookups and per-station fetches.
    reference_lookup_timeout: Duration,
    /// Bound on distinct NWS stations fetched / point lookups per cycle.
    max_reference_stations_per_cycle: usize,
}

/// Parse and validate the #3240 mode/radius pair. Pure, so the refusals are
/// testable without mutating process environment.
// determinism-ok: service-side startup configuration; never consensus
fn reference_selection_config(
    mode_raw: Option<&str>,
    max_km_raw: Option<&str>,
    // determinism-ok: service-side startup configuration (radius in km); never consensus
) -> Result<(ReferenceMode, Option<f64>)> {
    let mode = ReferenceMode::parse(mode_raw)?;
    // determinism-ok: service-side reference radius parse + validation
    let parsed: Result<Option<f64>> = (|| {
        let max_km = max_km_raw
            .map(str::trim)
            .filter(|v| !v.is_empty())
            // determinism-ok: service-side reference radius parse
            .map(str::parse::<f64>)
            .transpose()
            .context("WRFCOIN_ATTESTER_MAX_REFERENCE_KM must be a number of kilometres")?;
        if let Some(km) = max_km {
            // determinism-ok: service-side validation of the radius
            if !km.is_finite() || km <= 0.0 {
                bail!("WRFCOIN_ATTESTER_MAX_REFERENCE_KM must be a positive number, got {km}");
            }
        }
        Ok(max_km)
    })();
    if mode == ReferenceMode::Fixed {
        // #3240 review N2: `fixed` never reads the radius, so a malformed value
        // must not stop the attester — it is dropped with a warning. The modes
        // that use it (below) still refuse to start on it.
        if let Err(err) = &parsed {
            log::warn!(
                "[attester] ignoring WRFCOIN_ATTESTER_MAX_REFERENCE_KM in fixed mode: {err:#} \
                 (it is only read by shadow / per_station, which would refuse to start)"
            );
        }
        return Ok((mode, None));
    }
    let max_km = parsed?;
    if max_km.is_none() {
        bail!(
            "WRFCOIN_ATTESTER_REFERENCE_MODE={} requires WRFCOIN_ATTESTER_MAX_REFERENCE_KM. \
             There is deliberately no default: the nws_station bands were calibrated at \
             ~3 km separation, so the radius is an owner reward-policy choice (#3240).",
            mode.as_str()
        );
    }
    Ok((mode, max_km))
}

impl Config {
    fn from_env() -> Result<Self> {
        // determinism-ok: service startup configuration; nothing here reaches consensus state
        let node_url = std::env::var("WRFCOIN_ATTESTER_NODE_URL")
            .map_err(|_| anyhow!("WRFCOIN_ATTESTER_NODE_URL is not set"))?
            .trim_end_matches('/')
            .to_string();
        // determinism-ok: service startup configuration, same as node_url above
        let api_token = std::env::var("WRFCOIN_ATTESTER_API_TOKEN")
            .map_err(|_| anyhow!("WRFCOIN_ATTESTER_API_TOKEN is not set"))?;

        // #2713: an unset chain identity is a refusal, never a silent default
        // to the mainnet domain. `configured_*` returns None for unset OR
        // blank, which is exactly the distinction the incident was about.
        let chain_id = configured_chain_id().ok_or_else(|| {
            anyhow!(
                "WRFCOIN_CHAIN_ID is not set (or blank). Refusing to start: an attester \
                 with no explicit chain identity would sign the mainnet domain (#2713). \
                 Set it to the target chain's id before starting."
            )
        })?;
        let network_id = configured_network_id().ok_or_else(|| {
            anyhow!(
                "WRFCOIN_NETWORK_ID is not set (or blank). Refusing to start for the same \
                 reason as WRFCOIN_CHAIN_ID (#2713)."
            )
        })?;

        let poll_interval = Duration::from_secs(
            // determinism-ok: service startup configuration; nothing here reaches consensus state
            std::env::var("WRFCOIN_ATTESTER_POLL_SECS")
                .ok()
                .map(|v| v.parse::<u64>())
                .transpose()
                .context("WRFCOIN_ATTESTER_POLL_SECS must be an integer number of seconds")?
                .unwrap_or(5)
                .max(1),
        );
        // determinism-ok: smoke-test mode flag read at startup, service-side only
        let once = std::env::var("WRFCOIN_ATTESTER_ONCE").map(|v| v == "1").unwrap_or(false);

        // determinism-ok: startup configuration for the reference source
        let reference_station = std::env::var("WRFCOIN_ATTESTER_NWS_STATION")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        // determinism-ok: as above
        let reference_base_url = std::env::var("WRFCOIN_ATTESTER_NWS_BASE_URL")
            .unwrap_or_else(|_| "https://api.weather.gov".to_string())
            .trim_end_matches('/')
            .to_string();
        // determinism-ok: as above
        let reference_max_pair_secs = std::env::var("WRFCOIN_ATTESTER_REFERENCE_MAX_PAIR_SECS")
            .ok()
            .map(|v| v.parse::<i64>())
            .transpose()
            .context("WRFCOIN_ATTESTER_REFERENCE_MAX_PAIR_SECS must be an integer number of seconds")?
            .unwrap_or(DEFAULT_MAX_PAIR_DELTA_SECS)
            .max(1);

        let (reference_mode, max_reference_km) = reference_selection_config(
            // determinism-ok: startup configuration for the #3240 reference mode
            std::env::var("WRFCOIN_ATTESTER_REFERENCE_MODE").ok().as_deref(),
            // determinism-ok: as above
            std::env::var("WRFCOIN_ATTESTER_MAX_REFERENCE_KM").ok().as_deref(),
        )?;
        let reference_lookup_timeout = Duration::from_secs(
            // determinism-ok: startup configuration, as above
            std::env::var("WRFCOIN_ATTESTER_REFERENCE_LOOKUP_TIMEOUT_SECS")
                .ok()
                .map(|v| v.parse::<u64>())
                .transpose()
                .context("WRFCOIN_ATTESTER_REFERENCE_LOOKUP_TIMEOUT_SECS must be an integer")?
                .unwrap_or(5)
                .max(1),
        );
        // determinism-ok: startup configuration, as above
        let max_reference_stations_per_cycle = std::env::var("WRFCOIN_ATTESTER_MAX_REFERENCE_STATIONS_PER_CYCLE")
            .ok()
            .map(|v| v.parse::<usize>())
            .transpose()
            .context("WRFCOIN_ATTESTER_MAX_REFERENCE_STATIONS_PER_CYCLE must be an integer")?
            .unwrap_or(8)
            .max(1);

        Ok(Self {
            node_url,
            api_token,
            chain_id,
            network_id,
            poll_interval,
            once,
            reference_station,
            reference_base_url,
            reference_max_pair_secs,
            reference_mode,
            max_reference_km,
            reference_lookup_timeout,
            max_reference_stations_per_cycle,
        })
    }

    /// Explicit configuration for tests that drive this binary's real HTTP
    /// cycle without mutating process-global environment variables.
    #[cfg(test)]
    pub(crate) fn for_test(
        node_url: impl Into<String>,
        api_token: impl Into<String>,
        chain_id: impl Into<String>,
        network_id: impl Into<String>,
        reference_station: Option<String>,
        reference_base_url: impl Into<String>,
        reference_max_pair_secs: i64,
    ) -> Self {
        Self {
            node_url: node_url.into().trim_end_matches('/').to_string(),
            api_token: api_token.into(),
            chain_id: chain_id.into(),
            network_id: network_id.into(),
            poll_interval: Duration::from_secs(1),
            once: true,
            reference_station,
            reference_base_url: reference_base_url
                .into()
                .trim_end_matches('/')
                .to_string(),
            reference_max_pair_secs: reference_max_pair_secs.max(1),
            reference_mode: ReferenceMode::Fixed,
            max_reference_km: None,
            reference_lookup_timeout: Duration::from_secs(5),
            max_reference_stations_per_cycle: 8,
        }
    }

    /// Test-only: switch a `for_test` config to a #3240 selection mode.
    #[cfg(test)]
    pub(crate) fn with_reference_selection(
        mut self,
        mode: ReferenceMode,
        // determinism-ok: test-only configuration
        max_reference_km: f64,
        max_stations: usize,
    ) -> Self {
        self.reference_mode = mode;
        self.max_reference_km = Some(max_reference_km);
        self.max_reference_stations_per_cycle = max_stations.max(1);
        self.reference_lookup_timeout = Duration::from_secs(2);
        self
    }
}

// ---------------------------------------------------------------------------
// Local-sensor reference source (#2746 reference-scope ruling)
// ---------------------------------------------------------------------------

/// Parse an NWS `GET /stations/{id}/observations` GeoJSON body into normalized
/// reference observations. Mirrors the collector's `fetchNwsObservations`
/// exactly: temperature already °C, `barometricPressure` Pa → hPa (÷100 —
/// sea-level pressure, comparable to the WS-5000's submitted relative
/// pressure), `windSpeed` km/h → m/s (÷3.6), features without a timestamp
/// dropped. Pure so the wire contract is testable without the network.
fn parse_nws_observations(body: &str) -> Result<Vec<StationReferenceObservation>> {
    #[derive(serde::Deserialize)]
    struct Collection {
        #[serde(default)]
        features: Vec<Feature>,
    }
    #[derive(serde::Deserialize)]
    struct Feature {
        #[serde(default)]
        properties: Option<Properties>,
    }
    #[derive(serde::Deserialize)]
    struct Properties {
        timestamp: Option<String>,
        #[serde(default)]
        temperature: Measurement,
        #[serde(default, rename = "barometricPressure")]
        barometric_pressure: Measurement,
        #[serde(default, rename = "relativeHumidity")]
        relative_humidity: Measurement,
        #[serde(default, rename = "windSpeed")]
        wind_speed: Measurement,
    }
    #[derive(serde::Deserialize, Default)]
    struct Measurement {
        // determinism-ok: NWS wire value parsed by the attester service; influences only the confidence this process signs — consensus receives those bytes verbatim
        value: Option<f64>,
    }

    let collection: Collection =
        serde_json::from_str(body).context("NWS response is not observation GeoJSON")?;
    Ok(collection
        .features
        .into_iter()
        .filter_map(|f| {
            let p = f.properties?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(p.timestamp.as_deref()?)
                .ok()?
                .timestamp();
            Some(StationReferenceObservation {
                timestamp,
                temperature_c: p.temperature.value,
                // determinism-ok: attester-side unit conversion (Pa→hPa), mirroring the collector; never consensus-executed
                pressure_hpa: p.barometric_pressure.value.map(|pa| pa / 100.0),
                humidity_pct: p.relative_humidity.value,
                // determinism-ok: attester-side unit conversion (km/h→m/s), mirroring the collector; never consensus-executed
                wind_speed_ms: p.wind_speed.value.map(|kmh| kmh / 3.6),
            })
        })
        .collect())
}

/// Fetch the reference station's recent observations for this cycle.
///
/// Failure semantics are the visible-not-implicit rule: any error here means
/// this cycle attests PHYSICS-ONLY (capped confidence, loud log at the call
/// site) — it never refuses honest readings and never silently pretends the
/// comparison happened.
async fn fetch_reference_observations(
    client: &reqwest::Client,
    config: &Config,
    station: &str,
    timeout: Option<Duration>,
) -> Result<Vec<StationReferenceObservation>> {
    let mut request = client
        .get(format!(
            "{}/stations/{}/observations?limit=24",
            config.reference_base_url, station
        ))
        .header("Accept", "application/geo+json");
    // `None` keeps the pre-#3240 fixed-mode request exactly as it was.
    if let Some(timeout) = timeout {
        request = request.timeout(timeout);
    }
    let response = request.send().await.context("NWS observations request failed")?;
    let status = response.status();
    if !status.is_success() {
        bail!("NWS observations request for {station} returned {status}");
    }
    let body = response.text().await.context("NWS observations body read failed")?;
    parse_nws_observations(&body)
}

// ---------------------------------------------------------------------------
// Per-commit reference selection (#3240)
// ---------------------------------------------------------------------------

/// An NWS observation station with its coordinates.
#[derive(Debug, Clone, PartialEq)]
struct NwsStation {
    id: String,
    // determinism-ok: NWS wire coordinates, service-side distance input only
    latitude: f64,
    // determinism-ok: as above
    longitude: f64,
}

/// Why a lookup produced no station. `NoCoverage` (NWS answered 404: the
/// point is outside its grid) is permanent and cached for the process
/// lifetime; everything else is retried next cycle.
#[derive(Debug, Clone, PartialEq)]
enum LookupError {
    NoCoverage(String),
    Transient(String),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCoverage(detail) | Self::Transient(detail) => f.write_str(detail),
        }
    }
}

/// Parse `GET /points/{lat},{lon}` → `properties.observationStations` URL.
fn parse_points_observation_stations_url(body: &str) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("NWS points response is not JSON")?;
    value
        .pointer("/properties/observationStations")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("NWS points response has no properties.observationStations"))
}

/// Parse one GeoJSON station feature (`geometry.coordinates` is `[lon, lat]`).
fn parse_station_feature(feature: &serde_json::Value) -> Option<NwsStation> {
    let id = feature.pointer("/properties/stationIdentifier")?.as_str()?.trim();
    let coords = feature.pointer("/geometry/coordinates")?.as_array()?;
    // determinism-ok: NWS wire coordinates, service-side only
    let longitude = coords.first()?.as_f64()?;
    // determinism-ok: as above
    let latitude = coords.get(1)?.as_f64()?;
    (!id.is_empty() && latitude.is_finite() && longitude.is_finite()).then(|| NwsStation {
        id: id.to_string(),
        latitude,
        longitude,
    })
}

/// Parse the observation-station list for a grid point. NWS orders it by
/// distance from the point, so the first usable feature is the nearest.
fn parse_nearest_station(body: &str) -> Result<NwsStation> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("NWS station list is not JSON")?;
    value
        .get("features")
        .and_then(|v| v.as_array())
        .and_then(|features| features.iter().find_map(parse_station_feature))
        .ok_or_else(|| anyhow!("NWS station list has no usable station feature"))
}

/// Re-root an absolute URL NWS returned onto the configured base, so a mirror
/// or a test server stays in the loop and a response cannot steer this
/// process to an arbitrary host.
fn rebase_nws_url(base: &str, url: &str) -> Result<String> {
    if let Some(path) = url.strip_prefix(base) {
        return Ok(format!("{base}{path}"));
    }
    let path = if let Some(rest) =
        url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))
    {
        rest.find('/').map(|i| &rest[i..]).unwrap_or("/")
    } else if url.starts_with('/') {
        url
    } else {
        bail!("NWS returned an unusable URL {url:?}");
    };
    Ok(format!("{base}{path}"))
}

/// Great-circle distance between a station's registered (coarsened) location
/// and a reference station's own NWS coordinates, in km — the radius check.
/// Computed locally by `station_reference::great_circle_km`; the NWS ordering
/// of candidates never stands in for it (#3240).
// determinism-ok: service-side distance for reference selection; never consensus
fn reference_distance_km(location: ReferenceLocation, station: &NwsStation) -> f64 {
    let registered = wrfcoin_shared_protocol::station_registry::StationLocation {
        latitude: location.latitude,
        longitude: location.longitude,
        elevation_meters: None,
    };
    wrfcoin_shared_protocol::weather::station_reference::great_circle_km(
        &registered,
        station.latitude,
        station.longitude,
    )
}

/// Process-lifetime caches for reference selection. Stations do not move and
/// registry locations change only by transaction, so successful point and
/// station-coordinate lookups are kept for the life of the process; transient
/// failures are remembered only for the cycle they happened in.
#[derive(Debug, Default)]
pub(crate) struct ReferenceResolver {
    cycle: u64,
    // determinism-ok: service-side lookup cache, keyed lookups only (never iterated)
    nearest: HashMap<String, Result<NwsStation, LookupError>>,
    // determinism-ok: as above
    station_coords: HashMap<String, NwsStation>,
    // determinism-ok: as above — key → cycle the transient failure happened in
    failed_this_cycle: HashMap<String, u64>,
    /// Set by a 429/503 from NWS: no lookups until it passes.
    backoff_until: Option<Instant>,
    /// HTTP requests made (points + station lists + station coordinates);
    /// observable so tests can prove the caches work.
    pub(crate) lookup_requests: usize,
}

/// Per-cycle state: observations fetched once per station per cycle, and the
/// budgets that bound how much NWS traffic one cycle may generate.
#[derive(Debug, Default)]
struct CycleReferences {
    // determinism-ok: per-cycle keyed cache (never iterated)
    observations: HashMap<String, Result<Vec<StationReferenceObservation>, String>>,
    lookups: usize,
    /// First transport-level lookup failure this cycle. After one, the rest
    /// of the cycle skips lookups, so an NWS outage costs one timeout per
    /// cycle, not one per commit.
    lookup_outage: Option<String>,
    /// #3240 review N1 (narrowed after the #3341 review): transport failures
    /// (timeout, connect, body read) on per-station OBSERVATION fetches,
    /// counted per distinct station. A single slow station is that station's
    /// problem, so its commits alone go capped; once a second distinct
    /// station fails, api.weather.gov itself is treated as down and further
    /// per-station observation fetches are skipped for the cycle. Separate
    /// from `lookup_outage`, and never applied to the configured station.
    observation_transport_failures: usize,
    observation_outage: Option<String>,
}

/// The reference chosen for one commit, with every fallback step named.
#[derive(Debug, Default)]
struct ReferenceSelection {
    station: Option<String>,
    observations: Vec<StationReferenceObservation>,
    // determinism-ok: service-side distance, logged only
    distance_km: Option<f64>,
    notes: Vec<String>,
}

impl ReferenceSelection {
    fn label(&self) -> String {
        match (&self.station, self.distance_km) {
            (Some(station), Some(km)) => format!("{station}@{km:.1}km"),
            (Some(station), None) => format!("{station}@unknown-distance"),
            (None, _) => "none".to_string(),
        }
    }
}

impl ReferenceResolver {
    fn begin_cycle(&mut self) {
        self.cycle += 1;
        let cycle = self.cycle;
        self.failed_this_cycle.retain(|_, at| *at == cycle);
    }

    fn backoff_remaining(&self) -> Option<Duration> {
        self.backoff_until
            // determinism-ok: service-side rate-limit backoff clock
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .filter(|left| !left.is_zero())
    }

    /// One budgeted, rate-limit-aware NWS GET. Returns the body on 2xx.
    async fn lookup_get(
        &mut self,
        client: &reqwest::Client,
        config: &Config,
        refs: &mut CycleReferences,
        url: &str,
    ) -> Result<String, LookupError> {
        if let Some(left) = self.backoff_remaining() {
            return Err(LookupError::Transient(format!(
                "NWS lookups backing off after a rate limit ({}s left)",
                left.as_secs().max(1)
            )));
        }
        if let Some(outage) = &refs.lookup_outage {
            return Err(LookupError::Transient(format!(
                "NWS lookups skipped for the rest of this cycle after: {outage}"
            )));
        }
        // A new location costs two requests (point + station list), so the
        // lookup budget is twice the distinct-station budget.
        let lookup_budget = config.max_reference_stations_per_cycle.saturating_mul(2);
        if refs.lookups >= lookup_budget {
            return Err(LookupError::Transient(format!(
                "per-cycle NWS lookup budget ({lookup_budget}) exhausted"
            )));
        }
        refs.lookups += 1;
        self.lookup_requests += 1;
        let response = match client
            .get(url)
            .header("Accept", "application/geo+json")
            .timeout(config.reference_lookup_timeout)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                let detail = format!("NWS lookup {url} failed: {err}");
                refs.lookup_outage = Some(detail.clone());
                return Err(LookupError::Transient(detail));
            },
        };
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        {
            let secs = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(60)
                .clamp(1, 600);
            // determinism-ok: service-side rate-limit backoff clock
            self.backoff_until = Some(Instant::now() + Duration::from_secs(secs));
            let detail = format!("NWS lookup {url} returned {status}; backing off {secs}s");
            refs.lookup_outage = Some(detail.clone());
            return Err(LookupError::Transient(detail));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(LookupError::NoCoverage(format!(
                "NWS has no coverage for {url} (404)"
            )));
        }
        if !status.is_success() {
            return Err(LookupError::Transient(format!("NWS lookup {url} returned {status}")));
        }
        response
            .text()
            .await
            .map_err(|err| LookupError::Transient(format!("NWS lookup {url} body: {err}")))
    }

    /// Nearest NWS observation station for a (coarse) location.
    async fn nearest_station(
        &mut self,
        client: &reqwest::Client,
        config: &Config,
        refs: &mut CycleReferences,
        location: ReferenceLocation,
    ) -> Result<NwsStation, LookupError> {
        let key = location.point_key();
        if let Some(cached) = self.nearest.get(&key) {
            return cached.clone();
        }
        if self.failed_this_cycle.get(&key) == Some(&self.cycle) {
            return Err(LookupError::Transient(format!(
                "nearest-station lookup for {key} already failed this cycle"
            )));
        }
        let result = async {
            let points_url = format!("{}/points/{key}", config.reference_base_url);
            let body = self.lookup_get(client, config, refs, &points_url).await?;
            let stations_url = parse_points_observation_stations_url(&body)
                .and_then(|url| rebase_nws_url(&config.reference_base_url, &url))
                .map_err(|e| LookupError::Transient(format!("{e:#}")))?;
            let body = self.lookup_get(client, config, refs, &stations_url).await?;
            parse_nearest_station(&body).map_err(|e| LookupError::Transient(format!("{e:#}")))
        }
        .await;
        match &result {
            Ok(station) => {
                self.station_coords.insert(station.id.clone(), station.clone());
                self.nearest.insert(key, result.clone());
            },
            Err(LookupError::NoCoverage(_)) => {
                self.nearest.insert(key, result.clone());
            },
            Err(LookupError::Transient(_)) => {
                self.failed_this_cycle.insert(key, self.cycle);
            },
        }
        result
    }

    /// Coordinates of a named station (the configured fallback), cached.
    async fn station_coordinates(
        &mut self,
        client: &reqwest::Client,
        config: &Config,
        refs: &mut CycleReferences,
        station: &str,
    ) -> Result<NwsStation, LookupError> {
        if let Some(known) = self.station_coords.get(station) {
            return Ok(known.clone());
        }
        let key = format!("station:{station}");
        if self.failed_this_cycle.get(&key) == Some(&self.cycle) {
            return Err(LookupError::Transient(format!(
                "location lookup for {station} already failed this cycle"
            )));
        }
        let url = format!("{}/stations/{station}", config.reference_base_url);
        let result = match self.lookup_get(client, config, refs, &url).await {
            Ok(body) => serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .as_ref()
                .and_then(parse_station_feature)
                .ok_or_else(|| {
                    LookupError::Transient(format!("NWS station {station} has no coordinates"))
                }),
            Err(err) => Err(err),
        };
        match &result {
            Ok(found) => {
                self.station_coords.insert(station.to_string(), found.clone());
            },
            Err(_) => {
                self.failed_this_cycle.insert(key, self.cycle);
            },
        }
        result
    }
}

/// Distinct non-configured stations whose observation fetch must fail at the
/// transport level before the rest of the cycle's per-station observation
/// fetches are skipped (#3240 review N1, narrowed by the #3341 review). With
/// the lookup outage (one timeout) and the configured station (at most one
/// attempt), a fully hung api.weather.gov costs at most four lookup timeouts
/// per cycle.
const OBSERVATION_OUTAGE_STATIONS: usize = 2;

impl CycleReferences {
    /// Observations for `station`, fetched at most once per cycle and within
    /// the per-cycle distinct-station budget.
    async fn observations_for(
        &mut self,
        client: &reqwest::Client,
        config: &Config,
        station: &str,
    ) -> Result<Vec<StationReferenceObservation>, String> {
        if let Some(cached) = self.observations.get(station) {
            return cached.clone();
        }
        // The configured station is the fallback many commits share (and
        // the only reference an unregistered station ever uses), so it is
        // exempt from the observation outage: at most one attempt per cycle,
        // because the result — success or error — is cached above.
        let configured = config.reference_station.as_deref() == Some(station);
        if !configured {
            if let Some(outage) = &self.observation_outage {
                return Err(format!(
                    "NWS observation fetches skipped for the rest of this cycle after \
                     {OBSERVATION_OUTAGE_STATIONS} stations failed; last: {outage}"
                ));
            }
        }
        if self.observations.len() >= config.max_reference_stations_per_cycle {
            return Err(format!(
                "per-cycle distinct reference station budget ({}) exhausted",
                config.max_reference_stations_per_cycle
            ));
        }
        let fetched = fetch_reference_observations(
            client,
            config,
            station,
            Some(config.reference_lookup_timeout),
        )
        .await;
        // A transport failure (timeout, connect, body read) is an NWS outage
        // signal; a per-station HTTP status or an unparseable body is not.
        if let Err(err) = &fetched {
            let transport =
                err.chain().any(|cause| cause.downcast_ref::<reqwest::Error>().is_some());
            if transport && !configured {
                self.observation_transport_failures += 1;
                if self.observation_transport_failures >= OBSERVATION_OUTAGE_STATIONS {
                    self.observation_outage =
                        Some(format!("NWS observations for {station} failed: {err:#}"));
                }
            }
        }
        let result = fetched.map_err(|err| format!("{err:#}"));
        if let Err(err) = &result {
            log::warn!("[attester] REFERENCE UNAVAILABLE: NWS station {station}: {err}");
        }
        self.observations.insert(station.to_string(), result.clone());
        result
    }
}

/// Choose the reference for one commit (#3240). Every step that is not "the
/// nearest station, within the radius, with observations" is recorded in
/// `notes` — the fallbacks are never silent.
async fn select_reference(
    client: &reqwest::Client,
    config: &Config,
    resolver: &mut ReferenceResolver,
    refs: &mut CycleReferences,
    station_location: Option<ReferenceLocation>,
) -> ReferenceSelection {
    let mut selection = ReferenceSelection::default();
    // determinism-ok: service-side reference radius
    let max_km = config.max_reference_km.unwrap_or(f64::INFINITY);
    let location = station_location.and_then(ReferenceLocation::coarse);
    let mut tried: Option<String> = None;

    match location {
        None => selection.notes.push(
            "reference: station has no registry location on the pending surface — \
             falling back to the configured station"
                .to_string(),
        ),
        Some(location) => match resolver.nearest_station(client, config, refs, location).await {
            Ok(nearest) => {
                let km = reference_distance_km(location, &nearest);
                if km > max_km {
                    selection.notes.push(format!(
                        "reference: nearest NWS station {} is {km:.1} km away (> {max_km} km) — \
                         PHYSICS-ONLY",
                        nearest.id
                    ));
                    return selection;
                }
                match refs.observations_for(client, config, &nearest.id).await {
                    Ok(observations) if !observations.is_empty() => {
                        selection.station = Some(nearest.id);
                        selection.distance_km = Some(km);
                        selection.observations = observations;
                        return selection;
                    },
                    Ok(_) => selection.notes.push(format!(
                        "reference: nearest NWS station {} returned zero observations — \
                         falling back to the configured station",
                        nearest.id
                    )),
                    Err(err) => selection.notes.push(format!(
                        "reference: fetch from nearest NWS station {} failed ({err}) — \
                         falling back to the configured station",
                        nearest.id
                    )),
                }
                tried = Some(nearest.id);
            },
            Err(LookupError::NoCoverage(detail)) => selection.notes.push(format!(
                "reference: {detail} — falling back to the configured station"
            )),
            Err(LookupError::Transient(detail)) => selection.notes.push(format!(
                "reference: nearest-station lookup failed ({detail}) — falling back to the \
                 configured station"
            )),
        },
    }

    // Fallback 1: the configured station.
    let Some(configured) = config.reference_station.clone() else {
        selection.notes.push(
            "reference: no WRFCOIN_ATTESTER_NWS_STATION configured — PHYSICS-ONLY".to_string(),
        );
        return selection;
    };
    if tried.as_deref() == Some(configured.as_str()) {
        selection
            .notes
            .push(format!("reference: configured station {configured} already failed — PHYSICS-ONLY"));
        return selection;
    }
    if let Some(location) = location {
        // A located station only falls back inside the same radius: a remote
        // station scored against a far-away configured station would tier as
        // an outlier (×0.1) for honest data, which is worse than capped.
        match resolver.station_coordinates(client, config, refs, &configured).await {
            Ok(found) => {
                let km = reference_distance_km(location, &found);
                if km > max_km {
                    selection.notes.push(format!(
                        "reference: configured station {configured} is {km:.1} km away \
                         (> {max_km} km) — PHYSICS-ONLY"
                    ));
                    return selection;
                }
                selection.distance_km = Some(km);
            },
            Err(err) => {
                selection.notes.push(format!(
                    "reference: cannot locate configured station {configured} to check the \
                     radius ({err}) — PHYSICS-ONLY"
                ));
                return selection;
            },
        }
    }
    match refs.observations_for(client, config, &configured).await {
        Ok(observations) if !observations.is_empty() => {
            selection
                .notes
                .push(format!("reference: using configured station {configured} (fallback)"));
            selection.station = Some(configured);
            selection.observations = observations;
        },
        Ok(_) => {
            selection.distance_km = None;
            selection.notes.push(format!(
                "reference: configured station {configured} returned zero observations — \
                 PHYSICS-ONLY"
            ));
        },
        Err(err) => {
            selection.distance_km = None;
            selection.notes.push(format!(
                "reference: fetch from configured station {configured} failed ({err}) — \
                 PHYSICS-ONLY"
            ));
        },
    }
    selection
}

fn mechanical_policy_from_env() -> Result<MechanicalPolicy> {
    let mut policy = MechanicalPolicy::default();
    // determinism-ok: service configuration read at startup only
    if let Ok(v) = std::env::var("WRFCOIN_ATTESTER_MAX_AGE_SECS") {
        policy.max_age_secs = v
            .parse()
            .context("WRFCOIN_ATTESTER_MAX_AGE_SECS must be an integer number of seconds")?;
    }
    // determinism-ok: as above
    if let Ok(v) = std::env::var("WRFCOIN_ATTESTER_MAX_FUTURE_SKEW_SECS") {
        policy.max_future_skew_secs = v
            .parse()
            .context("WRFCOIN_ATTESTER_MAX_FUTURE_SKEW_SECS must be an integer number of seconds")?;
    }
    Ok(policy)
}

/// Build the attester with a COMPLETE custody inventory (#2464).
///
/// On an attester host the snapshot/producer signers are deliberately NOT
/// configured (they live on other hosts), so their key objects cannot be
/// inventoried from the local environment — and `governance` never can be.
/// Each such role's PUBLIC key (recorded in the key ceremony) is supplied via
/// `WRFCOIN_ATTESTER_FORBID_<ROLE>_PUBKEY` instead. An inventory still
/// missing a role after that refuses, which is the same fail-closed rule
/// `Attester::from_env` enforces — this wrapper just names the env vars in
/// the error instead of leaving a generic refusal.
fn add_forbidden_role_keys(
    mut forbidden: ForbiddenRoleKeys,
    role: &str,
    var: &str,
    raw: &str,
) -> Result<ForbiddenRoleKeys> {
    for (index, candidate) in raw.split(',').enumerate() {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            bail!(
                "{var} entry {} is blank; supply a comma-separated public-key list without empty entries",
                index + 1
            );
        }
        // Do not echo `candidate`: this inventory is public by contract, but a
        // miswired secret must not be reflected into logs while reporting the
        // configuration error.
        let key = PublicKey::from_hex(candidate).map_err(|_| {
            anyhow!(
                "{var} entry {} is not a valid ed25519 public key hex value",
                index + 1
            )
        })?;
        forbidden = forbidden.with_key(role, key);
    }
    Ok(forbidden)
}

fn attester_from_env() -> Result<Attester> {
    let mut forbidden = ForbiddenRoleKeys::from_env();
    for (role, var) in [
        ("snapshot", "WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY"),
        ("producer", "WRFCOIN_ATTESTER_FORBID_PRODUCER_PUBKEY"),
        ("governance", "WRFCOIN_ATTESTER_FORBID_GOVERNANCE_PUBKEY"),
    ] {
        // determinism-ok: startup custody inventory, never consensus input
        let Ok(hexes) = std::env::var(var) else {
            continue;
        };
        forbidden = add_forbidden_role_keys(forbidden, role, var, &hexes)?;
    }
    if !forbidden.is_complete() {
        bail!(
            "custody inventory incomplete: no key could be resolved for role(s) {:?}. \
             The #2464 custody rule requires proving the oracle key differs from every \
             other role's key material. Supply each missing role's PUBLIC key hex \
             (recorded in the key ceremony) via WRFCOIN_ATTESTER_FORBID_<ROLE>_PUBKEY.",
            forbidden.uninventoried()
        );
    }
    match Attester::from_env(&forbidden)? {
        Some(attester) => Ok(attester),
        // Names ENV VARS, never their values: a static string with no format
        // arguments, so nothing from the environment, the keystore or the PKCS#11
        // session can reach it. The only interpolation in this function is
        // `forbidden.uninventoried()` above, which yields role names.
        None => bail!( // secret-leak-ok: static help text naming env vars, no values interpolated
            "no oracle signer is configured. Set WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY / \
             WRFCOIN_ORACLE_SIGNER_KEY_FILE (debug builds) or WRFCOIN_HSM_MODULE_PATH + \
             WRFCOIN_HSM_ORACLE_KEY_LABEL (PKCS#11) — an oracle that cannot sign cannot \
             attest, and a node short of quorum pays no contributor."
        ),
    }
}

async fn fetch_pending(
    client: &reqwest::Client,
    config: &Config,
) -> Result<PendingResponse> {
    let response = client
        .get(format!("{}/transactions/oracle/pending", config.node_url))
        .bearer_auth(&config.api_token)
        .send()
        .await
        .context("GET /transactions/oracle/pending failed")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("GET /transactions/oracle/pending returned {status}: {body}");
    }
    response
        .json::<PendingResponse>()
        .await
        .context("pending response did not parse")
}

/// #3095: refresh this process's view of the upgrade notice from the node it
/// attests through, and answer whether it may still sign for `height`.
///
/// The node is a transport, not an authority: whatever it serves is offered to
/// `accept_upgrade_notice`, which verifies the governance signatures against
/// this attester's own chain identity and allowlist before anything is stored.
/// A node that serves nothing, serves garbage, or is unreachable leaves the
/// attester exactly as it was.
///
/// Returns `false` when this binary must stop attesting: an attester on an old
/// rule set signing for a height on the new one is the #1972 failure with a
/// signature attached — the signatures would be worthless at best and a fork
/// endorsement at worst.
async fn upgrade_gate(client: &reqwest::Client, config: &Config, height: u64) -> bool {
    use wrfcoin_native_chain::upgrade_notice::{
        accept_upgrade_notice, log_run_up, upgrade_notice, upgrade_status,
    };

    let url = format!("{}/network/upgrade", config.node_url);
    match client.get(&url).bearer_auth(&config.api_token).send().await {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(body) => {
                if let Some(raw) = body.get("notice").filter(|v| !v.is_null()) {
                    match serde_json::from_value(raw.clone()) {
                        Ok(candidate) => {
                            let _ = accept_upgrade_notice(
                                candidate,
                                &config.chain_id,
                                &config.network_id,
                                &oracle_governance_pubkeys(),
                                // determinism-ok: attester-side expiry check on a notice served by the node; this process signs attestations, never blocks, and the value never enters a preimage, a state transition or the state root
                                chrono::Utc::now().timestamp(),
                                &url,
                            );
                        },
                        Err(e) => log::warn!(
                            "[attester] ignoring upgrade notice from {url}: not a signed \
                             notice ({e}) (#3095)"
                        ),
                    }
                }
            },
            Err(e) => log::debug!("[attester] {url}: unreadable body ({e})"),
        },
        // Not fatal: an older node has no such route, and a transient failure
        // must not stop attestation. What IS already known stays in force.
        Err(e) => log::debug!("[attester] {url}: {e}"),
    }

    // Same cadence the node logs, from the same pure rule.
    let _ = log_run_up(height);

    let status = upgrade_status(height);
    if !status.requires_upgrade() {
        return true;
    }
    let activation = upgrade_notice()
        .map(|signed| signed.notice.activation_height)
        .unwrap_or(u64::MAX);
    if height < activation {
        return true;
    }
    log::error!(
        "[attester] REFUSING TO ATTEST at height {height}: the governance-signed upgrade notice \
         activates at height {activation} and this attester is protocol v{} — an attester on the \
         old rule set must not sign for the new one. Install the announced release and restart \
         (status={status}, #3095).",
        wrfcoin_shared_protocol::crypto::protocol_support::PROTOCOL_VERSION
    );
    false
}

async fn submit_attestation(
    client: &reqwest::Client,
    config: &Config,
    submission: &AttestationSubmission,
) -> Result<()> {
    let response = client
        .post(format!("{}/transactions/oracle/attestation", config.node_url))
        .bearer_auth(&config.api_token)
        .header("Content-Type", "application/json")
        .body(submission.to_json())
        .send()
        .await
        .context("POST /transactions/oracle/attestation failed")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("attestation rejected with {status}: {body}");
    }
    Ok(())
}

/// Derive a replay nonce that is deterministic for one oracle submission but
/// cannot collide with another oracle processing the same commit in the same
/// second. Oracle nonces are globally spent in consensus state, so the signer
/// identity is part of the nonce domain rather than merely part of the signed
/// payload (#2775).
fn attestation_nonce(
    oracle_id: &str,
    commit_id: &str,
    timestamp: i64,
    submitted_index: usize,
) -> String {
    format!("att-{oracle_id}-{commit_id}-{timestamp}-{submitted_index}")
}

/// The freshness-evaluation and attestation timestamp for one cycle.
///
/// Production: the wall clock. Under `cfg(test)` (unit tests and the `#[path]`-including integration
/// tests) a test may pin it per thread with [`pin_evaluation_clock`], so a date-dependent golden does
/// not depend on how long the test takes to run (#3369). Compiled out of the shipped binary.
fn evaluation_now() -> i64 {
    #[cfg(test)]
    if let Some(pinned) = EVALUATION_CLOCK_PIN.with(|c| c.get()) {
        return pinned;
    }
    chrono::Utc::now().timestamp() // determinism-ok: service-side freshness evaluation and attestation timestamp; consensus re-validates on its own clock
}

#[cfg(test)]
thread_local! {
    static EVALUATION_CLOCK_PIN: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
}

/// Pin this thread's evaluation clock (test builds only). Each `#[actix_web::test]` runs on its own
/// thread, so a pin never leaks into another test.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn pin_evaluation_clock(unix_secs: i64) {
    EVALUATION_CLOCK_PIN.with(|c| c.set(Some(unix_secs)));
}

/// One poll cycle. Returns how many attestations were submitted.
///
/// Uses a fresh [`ReferenceResolver`]; the daemon itself calls
/// [`run_cycle_with_resolver`] so its lookup caches outlive a cycle.
// Called by `tests/oracle_attester_runtime_2464.rs`, which includes this file.
#[allow(dead_code)]
pub(crate) async fn run_cycle(
    client: &reqwest::Client,
    config: &Config,
    attester: &Attester,
    policy: &MechanicalPolicy,
    recently_submitted: &mut HashMap<String, Instant>,
) -> Result<usize> {
    let mut resolver = ReferenceResolver::default();
    run_cycle_with_resolver(client, config, attester, policy, recently_submitted, &mut resolver)
        .await
}

/// One poll cycle with a caller-owned (process-lifetime) reference resolver.
pub(crate) async fn run_cycle_with_resolver(
    client: &reqwest::Client,
    config: &Config,
    attester: &Attester,
    policy: &MechanicalPolicy,
    recently_submitted: &mut HashMap<String, Instant>,
    resolver: &mut ReferenceResolver,
) -> Result<usize> {
    let pending = fetch_pending(client, config).await?;

    // #2744-class guard: the domain this process signs must be the domain the
    // node verifies. Checked every cycle — the node can be redeployed under a
    // different identity while this daemon keeps running.
    if pending.chain_id != config.chain_id || pending.network_id != config.network_id {
        bail!(
            "chain identity mismatch: this attester signs for {}/{} but the node at {} \
             verifies {}/{}. Refusing to attest — these signatures could never authorize, \
             and the failure would otherwise surface as opaque rejections (#2744).",
            config.chain_id,
            config.network_id,
            config.node_url,
            pending.chain_id,
            pending.network_id
        );
    }

    // #3095: before anything is signed, ask whether this binary may still
    // attest at this height at all.
    if !upgrade_gate(client, config, pending.height).await {
        return Ok(0);
    }

    let oracle_id = attester.oracle_id().to_string();
    if !pending.oracle_set.iter().any(|id| id == &oracle_id) {
        log::warn!(
            "[attester] oracle id {oracle_id} is NOT in the node's oracle set \
             ({} members) — attestations will be rejected at admission until an \
             OracleRegistration for this identity lands",
            pending.oracle_set.len()
        );
    }
    if !pending.unrecoverable.is_empty() {
        log::warn!(
            "[attester] {} commit(s) have unrecoverable reveal preimages (evicted \
             blocks): {:?}",
            pending.unrecoverable.len(),
            pending.unrecoverable
        );
    }

    recently_submitted.retain(|_, at| at.elapsed() < RESUBMIT_SUPPRESSION);

    // #3131/#3285: the reward-tier and pressure-pairing rules follow the
    // chain's feature schedule as the node reports it for the next block,
    // never this process's config.
    let cycle_policy = cycle_policy_override(policy, &pending.active_features);
    let policy = cycle_policy.as_ref().unwrap_or(policy);
    let guard_active = policy.gross_outlier_guard;
    let sea_level_active = policy.sea_level_pressure;

    // Reference observations for this cycle. One fetch covers every pending
    // observation; an empty vec means every evaluation below runs
    // PHYSICS-ONLY (visibly capped by the mechanical policy, never silent).
    // `per_station` (#3240) does not use this set: it fetches per selected
    // station, lazily, through `CycleReferences`.
    let fixed_reference_needed = config.reference_mode != ReferenceMode::PerStation;
    let reference_observations: Vec<StationReferenceObservation> =
        match config.reference_station.as_ref().filter(|_| fixed_reference_needed) {
            Some(station) => {
                match fetch_reference_observations(client, config, station, None).await {
                    Ok(observations) if observations.is_empty() => {
                        log::warn!(
                            "[attester] REFERENCE UNAVAILABLE: NWS station {station} returned \
                             zero observations — this cycle attests PHYSICS-ONLY with capped \
                             confidence"
                        );
                        Vec::new()
                    },
                    Ok(observations) => observations,
                    Err(err) => {
                        log::warn!(
                            "[attester] REFERENCE UNAVAILABLE: fetch from NWS station \
                             {station} failed ({err:#}) — this cycle attests PHYSICS-ONLY \
                             with capped confidence"
                        );
                        Vec::new()
                    },
                }
            },
            None if !fixed_reference_needed => Vec::new(),
            None => {
                if !pending.observations.is_empty() {
                    log::warn!(
                        "[attester] PHYSICS-ONLY MODE: WRFCOIN_ATTESTER_NWS_STATION is not \
                         set — no local-sensor reference comparison; every attestation's \
                         confidence is capped"
                    );
                }
                Vec::new()
            },
        };

    resolver.begin_cycle();
    let mut cycle_references = CycleReferences::default();
    let mut shadow_would_change = 0usize;

    let now = evaluation_now();
    let mut submitted = 0usize;
    // #2833: signing refusals and submission failures were per-commit ERROR
    // lines only — invisible in the cycle summary that operators (and
    // watchdogs) actually read. A dead signer now shows up on EVERY line.
    let mut refused = 0usize;
    let mut submit_failed = 0usize;

    for entry in &pending.observations {
        if entry.has_quorum
            || entry.attesting_oracles.contains(&oracle_id)
            || recently_submitted.contains_key(&entry.commit_id)
        {
            continue;
        }

        // #3240: the reference observations are chosen per `reference_mode`;
        // the evaluation itself (incl. the #3285 sea-level pairing) is the
        // shared pure `evaluate_entry` in every mode.
        let evaluate = |references: &[StationReferenceObservation]| {
            evaluate_entry(entry, policy, references, config.reference_max_pair_secs, now)
        };
        let fixed_label = || {
            config.reference_station.clone().unwrap_or_else(|| "none".to_string())
        };
        let (EntryEvaluation { observation, comparison, verdict, sea_level }, reference_label) =
            match config.reference_mode {
                ReferenceMode::Fixed => (evaluate(&reference_observations), None),
                ReferenceMode::PerStation => {
                    let selection = select_reference(
                        client,
                        config,
                        resolver,
                        &mut cycle_references,
                        entry.station_location,
                    )
                    .await;
                    let mut evaluation = evaluate(&selection.observations);
                    if !selection.notes.is_empty() {
                        log::warn!(
                            "[attester] reference fallback for commit {} (station {}): {}",
                            entry.commit_id,
                            entry.station_id,
                            selection.notes.join("; ")
                        );
                    }
                    evaluation.verdict.reasons.extend(selection.notes.iter().cloned());
                    (evaluation, Some(selection.label()))
                },
                ReferenceMode::Shadow => {
                    // Submit exactly what `Fixed` produces; the per-station
                    // verdict is computed and logged, never signed.
                    let fixed = evaluate(&reference_observations);
                    // Seed the configured station's fetch so the shadow path
                    // does not fetch it twice this cycle.
                    if let Some(station) = &config.reference_station {
                        if !reference_observations.is_empty() {
                            cycle_references
                                .observations
                                .entry(station.clone())
                                .or_insert_with(|| Ok(reference_observations.clone()));
                        }
                    }
                    let selection = select_reference(
                        client,
                        config,
                        resolver,
                        &mut cycle_references,
                        entry.station_location,
                    )
                    .await;
                    let shadow = evaluate(&selection.observations);
                    // determinism-ok: service-side log comparison of two attester-local verdicts
                    let would_change = shadow.verdict.approved != fixed.verdict.approved
                        || shadow.verdict.confidence.to_bits()
                            != fixed.verdict.confidence.to_bits();
                    if would_change {
                        shadow_would_change += 1;
                    }
                    log::info!(
                        "[attester] SHADOW reference: commit_id={} station_id={} fixed_ref={} \
                         fixed_tier={} fixed_confidence={:.3} resolved_ref={} distance_km={} \
                         per_station_tier={} per_station_confidence={:.3} would_change={} \
                         notes={:?}",
                        entry.commit_id,
                        entry.station_id,
                        fixed_label(),
                        fixed.tier_label(),
                        fixed.verdict.confidence,
                        selection.station.as_deref().unwrap_or("none"),
                        selection
                            .distance_km
                            .map(|km| format!("{km:.1}"))
                            .unwrap_or_else(|| "n/a".to_string()),
                        shadow.tier_label(),
                        shadow.verdict.confidence,
                        would_change,
                        selection.notes
                    );
                    (fixed, None)
                },
            };

        // Sign. `attest` re-verifies the commit id against the reveal
        // preimage before anything is signed, and self-verifies after.
        let nonce = attestation_nonce(&oracle_id, &entry.commit_id, now, submitted);
        let payload = match attester.attest(
            &observation,
            verdict.approved,
            Some(verdict.confidence),
            now,
            &nonce,
        ) {
            Ok(payload) => payload,
            Err(err) => {
                refused += 1;
                log::error!(
                    "[attester] refusing commit {} ({}): {err}",
                    entry.commit_id,
                    entry.source
                );
                continue;
            },
        };

        // Pre-submit wire check: the node verifies the signature against the
        // values it PARSES out of the JSON body, and serde_json's default
        // float parse is not correctly rounded — a confidence that shifts a
        // ulp in transit turns a valid signature into a rejection. The policy
        // emits wire-stable values, and this proves it per submission rather
        // than trusting it.
        let submission = AttestationSubmission::from(&payload);
        let survives = serde_json::from_str::<
            wrfcoin_shared_protocol::blockchain::transaction::OracleAttestationPayload,
        >(&submission.to_json())
        .map(|parsed| parsed.has_valid_signature())
        .unwrap_or(false);
        if !survives {
            refused += 1;
            log::error!(
                "[attester] NOT submitting commit {}: the attestation does not survive \
                 the JSON wire (float parse drift) — the node would reject a valid \
                 signature. confidence={:?}",
                entry.commit_id,
                payload.confidence
            );
            continue;
        }

        match submit_attestation(client, config, &submission).await {
            Ok(()) => {
                submitted += 1;
                // determinism-ok: process-local resubmit-suppression cache; chain state (attesting_oracles) is the deduplicator of record
                recently_submitted.insert(entry.commit_id.clone(), Instant::now());
                log::info!(
                    "[attester] attested commit {} ({}): approved={} confidence={:.3} \
                     tier={} reference={} sea_level_pressure={:?} reasons={:?}",
                    entry.commit_id,
                    entry.source,
                    verdict.approved,
                    verdict.confidence,
                    verdict.tier,
                    match (&reference_label, comparison.is_some()) {
                        (None, true) => "checked".to_string(),
                        (Some(label), true) => format!("checked:{label}"),
                        (_, false) => "PHYSICS-ONLY".to_string(),
                    },
                    sea_level,
                    verdict.reasons
                );
            },
            Err(err) => {
                submit_failed += 1;
                log::error!(
                    "[attester] submission failed for commit {}: {err}",
                    entry.commit_id
                );
            },
        }
    }

    if config.reference_mode == ReferenceMode::Shadow {
        log::info!(
            "[attester] SHADOW summary: would_change={shadow_would_change} \
             distinct_reference_stations={} nws_lookups_this_cycle={}",
            cycle_references.observations.len(),
            cycle_references.lookups
        );
    }
    if refused > 0 || submit_failed > 0 {
        log::error!(
            "[attester] cycle done: height={} pending={} ready_to_mine={} submitted={}              refused={} submit_failed={} quorum={} — the signer or node path is              failing; quorum may be starving (#2833)",
            pending.height,
            pending.observations.len(),
            pending.ready_to_mine.len(),
            submitted,
            refused,
            submit_failed,
            pending.effective_quorum
        );
    } else {
        log::info!(
            "[attester] cycle done: height={} pending={} ready_to_mine={} submitted={} quorum={} \
             gross_outlier_guard={} sea_level_pressure={}",
            pending.height,
            pending.observations.len(),
            pending.ready_to_mine.len(),
            submitted,
            pending.effective_quorum,
            guard_active,
            sea_level_active
        );
    }
    Ok(submitted)
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let config = Config::from_env()?;
    let policy = mechanical_policy_from_env()?;
    let attester = attester_from_env()?;
    log::info!(
        "[attester] oracle id {} signing for {}/{}, node {}, poll {:?}",
        attester.oracle_id(),
        config.chain_id,
        config.network_id,
        config.node_url,
        config.poll_interval
    );

    match &config.reference_station {
        Some(station) => log::info!(
            "[attester] reference source: NWS station {station} via {} (pair window {}s)",
            config.reference_base_url,
            config.reference_max_pair_secs
        ),
        None => log::warn!(
            "[attester] PHYSICS-ONLY MODE: WRFCOIN_ATTESTER_NWS_STATION is not set. \
             No attestation from this process will exceed the unchecked confidence \
             cap. Set the station (e.g. KDLH for the Duluth deployments) to enable \
             local-sensor reference comparison."
        ),
    }

    if config.reference_mode != ReferenceMode::Fixed {
        log::warn!(
            "[attester] reference mode {}: per-commit nearest NWS station within {} km \
             (lookup timeout {:?}, at most {} stations per cycle); fallback = configured \
             station {:?}, then PHYSICS-ONLY (#3240){}",
            config.reference_mode.as_str(),
            config.max_reference_km.unwrap_or_default(),
            config.reference_lookup_timeout,
            config.max_reference_stations_per_cycle,
            config.reference_station,
            if config.reference_mode == ReferenceMode::Shadow {
                " — SHADOW: the submitted verdict is still the fixed one"
            } else {
                ""
            }
        );
    }

    let client = build_attester_http_client()?;
    let mut recently_submitted: HashMap<String, Instant> = HashMap::new();
    let mut resolver = ReferenceResolver::default();

    loop {
        let cycle = run_cycle_with_resolver(
            &client,
            &config,
            &attester,
            &policy,
            &mut recently_submitted,
            &mut resolver,
        )
        .await;
        if let Err(err) = &cycle {
            // A chain-identity mismatch is fatal by design; transient HTTP
            // errors are retried on the next tick.
            let message = format!("{err:#}");
            if message.contains("chain identity mismatch") {
                return cycle.map(|_| ());
            }
            log::error!("[attester] cycle failed: {message}");
        }
        if config.once {
            // Smoke-test mode: one cycle, and its outcome IS the exit status.
            return cycle.map(|_| ());
        }
        tokio::time::sleep(config.poll_interval).await;
    }
}

/// How long an idle pooled connection may be retained before this client drops
/// it (#2784).
///
/// hyper's default is **90 seconds**, and leaving it there is the client half of
/// the #2782 reuse race: a client that retains a connection longer than the
/// server holds it open will eventually write a request onto a socket the server
/// has already closed, and hyper reports that as
/// `connection closed before message completed`.
///
/// The value is bounded from both sides:
///
/// - **Upper bound — shorter than the shortest keep-alive we may meet.**
///   actix-web's default, which is what a node without #2783 serves, is 5s. The
///   measured inter-cycle gap is 5.05–5.18s (`WRFCOIN_ATTESTER_POLL_SECS=5` plus
///   processing), so every between-cycle reuse would otherwise land just past
///   that boundary. 2s evicts deterministically well before it.
/// - **Lower bound — longer than the intra-cycle request spacing.** One cycle
///   issues `GET /transactions/oracle/pending` and then its
///   `POST /transactions/oracle/attestation` calls milliseconds apart. Those
///   still share a pooled connection, which is where reuse actually pays.
///
/// So this is not "pooling off": it is pooling scoped to a single cycle. The
/// only reuse it gives up is the between-cycle reuse that cannot be made safe
/// against a 5s server anyway — there is no value that both survives a 5.05s gap
/// and stays under a 5s keep-alive.
///
/// It also covers the leg no server-side fix can reach: the same client fetches
/// reference observations from `api.weather.gov`, whose keep-alive is not ours
/// to configure. Those connections idle far longer than 2s between attestations,
/// so nothing is lost there either.
///
/// Complementary to #2783, not a replacement: that one stops our node closing
/// first for every client; this one stops this client reusing stale connections
/// against any server.
const ATTESTER_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(2);

/// Build the daemon's one and only HTTP client.
///
/// Extracted so the pool settings are testable on the production path rather
/// than re-declared in a test. `main` must call this and nothing else — see the
/// single-construction guard in the tests below.
fn build_attester_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        // Bound idle reuse below the keep-alives we actually know about: a
        // node without #2783 serves actix-web's 5s default, and the measured
        // inter-cycle gap is 5.05-5.18s (#2784). This does not establish
        // safety against a server whose keep-alive is unknown and shorter.
        .pool_idle_timeout(ATTESTER_POOL_IDLE_TIMEOUT)
        // api.weather.gov rejects requests without a User-Agent; same
        // identification convention as the collector's nwsFetch.
        .user_agent("wrfcoin-oracle-attester/1.0 (weather@wrfcoin.org)")
        .build()
        .context("failed to build HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

    /// #2833: entries with quorum land in `ready_to_mine`, which this binary
    /// previously did not even deserialize — a fully-attested backlog read as
    /// `pending=0` and looked idle during the 2026-08-28 quorum outage. The
    /// field must parse when present and default empty when absent (older
    /// nodes).
    #[test]
    fn pending_response_surfaces_ready_to_mine_and_defaults_empty() {
        let with_ready: PendingResponse = serde_json::from_str(
            r#"{
                "chain_id": "777", "network_id": "testnet", "height": 33500,
                "oracle_set": ["a", "b"], "oracle_quorum": 2, "effective_quorum": 2,
                "observations": [],
                "ready_to_mine": [{"commit_id": "c1"}, {"commit_id": "c2"}],
                "unrecoverable": []
            }"#,
        )
        .expect("ready_to_mine must deserialize");
        assert_eq!(with_ready.ready_to_mine.len(), 2);
        assert!(with_ready.observations.is_empty());

        let without: PendingResponse = serde_json::from_str(
            r#"{
                "chain_id": "777", "network_id": "testnet", "height": 1,
                "oracle_set": [], "oracle_quorum": 1, "effective_quorum": 1,
                "observations": []
            }"#,
        )
        .expect("absent ready_to_mine must default");
        assert!(without.ready_to_mine.is_empty());
        assert!(without.unrecoverable.is_empty());
        // #3131: an older node omits active_features → pre-activation rule.
        assert!(without.active_features.is_empty());
        assert!(!gross_outlier_guard_active(&without.active_features));
    }

    /// #3131: the guard follows the node's report of the feature schedule.
    #[test]
    fn gross_outlier_guard_follows_the_nodes_active_features() {
        let active: PendingResponse = serde_json::from_str(
            r#"{
                "chain_id": "777", "network_id": "testnet", "height": 20000,
                "oracle_set": [], "oracle_quorum": 1, "effective_quorum": 1,
                "observations": [],
                "active_features": ["signing_v2", "reward_gross_outlier_guard"]
            }"#,
        )
        .expect("active_features must deserialize");
        assert!(gross_outlier_guard_active(&active.active_features));

        let other_features: Vec<String> = vec!["signing_v2".into(), "paymaster".into()];
        assert!(!gross_outlier_guard_active(&other_features));
        assert!(!gross_outlier_guard_active(&[]));
    }

    /// #3285: the sea-level pressure pairing follows the node's report of
    /// the feature schedule, independently of the #3131 guard.
    #[test]
    fn sea_level_pressure_follows_the_nodes_active_features() {
        let features = |names: &[&str]| -> Vec<String> {
            names.iter().map(|n| n.to_string()).collect()
        };
        assert!(!sea_level_pressure_active(&[]));
        assert!(!sea_level_pressure_active(&features(&["reward_gross_outlier_guard"])));
        assert!(sea_level_pressure_active(&features(&["reference_sea_level_pressure"])));

        let base = MechanicalPolicy::default();
        assert!(cycle_policy_override(&base, &[]).is_none(), "no features: no clone");
        let guard_only =
            cycle_policy_override(&base, &features(&["reward_gross_outlier_guard"])).unwrap();
        assert!(guard_only.gross_outlier_guard && !guard_only.sea_level_pressure);
        let both = cycle_policy_override(
            &base,
            &features(&["reward_gross_outlier_guard", "reference_sea_level_pressure"]),
        )
        .unwrap();
        assert!(both.gross_outlier_guard && both.sea_level_pressure);
        // A process-configured bit never survives a node that reports it off.
        let misconfigured = MechanicalPolicy { sea_level_pressure: true, ..base };
        assert!(!cycle_policy_override(&misconfigured, &[]).unwrap().sea_level_pressure);
    }

    /// the reference station’s replayed pair (#3285, 2026-09-24): WS-5000 raw station pressure
    /// 1001.2 hPa at the registered 227.7 m, submitted with its altimeter
    /// setting as `pressure_sea_level_hpa`, against KDLH's 1027.8 hPa. The
    /// other variables agree with KDLH.
    const JON_PENDING: &str = r#"{
        "chain_id": "777", "network_id": "testnet", "height": 21400,
        "oracle_set": [], "oracle_quorum": 2, "effective_quorum": 2,
        "observations": [{
            "commit_id": "c0ffee", "station_id": "PWS-EXAMPLE-WS5000",
            "timestamp": 1790224800, "nonce": "n", "contribution_type": "contribution",
            "source": "mempool", "attesting_oracles": [], "has_quorum": false,
            "elevation_meters": 227.7,
            "data": {
                "temperature_c": 10.2, "humidity_percent": 97.0,
                "pressure_hpa": 1001.2, "pressure_sea_level_hpa": 1028.29,
                "pressure_kind": "station",
                "wind_speed_ms": 1.0, "precipitation_mm": 0.0
            }
        }],
        "active_features": ACTIVE
    }"#;

    fn jon_verdict(active_features: &str) -> (MechanicalVerdict, SeaLevelPressureDecision) {
        use wrfcoin_shared_protocol::weather::station_reference::StationReferenceObservation;
        let pending: PendingResponse =
            serde_json::from_str(&JON_PENDING.replace("ACTIVE", active_features))
                .expect("the replay fixture must deserialize");
        let entry = &pending.observations[0];
        let kdlh = vec![StationReferenceObservation {
            timestamp: entry.timestamp - 300,
            temperature_c: Some(10.0),
            pressure_hpa: Some(1027.8),
            humidity_pct: Some(93.0),
            wind_speed_ms: Some(2.1),
        }];
        let base = MechanicalPolicy::default();
        let cycle = cycle_policy_override(&base, &pending.active_features);
        let policy = cycle.as_ref().unwrap_or(&base);
        let eval = evaluate_entry(
            entry,
            policy,
            &kdlh,
            DEFAULT_MAX_PAIR_DELTA_SECS,
            entry.timestamp + 60,
        );
        assert!(eval.comparison.is_some(), "KDLH is inside the pair window");
        assert!(eval.verdict.approved);
        (eval.verdict, eval.sea_level)
    }

    /// Inactive (live 777 today, with or without the #3131 guard): pressure
    /// is the lone Outlier (−26.6 hPa) and the #3098 fold tiers the reading
    /// Degraded. Active: the verified sea-level value pairs (+0.5 hPa) and
    /// the reading tiers Good.
    #[test]
    fn replayed_pair_is_degraded_before_and_good_after_activation_3285() {
        use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;

        for inactive in [r#"[]"#, r#"["reward_gross_outlier_guard"]"#] {
            let (verdict, outcome) = jon_verdict(inactive);
            assert_eq!(verdict.tier, ReadingQualityTier::Degraded, "{inactive}");
            assert_eq!(outcome.outcome, SeaLevelPressureOutcome::Inactive);
        }

        let (verdict, outcome) =
            jon_verdict(r#"["reward_gross_outlier_guard","reference_sea_level_pressure"]"#);
        assert_eq!(verdict.tier, ReadingQualityTier::Good);
        assert!(matches!(outcome.outcome, SeaLevelPressureOutcome::Verified { .. }), "{outcome:?}");
        assert!(verdict.reasons.iter().all(|r| !r.contains("sea-level")));
    }

    fn jon_eval_with(data_edit: impl Fn(String) -> String, active: &str) -> EntryEvaluation {
        let pending: PendingResponse =
            serde_json::from_str(&data_edit(JON_PENDING.to_string()).replace("ACTIVE", active))
                .unwrap();
        let entry = &pending.observations[0];
        let kdlh = vec![StationReferenceObservation {
            timestamp: entry.timestamp,
            temperature_c: Some(10.0),
            pressure_hpa: Some(1027.8),
            humidity_pct: Some(93.0),
            wind_speed_ms: Some(2.1),
        }];
        let base = MechanicalPolicy::default();
        let cycle = cycle_policy_override(&base, &pending.active_features);
        let policy = cycle.as_ref().unwrap_or(&base);
        evaluate_entry(entry, policy, &kdlh, DEFAULT_MAX_PAIR_DELTA_SECS, entry.timestamp)
    }

    fn has_pressure_residual(eval: &EntryEvaluation) -> bool {
        eval.comparison
            .as_ref()
            .is_some_and(|c| c.residuals.iter().any(|r| r.variable == "pressure_hpa"))
    }

    /// Owner rulings (#3285): active + registered elevation + a reveal with
    /// NO sea-level claim ⇒ no raw pressure residual, and pressure is scored
    /// as an unverified Outlier: the reference station’s healthy reading is Degraded
    /// ([G,Out,G,G]) — never better than an honest verified claim (Good).
    /// Inactive, the same reveal is today's raw pairing: pressure Outlier →
    /// Degraded, and the verdict is exactly plain evaluate().
    #[test]
    fn active_with_elevation_and_no_claim_scores_pressure_unverified_3285() {
        use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;
        let strip = |s: String| {
            s.replace(r#""pressure_sea_level_hpa": 1028.29,"#, "")
                .replace(r#""pressure_kind": "station","#, "")
        };

        let active = jon_eval_with(strip, r#"["reference_sea_level_pressure"]"#);
        assert_eq!(active.sea_level.outcome, SeaLevelPressureOutcome::NotClaimed);
        assert_eq!(active.sea_level.pairing, PressurePairing::Unverified);
        assert!(!has_pressure_residual(&active));
        assert_eq!(active.verdict.tier, ReadingQualityTier::Degraded);
        assert!(active.verdict.reasons.iter().any(|r| r.contains("scored Outlier as unverified")));
        assert!(active.verdict.reasons.iter().any(|r| r.contains("unverified variable(s)")));

        let verified = jon_eval_with(|s| s, r#"["reference_sea_level_pressure"]"#);
        assert_eq!(verified.sea_level.pairing, PressurePairing::SeaLevel);
        assert_eq!(verified.verdict.tier, ReadingQualityTier::Good);
        assert!(verified.verdict.confidence > active.verdict.confidence);

        let inactive = jon_eval_with(strip, "[]");
        assert_eq!(inactive.sea_level.pairing, PressurePairing::Raw);
        assert!(has_pressure_residual(&inactive));
        assert_eq!(inactive.verdict.tier, ReadingQualityTier::Degraded);
        assert!(inactive.verdict.reasons.iter().all(|r| !r.contains("unverified")));
        assert_eq!(
            inactive.verdict,
            MechanicalPolicy::default().evaluate(
                &inactive.observation,
                inactive.comparison.as_ref(),
                inactive.observation.timestamp
            )
        );
    }

    /// Active, registered elevation, a claim with no `pressure_kind`: the
    /// attester cannot know whether reducing is valid, so (A1) pressure is
    /// skipped, and the verdict says why.
    #[test]
    fn a_claim_without_pressure_kind_scores_pressure_unverified_3285() {
        use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;
        let eval = jon_eval_with(
            |s| s.replace(r#""pressure_kind": "station","#, ""),
            r#"["reference_sea_level_pressure"]"#,
        );
        assert!(eval.observation.data.get("pressure_kind").is_none());
        assert_eq!(eval.sea_level.outcome, SeaLevelPressureOutcome::NoPressureKind);
        assert_eq!(eval.sea_level.pairing, PressurePairing::Unverified);
        assert!(!has_pressure_residual(&eval));
        assert_eq!(eval.verdict.tier, ReadingQualityTier::Degraded);
        assert!(eval.verdict.reasons.iter().any(|r| r.contains("pressure_kind")));
    }

    /// A deliberately wrong claim (+50 hPa over the correct 1028.29) through
    /// the real attester path: Mismatch → unverified Outlier → Degraded, the
    /// same verdict tier as sending no claim; the verified claim is Good.
    #[test]
    fn a_deliberately_wrong_claim_is_unverified_outlier_in_the_attester_3285() {
        use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;
        let eval = jon_eval_with(
            |s| s.replace(r#""pressure_sea_level_hpa": 1028.29,"#, r#""pressure_sea_level_hpa": 1078.29,"#),
            r#"["reference_sea_level_pressure"]"#,
        );
        assert!(matches!(eval.sea_level.outcome, SeaLevelPressureOutcome::Mismatch { .. }));
        assert_eq!(eval.sea_level.pairing, PressurePairing::Unverified);
        assert_eq!(eval.verdict.tier, ReadingQualityTier::Degraded);
        assert!(eval.verdict.reasons.iter().any(|r| r.contains("mismatch")));
    }

    /// review-trio Delta review 4 F1: guards that fail if the unverified
    /// scoring leaks beyond `PressurePairing::Unverified`. Each uses a
    /// reading whose pressure agrees with KDLH (1027.5 vs 1027.8), so the
    /// unleaked verdict is GOOD and any stray extra Outlier shows up as
    /// Degraded. The base reveal has no sea-level claim; `edit` adds one.
    fn good_reading_eval(
        edit: impl Fn(String) -> String,
        active: &str,
    ) -> EntryEvaluation {
        jon_eval_with(
            |s| {
                edit(
                    s.replace(
                        r#""pressure_hpa": 1001.2, "pressure_sea_level_hpa": 1028.29,"#,
                        r#""pressure_hpa": 1027.5,"#,
                    )
                    .replace(r#""pressure_kind": "station","#, ""),
                )
            },
            active,
        )
    }

    fn no_unverified_reason(eval: &EntryEvaluation) -> bool {
        eval.verdict.reasons.iter().all(|r| !r.contains("unverified"))
    }

    /// F1 (a): inactive equality on a reading that tiers GOOD. The verdict is
    /// exactly plain `evaluate()`, Good, with no unverified scoring — with or
    /// without a registered elevation, claim or no claim.
    #[test]
    fn f1a_inactive_on_a_good_reading_is_exactly_evaluate_3285() {
        use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;
        for edit in [
            (|s: String| s) as fn(String) -> String,
            |s: String| s.replace(r#""elevation_meters": 227.7,"#, ""),
            |s: String| {
                s.replace(
                    r#""pressure_hpa": 1027.5,"#,
                    r#""pressure_hpa": 1027.5, "pressure_sea_level_hpa": 1027.5, "pressure_kind": "relative","#,
                )
            },
        ] {
            let eval = good_reading_eval(edit, "[]");
            assert_eq!(eval.sea_level.pairing, PressurePairing::Raw);
            assert_eq!(eval.verdict.tier, ReadingQualityTier::Good, "{:?}", eval.verdict);
            assert!(no_unverified_reason(&eval));
            assert_eq!(
                eval.verdict,
                MechanicalPolicy::default().evaluate(
                    &eval.observation,
                    eval.comparison.as_ref(),
                    eval.observation.timestamp
                )
            );
        }
    }

    /// F1 (b): active + a consistent `relative` claim (Steven's shape) with a
    /// registered elevation ⇒ Raw pairing, Good, no unverified scoring and no
    /// audit reason.
    #[test]
    fn f1b_active_relative_claim_is_raw_good_with_no_unverified_scoring_3285() {
        use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;
        let eval = good_reading_eval(
            |s| {
                s.replace(
                    r#""pressure_hpa": 1027.5,"#,
                    r#""pressure_hpa": 1027.5, "pressure_sea_level_hpa": 1027.5, "pressure_kind": "relative","#,
                )
            },
            r#"["reference_sea_level_pressure"]"#,
        );
        assert_eq!(eval.sea_level.outcome, SeaLevelPressureOutcome::RelativeRaw);
        assert_eq!(eval.sea_level.pairing, PressurePairing::Raw);
        assert_eq!(eval.sea_level.audit_reason(), None);
        assert!(has_pressure_residual(&eval));
        assert_eq!(eval.verdict.tier, ReadingQualityTier::Good, "{:?}", eval.verdict);
        assert!(no_unverified_reason(&eval));
    }

    /// F1 (c): active + a station with NO registered elevation (and no claim)
    /// ⇒ today's raw pairing, Good, no unverified scoring.
    #[test]
    fn f1c_active_without_elevation_is_raw_good_with_no_unverified_scoring_3285() {
        use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;
        let eval = good_reading_eval(
            |s| s.replace(r#""elevation_meters": 227.7,"#, ""),
            r#"["reference_sea_level_pressure"]"#,
        );
        assert_eq!(eval.sea_level.outcome, SeaLevelPressureOutcome::NotClaimed);
        assert_eq!(eval.sea_level.pairing, PressurePairing::Raw);
        assert!(has_pressure_residual(&eval));
        assert_eq!(eval.verdict.tier, ReadingQualityTier::Good, "{:?}", eval.verdict);
        assert!(no_unverified_reason(&eval));
    }

    /// Active, but the node reports no registered elevation (older node, or
    /// a station registered without one): the claim is not trusted, pressure
    /// pairs raw, and the verdict says why.
    #[test]
    fn an_unregistered_elevation_keeps_the_raw_pairing_after_activation_3285() {
        use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;

        let pending: PendingResponse = serde_json::from_str(
            &JON_PENDING
                .replace(r#""elevation_meters": 227.7,"#, "")
                .replace("ACTIVE", r#"["reference_sea_level_pressure"]"#),
        )
        .unwrap();
        let entry = &pending.observations[0];
        assert_eq!(entry.elevation_meters, None);
        let kdlh = vec![StationReferenceObservation {
            timestamp: entry.timestamp,
            temperature_c: Some(10.0),
            pressure_hpa: Some(1027.8),
            humidity_pct: Some(93.0),
            wind_speed_ms: Some(2.1),
        }];
        let policy = cycle_policy_override(&MechanicalPolicy::default(), &pending.active_features)
            .unwrap();
        let eval =
            evaluate_entry(entry, &policy, &kdlh, DEFAULT_MAX_PAIR_DELTA_SECS, entry.timestamp);
        assert_eq!(eval.sea_level.outcome, SeaLevelPressureOutcome::NoElevation);
        assert_eq!(eval.verdict.tier, ReadingQualityTier::Degraded);
        assert!(eval.verdict.reasons.iter().any(|r| r.contains("no registered elevation")));
    }

    fn fixture_public_key(seed_byte: u8) -> PublicKey {
        // secret-leak-ok: deterministic test-only fixture, never production custody material
        let private = PrivateKey::from_bytes(&[seed_byte; 32]).expect("fixture key seed");
        PublicKey::from_private_key(&private)
    }

    /// A trimmed but shape-faithful api.weather.gov observations response:
    /// GeoJSON features, `properties.timestamp` RFC3339, temperature in °C,
    /// barometricPressure in Pa, windSpeed in km/h, and the null-value /
    /// missing-timestamp cases NWS actually serves.
    const NWS_FIXTURE: &str = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "properties": {
                    "timestamp": "2026-08-06T14:54:00+00:00",
                    "temperature": { "unitCode": "wmoUnit:degC", "value": 21.7 },
                    "barometricPressure": { "unitCode": "wmoUnit:Pa", "value": 101320 },
                    "relativeHumidity": { "unitCode": "wmoUnit:percent", "value": 62.5 },
                    "windSpeed": { "unitCode": "wmoUnit:km_h-1", "value": 14.4 }
                }
            },
            {
                "properties": {
                    "timestamp": "2026-08-06T13:54:00+00:00",
                    "temperature": { "unitCode": "wmoUnit:degC", "value": null },
                    "barometricPressure": { "unitCode": "wmoUnit:Pa", "value": null },
                    "relativeHumidity": { "unitCode": "wmoUnit:percent", "value": 60.0 },
                    "windSpeed": { "unitCode": "wmoUnit:km_h-1", "value": null }
                }
            },
            {
                "properties": {
                    "timestamp": null,
                    "temperature": { "value": 5.0 }
                }
            }
        ]
    }"#;

    #[test]
    fn forbidden_role_list_inventories_every_public_key_2766() {
        let first = fixture_public_key(1);
        let second = fixture_public_key(2);
        let inventory = add_forbidden_role_keys(
            ForbiddenRoleKeys::default(),
            "snapshot",
            "WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY",
            &format!(" {}, {} ", first.to_hex(), second.to_hex()),
        )
        .expect("both live snapshot public keys should parse");

        assert_eq!(inventory.conflict(&first), Some("snapshot"));
        assert_eq!(
            inventory.conflict(&second),
            Some("snapshot"),
            "a collision with the second key in the role list must not pass"
        );
    }

    #[test]
    fn forbidden_role_list_rejects_blank_or_invalid_entries_without_echoing_them_2766() {
        let first_hex = fixture_public_key(1).to_hex();
        let second_hex = fixture_public_key(2).to_hex();
        let blank = add_forbidden_role_keys(
            ForbiddenRoleKeys::default(),
            "snapshot",
            "WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY",
            &format!("{first_hex},,{second_hex}"),
        )
        .expect_err("an empty list element must fail closed")
        .to_string();
        assert!(blank.contains("WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY entry 2"));
        assert!(blank.contains("blank"));

        let invalid_value = "accidentally-wired-secret";
        let invalid = add_forbidden_role_keys(
            ForbiddenRoleKeys::default(),
            "snapshot",
            "WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY",
            &format!("{first_hex},{invalid_value}"),
        )
        .expect_err("invalid public-key material must fail closed")
        .to_string();
        assert!(invalid.contains("WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY entry 2"));
        assert!(!invalid.contains(invalid_value));
    }

    #[test]
    fn attestation_nonce_is_scoped_to_oracle_identity_2775() {
        let first_oracle = fixture_public_key(1).to_hex();
        let second_oracle = fixture_public_key(2).to_hex();
        let commit_id = "888c527356c0838a59a93e0cadc558a6676828be7e992c4b85cb5bd18ba9ed71";
        let timestamp = 1_786_499_218;

        let first = attestation_nonce(&first_oracle, commit_id, timestamp, 0);
        let first_retry = attestation_nonce(&first_oracle, commit_id, timestamp, 0);
        let second = attestation_nonce(&second_oracle, commit_id, timestamp, 0);

        assert_eq!(
            first, first_retry,
            "one oracle's nonce must be deterministic"
        );
        assert_ne!(
            first, second,
            "distinct oracles voting on one commit in the same second must not collide"
        );
        assert!(first.contains(&first_oracle));
        assert!(second.contains(&second_oracle));
        assert!(first.contains(commit_id));
        assert!(second.contains(commit_id));
        assert!(first.len() <= 256 && second.len() <= 256);
    }

    #[test]
    fn nws_geojson_parses_with_the_collectors_unit_conversions() {
        let observations = parse_nws_observations(NWS_FIXTURE).expect("fixture parses");
        // The timestampless feature is dropped, like the collector's filter.
        assert_eq!(observations.len(), 2);

        let first = &observations[0];
        assert_eq!(first.timestamp, 1786028040); // 2026-08-06T14:54:00Z (`date -u +%s`)
        assert_eq!(first.temperature_c, Some(21.7));
        assert_eq!(first.pressure_hpa, Some(1013.2)); // Pa / 100
        assert_eq!(first.humidity_pct, Some(62.5));
        assert_eq!(first.wind_speed_ms, Some(4.0)); // km/h / 3.6

        // Null measurement values stay None rather than becoming zeros a
        // pairing would read as huge residuals.
        let second = &observations[1];
        assert_eq!(second.temperature_c, None);
        assert_eq!(second.pressure_hpa, None);
        assert_eq!(second.humidity_pct, Some(60.0));
        assert_eq!(second.wind_speed_ms, None);
    }

    #[test]
    fn a_non_geojson_body_is_an_error_not_an_empty_success() {
        // An HTML error page or a JSON error object must surface as a fetch
        // failure (→ loud PHYSICS-ONLY cycle), never parse as zero
        // observations that quietly look like "no data".
        assert!(parse_nws_observations("<html>rate limited</html>").is_err());
    }

    #[test]
    fn an_empty_feature_list_parses_as_zero_observations() {
        let observations =
            parse_nws_observations(r#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        assert!(observations.is_empty());
    }

    // ---- connection-pool idle timeout (#2784) ------------------------------
    //
    // The pair mirrors #2783's shape: one test proves the BEHAVIOUR of the
    // production client, the other proves the daemon has no second client that
    // silently skips it. Either alone is passable while the defect is live.

    /// A minimal HTTP/1.1 server that counts accepted TCP connections and never
    /// closes one itself, so any new connection observed here is the *client's*
    /// decision, not the server's.
    async fn connection_counting_server()
    -> (std::net::SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // determinism-ok: scanner false positive — "127.0.0.1:0" is a loopback address in a test-only helper, not a float
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind an ephemeral port");
        let addr = listener.local_addr().expect("resolve the bound address");
        let accepted = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();

        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    // Serve every request on this connection until the peer
                    // hangs up. The server never initiates a close.
                    while let Ok(read) = sock.read(&mut buf).await {
                        if read == 0 {
                            break;
                        }
                        if sock
                            // determinism-ok: scanner false positive — "HTTP/1.1" is the protocol version in a test-only byte string, not a float
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }
        });

        (addr, accepted)
    }

    /// The production client must pool across a short burst and evict once the
    /// idle window has passed.
    ///
    /// What this proves is bounded on purpose. It proves reuse across a 250ms
    /// gap and eviction past the timeout — not that every production cycle
    /// reuses its connection from pending-GET to attestation-POST: `run_cycle`
    /// calls `fetch_reference_observations` in between, and a slow NWS response
    /// under the shared 30s request timeout can push the node connection past
    /// two idle seconds. That eviction is safe, it is simply not reuse.
    ///
    /// Mutation proof: delete `.pool_idle_timeout(ATTESTER_POOL_IDLE_TIMEOUT)`
    /// from `build_attester_http_client` and the final assertion fails — hyper's
    /// 90s default keeps the connection, so the third request reuses it and the
    /// accept count stays at 1. That reuse is precisely what fails when a real
    /// server has closed the socket in the meantime (#2782).
    #[tokio::test]
    async fn attester_client_pools_within_a_cycle_and_evicts_between_cycles_2784() {
        use std::sync::atomic::Ordering;

        // The bounds this value has to satisfy, asserted rather than described,
        // against the timings we have actually measured: below actix-web's 5s
        // default keep-alive (what a node without #2783 serves) and below the
        // measured 5.05-5.18s inter-cycle gap, with margin. A custom poll is
        // clamped at 1s, so a 1s-configured attester can reuse a connection
        // across cycles — that reuse stays inside both this 2s client bound and
        // the known 5s server bound.
        assert!(
            ATTESTER_POOL_IDLE_TIMEOUT <= Duration::from_secs(3),
            "pool idle timeout must stay well under a 5s server keep-alive, got \
             {ATTESTER_POOL_IDLE_TIMEOUT:?}"
        );
        assert!(
            ATTESTER_POOL_IDLE_TIMEOUT >= Duration::from_secs(1),
            "pool idle timeout must still cover an intra-cycle request burst, got \
             {ATTESTER_POOL_IDLE_TIMEOUT:?}"
        );

        let (addr, accepted) = connection_counting_server().await;
        let url = format!("http://{addr}/probe");
        let client = build_attester_http_client().expect("build the production client");

        client.get(&url).send().await.expect("first request");
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            1,
            "the first request must open exactly one connection"
        );

        // Short burst: the pending GET and its attestation POSTs are typically
        // milliseconds apart when no reference fetch intervenes. A gap far
        // below the idle timeout must reuse the pooled connection.
        tokio::time::sleep(Duration::from_millis(250)).await;
        client.get(&url).send().await.expect("intra-cycle request");
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            1,
            "a request inside the idle window must reuse the pooled connection; \
             pooling is scoped, not disabled (#2784)"
        );

        // Past the idle window: the default 5s poll puts the next cycle
        // 5.05-5.18s out. Anything past the timeout must come up fresh.
        tokio::time::sleep(ATTESTER_POOL_IDLE_TIMEOUT + Duration::from_millis(750)).await;
        client.get(&url).send().await.expect("inter-cycle request");
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            2,
            "a request after the idle timeout must open a NEW connection; if it \
             reuses the old one the client can hand out a socket the server has \
             already closed (#2782/#2784)"
        );
    }

    /// Strip Rust comments so a source guard cannot be satisfied by
    /// commented-out code.
    ///
    /// The first version of the guard below searched raw source, so deleting
    /// the production call and leaving its exact text in a `//` comment kept
    /// the guard green — the guard asserted that the *characters* existed, not
    /// that the code did. String and raw-string literals are preserved verbatim,
    /// so the `//` inside a URL is never mistaken for a comment.
    fn strip_rust_comments(source: &str) -> String {
        let bytes = source.as_bytes();
        let mut out = String::with_capacity(source.len());
        let mut i = 0usize;

        while i < bytes.len() {
            // Raw string: r"..." or r#*"..."#* — copied through untouched.
            if bytes[i] == b'r' {
                let mut hashes = 0usize;
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'"' {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let body_start = j + 1;
                    match source[body_start..].find(&close) {
                        Some(rel) => {
                            let end = body_start + rel + close.len();
                            out.push_str(&source[i..end]);
                            i = end;
                            continue;
                        },
                        None => {
                            out.push_str(&source[i..]);
                            break;
                        },
                    }
                }
            }

            match bytes[i] {
                // Ordinary or byte string literal, honouring backslash escapes.
                b'"' => {
                    let start = i;
                    i += 1;
                    while i < bytes.len() {
                        match bytes[i] {
                            b'\\' => i += 2,
                            b'"' => {
                                i += 1;
                                break;
                            },
                            _ => i += 1,
                        }
                    }
                    out.push_str(&source[start..i.min(source.len())]);
                },
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                },
                b'/' if bytes.get(i + 1) == Some(&b'*') => {
                    let mut depth = 1usize;
                    i += 2;
                    while i < bytes.len() && depth > 0 {
                        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                            depth += 1;
                            i += 2;
                        } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                            depth -= 1;
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                },
                _ => {
                    let ch = source[i..].chars().next().expect("in-bounds char");
                    out.push(ch);
                    i += ch.len_utf8();
                },
            }
        }

        out
    }

    /// The invariant the guard enforces, factored out so an adversarial control
    /// can run the real checker against a mutated source image.
    ///
    /// Returns `None` when the source is compliant, or the reason it is not.
    fn single_hardened_client_violation(source: &str) -> Option<String> {
        // Needles are split across a concatenation so this module's own source
        // does not match them.
        let builder_needle = concat!("reqwest::Client", "::builder()");
        let setting_needle = concat!(".pool_idle", "_timeout(ATTESTER_POOL_IDLE_TIMEOUT)");
        let code = strip_rust_comments(source);

        let builders = code.matches(builder_needle).count();
        if builders != 1 {
            return Some(format!(
                "oracle_attester must construct exactly one reqwest client, found {builders}; \
                 a second one would not inherit the #2784 pool settings"
            ));
        }

        let Some(start) = code.find("fn build_attester_http_client") else {
            return Some("the production client builder must exist".to_string());
        };
        let body = &code[start..];
        let Some(end) = body.find("\n}\n") else {
            return Some("the production client builder must have a body".to_string());
        };
        let body = &body[..end];

        if !body.contains(builder_needle) {
            return Some(
                "the single client construction must live inside build_attester_http_client"
                    .to_string(),
            );
        }
        if !body.contains(setting_needle) {
            return Some(
                "build_attester_http_client must SET the pool idle timeout as code; without \
                 it hyper's 90s default restores the #2782 reuse race"
                    .to_string(),
            );
        }
        None
    }

    /// The daemon must have exactly one HTTP client construction, and it must be
    /// the hardened one.
    ///
    /// `build_attester_http_client` being correct is worth nothing if `main`
    /// builds its own client beside it, or if a later change adds a second one
    /// for the reference fetch.
    #[test]
    fn the_daemon_constructs_exactly_one_http_client_2784() {
        assert_eq!(
            single_hardened_client_violation(include_str!("oracle_attester.rs")),
            None,
            "the shipped source must satisfy the single-hardened-client invariant"
        );
    }

    /// Adversarial control for the guard itself (#2784 review finding 2).
    ///
    /// A commented-out setting is not a setting. This removes the production
    /// call and leaves its exact text in a `//` comment — the precise mutation
    /// that the raw-source guard accepted — and requires the checker to reject
    /// it. Deleting the comment-stripping pass makes this test REAL-RED.
    #[test]
    fn commented_out_pool_idle_timeout_does_not_satisfy_the_guard_2784() {
        let setting = concat!(".pool_idle", "_timeout(ATTESTER_POOL_IDLE_TIMEOUT)");
        let source = include_str!("oracle_attester.rs");

        // Precondition: the real source is compliant, so any rejection below is
        // caused by the mutation and not by an unrelated drift.
        assert_eq!(single_hardened_client_violation(source), None);

        let commented_out = source.replacen(setting, &format!("/* {setting} */"), 1);
        assert!(
            commented_out.contains(setting),
            "the mutated image must still contain the exact setting text"
        );
        let violation = single_hardened_client_violation(&commented_out)
            .expect("a commented-out setting must NOT satisfy the guard");
        assert!(
            violation.contains("SET the pool idle timeout"),
            "the guard must fail specifically on the missing setting, got: {violation}"
        );

        // The stripper must not swallow code that merely looks like a comment:
        // `//` inside a string literal is data, not a comment.
        let kept = strip_rust_comments("let u = \"http://host/path\"; // trailing\nlet v = 1;");
        assert!(kept.contains("http://host/path"), "string literals survive: {kept}");
        assert!(!kept.contains("trailing"), "line comments are removed: {kept}");
    }

    // ---- per-commit reference selection (#3240) ----------------------------
    //
    // One mock HTTP server plays both the node (pending / upgrade / attestation
    // POST) and api.weather.gov (points / station lists / stations /
    // observations), so these drive the real `run_cycle_with_resolver` end to
    // end: HTTP discovery, selection, pairing, policy, signing, submission.

    use std::sync::{Arc, Mutex};
    use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
    use wrfcoin_shared_protocol::crypto::domain_signing::{signing_chain_id, signing_network_id};
    use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;
    use wrfcoin_shared_protocol::oracle::mechanical::UNCHECKED_CONFIDENCE_CAP;

    /// Observation values served for a station: (temp °C, pressure Pa, RH %, wind km/h).
    type ObsValues = (f64, f64, f64, f64);

    #[derive(Default)]
    struct MockWorld {
        pending: serde_json::Value,
        /// point key "lat,lon" → grid id, or an HTTP status to fail with.
        points: HashMap<String, Result<String, u16>>,
        /// grid id → station list (id, lat, lon), nearest first.
        grids: HashMap<String, Vec<(String, f64, f64)>>,
        /// station id → (lat, lon) for `GET /stations/{id}`.
        stations: HashMap<String, (f64, f64)>,
        /// station id → observation values, or an HTTP status to fail with.
        observations: HashMap<String, Result<ObsValues, u16>>,
        retry_after: Option<u64>,
        hits: HashMap<String, usize>,
        posts: Vec<serde_json::Value>,
    }

    impl MockWorld {
        fn hits(&self, prefix: &str) -> usize {
            self.hits.iter().filter(|(path, _)| path.starts_with(prefix)).map(|(_, n)| n).sum()
        }

        fn confidence_for(&self, commit_id: &str) -> f64 {
            let post = self
                .posts
                .iter()
                .find(|p| p["commit_hash"] == commit_id)
                .unwrap_or_else(|| panic!("no attestation posted for {commit_id}"));
            assert_eq!(post["approved"], true, "reference choice must never flip approval");
            post["confidence"].as_f64().expect("confidence is a number")
        }
    }

    fn route(world: &mut MockWorld, method: &str, path: &str, body: &str) -> (u16, String, String) {
        *world.hits.entry(path.to_string()).or_default() += 1;
        let ok = |v: serde_json::Value| (200, String::new(), v.to_string());
        let fail = |status: u16, retry: Option<u64>| {
            let extra = retry.map(|s| format!("Retry-After: {s}\r\n")).unwrap_or_default();
            (status, extra, r#"{"error":"mock"}"#.to_string())
        };
        if method == "POST" && path == "/transactions/oracle/attestation" {
            world.posts.push(serde_json::from_str(body).expect("attestation JSON"));
            return ok(serde_json::json!({"accepted": true}));
        }
        if path == "/transactions/oracle/pending" {
            return ok(world.pending.clone());
        }
        if path == "/network/upgrade" {
            return ok(serde_json::json!({"notice": null}));
        }
        if let Some(point) = path.strip_prefix("/points/") {
            return match world.points.get(point) {
                Some(Ok(grid)) => ok(serde_json::json!({"properties": {
                    // Absolute production URL: the attester must re-root it
                    // onto the configured base.
                    "observationStations": format!("https://api.weather.gov/gridpoints/{grid}/stations")
                }})),
                Some(Err(status)) => fail(*status, world.retry_after),
                None => fail(404, None),
            };
        }
        if let Some(rest) = path.strip_prefix("/gridpoints/") {
            let grid = rest.trim_end_matches("/stations");
            let features: Vec<_> = world
                .grids
                .get(grid)
                .into_iter()
                .flatten()
                .map(|(id, lat, lon)| serde_json::json!({
                    "geometry": {"type": "Point", "coordinates": [lon, lat]},
                    "properties": {"stationIdentifier": id}
                }))
                .collect();
            return ok(serde_json::json!({"type": "FeatureCollection", "features": features}));
        }
        if let Some(rest) = path.strip_prefix("/stations/") {
            let (station, is_obs) = match rest.split_once("/observations") {
                Some((station, _)) => (station, true),
                None => (rest, false),
            };
            if !is_obs {
                return match world.stations.get(station) {
                    Some((lat, lon)) => ok(serde_json::json!({
                        "geometry": {"type": "Point", "coordinates": [lon, lat]},
                        "properties": {"stationIdentifier": station}
                    })),
                    None => fail(404, None),
                };
            }
            return match world.observations.get(station) {
                Some(Ok((t, p, rh, w))) => ok(serde_json::json!({
                    "type": "FeatureCollection",
                    "features": [{"properties": {
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "temperature": {"value": t},
                        "barometricPressure": {"value": p},
                        "relativeHumidity": {"value": rh},
                        "windSpeed": {"value": w}
                    }}]
                })),
                Some(Err(status)) => fail(*status, world.retry_after),
                None => fail(404, None),
            };
        }
        fail(404, None)
    }

    async fn mock_server(world: Arc<Mutex<MockWorld>>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // determinism-ok: scanner false positive — loopback address in a test-only helper
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let world = world.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let (head_end, content_len) = loop {
                        let Ok(n) = sock.read(&mut chunk).await else { return };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                            let len = head
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break (i + 4, len);
                        }
                    };
                    while buf.len() < head_end + content_len {
                        let Ok(n) = sock.read(&mut chunk).await else { return };
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
                    let mut parts = head.split_whitespace();
                    let method = parts.next().unwrap_or("").to_string();
                    let target = parts.next().unwrap_or("");
                    let path = target.split('?').next().unwrap_or("").to_string();
                    let (status, extra, payload) =
                        route(&mut world.lock().unwrap(), &method, &path, &body);
                    let response = format!(
                        // determinism-ok: scanner false positive — HTTP version in a test-only response
                        "HTTP/1.1 {status} MOCK\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    /// Duluth-like reading, matching the Duluth reference values below.
    fn duluth_reading() -> serde_json::Value {
        serde_json::json!({
            "temperature_c": 21.5, "humidity_percent": 60.0, "pressure_hpa": 1014.0,
            "wind_speed_ms": 2.8, "precipitation_mm": 0.0, "dewpoint_c": 13.5
        })
    }
    const DULUTH_REF: ObsValues = (21.0, 101_350.0, 58.0, 10.8);

    /// Minneapolis-like reading: honest, and far from Duluth's weather.
    fn minneapolis_reading() -> serde_json::Value {
        serde_json::json!({
            "temperature_c": 30.0, "humidity_percent": 30.0, "pressure_hpa": 1000.0,
            "wind_speed_ms": 8.0, "precipitation_mm": 0.0, "dewpoint_c": 10.5
        })
    }
    const MINNEAPOLIS_REF: ObsValues = (29.8, 100_000.0, 31.0, 28.8);

    const DULUTH: (f64, f64) = (46.7867, -92.1005);
    const MINNEAPOLIS: (f64, f64) = (44.9778, -93.2650);

    fn pending_entry(
        tag: &str,
        data: serde_json::Value,
        location: Option<(f64, f64)>,
    ) -> serde_json::Value {
        let timestamp = chrono::Utc::now().timestamp();
        let station_id = format!("PWS-3240-{tag}");
        let nonce = format!("nonce-{tag}");
        let commit_id =
            Transaction::compute_commit_id(&data, &nonce, &station_id, timestamp, "contribution");
        let mut entry = serde_json::json!({
            "commit_id": commit_id, "station_id": station_id, "timestamp": timestamp,
            "nonce": nonce, "contribution_type": "contribution", "data": data,
            "source": "mempool", "attesting_oracles": [], "has_quorum": false
        });
        if let Some((lat, lon)) = location {
            entry["station_location"] = serde_json::json!({"latitude": lat, "longitude": lon});
        }
        entry
    }

    fn test_attester_3240() -> Attester {
        // secret-leak-ok: deterministic test-only fixture, never production custody material
        let key = PrivateKey::from_bytes(&[0x32; 32]).expect("fixture key");
        Attester::new(Arc::new(EnvKeySigner::from_private_key(
            key,
            "WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY",
        )))
    }

    /// The shared world: KDLH near Duluth, KMSP near Minneapolis (≈ 210 km
    /// apart), both points covered, both stations serving matching weather.
    fn two_city_world(attester: &Attester, entries: Vec<serde_json::Value>) -> MockWorld {
        let mut world = MockWorld::default();
        world.pending = serde_json::json!({
            "chain_id": signing_chain_id(), "network_id": signing_network_id(),
            "height": 100, "oracle_set": [attester.oracle_id()], "effective_quorum": 1,
            "observations": entries
        });
        world.points.insert("46.79,-92.10".into(), Ok("DLH/40,70".into()));
        world.points.insert("44.98,-93.27".into(), Ok("MPX/107,71".into()));
        world.grids.insert("DLH/40,70".into(), vec![("KDLH".into(), 46.8369, -92.1833)]);
        world.grids.insert(
            "MPX/107,71".into(),
            vec![("KMSP".into(), 44.8831, -93.2289), ("KDLH".into(), 46.8369, -92.1833)],
        );
        world.stations.insert("KDLH".into(), (46.8369, -92.1833));
        world.stations.insert("KMSP".into(), (44.8831, -93.2289));
        world.observations.insert("KDLH".into(), Ok(DULUTH_REF));
        world.observations.insert("KMSP".into(), Ok(MINNEAPOLIS_REF));
        world
    }

    async fn cycle(
        config: &Config,
        attester: &Attester,
        resolver: &mut ReferenceResolver,
    ) -> usize {
        let client = build_attester_http_client().expect("client");
        let mut recent = HashMap::new();
        run_cycle_with_resolver(
            &client,
            config,
            attester,
            &MechanicalPolicy::default(),
            &mut recent,
            resolver,
        )
        .await
        .expect("cycle completes")
    }

    fn config_3240(base: &str, mode: ReferenceMode, max_km: f64, max_stations: usize) -> Config {
        Config::for_test(
            base,
            "token",
            signing_chain_id(),
            signing_network_id(),
            Some("KDLH".to_string()),
            base,
            DEFAULT_MAX_PAIR_DELTA_SECS,
        )
        .with_reference_selection(mode, max_km, max_stations)
    }

    /// Mixed batch, one cycle, two cities: each commit is paired with its own
    /// nearest station, each station is fetched exactly once, and the
    /// Minneapolis reading — an outlier against Duluth — is fully checked.
    /// The `fixed` control on the same batch proves the choice is what moved
    /// the Minneapolis confidence.
    #[tokio::test]
    async fn per_station_mixed_batch_pairs_each_commit_with_its_nearest_station_3240() {
        let attester = test_attester_3240();
        let duluth = pending_entry("dlh", duluth_reading(), Some(DULUTH));
        let duluth_2 = pending_entry("dlh2", duluth_reading(), Some(DULUTH));
        let msp = pending_entry("msp", minneapolis_reading(), Some(MINNEAPOLIS));
        let ids: Vec<String> = [&duluth, &duluth_2, &msp]
            .iter()
            .map(|e| e["commit_id"].as_str().unwrap().to_string())
            .collect();
        let world = Arc::new(Mutex::new(two_city_world(&attester, vec![duluth, duluth_2, msp])));
        let base = mock_server(world.clone()).await;

        let config = config_3240(&base, ReferenceMode::PerStation, 50.0, 8);
        let mut resolver = ReferenceResolver::default();
        assert_eq!(cycle(&config, &attester, &mut resolver).await, 3);
        let (per_station, kdlh_fetches, kmsp_fetches, points) = {
            let w = world.lock().unwrap();
            (
                ids.iter().map(|id| w.confidence_for(id)).collect::<Vec<_>>(),
                w.hits("/stations/KDLH/observations"),
                w.hits("/stations/KMSP/observations"),
                w.hits("/points/"),
            )
        };
        for (id, confidence) in ids.iter().zip(&per_station) {
            assert!(
                *confidence > UNCHECKED_CONFIDENCE_CAP,
                "commit {id} must be reference-checked against its own city, got {confidence}"
            );
        }
        assert_eq!(kdlh_fetches, 1, "two Duluth commits share one KDLH fetch per cycle");
        assert_eq!(kmsp_fetches, 1, "Minneapolis commit fetches KMSP once");
        assert_eq!(points, 2, "one point lookup per distinct location");

        // Control: the same batch in fixed mode scores Minneapolis vs KDLH.
        world.lock().unwrap().posts.clear();
        let fixed = config_3240(&base, ReferenceMode::Fixed, 50.0, 8);
        assert_eq!(cycle(&fixed, &attester, &mut ReferenceResolver::default()).await, 3);
        let fixed_msp = world.lock().unwrap().confidence_for(&ids[2]);
        assert!(
            fixed_msp < per_station[2],
            "fixed mode must score the Minneapolis reading worse against Duluth \
             ({fixed_msp} vs per-station {})",
            per_station[2]
        );
    }

    /// Point → station lookups are cached across cycles (stations do not
    /// move); observations are cached per cycle only.
    #[tokio::test]
    async fn point_lookups_are_cached_across_cycles_observations_per_cycle_3240() {
        let attester = test_attester_3240();
        let entry = pending_entry("cache", duluth_reading(), Some(DULUTH));
        let world = Arc::new(Mutex::new(two_city_world(&attester, vec![entry])));
        let base = mock_server(world.clone()).await;
        let config = config_3240(&base, ReferenceMode::PerStation, 50.0, 8);
        let mut resolver = ReferenceResolver::default();

        assert_eq!(cycle(&config, &attester, &mut resolver).await, 1);
        assert_eq!(cycle(&config, &attester, &mut resolver).await, 1);
        let w = world.lock().unwrap();
        assert_eq!(w.hits("/points/"), 1, "the second cycle must reuse the cached point lookup");
        assert_eq!(w.hits("/gridpoints/"), 1);
        assert_eq!(resolver.lookup_requests, 2, "points + station list, once for the process");
        assert_eq!(
            w.hits("/stations/KDLH/observations"),
            2,
            "observations are fresh every cycle"
        );
    }

    /// Lookup failure → the configured station (inside the radius), named in
    /// the reasons; a station with no registry location → configured station
    /// without any lookup.
    #[tokio::test]
    async fn lookup_failure_and_missing_location_fall_back_to_configured_station_3240() {
        let attester = test_attester_3240();
        let failed_lookup = pending_entry("lookupfail", duluth_reading(), Some(DULUTH));
        let no_location = pending_entry("noloc", duluth_reading(), None);
        let ids = [
            failed_lookup["commit_id"].as_str().unwrap().to_string(),
            no_location["commit_id"].as_str().unwrap().to_string(),
        ];
        let mut w = two_city_world(&attester, vec![failed_lookup, no_location]);
        w.points.insert("46.79,-92.10".into(), Err(500));
        let world = Arc::new(Mutex::new(w));
        let base = mock_server(world.clone()).await;
        let config = config_3240(&base, ReferenceMode::PerStation, 50.0, 8);

        let client = build_attester_http_client().unwrap();
        let mut resolver = ReferenceResolver::default();
        resolver.begin_cycle();
        let mut refs = CycleReferences::default();
        let selection = select_reference(
            &client,
            &config,
            &mut resolver,
            &mut refs,
            Some(ReferenceLocation { latitude: DULUTH.0, longitude: DULUTH.1 }),
        )
        .await;
        assert_eq!(selection.station.as_deref(), Some("KDLH"));
        assert!(!selection.observations.is_empty());
        let notes = selection.notes.join(" | ");
        assert!(notes.contains("nearest-station lookup failed"), "{notes}");
        assert!(notes.contains("using configured station KDLH (fallback)"), "{notes}");

        let unlocated = select_reference(&client, &config, &mut resolver, &mut refs, None).await;
        assert_eq!(unlocated.station.as_deref(), Some("KDLH"));
        assert!(unlocated.notes.join(" ").contains("no registry location"), "{:?}", unlocated.notes);

        assert_eq!(cycle(&config, &attester, &mut ReferenceResolver::default()).await, 2);
        let w = world.lock().unwrap();
        for id in &ids {
            assert!(w.confidence_for(id) > UNCHECKED_CONFIDENCE_CAP, "{id} falls back to KDLH");
        }
    }

    /// Lookup fails AND the configured station's fetch fails → PHYSICS-ONLY
    /// with the capped confidence, still approved, and the reasons say why.
    #[tokio::test]
    async fn lookup_and_reference_fetch_failure_is_physics_only_capped_3240() {
        let attester = test_attester_3240();
        let entry = pending_entry("allfail", duluth_reading(), Some(DULUTH));
        let id = entry["commit_id"].as_str().unwrap().to_string();
        let mut w = two_city_world(&attester, vec![entry]);
        w.points.insert("46.79,-92.10".into(), Err(500));
        w.observations.insert("KDLH".into(), Err(503));
        let world = Arc::new(Mutex::new(w));
        let base = mock_server(world.clone()).await;
        let config = config_3240(&base, ReferenceMode::PerStation, 50.0, 8);

        let client = build_attester_http_client().unwrap();
        let mut resolver = ReferenceResolver::default();
        resolver.begin_cycle();
        let selection = select_reference(
            &client,
            &config,
            &mut resolver,
            &mut CycleReferences::default(),
            Some(ReferenceLocation { latitude: DULUTH.0, longitude: DULUTH.1 }),
        )
        .await;
        assert_eq!(selection.station, None);
        assert!(selection.observations.is_empty());
        let notes = selection.notes.join(" | ");
        assert!(notes.contains("fetch from configured station KDLH failed"), "{notes}");
        assert!(notes.contains("PHYSICS-ONLY"), "{notes}");

        assert_eq!(cycle(&config, &attester, &mut ReferenceResolver::default()).await, 1);
        let confidence = world.lock().unwrap().confidence_for(&id);
        assert!(confidence <= UNCHECKED_CONFIDENCE_CAP, "must be capped, got {confidence}");
    }

    /// A station whose nearest NWS station is beyond the radius is
    /// PHYSICS-ONLY (capped 0.5) — never scored against a far station (0.1
    /// outlier multiplier) and never against the even farther configured one.
    /// Mutation control: drop the `km > max_km` check in `select_reference`
    /// and this goes RED (the far station's mismatched weather tiers it).
    #[tokio::test]
    async fn remote_station_beyond_radius_is_physics_only_not_outlier_3240() {
        let attester = test_attester_3240();
        let entry = pending_entry("remote", minneapolis_reading(), Some(MINNEAPOLIS));
        let id = entry["commit_id"].as_str().unwrap().to_string();
        let mut w = two_city_world(&attester, vec![entry]);
        // Coverage only via a far station serving Duluth weather.
        w.grids.insert("MPX/107,71".into(), vec![("KDLH".into(), 46.8369, -92.1833)]);
        let world = Arc::new(Mutex::new(w));
        let base = mock_server(world.clone()).await;
        let config = config_3240(&base, ReferenceMode::PerStation, 25.0, 8);

        assert_eq!(cycle(&config, &attester, &mut ReferenceResolver::default()).await, 1);
        let w = world.lock().unwrap();
        let confidence = w.confidence_for(&id);
        assert_eq!(
            confidence.to_bits(),
            UNCHECKED_CONFIDENCE_CAP.to_bits(),
            "beyond the radius the commit is capped physics-only, not outlier-scored"
        );
        assert_eq!(w.hits("/stations/KDLH/observations"), 0, "no far reference is fetched");
    }

    /// The distinct-station budget bounds NWS traffic per cycle. With a
    /// budget of one station (two lookups), the Duluth commit spends it; the
    /// Minneapolis commit may not open a second station, and its fallback
    /// (KDLH, ~210 km) is outside the radius — so it is visibly capped rather
    /// than fetched or mis-scored.
    #[tokio::test]
    async fn per_cycle_station_budget_bounds_fetches_3240() {
        let attester = test_attester_3240();
        let duluth = pending_entry("budget-dlh", duluth_reading(), Some(DULUTH));
        let msp = pending_entry("budget-msp", minneapolis_reading(), Some(MINNEAPOLIS));
        let ids = [
            duluth["commit_id"].as_str().unwrap().to_string(),
            msp["commit_id"].as_str().unwrap().to_string(),
        ];
        let world = Arc::new(Mutex::new(two_city_world(&attester, vec![duluth, msp])));
        let base = mock_server(world.clone()).await;
        let config = config_3240(&base, ReferenceMode::PerStation, 25.0, 1);
        let mut resolver = ReferenceResolver::default();

        assert_eq!(cycle(&config, &attester, &mut resolver).await, 2);
        let w = world.lock().unwrap();
        assert!(w.confidence_for(&ids[0]) > UNCHECKED_CONFIDENCE_CAP);
        assert_eq!(
            w.confidence_for(&ids[1]).to_bits(),
            UNCHECKED_CONFIDENCE_CAP.to_bits(),
            "over budget and outside the fallback radius ⇒ capped physics-only"
        );
        assert_eq!(w.hits("/stations/KMSP/observations"), 0, "budget blocks a second station");
        assert_eq!(resolver.lookup_requests, 2, "lookups bounded at 2 × station budget");
    }

    /// A 429 with Retry-After stops lookups for that long, across cycles.
    #[tokio::test]
    async fn rate_limited_lookup_backs_off_across_cycles_3240() {
        let attester = test_attester_3240();
        let entry = pending_entry("ratelimit", duluth_reading(), Some(DULUTH));
        let mut w = two_city_world(&attester, vec![entry]);
        w.points.insert("46.79,-92.10".into(), Err(429));
        w.retry_after = Some(120);
        let world = Arc::new(Mutex::new(w));
        let base = mock_server(world.clone()).await;
        let config = config_3240(&base, ReferenceMode::PerStation, 50.0, 8);
        let mut resolver = ReferenceResolver::default();

        assert_eq!(cycle(&config, &attester, &mut resolver).await, 1);
        assert_eq!(cycle(&config, &attester, &mut resolver).await, 1);
        let w = world.lock().unwrap();
        assert_eq!(w.hits("/points/"), 1, "the backoff must suppress the second cycle's lookup");
        assert!(resolver.backoff_remaining().is_some());
        // Both cycles still attested through the configured-station fallback.
        assert_eq!(w.posts.len(), 2);
    }

    /// Shadow mode submits exactly the fixed verdict even when the per-station
    /// verdict differs.
    #[tokio::test]
    async fn shadow_mode_submits_the_fixed_verdict_3240() {
        let attester = test_attester_3240();
        let entry = pending_entry("shadow", minneapolis_reading(), Some(MINNEAPOLIS));
        let id = entry["commit_id"].as_str().unwrap().to_string();
        let world = Arc::new(Mutex::new(two_city_world(&attester, vec![entry])));
        let base = mock_server(world.clone()).await;

        let fixed = config_3240(&base, ReferenceMode::Fixed, 50.0, 8);
        assert_eq!(cycle(&fixed, &attester, &mut ReferenceResolver::default()).await, 1);
        let fixed_confidence = world.lock().unwrap().confidence_for(&id);
        world.lock().unwrap().posts.clear();

        let per_station = config_3240(&base, ReferenceMode::PerStation, 50.0, 8);
        assert_eq!(cycle(&per_station, &attester, &mut ReferenceResolver::default()).await, 1);
        let per_station_confidence = world.lock().unwrap().confidence_for(&id);
        world.lock().unwrap().posts.clear();
        assert!(
            per_station_confidence - fixed_confidence > 0.3,
            "precondition: the two modes must disagree widely on this commit \
             (fixed {fixed_confidence}, per-station {per_station_confidence})"
        );

        let shadow = config_3240(&base, ReferenceMode::Shadow, 50.0, 8);
        assert_eq!(cycle(&shadow, &attester, &mut ReferenceResolver::default()).await, 1);
        let w = world.lock().unwrap();
        // Tolerance, not bit equality: the runs straddle wall-clock seconds and
        // confidence carries linear freshness decay (≈ 0.00014 per second).
        let shadow_confidence = w.confidence_for(&id);
        assert!(
            (shadow_confidence - fixed_confidence).abs() < 0.01,
            "shadow must submit the fixed verdict: shadow {shadow_confidence}, fixed \
             {fixed_confidence}, per-station {per_station_confidence}"
        );
        assert_eq!(w.hits("/stations/KMSP/observations"), 2, "shadow did compute per-station");
    }

    #[test]
    fn reference_mode_config_refuses_unknown_mode_and_missing_radius_3240() {
        assert_eq!(reference_selection_config(None, None).unwrap(), (ReferenceMode::Fixed, None));
        assert_eq!(reference_selection_config(Some(" "), None).unwrap().0, ReferenceMode::Fixed);
        assert_eq!(
            reference_selection_config(Some("per_station"), Some("25")).unwrap(),
            (ReferenceMode::PerStation, Some(25.0))
        );
        let unknown = reference_selection_config(Some("nearest"), Some("25")).unwrap_err();
        assert!(unknown.to_string().contains("not one of"), "{unknown}");
        for mode in ["shadow", "per_station"] {
            let missing = reference_selection_config(Some(mode), None).unwrap_err();
            assert!(missing.to_string().contains("requires WRFCOIN_ATTESTER_MAX_REFERENCE_KM"));
        }
        assert!(reference_selection_config(Some("per_station"), Some("-1")).is_err());
        assert!(reference_selection_config(Some("per_station"), Some("NaN")).is_err());
    }

    /// #3240 review N2: `fixed` never reads the radius, so a malformed
    /// `WRFCOIN_ATTESTER_MAX_REFERENCE_KM` must not stop a fixed-mode attester
    /// (it is ignored with a warning). `shadow` / `per_station` DO use it and
    /// must still refuse every malformed value.
    #[test]
    fn fixed_mode_ignores_a_malformed_radius_shadow_and_per_station_refuse_3240() {
        for bad in ["abc", "-5", "0", "NaN", "inf", "25km"] {
            for fixed in [None, Some("fixed"), Some(" ")] {
                assert_eq!(
                    reference_selection_config(fixed, Some(bad)).unwrap_or_else(|e| panic!(
                        "fixed mode ({fixed:?}) must not refuse radius {bad:?}: {e:#}"
                    )),
                    (ReferenceMode::Fixed, None),
                    "a malformed radius is dropped, never carried, in fixed mode"
                );
            }
            for mode in ["shadow", "per_station"] {
                let err = reference_selection_config(Some(mode), Some(bad))
                    .expect_err("shadow/per_station must refuse a malformed radius");
                assert!(
                    err.to_string().contains("WRFCOIN_ATTESTER_MAX_REFERENCE_KM"),
                    "{mode} {bad:?}: {err:#}"
                );
            }
        }
        // A valid radius in fixed mode is harmless and still accepted.
        assert_eq!(
            reference_selection_config(Some("fixed"), Some("25")).unwrap().0,
            ReferenceMode::Fixed
        );
    }

    #[test]
    fn pending_entry_parses_station_location_and_defaults_absent_3240() {
        let with: PendingEntry = serde_json::from_value(pending_entry("p", duluth_reading(), Some(DULUTH)))
            .expect("parses");
        assert_eq!(
            with.station_location,
            Some(ReferenceLocation { latitude: DULUTH.0, longitude: DULUTH.1 })
        );
        let without: PendingEntry =
            serde_json::from_value(pending_entry("q", duluth_reading(), None)).expect("parses");
        assert_eq!(without.station_location, None);
        // The attester re-coarsens before any NWS request.
        let coarse = with.station_location.unwrap().coarse().unwrap();
        assert_eq!(coarse.point_key(), "46.79,-92.10");
        assert_eq!(
            ReferenceLocation { latitude: 91.0, longitude: 0.0 }.coarse(),
            None,
            "out-of-range coordinates are unusable, not clamped"
        );
    }

    #[test]
    fn nws_urls_are_rebased_onto_the_configured_base_3240() {
        let base = "http://127.0.0.1:9";
        assert_eq!(
            rebase_nws_url(base, "https://api.weather.gov/gridpoints/DLH/40,70/stations").unwrap(),
            "http://127.0.0.1:9/gridpoints/DLH/40,70/stations"
        );
        assert_eq!(
            rebase_nws_url(base, "http://127.0.0.1:9/gridpoints/X/stations").unwrap(),
            "http://127.0.0.1:9/gridpoints/X/stations"
        );
        assert!(rebase_nws_url(base, "ftp:evil").is_err());
    }
}
