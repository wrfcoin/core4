//! #3277 S5, core4#3364 review P3-3: the apply arm rejects EVERY extension
//! while no schedule-extension authority is installed, so every way a node
//! comes to hold the live chain must install it (and the bootstrap), or that
//! node rejects blocks its peers accept after `H_ENABLE` — a config-dependent
//! split.
//!
//! `every_node_path_reaches_the_block1_inputs` ENUMERATES every production
//! function that installs, extends or loads a chain (a fixed list of
//! primitives) and requires each to be in the `SITES` ledger, mapped to the
//! site that gives it the block-1 inputs:
//!
//! | Path | Site |
//! |---|---|
//! | boot with a persisted chain (incl. after a snapshot join + restart) | `launch_api` main → `upgrade::arm_block1_protocol_at_boot` (fail-closed; behaviour in `schedule_extension_boot_3277`) |
//! | fresh node, IBD / live P2P join | `p2p_events::apply_p2p_block_with_persist_control_at` → `upgrade::reresolve_base_schedule_if_block1` |
//! | fresh node, snapshot join (never applies block 1) | `routes/snapshot.rs` `install_verified_snapshot` → the same hook on the carried, pin-verified block 1 |
//! | a node that mines block 1 | `routes/mining.rs` `mine_block` → the same hook |
//! | `replay_verify` | `replay_core` → `install_protocol_from_block1` + the authority installer |
//!
//! A new site anywhere in `native-chain/src` fails the test until it is
//! listed (core4#3368 review P3-1). `the_block1_hook_installs_bootstrap_and_authority_on_a_fresh_node`
//! drives the shared hook exactly as the IBD, snapshot-join and mining paths
//! do, on the live chain's block 1. The snapshot join end to end is
//! `snapshot_tip_carry_3288::a_snapshot_joined_fresh_node_resolves_block1_inputs_3277`
//! in `launch_api`.
//!
//! Own process: the base schedule and the authority are process-wide.

use std::str::FromStr;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE, base_feature_schedule_is_provisional,
    install_uncut_chain_protocol, resolve_uncut_chain_protocol_with_schedule,
    try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    SCHEDULE_EXTENSION_FEATURE, schedule_extension_authority,
};

const LIVE_777_BLOCK1: &str = "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash

#[test]
fn the_block1_hook_installs_bootstrap_and_authority_on_a_fresh_node() {
    // A fresh node boots on an empty volume: its base is provisional
    // (cut-time configuration, here none) and it holds no authority.
    let identity = ChainIdentity::new("777", "testnet");
    let uncut = resolve_uncut_chain_protocol_with_schedule(None, &identity, None).expect("uncut");
    install_uncut_chain_protocol(uncut);
    assert!(base_feature_schedule_is_provisional());
    assert!(schedule_extension_authority().is_none());
    assert_eq!(
        try_feature_schedule().and_then(|base| base.activation_height(SCHEDULE_EXTENSION_FEATURE)),
        None
    );

    // Block 1 of the live chain becomes canonical (applied by IBD, carried by
    // a snapshot, or mined here): the hook re-resolves the base WITH the
    // bootstrap and installs the pinned authority.
    let mut block1 = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    block1
        .metadata
        .insert("protocol_version".to_string(), "v1".to_string());
    block1.set_hash(Hash::from_str(LIVE_777_BLOCK1).expect("hash"));
    wrfcoin_native_chain::upgrade::reresolve_base_schedule_if_block1(&block1);

    assert!(!base_feature_schedule_is_provisional(), "the base now comes from block 1");
    assert_eq!(
        try_feature_schedule().and_then(|base| base.activation_height(SCHEDULE_EXTENSION_FEATURE)),
        Some(H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE),
        "the bootstrap opens the gate at H_ENABLE, as on every booted peer"
    );
    let authority = schedule_extension_authority().expect("the pinned authority is installed");
    assert_eq!(authority.genesis_hash(), LIVE_777_BLOCK1);
    assert_eq!(authority.chain_id(), "777");
    assert_eq!(authority.network_id(), "testnet");
    assert_eq!(authority.allowlist().len(), 1);
}

/// Every function in production `native-chain` code that installs,
/// replaces, extends or loads a canonical chain.
const CHAIN_INSTALL_PRIMITIVES: &[&str] = &[
    "Blockchain::from_persisted(",
    "install_snapshot_state(",
    ".add_block(",
    "execute_reorg(",
    "create_pop_block_with_state(",
    "gate_then_load_state(",
    "load_state_observing_replay(",
    ".load_state(",
    "replay_persisted_chain(",
    "replay_persisted_chain_with_checkpoint_observer(",
];

const HOOK: &str = "reresolve_base_schedule_if_block1(";

/// How one chain-install site reaches the block-1 inputs.
enum Site {
    /// This function itself calls every listed needle.
    Calls(&'static [&'static str]),
    /// Reached only through this other listed `(file, fn)`, which must be
    /// a `Calls` site.
    Via(&'static str, &'static str),
    /// Reviewed: not a way for a node to come to hold a canonical block 1.
    NotANodePath(&'static str),
}

/// THE node-path ledger (core4#3368 review P3-1). A new chain-install site
/// anywhere in production code fails this test until it is added here with
/// the site that gives it the block-1 inputs; a removed site fails it too.
const SITES: &[(&str, &str, Site)] = &[
    // Boot with a persisted chain.
    ("src/bin/launch_api/main.rs", "main", Site::Calls(&["arm_block1_protocol_at_boot("])),
    ("src/storage.rs", "gate_then_load_state", Site::Via("src/bin/launch_api/main.rs", "main")),
    ("src/storage.rs", "load_state", Site::Via("src/bin/launch_api/main.rs", "main")),
    (
        "src/storage.rs",
        "load_state_observing_replay",
        Site::Via("src/bin/launch_api/main.rs", "main"),
    ),
    // IBD / live P2P join.
    ("src/bin/launch_api/p2p_events.rs", "apply_p2p_block_with_persist_control_at", Site::Calls(&[HOOK])),
    // A node that mines a block (block 1 on a fresh cut).
    ("src/bin/launch_api/routes/mining.rs", "mine_block", Site::Calls(&[HOOK])),
    // Snapshot join.
    ("src/bin/launch_api/routes/snapshot.rs", "install_verified_snapshot", Site::Calls(&[HOOK])),
    (
        "src/bin/launch_api/routes/snapshot.rs",
        "install_snapshot_state",
        Site::Via("src/bin/launch_api/routes/snapshot.rs", "install_verified_snapshot"),
    ),
    // replay_verify.
    (
        "src/bin/replay_verify/replay_core.rs",
        "replay_persisted_chain_observed_and_checkpoints",
        Site::Calls(&[
            "install_protocol_from_block1(",
            "install_schedule_extension_authority_for_block1(",
        ]),
    ),
    (
        "src/bin/replay_verify/replay_core.rs",
        "replay_persisted_chain",
        Site::Via("src/bin/replay_verify/replay_core.rs", "replay_persisted_chain_observed_and_checkpoints"),
    ),
    (
        "src/bin/replay_verify/main.rs",
        "main",
        Site::Via("src/bin/replay_verify/replay_core.rs", "replay_persisted_chain_observed_and_checkpoints"),
    ),
    // Reviewed, not node paths.
    (
        "src/bin/launch_api/p2p_events.rs",
        "try_reorg_inner",
        Site::NotANodePath(
            "REFUSAL SITE (core4#3370, #3373): try_reorg_inner returns before installing any \
             chain when `plan.replaces_block_one()`, so a genesis reorg never swaps an armed \
             block 1 under a running node (the operator wipes and rejoins by snapshot join or \
             IBD, both of which arm block 1). A reorg forking at height >= 1 keeps block 1, \
             and one below the finalized height is refused by plan_reorg_with_loader (#1216). \
             Pinned by genesis_reorg_replacing_an_armed_block_one_is_refused_3370",
        ),
    ),
    (
        "src/legacy/proof_of_publishing.rs",
        "create_next_block",
        Site::NotANodePath("legacy producer: no production caller (pinned below)"),
    ),
];

fn production_source(path: &str) -> String {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    // Cut at the first test MODULE (some files carry #[cfg(test)] items
    // earlier, e.g. snapshot.rs's test genesis pin and main.rs's helpers).
    let end = text.find("#[cfg(test)]\nmod ").unwrap_or(text.len());
    text[..end].to_string()
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

fn fn_name(line: &str) -> Option<&str> {
    if is_comment(line) {
        return None;
    }
    let at = line.find("fn ")?;
    if at > 0 && !line[..at].ends_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let rest = &line[at + 3..];
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    (end > 0).then(|| &rest[..end])
}

/// Every production `.rs` file of this crate, relative to it.
fn production_files() -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(&root.join("src"), &mut files);
    files
        .iter()
        .map(|f| f.strip_prefix(root).expect("under the crate").to_string_lossy().replace('\\', "/"))
        .collect()
}

/// Every `(file, enclosing fn)` whose production code contains a primitive.
fn chain_install_sites() -> std::collections::BTreeSet<(String, String)> {
    let mut sites = std::collections::BTreeSet::new();
    for rel in production_files() {
        let mut current = String::from("<module>");
        for line in production_source(&rel).lines() {
            if let Some(name) = fn_name(line) {
                current = name.to_string();
            }
            if !is_comment(line) && CHAIN_INSTALL_PRIMITIVES.iter().any(|p| line.contains(p)) {
                sites.insert((rel.clone(), current.clone()));
            }
        }
    }
    sites
}

/// The body of `fn name` in `path`: from its signature to the next fn at the
/// same or a shallower indentation.
fn fn_body(path: &str, name: &str) -> String {
    let text = production_source(path);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| fn_name(l) == Some(name))
        .unwrap_or_else(|| panic!("{path}: fn {name} not found"));
    let indent = lines[start].len() - lines[start].trim_start().len();
    let end = lines[start + 1..]
        .iter()
        .position(|l| fn_name(l).is_some() && l.len() - l.trim_start().len() <= indent)
        .map_or(lines.len(), |i| start + 1 + i);
    lines[start..end].iter().filter(|l| !is_comment(l)).cloned().collect::<Vec<_>>().join("\n")
}

#[test]
fn every_replay_wrapper_reaches_the_shared_armed_loop_3317() {
    let path = "src/bin/replay_verify/replay_core.rs";
    for (name, target) in [
        ("replay_persisted_chain", "replay_persisted_chain_observed("),
        (
            "replay_persisted_chain_observed",
            "replay_persisted_chain_observed_and_checkpoints(",
        ),
        (
            "replay_persisted_chain_with_checkpoint_observer",
            "replay_persisted_chain_observed_and_checkpoints(",
        ),
    ] {
        assert!(fn_body(path, name).contains(target), "{name} must call {target}");
    }
    assert!(
        fn_body("src/bin/replay_verify/main.rs", "main")
            .contains("replay_persisted_chain_with_checkpoint_observer("),
        "the shipped CLI must use the checkpoint observer and shared armed loop"
    );
}

#[test]
fn every_node_path_reaches_the_block1_inputs() {
    let found = chain_install_sites();
    let listed: std::collections::BTreeSet<(String, String)> =
        SITES.iter().map(|(f, n, _)| (f.to_string(), n.to_string())).collect();
    let unlisted: Vec<_> = found.difference(&listed).collect();
    assert!(
        unlisted.is_empty(),
        "new production chain-install site(s) {unlisted:?}: every way a node comes to hold \
         the chain must reach the block-1 inputs (bootstrap + schedule-extension authority), \
         or that node rejects blocks its peers accept after H_ENABLE. Add each to SITES with \
         its hook site (#3277 S5 / core4#3368 P3-1)."
    );
    let stale: Vec<_> = listed.difference(&found).collect();
    assert!(stale.is_empty(), "SITES lists sites that no longer install a chain: {stale:?}");

    for (file, name, site) in SITES {
        match site {
            Site::Calls(needles) => {
                let body = fn_body(file, name);
                for needle in *needles {
                    assert!(body.contains(needle), "{file}::{name} must call {needle}");
                }
            },
            Site::Via(target_file, target_fn) => assert!(
                SITES.iter().any(|(f, n, s)| f == target_file
                    && n == target_fn
                    && matches!(s, Site::Calls(_))),
                "{file}::{name} is reached via {target_file}::{target_fn}, which must be a \
                 Calls site"
            ),
            Site::NotANodePath(reason) => assert!(!reason.is_empty(), "{file}::{name}: a reviewed non-node path must say why"),
        }
    }

    // The legacy producer really has no production caller.
    for file in production_files() {
        if file != "src/legacy/proof_of_publishing.rs" {
            assert!(
                !production_source(&file)
                    .lines()
                    .any(|l| !is_comment(l) && l.contains("create_next_block(")),
                "{file} calls the legacy producer: list it as a node path"
            );
        }
    }

    // The hook installs the authority before its provisional check.
    let hook = fn_body("src/upgrade.rs", "reresolve_base_schedule_if_block1");
    let install = hook.find("install_schedule_extension_authority_for_block1(").expect("hook installs");
    let provisional = hook.find("base_feature_schedule_is_provisional()").expect("provisional check");
    assert!(install < provisional, "the authority must not depend on the base being provisional");
    // The snapshot join takes the hook BEFORE installing the state, so the
    // install's schedule refresh already sees the re-resolved base.
    let join = fn_body("src/bin/launch_api/routes/snapshot.rs", "install_verified_snapshot");
    let hook_at = join.find(HOOK).expect("hook call");
    let state_at = join.find("install_snapshot_state(&mut chain").expect("state install");
    assert!(hook_at < state_at);
    // Boot: one fail-closed function arms every block-1 input.
    let boot = fn_body("src/upgrade.rs", "arm_block1_protocol_from_lowest_block");
    for needle in [
        "assert_startup_protocol_compatibility_for_block1(",
        "Some(&hash)",
        "install_schedule_extension_authority_for_block1(",
    ] {
        assert!(boot.contains(needle), "the boot gate must call {needle}");
    }
}
