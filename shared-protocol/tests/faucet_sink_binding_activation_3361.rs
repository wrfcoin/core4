//! #3361 option A (F1): `ConsensusFeature::FaucetSinkBinding`, scheduled at
//! `H_F` by a #3277 schedule extension, points the ADR-003 faucet slice of the
//! live 777 chain — whose genesis recorded no `faucet_address` — at the owner's
//! faucet wallet from exactly `H_F`, through the REAL `ApplyFaucetReward` arm
//! that producer, P2P applier and WAL replay all apply:
//!   - below `H_F` the slice stays unminted, exactly as today;
//!   - from `H_F` the wallet is credited exactly the faucet slice, the ledger is
//!     charged, the account is created and the whole credit is spendable;
//!   - a genesis-recorded sink always wins over an active binding;
//!   - a chain that is not 777 is never paid by the 777 row;
//!   - the mint never exceeds the slice nor the `MAX_SUPPLY` headroom, at a
//!     reduction boundary and where the emission is zero.
//!
//! Own process: the extension overlay `feature_active` reads is process-wide,
//! and every test here installs the SAME overlay, so they cannot disagree.

use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    EmissionPartition, EmissionRole, MAX_SUPPLY, adjusted_block_reward, base_block_reward,
    coinbase_allowance,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GenesisEmissionConfig, LEGACY_FAUCET_BINDINGS,
    refresh_feature_schedule_extensions,
};

/// The faucet wallet the owner bound on 2026-09-28 (#3361 Q4), written out
/// independently of the production table so a drifted row fails here.
const LIVE_FAUCET: &str = "0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"; // pragma: allowlist secret -- public faucet wallet ADDRESS (not key material), ruled on core4#3361
const BINDING: &str = "faucet_sink_binding";
const H_F: u64 = 3;
const PRODUCER: Address = Address([0x41; 20]);

fn schedule_binding() {
    refresh_feature_schedule_extensions(vec![(BINDING.to_string(), H_F)]);
}

fn live_faucet() -> Address {
    Address::from_hex(LIVE_FAUCET).unwrap()
}

fn balance(state: &State, address: &Address) -> u128 {
    state.get_account(address).map(|a| a.balance.into_inner()).unwrap_or(0)
}

fn slice(chain_id: &str, height: u64) -> u128 {
    EmissionPartition::at_height(height, Some(chain_id)).slice(EmissionRole::Faucet)
}

fn genesis(chain_id: &str, sink: Option<Address>) -> State {
    let mut state = State::new();
    state
        .apply_transitions_batch_at_height(1, vec![StateTransition::RecordGenesisEmissionConfig {
            config: GenesisEmissionConfig { chain_id: chain_id.into(), faucet_address: sink , ..Default::default() },
        }])
        .unwrap();
    state
}

/// One post-genesis block as every surface applies it: coinbase, then
/// `ApplyFaucetReward`.
fn apply_block(state: &mut State, chain_id: &str, height: u64) {
    state
        .apply_transitions_batch_at_height(height, vec![
            StateTransition::ApplyReward {
                address: PRODUCER,
                amount: Balance::new(coinbase_allowance(height, Some(chain_id))),
                reason: "coinbase".into(),
            },
            StateTransition::ApplyFaucetReward,
        ])
        .unwrap();
}

/// RED on main: nothing is ever minted to the live faucet.
#[test]
fn scheduled_binding_mints_the_live_faucet_exactly_its_slice_from_activation() {
    schedule_binding();
    let live = live_faucet();
    let mut state = genesis("777", None);
    let mut credited = 0u128;
    for height in 2..=H_F + 3 {
        let supply_before = state.total_minted_supply();
        apply_block(&mut state, "777", height);
        let slice = slice("777", height);
        assert!(slice > 0, "a 777 genesis carves a faucet slice at {height}");
        let minted = state.total_minted_supply() - supply_before;
        let coinbase = coinbase_allowance(height, Some("777"));
        if height >= H_F {
            credited += slice;
            assert_eq!(state.emission_ledger.spent(EmissionRole::Faucet), slice, "height {height}");
            assert_eq!(minted, coinbase + slice, "height {height}");
        } else {
            assert_eq!(state.emission_ledger.spent(EmissionRole::Faucet), 0, "height {height}");
            assert_eq!(minted, coinbase, "height {height}");
            assert!(state.get_account(&live).is_none(), "no account before H_F ({height})");
        }
        assert_eq!(balance(&state, &live), credited, "height {height}");
    }
    let account = state.get_account(&live).unwrap();
    assert_eq!(account.spendable_balance.into_inner(), credited, "no stake is required of the faucet");
    assert_eq!(account.stake_locked.into_inner(), 0);
    assert_eq!(account.earned_balance_total.into_inner(), credited);

    // Replaying the same height again credits nothing twice.
    let root = state.state_root;
    state
        .apply_transitions_batch_at_height(H_F + 3, vec![StateTransition::ApplyFaucetReward])
        .unwrap();
    assert_eq!(state.state_root, root, "the slice is once per block");
}

/// Genesis always wins: a chain that recorded a faucet sink at block 1 keeps
/// paying it, and never the bound wallet, even with the binding active.
#[test]
fn a_genesis_sink_wins_over_an_active_binding() {
    schedule_binding();
    let sink = Address([0x39; 20]);
    let mut state = genesis("777", Some(sink));
    let mut credited = 0u128;
    for height in 2..=H_F + 2 {
        apply_block(&mut state, "777", height);
        credited += slice("777", height);
        assert_eq!(balance(&state, &sink), credited, "height {height}");
        assert!(state.get_account(&live_faucet()).is_none(), "height {height}");
    }
}

/// The 777 row never pays another chain, and a mainnet genesis still carves a
/// zero faucet slice — with or without a genesis sink.
#[test]
fn an_active_binding_never_pays_a_chain_that_is_not_777() {
    schedule_binding();
    for (chain_id, sink) in [
        ("wrfcoin-main", None),
        ("wrfcoin-main", Some(Address([0x39; 20]))),
        ("777-other", None),
        ("testnet", None),
    ] {
        let mut state = genesis(chain_id, sink);
        for height in 2..=H_F + 2 {
            apply_block(&mut state, chain_id, height);
            assert_eq!(slice(chain_id, height), 0, "{chain_id} carves no faucet slice");
            assert_eq!(state.emission_ledger.spent(EmissionRole::Faucet), 0, "{chain_id} {height}");
        }
        assert!(state.get_account(&live_faucet()).is_none(), "{chain_id}");
        assert_eq!(state.total_minted_supply(), (2..=H_F + 2)
            .map(|h| coinbase_allowance(h, Some(chain_id))).sum::<u128>(), "{chain_id}");
    }
}

/// RED on main at the positive vectors. The mint is `min(slice, MAX_SUPPLY −
/// minted)`: exact at both sides of the first emission reduction, zero where
/// the emission is zero, and clipped to the remaining supply headroom.
#[test]
fn the_bound_mint_never_exceeds_the_slice_or_the_supply_headroom() {
    schedule_binding();
    let live = live_faucet();

    // The first height at which the base reward reduces.
    let (mut lo, mut hi) = (H_F, H_F);
    while base_block_reward(hi) == base_block_reward(H_F) {
        hi *= 2;
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if base_block_reward(mid) == base_block_reward(H_F) { lo = mid } else { hi = mid }
    }
    let reduction = hi;
    // A height at which the emission (and so every slice) is zero.
    let mut zero = reduction;
    while adjusted_block_reward(zero) != 0 {
        zero = zero.checked_mul(2).expect("emission terminates");
    }

    for height in [reduction - 1, reduction, zero] {
        let mut state = genesis("777", None);
        state.apply_transitions_batch_at_height(height, vec![StateTransition::ApplyFaucetReward]).unwrap();
        let expected = slice("777", height);
        assert_eq!(balance(&state, &live), expected, "height {height}");
        assert_eq!(state.emission_ledger.spent(EmissionRole::Faucet), expected, "height {height}");
        assert_eq!(state.total_minted_supply(), expected, "height {height}");
    }
    assert!(slice("777", reduction - 1) > slice("777", reduction), "the boundary is a real reduction");
    assert!(slice("777", reduction) > 0);
    assert_eq!(slice("777", zero), 0);

    // Supply headroom below the slice: only the headroom is minted.
    let height = H_F + 1;
    assert!(slice("777", height) > 7);
    let mut state = genesis("777", None);
    state.create_account(Address([0x98; 20]), Balance::new(MAX_SUPPLY - 7));
    state.apply_transitions_batch_at_height(height, vec![StateTransition::ApplyFaucetReward]).unwrap();
    assert_eq!(balance(&state, &live), 7);
    assert_eq!(state.emission_ledger.spent(EmissionRole::Faucet), 7);
    assert_eq!(state.total_minted_supply(), MAX_SUPPLY);

    // No headroom at all: nothing minted, no account created.
    let mut state = genesis("777", None);
    state.create_account(Address([0x98; 20]), Balance::new(MAX_SUPPLY));
    state.apply_transitions_batch_at_height(height, vec![StateTransition::ApplyFaucetReward]).unwrap();
    assert!(state.get_account(&live).is_none());
    assert_eq!(state.total_minted_supply(), MAX_SUPPLY);
}

/// The committed row pins the same block-1 hash the rest of the tree pins for
/// the live public testnet, and the same wallet this file writes out
/// independently — not a retyped copy that could drift.
#[test]
fn the_committed_row_matches_the_trees_genesis_hash_and_the_ruled_wallet() {
    let bound = LEGACY_FAUCET_BINDINGS
        .iter()
        .find(|b| b.feature == ConsensusFeature::FaucetSinkBinding)
        .unwrap();
    assert_eq!(bound.genesis_chain_id, "777");
    assert_eq!(Address::from_hex(bound.address.unwrap()).unwrap(), live_faucet());
    // The block-1 hash is the one the rest of the tree pins for the live
    // public testnet, not a retyped copy that could drift.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    for (file, spelled) in [
        ("p2p/src/config.rs", bound.genesis_hash.to_string()),
        // The public snapshot ships this schedule record; both pins are required.
        ("docs/deployment/live-feature-schedule/777-testnet.json", bound.genesis_hash.to_string()),
    ] {
        let text = std::fs::read_to_string(format!("{root}/{file}")).unwrap();
        assert!(text.contains(&spelled), "{file} must pin the same genesis hash");
    }
}

fn is_config_shaped(file_name: &str) -> bool {
    file_name.ends_with(".env")
        || file_name.starts_with(".env")
        || [".json", ".jsonl", ".toml", ".yml", ".yaml", ".sh", ".conf"]
            .iter()
            .any(|ext| file_name.ends_with(ext))
}

/// The repository's files under `root`, as `/`-separated relative paths, and
/// the mode that listed them.
///
/// - `git-ls-files`: `root/.git` exists (a checkout or worktree). Only
///   TRACKED files count, so build and CI output left in the checkout (e.g.
///   the untracked `compile-messages.json` the integration ratchet writes)
///   is not a repository input. An `ls-files` error here fails loudly.
/// - `archive-walk`: no `.git` (a `git archive` tree, the build container manual
///   transfer path). Such a tree holds only tracked files by construction, so
///   the on-disk walk IS the tracked set; only cargo's target dir (`target/`,
///   and `CARGO_TARGET_DIR` when it lies inside `root`) is pruned.
fn repository_files(root: &std::path::Path) -> (&'static str, Vec<String>) {
    if root.join(".git").exists() {
        let listing = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "-z", "--cached"])
            .output()
            .expect("git must be runnable inside a git checkout");
        assert!(
            listing.status.success(),
            "git ls-files failed inside a git checkout: {}",
            String::from_utf8_lossy(&listing.stderr)
        );
        let files = listing
            .stdout
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        return ("git-ls-files", files);
    }
    let mut pruned = vec![root.join("target")];
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        let dir = std::path::PathBuf::from(dir);
        let dir = if dir.is_absolute() { dir } else { root.join(dir) };
        if dir.starts_with(root) {
            pruned.push(dir);
        }
    }
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if !pruned.contains(&path) {
                    stack.push(path);
                }
            } else {
                files.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    ("archive-walk", files)
}

/// Every configuration-shaped repository file under `root` that names
/// `name`, how many such files were scanned, and the listing mode (printed so
/// the evidence is attributable).
fn config_hits(root: &std::path::Path, name: &str) -> (&'static str, usize, Vec<String>) {
    // #3436 records the already-applied #3277 extension in documentation.
    // No node, build, cutter or signer loads this exact file as configuration.
    // Keep all other JSON files (including siblings) inside the input guard.
    //
    // #3471's CI admission manifest lists this very test file by path
    // (`corpus.must_be_relevant`), so it names the feature without scheduling it. Its only
    // consumers are `scripts/check_repository_input_admission.py` and that script's unit
    // test: no node, build, cutter or signer reads it as configuration, genesis or a
    // schedule. Exactly this one path is allowed, never a `.github/` glob.
    let allowed = [
        "scripts/integration_test_ratchet.json",
        "docs/deployment/live-feature-schedule/777-testnet.json",
        ".github/repository-input-admission.json",
    ];
    let (mode, files) = repository_files(root);
    println!("scan mode: {mode} ({})", root.display());
    let mut scanned = 0usize;
    let mut hits = Vec::new();
    for relative in files {
        let file_name = relative.rsplit('/').next().unwrap_or(&relative);
        if !is_config_shaped(file_name) || allowed.contains(&relative.as_str()) {
            continue;
        }
        scanned += 1;
        if std::fs::read(root.join(&relative)).is_ok_and(|bytes| {
            bytes.windows(name.len()).any(|w| w == name.as_bytes())
        }) {
            hits.push(relative);
        }
    }
    hits.sort();
    (mode, scanned, hits)
}

/// No genesis, default schedule, cut pin or fixture in this repository
/// names the feature: it is activated only by a governance-signed #3277
/// extension (E0f). Scans the configuration-shaped files of the tree.
#[test]
fn no_schedule_or_genesis_input_in_the_tree_names_the_binding() {
    let name = ConsensusFeature::FaucetSinkBinding.name();
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    let (_, scanned, hits) = config_hits(&root, name);
    assert!(scanned > 50, "the scan must actually see the tree ({scanned} files)");
    assert!(hits.is_empty(), "{name} must not be scheduled by any tree input: {hits:?}");
}

/// The scan judges what the repository COMMITS, not whatever a build left
/// in the checkout: CI's integration ratchet tees cargo's JSON messages,
/// which name this file's test target, into an untracked
/// `compile-messages.json` at the root before tests run (#3363 review P2-3).
/// An untracked file naming the feature must pass; a tracked one must fail.
#[test]
fn the_tree_scan_sees_tracked_inputs_only() {
    let name = ConsensusFeature::FaucetSinkBinding.name();
    let root = std::env::temp_dir().join(format!("faucet-scan-3361-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").arg("-C").arg(&root).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(root.join("genesis.json"), b"{\"schedule\": \"0:v1\"}").unwrap();
    git(&["add", "genesis.json"]);
    // Untracked build output that names the feature, exactly as CI leaves it.
    std::fs::write(
        root.join("compile-messages.json"),
        format!("{{\"target\":{{\"name\":\"{name}_activation_3361\"}}}}"),
    )
    .unwrap();
    let (mode, scanned, hits) = config_hits(&root, name);
    assert_eq!(mode, "git-ls-files");
    assert!(hits.is_empty(), "an untracked build artifact is not a tree input: {hits:?}");
    assert_eq!(scanned, 1, "only the tracked input is scanned");

    // A TRACKED input that schedules the feature is still caught.
    std::fs::write(root.join("cut.env"), format!("SCHEDULE=0:v1,40000:{name}\n")).unwrap();
    git(&["add", "cut.env"]);
    let (_, _, hits) = config_hits(&root, name);
    assert_eq!(hits, vec!["cut.env".to_string()]);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A `git archive` tree (no `.git`, the build container manual transfer path) is
/// scanned by walking it, never skipped: a violation in it is still reported,
/// at any depth, while cargo's `target/` is pruned.
#[test]
fn the_tree_scan_walks_an_archive_tree_without_git() {
    let name = ConsensusFeature::FaucetSinkBinding.name();
    let root = std::env::temp_dir().join(format!("faucet-scan-archive-3361-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("infra/testnet")).unwrap();
    std::fs::create_dir_all(root.join("target/debug")).unwrap();
    std::fs::write(root.join("genesis.json"), b"{}").unwrap();
    std::fs::write(root.join("infra/testnet/cut.env"), format!("SCHEDULE=0:v1,40000:{name}\n")).unwrap();
    std::fs::write(root.join("target/debug/messages.json"), name).unwrap();
    assert!(!root.join(".git").exists());
    let (mode, scanned, hits) = config_hits(&root, name);
    assert_eq!(mode, "archive-walk");
    assert_eq!(scanned, 2, "genesis.json and cut.env; target/ is pruned");
    assert_eq!(hits, vec!["infra/testnet/cut.env".to_string()]);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn deployment_record_exception_is_exact_and_prose_remains_outside_config_scan() {
    let name = ConsensusFeature::FaucetSinkBinding.name();
    let root = std::env::temp_dir().join(format!(
        "faucet-record-exception-3361-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("docs/deployment/live-feature-schedule")).unwrap();
    for file in [
        "777-testnet.json",
        "other-chain.json",
        "README.md",
        "test_check.py",
    ] {
        std::fs::write(
            root.join("docs/deployment/live-feature-schedule")
                .join(file),
            name,
        )
        .unwrap();
    }
    let (mode, scanned, hits) = config_hits(&root, name);
    assert_eq!(mode, "archive-walk");
    assert_eq!(scanned, 1, "the sibling JSON must still be scanned");
    assert_eq!(
        hits,
        ["docs/deployment/live-feature-schedule/other-chain.json"]
    );
    assert!(!is_config_shaped("README.md"));
    assert!(!is_config_shaped("test_check.py"));
    std::fs::remove_dir_all(root).unwrap();
}

/// #3471: the CI admission manifest names this test file, so it is the one allowed `.github`
/// path. The exception is exact: the same bytes in a sibling, a nested copy or any other
/// directory are still caught, and a real schedule input naming the feature still fails.
#[test]
fn admission_manifest_exception_is_exact_and_schedule_inputs_still_fail() {
    let name = ConsensusFeature::FaucetSinkBinding.name();
    let root = std::env::temp_dir().join(format!(
        "faucet-admission-exception-3361-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    for dir in [".github", "sub/.github", "docs", "config"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let naming = format!("{{\"must_be_relevant\": [\"shared-protocol/tests/{name}_activation_3361.rs\"]}}");
    for file in [
        ".github/repository-input-admission.json", // the exception
        ".github/other-manifest.json",             // sibling: caught
        "sub/.github/repository-input-admission.json", // nested copy: caught
        "docs/repository-input-admission.json",    // same name elsewhere: caught
    ] {
        std::fs::write(root.join(file), &naming).unwrap();
    }
    // A real schedule input and a genesis input naming the feature: caught.
    std::fs::write(root.join("config/node.toml"), format!("schedule = \"0:v1,40000:{name}\"\n")).unwrap();
    std::fs::write(root.join("config/genesis.yaml"), format!("features: [{name}]\n")).unwrap();

    let (mode, scanned, hits) = config_hits(&root, name);
    assert_eq!(mode, "archive-walk");
    assert_eq!(scanned, 5, "six config-shaped files, the allowed manifest is skipped before scanning");
    assert_eq!(
        hits,
        [
            ".github/other-manifest.json",
            "config/genesis.yaml",
            "config/node.toml",
            "docs/repository-input-admission.json",
            "sub/.github/repository-input-admission.json",
        ]
    );
    assert!(!hits.contains(&".github/repository-input-admission.json".to_string()));
    std::fs::remove_dir_all(root).unwrap();
}
