// THE process-environment lock (#2290/#2398) — see the doc comment on the
// `mod test_env` declaration in helpers.rs for the full contract and the
// audited-caller list. This body lives in its own file so that bin crates
// which include launch_api sources via `#[path]` (replay_verify includes
// transition_extractor.rs, whose tests take this lock) can mount the same
// lock at `crate::helpers::test_env` without pulling in all of helpers.rs.
//
// NOTE: each bin crate that mounts this file gets its OWN static — that is
// correct, because `cargo test` runs each test binary as a separate process
// and process env is only shared within one process.
use std::sync::{Mutex, MutexGuard};

pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the process-env lock, recovering from poisoning so one failing
/// test does not cascade into unrelated env-mutating tests.
pub(crate) fn lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run an environment-dependent fixture in its own process (#3521).
/// Command::env configures only the child: no process-wide mutation can race
/// unrelated readers in a binary that mounts the extractor. The marker selects
/// the child body; the parent returns after checking the child's exact test.
#[cfg(test)]
pub(crate) fn isolated_chain_domain(
    test_name: &str,
    chain_id: &str,
    network_id: &str,
    faucet_address: &str,
    oracle_set: &str,
) -> bool {
    const CHILD: &str = "WRFCOIN_CHAIN_DOMAIN_FIXTURE_CHILD";
    // determinism-ok: test-only subprocess marker, never a consensus input.
    if std::env::var(CHILD).ok().as_deref() == Some(test_name) {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap()) // panic-ok: cfg(test)-only fixture executable discovery; no decoded input reaches this helper.
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD, test_name)
        .env("WRFCOIN_CHAIN_ID", chain_id)
        .env("WRFCOIN_NETWORK_ID", network_id)
        .env("WRFCOIN_GENESIS_FAUCET_ADDRESS", faucet_address)
        .env("WRFCOIN_GENESIS_ORACLE_SET", oracle_set)
        .output()
        .expect("isolated fixture process must start"); // panic-ok: cfg(test)-only child fixture startup failure must fail the test; no production input path.
    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{stdout}");
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        output.status.success(),
        "isolated fixture failed: {test_name}"
    );
    assert!(
        stdout.contains(&format!("test {test_name} ... ok"))
            && stdout.contains("test result: ok. 1 passed; 0 failed;"),
        "isolated fixture must execute exactly one test: {test_name}"
    );
    true
}
