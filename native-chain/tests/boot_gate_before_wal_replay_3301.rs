//! #3301: boot must install the protocol gate (signing version, activation
//! table, feature schedule — from durable block 1) BEFORE it loads state,
//! because loading replays the WAL tail through `feature_active`.
//!
//! Scenario: a V1 chain whose block 1 schedules `paymaster` at height 3, and
//! a paymaster `Stake` (a feature-gated transition) applied at height 4. No
//! checkpoint is taken (interval 100), so a restart rebuilds everything from
//! the genesis anchor plus the WAL.
//!
//! - The OLD order (load, then gate) replays height 4 with no schedule
//!   installed: `feature_active(Paymaster, 4)` is false, the arm refuses the
//!   Stake, and the load fails — the node cannot restart past an activation.
//! - The #3301 order (`gate_then_load_state`) installs the schedule first and
//!   reproduces the producer's post-state exactly.
//!
//! Own process: the schedule is a process-wide first-install-wins global,
//! which is precisely what makes the order observable here.

use std::path::{Path, PathBuf};

use wrfcoin_native_chain::storage::{gate_then_load_state, Storage};
use wrfcoin_shared_protocol::accounts::paymaster::PaymasterAction;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule, ConsensusFeature, SigningProtocolVersion,
    GENESIS_ACTIVATION_SCHEDULE_KEY,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;

const ACTIVATION: u64 = 3;
const TIP: u64 = 6;
const STAKE_AT: u64 = 4;
const PAYMASTER: Address = Address([0x2b; 20]);

fn temp_db() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrfcoin.redb");
    (dir, path)
}

fn schedule_raw() -> String {
    format!("0:v1,{ACTIVATION}:{}", ConsensusFeature::Paymaster.name())
}

fn genesis_state() -> State {
    let mut state = State::new();
    state.create_account(PAYMASTER, Balance::new(10_000));
    state.recalculate_state_root();
    state
}

fn transitions_at(height: u64) -> Vec<StateTransition> {
    if height == STAKE_AT {
        vec![StateTransition::ApplyPaymasterAction {
            action: PaymasterAction::Stake {
                paymaster: PAYMASTER,
                amount: 1_000,
                unstake_delay_blocks: 2,
            },
            block_height: height,
        }]
    } else {
        Vec::new()
    }
}

/// What the boot gate does, reduced to its schedule half: read DURABLE block
/// 1 and install the version table and feature schedule it declares.
fn install_gate_from_block1(storage: &Storage) -> Result<(), String> {
    let block1 = storage
        .load_lowest_block()
        .map_err(|e| e.to_string())?
        .ok_or("no block 1")?;
    let pairs: Vec<(&str, &str)> = block1
        .metadata
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .map_err(|e| e.to_string())?;
    let features = resolve_feature_schedule(pairs, &table).map_err(|e| e.to_string())?;
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    Ok(())
}

/// Persist exactly the rows the live producer writes: the genesis anchor,
/// then each block with its WAL transitions, no checkpoint (interval 100).
/// Block 1 carries the chain's activation schedule.
async fn persist_chain(path: &Path) {
    let mut storage = Storage::open(path, 100).expect("open");
    storage.persist_genesis_state(&genesis_state()).await.expect("anchor");
    let mut prev = Hash::default();
    for height in 1..=TIP {
        let mut block = Block::new(1, prev, vec![], 1, height);
        if height == 1 {
            block
                .metadata
                .insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), schedule_raw());
        }
        let hash = block.calculate_hash();
        block.set_hash(hash.clone());
        prev = hash;
        storage
            .persist_block(block, transitions_at(height), None)
            .await
            .expect("persist block + WAL rows");
    }
}

/// The producer's post-state: the same transitions applied block by block
/// with the chain's schedule in force. Call only once it is installed.
fn producer_state() -> State {
    let mut state = genesis_state();
    for height in 1..=TIP {
        state
            .apply_transitions_batch_at_height(height, transitions_at(height))
            .expect("the producer applies with the schedule in force");
        state.apply_block_finalization(height);
    }
    state
}

#[tokio::test]
async fn a_restart_past_an_activation_replays_under_the_chains_schedule() {
    let (_dir, path) = temp_db();
    persist_chain(&path).await;
    let storage = Storage::open(&path, 100).expect("reopen");

    // OLD ORDER (launch_api before #3301): load first, gate after. Nothing is
    // installed yet, so height 4's Stake replays with every feature off.
    assert!(try_feature_schedule().is_none(), "this file owns its process");
    let old_order = storage.load_state();
    assert!(
        old_order.is_err(),
        "loading before the gate must fail: height {STAKE_AT}'s Stake replays with every \
         feature off (got Ok)"
    );

    // #3301 ORDER: gate from durable block 1, then load.
    let ((), loaded) = gate_then_load_state(&storage, install_gate_from_block1)
        .unwrap_or_else(|e| panic!("gate-then-load must succeed: {e:?}"));
    let chain = loaded.expect("a persisted chain");
    assert_eq!(chain.height, TIP);

    let expected = producer_state();
    assert_eq!(
        chain.state.state_root, expected.state_root,
        "the replayed tip state_root must equal the producer's"
    );
    assert_eq!(
        chain.state.paymasters.get(&PAYMASTER).map(|p| p.deposit),
        Some(1_000),
        "the post-activation Stake was replayed, not refused"
    );

}

// ---------------------------------------------------------------------------
// F1 (review of #3301): pin launch_api's main.rs to the order. The test above
// drives `gate_then_load_state` with a stand-in gate; without this pin,
// moving `load_state` back above the gate in main.rs passes every test.
// ---------------------------------------------------------------------------

fn main_rs_production() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/launch_api/main.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    // Production code only: everything before the `#[cfg(test)] mod tests`.
    let cut = text
        .find("#[cfg(test)]\nmod tests")
        .expect("main.rs has its #[cfg(test)] mod tests");
    text[..cut].to_string()
}

/// Lines that are code, not comments: a comment naming a call is not a call.
fn code_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with('*') && !t.starts_with("/*")
        })
        .collect()
}

#[test]
fn launch_api_boots_through_gate_then_load_state_with_the_real_gate() {
    let main = main_rs_production();
    assert!(main.len() > 50_000, "CONTROL: main.rs is the real startup source");
    let code = code_lines(&main);

    let calls = code.iter().filter(|l| l.contains("gate_then_load_state(")).count();
    assert_eq!(
        calls, 1,
        "main.rs must load state through gate_then_load_state exactly once (#3301)"
    );
    // No direct load anywhere in launch_api production code — main.rs or any
    // helper module, method syntax or UFCS (`Storage::load_state(&s)`).
    let direct = direct_load_state_calls();
    assert!(
        direct.is_empty(),
        "launch_api production code must not call load_state directly — only through \
         gate_then_load_state, or the WAL replays under pre-activation rules (#3301): {direct:?}"
    );

    // The closure handed to gate_then_load_state is the REAL gate, not a stub:
    // it both arms an uncut chain and checks a chain with blocks.
    let start = main.find("gate_then_load_state(").expect("call");
    let end = start
        + main[start..]
            .find("let ((protocol_identity")
            .expect("the gate's result is destructured after the call");
    let closure = &main[start..end];
    for gate_fn in [
        // #3277 S5: the gate for a chain with blocks also merges the
        // block-1-keyed bootstrap.
        "arm_block1_protocol_at_boot(",
        "install_uncut_chain_protocol(",
        "archive_empty_but_height_recorded(",
    ] {
        assert!(
            code_lines(closure).iter().any(|l| l.contains(gate_fn)),
            "the closure passed to gate_then_load_state must contain {gate_fn}"
        );
    }
}

/// Every `load_state(` call in launch_api PRODUCTION code (each file with its
/// `#[cfg(test)] mod … { … }` blocks cut out, comment lines excluded) that is not part of
/// a longer identifier — so `gate_then_load_state(` is not counted, while
/// `.load_state()` and `Storage::load_state(&storage)` both are.
fn direct_load_state_calls() -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read launch_api dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/launch_api");
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(files.len() > 10, "CONTROL: scanned the real launch_api tree");
    let mut hits = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("read source");
        let production = strip_test_modules(&text);
        for line in code_lines(&production) {
            let mut from = 0;
            while let Some(at) = line[from..].find("load_state(") {
                let start = from + at;
                let preceded_by_ident = line[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
                if !preceded_by_ident {
                    hits.push(format!("{}: {}", file.display(), line.trim()));
                }
                from = start + "load_state(".len();
            }
        }
    }
    hits
}

/// `text` with every `#[cfg(test)] mod name { … }` block removed (brace
/// matched), keeping production code that follows a test module.
fn strip_test_modules(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("#[cfg(test)]\nmod ") {
        out.push_str(&rest[..at]);
        let module = &rest[at..];
        let Some(open) = module.find('{') else {
            // `mod name;` declaration of a test-only file: drop the line.
            let end = module.find('\n').map_or(module.len(), |i| i + 1);
            rest = &module[end..];
            continue;
        };
        if module[..open].contains(';') {
            let end = module.find(';').map_or(module.len(), |i| i + 1);
            rest = &module[end..];
            continue;
        }
        let mut depth = 0usize;
        let mut end = module.len();
        for (i, c) in module[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i + 1;
                        break;
                    }
                },
                _ => {},
            }
        }
        rest = &module[end..];
    }
    out.push_str(rest);
    out
}
