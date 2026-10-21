//! Which word a binary is allowed to say about the tree it was built from.
//!
//! THE PROBLEM THIS SOLVES, AFTER TWO FAILED ROUNDS
//! ------------------------------------------------
//! A build script's output is CACHED. Cargo replays it into binaries compiled
//! later, and when it replays, no code of ours runs. So any value the script
//! can emit may end up stamped on a binary built from a tree it never saw.
//!
//! Rounds 1 and 2 tried to make the script rerun whenever that could matter, by
//! enumerating the inputs that cause a rebuild — first `src`/`examples`, then
//! Cargo's dep-info. Each round shipped with a completeness argument and each
//! was falsified by one more mechanism: `include_str!` of a file outside the
//! crate directory (round 1), and a tracked file read by ANOTHER crate's build
//! script, which appears in no dep-info at all because rustc never opened it
//! (round 2). The lesson is not that the list was short twice. It is that the
//! list has to be complete, completeness was never demonstrable, and a third
//! list would fail for the same structural reason.
//!
//! THE INVERSION
//! -------------
//! Stop trying to make every value fresh, and make the CACHED value harmless.
//!
//! A replayed `dirty` is conservative — it under-claims, and nobody is misled
//! by a binary that refuses to vouch for itself. A replayed `unknown` is
//! likewise harmless. Only a replayed **`clean`** does damage, because it is an
//! affirmative statement an operator will trust. So `clean` is the only value
//! that needs proof, and it is emitted only where freshness is structural
//! rather than enumerated:
//!
//!   * ATTESTED builds set `WRFCOIN_BUILD_PROVENANCE_ATTEST`. Those builds also
//!     force the provenance script to rerun on EVERY build (see `build.rs`), so
//!     a stale value cannot exist in them at all — there is nothing to
//!     enumerate because nothing is cached. That is the ceremony and release
//!     path, which is a cold build of a fresh checkout and single-shot, so the
//!     recompile cost lands where it does not matter.
//!   * Every other build is a warm developer build whose value may be replayed.
//!     A clean tree there yields [`UNVERIFIED`], never `clean`.
//!
//! This has no rot surface. There is no list to keep complete, so neither
//! falsifying mechanism — nor the next one — changes the outcome: a
//! non-attested build never says `clean` regardless of what rebuilt it.
//!
//! WHY FOUR WORDS AND NOT THREE
//! ----------------------------
//! [`UNVERIFIED`] and [`UNKNOWN`] are deliberately distinct. They describe
//! different situations and an operator should act differently:
//!
//!   `unverified` — provenance WAS computed and the tree was clean at that
//!                  moment, but this build did not attest, so the claim may
//!                  have been cached from an earlier tree. Re-run the build
//!                  with attestation to get a trustworthy answer.
//!   `unknown`    — provenance could not be established at all: no `git`, or
//!                  not a repository (a source tarball, for instance). Nothing
//!                  to re-run; the SHA itself is unverified.
//!
//! Collapsing them would lose exactly the distinction the previous review asked
//! to preserve.

/// The tree was clean AND this build attested that its provenance is fresh.
pub const CLEAN: &str = "clean";

/// The tree had uncommitted changes when provenance was computed. Safe to
/// replay: it under-claims rather than over-claims.
pub const DIRTY: &str = "dirty";

/// The tree was clean when provenance was computed, but this build did not
/// attest, so the value may be a cached one describing an earlier tree.
pub const UNVERIFIED: &str = "unverified";

/// Provenance could not be established at all.
pub const UNKNOWN: &str = "unknown";

/// Environment variable carrying the wrapper's tree digest for this run.
pub const ATTEST_ENV: &str = "WRFCOIN_BUILD_PROVENANCE_ATTEST";

/// Environment variable naming the wrapper's live token file. Its contents must
/// equal [`ATTEST_ENV`]; the file lives in the wrapper's private staging
/// directory and does not survive a failed post-build check.
pub const ATTEST_FILE_ENV: &str = "WRFCOIN_BUILD_PROVENANCE_ATTEST_FILE";

/// What `git status` reported at the moment provenance was computed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Worktree {
    /// `git status --porcelain` was empty.
    Clean,
    /// `git status --porcelain` was non-empty.
    Dirty,
    /// `git` could not be run, or this is not a repository.
    Unavailable,
}

/// The only place the four words are chosen.
///
/// Pure and total, so every branch — `clean` included — is exercised by this
/// crate's tests on every CI run. That matters: under this design the `clean`
/// branch is otherwise reached only during a release ceremony, and a path that
/// runs only at ceremony is a path that rots unobserved, which is the same
/// failure mode as a gate that never runs.
pub fn classify(worktree: Worktree, attested: bool) -> &'static str {
    match (worktree, attested) {
        // No provenance at all: attestation cannot manufacture it.
        (Worktree::Unavailable, _) => UNKNOWN,
        // Under-claiming is always safe, attested or not.
        (Worktree::Dirty, _) => DIRTY,
        // The only path to an affirmative claim.
        (Worktree::Clean, true) => CLEAN,
        // Clean, but this value may be replayed onto a later binary.
        (Worktree::Clean, false) => UNVERIFIED,
    }
}

/// True only for a positively attested clean tree.
pub fn is_attested_clean(state: &str) -> bool {
    state == CLEAN
}

/// Whether the environment carries a LIVE, wrapper-issued attestation token.
///
/// Requires two things that must agree:
///   * `token` — a 64-character lowercase hex tree digest, and
///   * `token_file` — the contents of the file the wrapper created for THIS run,
///     which must equal the token.
///
/// WHY BOTH (#2882 round 5). Round 4 accepted any 64-hex value. A reviewer
/// replayed the digest printed by a *rejected* wrapper run into a direct cargo
/// invocation and reproduced the original TOCTOU: an artifact claiming `clean`
/// built after the tree went dirty. The token proved SHAPE, not that a wrapper
/// run was underway — so it bound nothing.
///
/// The file binds the token to a live run: it lives inside the wrapper's private
/// staging directory, which is removed when the post-build check fails. The
/// wrapper also no longer prints the digest, so there is nothing in a log to
/// replay in the first place.
///
/// WHAT THIS STILL DOES NOT DO, stated because round 4 overclaimed exactly here:
/// it does not stop a builder who controls the machine from constructing both
/// values. Nothing available to a build script could. It stops *replay* of a
/// rejected run's output, and it makes accidental attestation impossible. The
/// guarantee remains "an operator building through the wrapper cannot be fooled
/// by a concurrent edit", and the artifact-level guarantee comes from the
/// wrapper publishing only after the post-check, not from this function.
pub fn attestation_requested(token: Option<&str>, token_file: Option<&str>) -> bool {
    let (Some(token), Some(contents)) = (token, token_file) else {
        return false;
    };
    let token = token.trim();
    let well_formed =
        token.len() == 64 && token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    well_formed && contents.trim() == token
}
