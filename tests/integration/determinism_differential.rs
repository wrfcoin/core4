//! Differential determinism test (#2102 story #2103).
//!
//! Replays a committed block-sequence fixture from a fixed genesis and asserts the resulting
//! `state_root` at *every* height matches the recorded expected value. See
//! `src/determinism.rs` for the reusable [`replay_fixture`] primitive that #2104 / #2183 /
//! #2204 consume.
//!
//! Regenerate the expected roots (a reviewable change, never a test fix) with:
//! `REGENERATE_DETERMINISM_FIXTURE=1 cargo test -p test_server --test determinism_differential`

use test_server::determinism::{
    assert_root_parity, fixture_dir, replay_fixture, replay_fixture_with_restart,
};

/// Full-sequence parity: every recorded height must match the committed `expected_roots.json`.
#[test]
fn full_sequence_root_parity() {
    let dir = fixture_dir();
    let roots = replay_fixture(&dir);
    // Genesis (height 0) plus one entry per block line.
    assert!(
        roots.len() >= 2,
        "fixture must have genesis + at least one block"
    );
    assert_root_parity(&roots, &dir);
}

/// Determinism: replaying the identical fixture twice yields byte-identical roots at every height.
#[test]
fn replay_is_deterministic_across_runs() {
    let dir = fixture_dir();
    let a = replay_fixture(&dir);
    let b = replay_fixture(&dir);
    assert_eq!(
        a, b,
        "two replays of the same fixture must produce identical roots"
    );
}

/// Restart parity: serialising and restoring the State mid-sequence (an IBD-reload analogue)
/// must not change any subsequent root.
#[test]
fn restart_midsequence_preserves_roots() {
    let dir = fixture_dir();
    let straight = replay_fixture(&dir);
    // Restart after the second block; must be within range of the fixture.
    let restart_after = 2usize.min(straight.len().saturating_sub(2));
    let restarted = replay_fixture_with_restart(&dir, restart_after);
    assert_eq!(
        straight, restarted,
        "a mid-sequence state serialize/restore must not change any state_root"
    );
}

/// The test can actually fail: a mutated block must produce a different root than the fixture
/// records, so a real determinism regression cannot pass silently. We mutate in-memory by
/// replaying a doctored copy of the fixture and asserting divergence from the committed roots.
#[test]
fn mutated_block_diverges_from_expected() {
    use std::fs;
    use test_server::determinism::StateRoot;

    let dir = fixture_dir();
    let baseline = replay_fixture(&dir);

    // Build a mutated fixture in a temp dir: bump the first reward amount by 1 base unit.
    let tmp = std::env::temp_dir().join(format!("det-fixture-mutant-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("mk tmp fixture dir");
    fs::copy(dir.join("genesis.json"), tmp.join("genesis.json")).expect("copy genesis");
    fs::copy(
        dir.join("expected_roots.json"),
        tmp.join("expected_roots.json"),
    )
    .expect("copy expected");
    let blocks = fs::read_to_string(dir.join("blocks.jsonl")).expect("read blocks");
    // Perturb the first "amount": <n> occurrence.
    let mutated = perturb_first_amount(&blocks);
    assert_ne!(
        mutated, blocks,
        "mutation must actually change the fixture text"
    );
    fs::write(tmp.join("blocks.jsonl"), mutated).expect("write mutated blocks");

    let mutant = replay_fixture(&tmp);
    let baseline_roots: Vec<&StateRoot> = baseline.iter().map(|(_, r)| r).collect();
    let mutant_roots: Vec<&StateRoot> = mutant.iter().map(|(_, r)| r).collect();
    assert_ne!(
        baseline_roots, mutant_roots,
        "a mutated block must produce at least one divergent state_root (test must be able to fail)"
    );
    let _ = fs::remove_dir_all(&tmp);
}

/// Bump the first `"amount": N` integer in the fixture text by 1. Deterministic, no regex.
fn perturb_first_amount(blocks: &str) -> String {
    let needle = "\"amount\": ";
    if let Some(start) = blocks.find(needle) {
        let num_start = start + needle.len();
        let rest = &blocks[num_start..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if end > 0 {
            let n: u128 = rest[..end].parse().unwrap_or(0);
            return format!(
                "{}{}{}",
                &blocks[..num_start],
                n + 1,
                &blocks[num_start + end..]
            );
        }
    }
    blocks.to_string()
}
