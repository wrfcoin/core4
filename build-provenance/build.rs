//! Emit build provenance for #2721: which commit is this binary, and may it
//! affirmatively claim the tree was clean.
//!
//! READ `src/claim.rs` FIRST. It carries the argument; this file is the
//! plumbing.
//!
//! THE CACHE IS THE WHOLE PROBLEM (#2882)
//! --------------------------------------
//! A build script's output is cached, and cargo replays it into binaries
//! compiled later without running any of our code. So a value emitted while
//! the tree was clean can be stamped onto a binary built after it was dirtied.
//!
//! Two rounds tried to prevent that by making the script rerun whenever it
//! could matter, enumerating the inputs that cause a rebuild: `src`/`examples`
//! in round 1, cargo's dep-info in round 2. Both shipped with a completeness
//! argument; both were falsified by one more mechanism — `include_str!` of a
//! file outside the crate directory, then a tracked file read by ANOTHER
//! crate's build script, which appears in no dep-info because rustc never
//! opened it. Enumeration requires completeness, completeness was never
//! demonstrable, and a third list would fail identically.
//!
//! So this round does not enumerate. It makes the cached value harmless:
//! `clean` is emitted ONLY by an attested build, and an attested build reruns
//! this script on every build, so nothing in it is ever cached. Every other
//! build reports `unverified` for a clean tree — honest, and impossible to
//! mistake for a guarantee.
//!
//! WHY THE ALWAYS-RERUN IS AFFORDABLE HERE
//! ---------------------------------------
//! Measured on cargo 1.88.0: a build-script rerun recompiles the crate AND
//! every dependent, even when the emitted output is byte-identical (verified
//! with `SOURCE_DATE_EPOCH` pinned — same output sha256, still `Compiling
//! wrfcoin-build-provenance` then `Compiling wrfcoin_cli`). That is why
//! "rerun always" was rejected as a general policy in round 1, and it still is.
//!
//! It is affordable under attestation because attestation marks a cold,
//! single-shot build of a fresh checkout — the ceremony and release path, where
//! everything compiles once anyway. Developer builds never set it and never pay
//! it: with no attestation this script watches only the paths that move when
//! the COMMIT moves, exactly as before, so an ordinary edit/check loop is
//! untouched.
//!
//! WORKTREE SAFETY: every git path is resolved through `git rev-parse`, never
//! by joining `.git/`. In a linked worktree `.git` is a FILE, HEAD lives in
//! `<common>/worktrees/<name>/`, and refs live in the common dir. This repo's
//! lanes all run in linked worktrees, so hand-rolled path math is wrong here by
//! default rather than by accident.

// Shared with the lib so the four-way decision is unit-tested on every CI run,
// including the `clean` branch, which is otherwise reached only at ceremony.
#[path = "src/claim.rs"]
#[allow(dead_code)]
mod claim;

use claim::{
    attestation_requested,
    classify,
    Worktree,
    ATTEST_ENV,
    ATTEST_FILE_ENV,
    UNKNOWN,
};
use std::path::Path;
use std::process::Command;
use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}

/// Watch `path` only when it exists — a missing path means "rerun always".
fn watch_if_present(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn emit_rerun_triggers(attested: bool) {
    // Bumping this forces a recomputation on a warm tree.
    println!("cargo:rerun-if-env-changed=WRFCOIN_BUILD_PROVENANCE_REFRESH");
    // Reproducible-build override; see the timestamp section below.
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    // Turning attestation on or off must recompute the claim.
    println!("cargo:rerun-if-env-changed={ATTEST_ENV}");
    println!("cargo:rerun-if-env-changed={ATTEST_FILE_ENV}");

    if attested {
        // DELIBERATE always-rerun. `cargo:rerun-if-changed` on a path that does
        // not exist makes cargo rerun this script on EVERY build — normally the
        // trap this file most guards against, and here the entire mechanism.
        //
        // An attested build therefore has NO cached provenance: the value is
        // recomputed for every artifact it produces, so `clean` cannot be
        // stale no matter which input changed or how. That is what replaces
        // the enumeration, and it is why neither falsifying mechanism from
        // rounds 1 and 2 applies: nothing is being enumerated.
        let sentinel = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
        println!("cargo:rerun-if-changed={sentinel}/wrfcoin-provenance-always-rerun");
        return;
    }

    // Unattested: watch only what moves when the COMMIT moves. A clean tree
    // here reports `unverified`, so these triggers are a convenience that keeps
    // the SHA current — not a correctness mechanism, and nothing depends on
    // this list being complete.
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    watch_if_present(Path::new(&format!("{git_dir}/HEAD")));
    watch_if_present(Path::new(&format!("{git_dir}/index")));
    watch_if_present(Path::new(&format!("{git_dir}/logs/HEAD")));

    // A packed ref has no loose file; HEAD/index/logs still catch the commit.
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            watch_if_present(Path::new(&path));
        }
    }
}

/// `--no-optional-locks` stops git rewriting `.git/index` to refresh its stat
/// cache. Without it this script would dirty a path it watches on every run and
/// so rerun on every build, recompiling the CLI each time.
fn worktree_state() -> Worktree {
    match Command::new("git")
        .args(["--no-optional-locks", "status", "--porcelain", "--untracked-files=no"])
        .output()
    {
        Ok(output) if output.status.success() => {
            if output.stdout.is_empty() {
                Worktree::Clean
            } else {
                Worktree::Dirty
            }
        },
        _ => Worktree::Unavailable,
    }
}

/// Days-from-civil, inverted (Howard Hinnant's algorithm), so this crate needs
/// no date dependency. Returns (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn format_utc(epoch_seconds: i64) -> String {
    let days = epoch_seconds.div_euclid(86_400);
    let seconds = epoch_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

fn main() {
    // The token alone proves only shape. The file proves a wrapper run is LIVE:
    // it sits in the wrapper's private staging directory, which is destroyed when
    // the post-build check fails. Round 4 checked shape only, and a reviewer
    // replayed a rejected run's printed digest straight through it (#2882 rd5).
    let attest_raw = std::env::var(ATTEST_ENV).ok();
    let token_file = std::env::var(ATTEST_FILE_ENV)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok());
    let attested = attestation_requested(attest_raw.as_deref(), token_file.as_deref());

    emit_rerun_triggers(attested);

    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| UNKNOWN.to_string());
    let short = if commit == UNKNOWN {
        UNKNOWN.to_string()
    } else {
        commit.chars().take(9).collect()
    };
    // A SHA with no cleanliness claim beside it is the lie #2721 is about, so
    // the state is emitted unconditionally, `unverified` and `unknown`
    // included.
    let state = classify(worktree_state(), attested);

    // TIMESTAMP AND REPRODUCIBILITY. A wall-clock stamp makes a build
    // non-reproducible byte-for-byte, and that is not academic here: #2721
    // measured that `oracle_ceremony_sign` and `oracle_ceremony_verify`
    // currently build BYTE-IDENTICALLY across build paths, which makes a
    // recorded sha256 a real check on those two today. Defaulting to
    // wall-clock would destroy a working mitigation. SOURCE_DATE_EPOCH — the
    // reproducible-builds standard — is honoured, so a build that sets it
    // stays byte-identical; the source of the value is embedded too, so a
    // reader can tell which kind of stamp they are looking at.
    let (timestamp, source) = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(raw) => match raw.trim().parse::<i64>() {
            Ok(epoch) => (format_utc(epoch), "SOURCE_DATE_EPOCH"),
            // Fail closed rather than silently fall back to wall clock: a
            // malformed value means the builder INTENDED reproducibility.
            Err(_) => panic!(
                "SOURCE_DATE_EPOCH is set to {raw:?}, which is not an integer number of seconds. \
                 Refusing to fall back to a wall-clock timestamp, which would silently produce a \
                 non-reproducible build for someone who asked for a reproducible one."
            ),
        },
        Err(_) => match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(since) => (format_utc(since.as_secs() as i64), "build-time"),
            Err(_) => (UNKNOWN.to_string(), UNKNOWN),
        },
    };

    println!("cargo:rustc-env=WRFCOIN_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=WRFCOIN_BUILD_COMMIT_SHORT={short}");
    println!("cargo:rustc-env=WRFCOIN_BUILD_STATE={state}");
    println!("cargo:rustc-env=WRFCOIN_BUILD_TIMESTAMP={timestamp}");
    println!("cargo:rustc-env=WRFCOIN_BUILD_TIMESTAMP_SOURCE={source}");
}
