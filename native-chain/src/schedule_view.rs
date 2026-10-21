//! Read-only views behind `GET /network/upgrade` (#3277 S6, #3361 F4).
//!
//! - [`build_scheduled_view`]: the EFFECTIVE feature schedule this node runs —
//!   the base (genesis, plus from S5 the per-chain bootstrap) and every applied
//!   `feature_schedule_extensions` entry — with where each entry came from,
//!   whether this binary implements it, and a run-up warning for every entry
//!   this binary cannot follow that is inside the #3095 run-up window. The
//!   window and severities are [`crate::upgrade_notice::run_up_severity`], the
//!   rule the signed-notice run-up already uses; the clean-halt height is the
//!   #3093 deadline ([`crate::upgrade::upgrade_deadline`]) that
//!   `refresh_schedule_from_canonical_state` already installs for such an
//!   entry. Nothing here installs, tightens or logs anything.
//! - [`describe_faucet_sink`]: the ADR-003 faucet sink in force at a height,
//!   and why — the genesis-recorded address or a committed #3361 binding row.
//!
//! Both are pure: the route passes the process-wide schedule and the
//! canonical `State`, so a node that restarted from a checkpoint or joined
//! from a snapshot reports exactly what a node that produced the chain does.
//! Neither carries secrets: feature names, heights, sequence numbers and a
//! public faucet ADDRESS only (no signer keys, no release artifacts beyond
//! what the signed notice already publishes, no operator tokens).

use serde::{Deserialize, Serialize};

use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GenesisEmissionConfig, LegacyFaucetBinding, SigningProtocolVersion,
    parse_genesis_activation_schedule, resolve_faucet_sink,
};
use wrfcoin_shared_protocol::governance::schedule_extension::AppliedExtension;

use crate::upgrade::UpgradeDeadline;
use crate::upgrade_notice::{RunUpLevel, run_up_severity};

/// Most schedule entries (and warnings) one response carries. The base is a
/// handful of names and each extension at most
/// `MAX_ENTRIES_PER_EXTENSION`; the cap only bounds the response on a chain
/// that has applied a very long extension history. When it bites, the
/// highest activations (the pending end, which is what an operator acts on)
/// are kept and `truncated` is set.
pub const MAX_SCHEDULED_VIEW_ENTRIES: usize = 256;

/// Where an effective schedule entry came from.
pub const SOURCE_GENESIS: &str = "genesis";
/// A base entry that the genesis schedule string does not carry: the per-chain
/// `LEGACY_CHAIN_BOOTSTRAP` (#3277 S5). Derived, not looked up, so the view
/// needs no change when S5 lands.
pub const SOURCE_BOOTSTRAP: &str = "bootstrap";
/// An entry carried by an applied governance-signed extension.
pub const SOURCE_EXTENSION: &str = "extension";

/// One entry of the effective feature schedule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledFeature {
    pub feature: String,
    /// The FIRST height at which the feature applies.
    pub activation_height: u64,
    /// `genesis` | `bootstrap` | `extension`.
    pub source: String,
    /// The applied extension's `sequence`, for `source == "extension"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_sequence: Option<u64>,
    /// Height of the block that carried that extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carried_at_height: Option<u64>,
    /// Whether this binary implements the feature (`KNOWN_FEATURES`).
    pub known: bool,
    /// Whether the feature is in force for the next block.
    pub active_at_next: bool,
    /// `activation_height - tip_height`, saturating at zero.
    pub blocks_remaining: u64,
}

/// A run-up warning: a scheduled activation this binary cannot follow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRunUpWarning {
    pub feature: String,
    pub activation_height: u64,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_sequence: Option<u64>,
    pub blocks_remaining: u64,
    /// `warn` (inside `RUN_UP_BLOCKS`) | `error` (inside `URGENT_BLOCKS`, or
    /// at/past the activation).
    pub level: String,
    pub message: String,
}

/// The #3093 clean-halt deadline currently installed on this node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HaltDeadlineView {
    pub feature: String,
    /// First height this node will not produce, apply or relay.
    pub height: u64,
}

/// The `scheduled` object of `GET /network/upgrade`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledView {
    /// `ready` (every entry known) | `upgrade_required` (an unknown entry is
    /// pending) | `past_deadline` (an unknown entry is already in force).
    pub status: String,
    /// The height the next block will carry (`tip + 1`).
    pub next_height: u64,
    /// The signing protocol version in force at `next_height` — what a client
    /// signing a transaction for the next block must use.
    pub version_at_next: u32,
    /// The base schedule was resolved from cut-time configuration and still
    /// awaits re-resolution from the synced block 1 (#3277 F4).
    pub base_provisional: bool,
    /// Every effective entry, sorted by activation height then name.
    pub entries: Vec<ScheduledFeature>,
    /// Total effective entries before the [`MAX_SCHEDULED_VIEW_ENTRIES`] cap.
    pub total_entries: usize,
    pub truncated: bool,
    /// Unknown entries inside the run-up window (or already past it).
    pub warnings: Vec<ScheduleRunUpWarning>,
    /// The installed #3093 deadline, from whichever source set it (genesis,
    /// a signed notice, or an applied extension).
    pub halt_deadline: Option<HaltDeadlineView>,
}

/// Inputs to [`build_scheduled_view`], so the route and the tests pass the
/// same things.
pub struct ScheduleViewInputs<'a> {
    /// The installed BASE schedule entries (`State::base_feature_schedule`).
    pub base: &'a [(String, u64)],
    /// The raw genesis activation-schedule string, if genesis carries one.
    pub genesis_schedule_raw: Option<&'a str>,
    /// The canonical state's applied extensions.
    pub extensions: &'a [AppliedExtension],
    pub tip_height: u64,
    /// The names this binary implements (`KNOWN_FEATURES`).
    pub known: &'a [&'a str],
    pub local_protocol_version: u32,
    pub version_at_next: u32,
    pub base_provisional: bool,
    pub halt_deadline: Option<UpgradeDeadline>,
}

/// The names the genesis schedule string itself schedules, including
/// `signing_v2` when it pins a v2 step (the base derives that entry from the
/// version table). A string that does not parse contributes nothing — the
/// node would not have booted on it, and the view must never panic.
pub fn genesis_schedule_names(raw: Option<&str>) -> Vec<String> {
    let Some(parsed) = raw.and_then(|raw| parse_genesis_activation_schedule(raw).ok()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = parsed.features.into_iter().map(|(name, _)| name).collect();
    if parsed
        .versions
        .iter()
        .any(|(_, version)| *version >= SigningProtocolVersion::V2)
    {
        names.push(ConsensusFeature::SigningV2.name().to_string());
    }
    names
}

/// The operator line for an unfollowable scheduled activation — the
/// schedule-side twin of [`crate::upgrade_notice::run_up_message`].
pub fn schedule_run_up_message(
    entry: &ScheduledFeature,
    tip_height: u64,
    local_protocol_version: u32,
) -> String {
    let origin = match entry.extension_sequence {
        Some(seq) => format!("{} seq {seq}", entry.source),
        None => entry.source.clone(),
    };
    if tip_height >= entry.activation_height {
        format!(
            "UPGRADE REQUIRED: feature {:?} ({origin}) has been in force since height {}; this \
             binary (protocol v{local_protocol_version}) does not implement it and must not \
             produce, apply or relay — install the release that does (#3093/#3277)",
            entry.feature, entry.activation_height
        )
    } else {
        format!(
            "UPGRADE REQUIRED by height {} (in {} blocks): the effective feature schedule \
             activates {:?} ({origin}), which this binary (protocol v{local_protocol_version}) \
             does not implement; it will stop cleanly at height {} — see /network/upgrade",
            entry.activation_height,
            entry.blocks_remaining,
            entry.feature,
            entry.activation_height.saturating_sub(1)
        )
    }
}

/// Build the `scheduled` view. Pure and total.
///
/// The entry set mirrors `effective_feature_schedule`: every base entry, then
/// every extension entry whose name the base (or an earlier extension) does
/// not already carry — V6 makes that set disjoint, and a repeat is ignored
/// rather than allowed to move an activation.
pub fn build_scheduled_view(inputs: &ScheduleViewInputs<'_>) -> ScheduledView {
    let next_height = inputs.tip_height.saturating_add(1);
    let genesis = genesis_schedule_names(inputs.genesis_schedule_raw);
    let known = |name: &str| inputs.known.contains(&name);
    let make = |feature: &str,
                activation_height: u64,
                source: &str,
                extension: Option<&AppliedExtension>| ScheduledFeature {
        feature: feature.to_string(),
        activation_height,
        source: source.to_string(),
        extension_sequence: extension.map(|e| e.sequence),
        carried_at_height: extension.map(|e| e.carried_at_height),
        known: known(feature),
        active_at_next: next_height >= activation_height,
        blocks_remaining: activation_height.saturating_sub(inputs.tip_height),
    };

    let mut entries: Vec<ScheduledFeature> = inputs
        .base
        .iter()
        .map(|(name, height)| {
            let source = if genesis.iter().any(|g| g == name) {
                SOURCE_GENESIS
            } else {
                SOURCE_BOOTSTRAP
            };
            make(name, *height, source, None)
        })
        .collect();
    for applied in inputs.extensions {
        for entry in &applied.entries {
            if entries.iter().any(|e| e.feature == entry.feature) {
                continue;
            }
            entries.push(make(
                &entry.feature,
                entry.activation_height,
                SOURCE_EXTENSION,
                Some(applied),
            ));
        }
    }
    entries.sort_by(|a, b| {
        a.activation_height
            .cmp(&b.activation_height)
            .then_with(|| a.feature.cmp(&b.feature))
    });

    let status = match entries.iter().find(|e| !e.known) {
        None => "ready",
        Some(first) if inputs.tip_height >= first.activation_height => "past_deadline",
        Some(_) => "upgrade_required",
    };

    let mut warnings: Vec<ScheduleRunUpWarning> = entries
        .iter()
        .filter(|e| !e.known)
        .filter_map(|e| {
            let level = match run_up_severity(e.activation_height, inputs.tip_height)? {
                RunUpLevel::Error => "error",
                RunUpLevel::Warn => "warn",
                RunUpLevel::Satisfied => return None,
            };
            Some(ScheduleRunUpWarning {
                feature: e.feature.clone(),
                activation_height: e.activation_height,
                source: e.source.clone(),
                extension_sequence: e.extension_sequence,
                blocks_remaining: e.blocks_remaining,
                level: level.to_string(),
                message: schedule_run_up_message(
                    e,
                    inputs.tip_height,
                    inputs.local_protocol_version,
                ),
            })
        })
        .collect();

    let total_entries = entries.len();
    let truncated = total_entries > MAX_SCHEDULED_VIEW_ENTRIES;
    if truncated {
        entries.drain(..total_entries - MAX_SCHEDULED_VIEW_ENTRIES);
    }
    if warnings.len() > MAX_SCHEDULED_VIEW_ENTRIES {
        // Warnings are sorted ascending too; the most urgent come first.
        warnings.truncate(MAX_SCHEDULED_VIEW_ENTRIES);
    }

    ScheduledView {
        status: status.to_string(),
        next_height,
        version_at_next: inputs.version_at_next,
        base_provisional: inputs.base_provisional,
        entries,
        total_entries,
        truncated,
        warnings,
        halt_deadline: inputs.halt_deadline.as_ref().map(|d| HaltDeadlineView {
            feature: d.feature.clone(),
            height: d.height,
        }),
    }
}

/// Where the faucet sink comes from.
pub const FAUCET_SOURCE_GENESIS: &str = "genesis";
/// A committed #3361 binding row whose feature is in force.
pub const FAUCET_SOURCE_BINDING: &str = "binding";
/// No sink: genesis recorded none and no binding row is in force (or the
/// state carries no genesis emission config at all). The slice mints zero.
pub const FAUCET_SOURCE_NONE: &str = "none";

/// The `faucet_sink` object of `GET /network/upgrade` (#3361 F4).
///
/// Note for readers of block metadata: `faucet_rewards_minted_total` is a
/// PER-BLOCK value (the faucet slice minted in that block), not a running
/// total, despite its name. The key is not renamed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaucetSinkView {
    /// The sink address (`0x…`), or `null` when the slice mints nothing —
    /// including an explicit unbind row.
    pub address: Option<String>,
    /// `genesis` | `binding` | `none`.
    pub source: String,
    /// First height this sink applies from: `1` for a genesis address, the
    /// binding feature's effective activation for a row, `null` for `none`.
    pub active_from: Option<u64>,
    /// The height the answer is for (the next block).
    pub at_height: u64,
}

/// Describe the faucet sink in force at `height`. The ADDRESS is always
/// `resolve_faucet_sink` — the function `State::effective_faucet_address`
/// (and so `ApplyFaucetReward`) calls — so the view cannot disagree with
/// consensus; this only adds which rule produced it and since when.
/// `activation_of` answers a feature name's effective activation height.
pub fn describe_faucet_sink(
    config: Option<&GenesisEmissionConfig>,
    height: u64,
    bindings: &[LegacyFaucetBinding],
    is_active: impl Fn(ConsensusFeature, u64) -> bool,
    activation_of: impl Fn(&str) -> Option<u64>,
) -> FaucetSinkView {
    let address = resolve_faucet_sink(config, height, bindings, &is_active);
    let (source, active_from) = match config {
        Some(config) if config.faucet_address.is_some() => (FAUCET_SOURCE_GENESIS, Some(1)),
        Some(config) => match bindings
            .iter()
            .rev()
            .filter(|binding| binding.genesis_chain_id == config.chain_id)
            .find(|binding| is_active(binding.feature, height))
        {
            Some(row) => (FAUCET_SOURCE_BINDING, activation_of(row.feature.name())),
            None => (FAUCET_SOURCE_NONE, None),
        },
        None => (FAUCET_SOURCE_NONE, None),
    };
    FaucetSinkView {
        address: address.map(|a| a.to_string()),
        source: source.to_string(),
        active_from,
        at_height: height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::common::types::Address;
    use wrfcoin_shared_protocol::governance::schedule_extension::ScheduleEntry;

    const KNOWN: &[&str] = &["paymaster", "session_keys", "schedule_extension"];

    fn applied(sequence: u64, carried: u64, entries: &[(&str, u64)]) -> AppliedExtension {
        AppliedExtension {
            sequence,
            carried_at_height: carried,
            entries: entries
                .iter()
                .map(|(n, h)| ScheduleEntry::new(*n, *h))
                .collect(),
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: "c0ffee".into(),
            signers: vec!["ab".repeat(32)],
        }
    }

    fn inputs<'a>(
        base: &'a [(String, u64)],
        raw: Option<&'a str>,
        extensions: &'a [AppliedExtension],
        tip: u64,
    ) -> ScheduleViewInputs<'a> {
        ScheduleViewInputs {
            base,
            genesis_schedule_raw: raw,
            extensions,
            tip_height: tip,
            known: KNOWN,
            local_protocol_version: 2,
            version_at_next: 1,
            base_provisional: false,
            halt_deadline: None,
        }
    }

    #[test]
    fn sources_are_genesis_bootstrap_and_extension_seq() {
        // `schedule_extension` is in the base but not in the genesis string:
        // that is exactly what an S5 bootstrap row looks like.
        let base = vec![
            ("paymaster".to_string(), 10),
            ("schedule_extension".to_string(), 20),
        ];
        let exts = vec![
            applied(0, 30, &[("session_keys", 5_000)]),
            applied(1, 40, &[("unknown_x", 9_000)]),
        ];
        let view = build_scheduled_view(&inputs(&base, Some("0:v1,10:paymaster"), &exts, 100));
        let by = |f: &str| {
            view.entries
                .iter()
                .find(|e| e.feature == f)
                .unwrap()
                .clone()
        };
        assert_eq!(by("paymaster").source, "genesis");
        assert_eq!(by("schedule_extension").source, "bootstrap");
        let sk = by("session_keys");
        assert_eq!( // secret-leak-ok: `sk` is a test ScheduleEntry for the "session_keys" feature (source/sequence/height metadata), not key material
            (
                sk.source.as_str(),
                sk.extension_sequence,
                sk.carried_at_height
            ),
            ("extension", Some(0), Some(30))
        );
        assert!(sk.known && !sk.active_at_next && sk.blocks_remaining == 4_900); // secret-leak-ok: `sk` is a test ScheduleEntry for the "session_keys" feature (bool/count metadata), not key material
        let ux = by("unknown_x");
        assert_eq!((ux.extension_sequence, ux.known), (Some(1), false));
        assert!(by("paymaster").active_at_next);
        assert_eq!(view.status, "upgrade_required");
        // 8_900 blocks out: outside the run-up window, no warning yet.
        assert!(view.warnings.is_empty());
        let heights: Vec<u64> = view.entries.iter().map(|e| e.activation_height).collect();
        assert_eq!(heights, vec![10, 20, 5_000, 9_000]);
    }

    #[test]
    fn signing_v2_derived_from_a_genesis_version_step_is_genesis() {
        let base = vec![("signing_v2".to_string(), 50)];
        let view = build_scheduled_view(&inputs(&base, Some("0:v1,50:v2"), &[], 1));
        assert_eq!(view.entries[0].source, "genesis");
    }

    #[test]
    fn run_up_warnings_follow_the_notice_window_for_unknown_entries_only() {
        use crate::upgrade_notice::{RUN_UP_BLOCKS, URGENT_BLOCKS};
        let h = 10_000;
        let exts = vec![applied(0, 1, &[("unknown_x", h), ("session_keys", h)])];
        let at = |tip| build_scheduled_view(&inputs(&[], None, &exts, tip));
        assert!(
            at(h - RUN_UP_BLOCKS - 1).warnings.is_empty(),
            "outside the window"
        );
        let warn = at(h - RUN_UP_BLOCKS);
        assert_eq!(warn.warnings.len(), 1, "known session_keys never warns");
        assert_eq!(warn.warnings[0].level, "warn");
        assert_eq!(warn.warnings[0].extension_sequence, Some(0));
        assert!(warn.warnings[0].message.contains("\"unknown_x\""));
        assert!(warn.warnings[0].message.contains("extension seq 0"));
        assert!(warn.warnings[0].message.contains("/network/upgrade"));
        assert_eq!(at(h - URGENT_BLOCKS).warnings[0].level, "error");
        let past = at(h);
        assert_eq!(past.warnings[0].level, "error");
        assert_eq!(past.status, "past_deadline");
        // Every tip, not only cadence ticks: the view is not a log line.
        assert_eq!(at(h - RUN_UP_BLOCKS + 1).warnings.len(), 1);
    }

    #[test]
    fn a_repeated_name_never_moves_an_activation() {
        let base = vec![("paymaster".to_string(), 10)];
        let exts = vec![applied(0, 1, &[("paymaster", 9_999)])];
        let view = build_scheduled_view(&inputs(&base, Some("10:paymaster"), &exts, 1));
        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.entries[0].activation_height, 10);
    }

    #[test]
    fn output_is_bounded_and_keeps_the_pending_end() {
        let exts: Vec<AppliedExtension> = (0..(MAX_SCHEDULED_VIEW_ENTRIES as u64 + 40))
            .map(|i| applied(i, 1, &[(&format!("unknown_{i:04}"), 3_000 + i)]))
            .collect();
        let view = build_scheduled_view(&inputs(&[], None, &exts, 2_900));
        assert!(view.truncated);
        assert_eq!(view.total_entries, MAX_SCHEDULED_VIEW_ENTRIES + 40);
        assert_eq!(view.entries.len(), MAX_SCHEDULED_VIEW_ENTRIES);
        assert_eq!(
            view.entries.last().unwrap().extension_sequence,
            Some(MAX_SCHEDULED_VIEW_ENTRIES as u64 + 39)
        );
        assert!(view.warnings.len() <= MAX_SCHEDULED_VIEW_ENTRIES);
        assert_eq!(view.warnings[0].feature, "unknown_0000", "most urgent kept");
    }

    #[test]
    fn the_run_up_level_refactor_keeps_the_notice_cadence() {
        use crate::upgrade_notice::{RUN_UP_INTERVAL, run_up_level};
        use std::collections::BTreeMap;
        use wrfcoin_shared_protocol::governance::upgrade_notice::UpgradeNotice;
        let notice = UpgradeNotice {
            notice_id: "n".into(),
            chain_id: "c".into(),
            network_id: "t".into(),
            activation_height: 10_000,
            min_protocol_version: 99,
            features: vec![],
            release_commit: "r".into(),
            image_digests: BTreeMap::new(),
            issued_at: 0,
            expires_at: None,
        };
        let tick = 10_000 - 1_000;
        assert_eq!(tick % RUN_UP_INTERVAL, 0);
        assert_eq!(run_up_level(&notice, tick, 1), Some(RunUpLevel::Warn));
        assert_eq!(
            run_up_level(&notice, tick + 1, 1),
            None,
            "cadence still applies to logs"
        );
        assert_eq!(run_up_level(&notice, 9_950, 1), Some(RunUpLevel::Error));
        assert_eq!(run_up_level(&notice, 5, 99), Some(RunUpLevel::Satisfied));
    }

    fn row(
        feature: ConsensusFeature,
        chain: &'static str,
        address: Option<&'static str>,
    ) -> LegacyFaucetBinding {
        LegacyFaucetBinding {
            feature,
            genesis_chain_id: chain,
            genesis_hash: "00",
            address,
        }
    }

    #[test]
    fn faucet_sink_sources_and_parity_with_the_consensus_resolver() {
        const A: &str = "f72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"; // pragma: allowlist secret -- public faucet ADDRESS fixture
        let rows = [row(ConsensusFeature::FaucetSinkBinding, "777", Some(A))];
        let h_f = 5_000;
        let active =
            |f: ConsensusFeature, h: u64| f == ConsensusFeature::FaucetSinkBinding && h >= h_f;
        let activation = |n: &str| (n == "faucet_sink_binding").then_some(h_f);
        let unbound = GenesisEmissionConfig {
            chain_id: "777".into(),
            faucet_address: None,
         ..Default::default() };
        let genesis_addr = Address([0x11; 20]);
        let bound = GenesisEmissionConfig {
            chain_id: "777".into(),
            faucet_address: Some(genesis_addr),
         ..Default::default() };
        let other = GenesisEmissionConfig {
            chain_id: "888".into(),
            faucet_address: None,
         ..Default::default() };

        for (config, height) in [
            (Some(&unbound), h_f - 1),
            (Some(&unbound), h_f),
            (Some(&bound), 3),
            (Some(&other), h_f),
            (None, h_f),
        ] {
            let view = describe_faucet_sink(config, height, &rows, active, activation);
            let consensus = resolve_faucet_sink(config, height, &rows, active);
            assert_eq!(
                view.address,
                consensus.map(|a| a.to_string()),
                "{config:?}@{height}"
            );
            assert_eq!(view.at_height, height);
        }
        let before = describe_faucet_sink(Some(&unbound), h_f - 1, &rows, active, activation);
        assert_eq!(
            (before.source.as_str(), before.active_from, before.address),
            ("none", None, None)
        );
        let after = describe_faucet_sink(Some(&unbound), h_f, &rows, active, activation);
        assert_eq!(after.source, "binding");
        assert_eq!(after.active_from, Some(h_f));
        assert_eq!(after.address.as_deref(), Some(&*format!("0x{A}")));
        let g = describe_faucet_sink(Some(&bound), h_f, &rows, active, activation);
        assert_eq!((g.source.as_str(), g.active_from), ("genesis", Some(1)));
        assert_eq!(g.address, Some(genesis_addr.to_string()));

        // An unbind row is a binding source with a null address, never a
        // fall-through to an older row.
        let rows2 = [
            row(ConsensusFeature::FaucetSinkBinding, "777", Some(A)),
            row(ConsensusFeature::SessionKeys, "777", None),
        ];
        let all = |_: ConsensusFeature, _: u64| true;
        let unbind = describe_faucet_sink(Some(&unbound), 1, &rows2, all, |_| Some(7));
        assert_eq!(
            (unbind.source.as_str(), unbind.address, unbind.active_from),
            ("binding", None, Some(7))
        );
        assert_eq!(resolve_faucet_sink(Some(&unbound), 1, &rows2, all), None);
    }
}
