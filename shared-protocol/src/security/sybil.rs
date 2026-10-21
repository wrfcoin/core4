//! #2148 — contribution-side Sybil caps at the ingress edge.
//!
//! Limiting only the faucet pushes identity farming onto the data-contribution
//! reward, which is worth more. The faucet itself is not in core4 (its
//! emission slice is, #3039; the claiming service is backend/infra), so this
//! module bounds the half core4 owns: **how many stations one identity may
//! submit for, and how many commits one station may make per window**, before
//! a commit reaches the mempool. Both bounds are cheap to enforce and cheap to
//! reason about. They bound rate per key, NOT identity: the edge only
//! shape-validates `station_id`, and neither mempool admission nor
//! `RegisterCommit` consults the station registry. So a bearer can rotate
//! `station_id` and `contributor` to mint fresh budgets, and those commits
//! can land on-chain. What limits the payoff is downstream: a commit for an
//! unregistered station is recorded as `Unclassified` provenance and earns a
//! zero-amount reward record (#2460 gate). Refusing unregistered stations at
//! the edge would make the per-station cap a real bound; that is an owner
//! decision, left open on #2148 (Refs #2148), not a guarantee made here.
//!
//! What this is not: the per-operator **reward** cap per epoch and the
//! minimum station age before a station earns. Those change what consensus
//! pays and need an owner-ratified policy table plus a consensus-owned
//! operator/station relation; they are recorded on the story as the
//! feature-gated follow-up. This module never touches a reward.
//!
//! All state is bounded (#2077): the limiter holds at most
//! [`ContributionSybilPolicy::max_tracked_entries`] identity and station
//! entries; at capacity it sweeps expired entries and, if still full, refuses
//! new keys rather than growing — the same discipline as the per-contributor
//! commit limiter (#1609).

use std::collections::{BTreeSet, HashMap};

/// The caps. Defaults are the `[testnet]` values recorded in
/// `docs/security/ECONOMIC_ABUSE_MODEL.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributionSybilPolicy {
    /// Distinct station ids one identity may commit for.
    pub max_stations_per_identity: u32,
    /// Commits one station may make within a window.
    pub max_commits_per_station: u32,
    /// Window length for the per-station cap.
    pub window_secs: u64,
    /// Hard bound on tracked identity + station entries.
    pub max_tracked_entries: usize,
}

impl ContributionSybilPolicy {
    pub const DEFAULT_MAX_STATIONS_PER_IDENTITY: u32 = 8;
    pub const DEFAULT_MAX_COMMITS_PER_STATION: u32 = 120;
    pub const DEFAULT_WINDOW_SECS: u64 = 3_600;
    pub const DEFAULT_MAX_TRACKED_ENTRIES: usize = 100_000;
    /// Absolute ceiling on `max_tracked_entries`, whatever the environment says.
    pub const MAX_TRACKED_ENTRIES_CEILING: usize = 1_000_000;

    /// Clamp every field into its sane range: zero caps would refuse
    /// everything, an unbounded table would recreate #2077.
    pub fn normalised(mut self) -> Self {
        self.max_stations_per_identity = self.max_stations_per_identity.max(1);
        self.max_commits_per_station = self.max_commits_per_station.max(1);
        self.window_secs = self.window_secs.max(1);
        self.max_tracked_entries = self
            .max_tracked_entries
            .clamp(2, Self::MAX_TRACKED_ENTRIES_CEILING);
        self
    }
}

impl Default for ContributionSybilPolicy {
    fn default() -> Self {
        Self {
            max_stations_per_identity: Self::DEFAULT_MAX_STATIONS_PER_IDENTITY,
            max_commits_per_station: Self::DEFAULT_MAX_COMMITS_PER_STATION,
            window_secs: Self::DEFAULT_WINDOW_SECS,
            max_tracked_entries: Self::DEFAULT_MAX_TRACKED_ENTRIES,
        }
    }
}

/// Why a commit was refused. Each maps to one stable API error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SybilRefusal {
    /// The identity already commits for `max` distinct stations.
    TooManyStations { identity: String, max: u32 },
    /// The station already made `max` commits in the current window.
    StationCommitCapExceeded {
        station_id: String,
        max: u32,
        window_secs: u64,
        /// Seconds until the station's window resets.
        retry_after_secs: u64,
    },
    /// The limiter is at capacity with no expired entries to free; a new key
    /// is refused rather than tracked. Existing keys are unaffected.
    LimiterCapacity { tracked: usize, max: usize },
}

impl SybilRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooManyStations { .. } => "SYBIL_STATION_LIMIT",
            Self::StationCommitCapExceeded { .. } => "SYBIL_STATION_COMMIT_CAP",
            Self::LimiterCapacity { .. } => "SYBIL_LIMITER_CAPACITY",
        }
    }
}

impl std::fmt::Display for SybilRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyStations { identity, max } => write!(
                f,
                "identity {identity} already commits for {max} stations (the per-identity cap)"
            ),
            Self::StationCommitCapExceeded { station_id, max, window_secs, retry_after_secs } => {
                write!(
                    f,
                    "station {station_id} already made {max} commits in its {window_secs} s \
                     window; retry in {retry_after_secs} s"
                )
            },
            Self::LimiterCapacity { tracked, max } => write!(
                f,
                "sybil limiter at capacity ({tracked}/{max} tracked entries); new identities and \
                 stations are refused until entries expire"
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct IdentityEntry {
    stations: BTreeSet<String>,
    last_seen: i64,
}

#[derive(Debug, Clone)]
struct StationEntry {
    window_start: i64,
    count: u32,
    last_seen: i64,
}

/// Bounded limiter over identities and stations. Not thread-safe by itself;
/// the caller wraps it in a mutex.
#[derive(Debug, Clone)]
pub struct SybilLimiter {
    policy: ContributionSybilPolicy,
    identities: HashMap<String, IdentityEntry>,
    stations: HashMap<String, StationEntry>,
}

impl SybilLimiter {
    pub fn new(policy: ContributionSybilPolicy) -> Self {
        Self {
            policy: policy.normalised(),
            identities: HashMap::new(),
            stations: HashMap::new(),
        }
    }

    pub fn policy(&self) -> &ContributionSybilPolicy {
        &self.policy
    }

    /// Identity entries plus station entries currently held.
    pub fn tracked_entries(&self) -> usize {
        self.identities.len() + self.stations.len()
    }

    /// Distinct stations recorded for an identity.
    pub fn stations_for(&self, identity: &str) -> usize {
        self.identities
            .get(identity)
            .map(|e| e.stations.len())
            .unwrap_or(0)
    }

    /// Commits recorded for a station in the window containing `now_secs`.
    pub fn commits_in_window(&self, station_id: &str, now_secs: i64) -> u32 {
        self.stations
            .get(station_id)
            .filter(|e| !self.window_expired(e.window_start, now_secs))
            .map(|e| e.count)
            .unwrap_or(0)
    }

    fn window_expired(&self, window_start: i64, now_secs: i64) -> bool {
        now_secs.saturating_sub(window_start) >= self.policy.window_secs as i64
    }

    /// Drop entries nothing has touched for a full window.
    pub fn sweep_expired(&mut self, now_secs: i64) {
        let window = self.policy.window_secs as i64;
        self.identities
            .retain(|_, e| now_secs.saturating_sub(e.last_seen) < window);
        self.stations
            .retain(|_, e| now_secs.saturating_sub(e.last_seen) < window);
    }

    /// Admit one commit by `identity` for `station_id` at `now_secs`, or
    /// refuse it. A refused commit records nothing.
    pub fn admit_commit(
        &mut self,
        identity: &str,
        station_id: &str,
        now_secs: i64,
    ) -> Result<(), SybilRefusal> {
        // Capacity first, so a flood of new keys cannot grow the tables.
        let new_identity = !self.identities.contains_key(identity);
        let new_station = !self.stations.contains_key(station_id);
        let needed = usize::from(new_identity) + usize::from(new_station);
        if needed > 0 && self.tracked_entries() + needed > self.policy.max_tracked_entries {
            self.sweep_expired(now_secs);
            if self.tracked_entries() + needed > self.policy.max_tracked_entries {
                return Err(SybilRefusal::LimiterCapacity {
                    tracked: self.tracked_entries(),
                    max: self.policy.max_tracked_entries,
                });
            }
        }

        // Station cap: check before touching anything.
        if let Some(entry) = self.stations.get(station_id) {
            if !self.window_expired(entry.window_start, now_secs)
                && entry.count >= self.policy.max_commits_per_station
            {
                let elapsed = now_secs.saturating_sub(entry.window_start).max(0) as u64;
                return Err(SybilRefusal::StationCommitCapExceeded {
                    station_id: station_id.to_string(),
                    max: self.policy.max_commits_per_station,
                    window_secs: self.policy.window_secs,
                    retry_after_secs: self.policy.window_secs.saturating_sub(elapsed).max(1),
                });
            }
        }
        // Identity cap: a station this identity already commits for is free.
        if let Some(entry) = self.identities.get(identity) {
            if !entry.stations.contains(station_id)
                && entry.stations.len() as u32 >= self.policy.max_stations_per_identity
            {
                return Err(SybilRefusal::TooManyStations {
                    identity: identity.to_string(),
                    max: self.policy.max_stations_per_identity,
                });
            }
        }

        // Both checks passed: record.
        let window_secs = self.policy.window_secs as i64;
        let station = self
            .stations
            .entry(station_id.to_string())
            .or_insert(StationEntry { window_start: now_secs, count: 0, last_seen: now_secs });
        if now_secs.saturating_sub(station.window_start) >= window_secs {
            station.window_start = now_secs;
            station.count = 0;
        }
        station.count = station.count.saturating_add(1);
        station.last_seen = now_secs;
        let ident = self
            .identities
            .entry(identity.to_string())
            .or_insert(IdentityEntry { stations: BTreeSet::new(), last_seen: now_secs });
        ident.stations.insert(station_id.to_string());
        ident.last_seen = now_secs;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(stations: u32, commits: u32, window: u64, tracked: usize) -> ContributionSybilPolicy {
        ContributionSybilPolicy {
            max_stations_per_identity: stations,
            max_commits_per_station: commits,
            window_secs: window,
            max_tracked_entries: tracked,
        }
    }

    #[test]
    fn an_identity_is_capped_at_its_station_count_and_refusals_record_nothing() {
        let mut limiter = SybilLimiter::new(policy(2, 100, 60, 1_000));
        limiter.admit_commit("op-a", "st-1", 0).unwrap();
        limiter.admit_commit("op-a", "st-2", 1).unwrap();
        limiter.admit_commit("op-a", "st-1", 2).unwrap();
        let err = limiter.admit_commit("op-a", "st-3", 3).unwrap_err();
        assert_eq!(err, SybilRefusal::TooManyStations { identity: "op-a".into(), max: 2 });
        assert_eq!(err.code(), "SYBIL_STATION_LIMIT");
        assert_eq!(limiter.stations_for("op-a"), 2, "the refused station was not recorded");
        assert_eq!(limiter.commits_in_window("st-3", 3), 0);
        // Another identity is independent.
        limiter.admit_commit("op-b", "st-3", 4).unwrap();
    }

    #[test]
    fn a_station_is_capped_per_window_whoever_submits_for_it() {
        let mut limiter = SybilLimiter::new(policy(10, 3, 60, 1_000));
        limiter.admit_commit("op-a", "st-1", 0).unwrap();
        limiter.admit_commit("op-b", "st-1", 10).unwrap();
        limiter.admit_commit("op-c", "st-1", 20).unwrap();
        let err = limiter.admit_commit("op-d", "st-1", 30).unwrap_err();
        assert_eq!(
            err,
            SybilRefusal::StationCommitCapExceeded {
                station_id: "st-1".into(),
                max: 3,
                window_secs: 60,
                retry_after_secs: 30,
            }
        );
        assert_eq!(err.code(), "SYBIL_STATION_COMMIT_CAP");
        assert_eq!(limiter.stations_for("op-d"), 0, "a refused commit binds no station");
        // The window resets exactly at its length.
        limiter.admit_commit("op-d", "st-1", 60).unwrap();
        assert_eq!(limiter.commits_in_window("st-1", 60), 1);
    }

    /// #2148 review F6: an honest station at the shipped defaults (120 commits
    /// per 3 600 s) is never refused, at one commit a minute for three hours
    /// and at exactly the cap rate (one commit every 30 s) across three windows.
    #[test]
    fn an_honest_station_at_the_shipped_defaults_is_never_refused_2148_f6() {
        let defaults = ContributionSybilPolicy::default();
        assert_eq!(defaults.max_commits_per_station, 120);
        assert_eq!(defaults.window_secs, 3_600);

        let mut limiter = SybilLimiter::new(defaults.clone());
        for minute in 0..=180i64 {
            let now = 1_000_000 + minute * 60;
            if let Err(refusal) = limiter.admit_commit("wrf1honest", "st-honest", now) {
                panic!("1/min honest station refused at minute {minute}: {refusal:?}");
            }
        }

        let mut limiter = SybilLimiter::new(defaults);
        for i in 0..(3 * 120i64) {
            let now = 2_000_000 + i * 30;
            if let Err(refusal) = limiter.admit_commit("wrf1steady", "st-steady", now) {
                panic!("cap-rate station refused at commit {i} (t+{}s): {refusal:?}", i * 30);
            }
        }
    }

    #[test]
    fn state_stays_bounded_under_a_flood_of_identities() {
        let cap = 1_000;
        let mut limiter = SybilLimiter::new(policy(8, 120, 60, cap));
        let mut refused = 0;
        for i in 0..100_000u32 {
            match limiter.admit_commit(&format!("id-{i}"), &format!("st-{i}"), 0) {
                Ok(()) => {},
                Err(SybilRefusal::LimiterCapacity { tracked, max }) => {
                    assert!(tracked <= max);
                    refused += 1;
                },
                Err(other) => panic!("unexpected {other:?}"),
            }
            assert!(limiter.tracked_entries() <= cap);
        }
        assert!(refused > 0, "the flood must have been refused, not tracked");
        // Known keys keep working at capacity; new ones do not, until expiry.
        limiter.admit_commit("id-0", "st-0", 1).unwrap();
        assert!(matches!(
            limiter.admit_commit("id-new", "st-new", 1),
            Err(SybilRefusal::LimiterCapacity { .. })
        ));
        limiter.admit_commit("id-new", "st-new", 61).unwrap();
        assert!(limiter.tracked_entries() <= cap);
    }

    #[test]
    fn zero_or_absurd_policy_values_are_normalised() {
        let p = policy(0, 0, 0, 0).normalised();
        assert_eq!((p.max_stations_per_identity, p.max_commits_per_station, p.window_secs), (1, 1, 1));
        assert_eq!(p.max_tracked_entries, 2);
        let p = policy(1, 1, 1, usize::MAX).normalised();
        assert_eq!(p.max_tracked_entries, ContributionSybilPolicy::MAX_TRACKED_ENTRIES_CEILING);
        let d = ContributionSybilPolicy::default();
        assert_eq!(d, d.clone().normalised(), "defaults are already sane");
    }
}
