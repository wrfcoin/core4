#!/usr/bin/env bash
# Produce a build whose `clean` provenance claim covers the WHOLE build, not a
# point in time before it (#2882 round 4).
#
# WHY A WRAPPER AND NOT MORE BUILD SCRIPT
# ---------------------------------------
# `build-provenance/build.rs` samples `git status` and then exits. Cargo goes on
# to compile the dependent crates and link the artifact, and that takes seconds.
# Anything that dirties the tree inside that window is invisible to the sample,
# so the finished binary carries a `clean` the tree no longer justifies.
#
# Demonstrated on bc4ca80ec, not argued: an attested build was started on a clean
# tree, a tracked file was appended to 1.2s later while rustc was still running,
# the build exited 0, and the produced binary reported `tree: clean` with a
# timestamp from before the edit while `git status` showed the file modified.
#
# No amount of work inside build.rs fixes that. The script runs BEFORE the
# artifact exists; only something outside cargo can check AFTER it exists. So
# attestation is this wrapper, and `clean` means "produced by a run of this
# script that exited 0".
#
# WHAT IS SAMPLED
# ---------------
#   * HEAD
#   * `git status --porcelain` (must be empty at start)
#   * a digest of name+size+NANOSECOND mtime for every TRACKED file
#
# The third is what catches an edit that is reverted inside the build window. A
# revert restores content, so HEAD and `git status` both look untouched, but it
# rewrites the file and the mtime moves.
#
# `%y` (nanoseconds), NOT `%Y` (whole seconds). Round 4 used `%Y` and its own
# proof forced a detectable boundary with `sleep 1.1`; remove the sleep and a
# real edit-and-revert became invisible. The test passed because of its own
# scaffolding rather than because the detector worked. The self-test below now
# does the revert with NO sleep, so it can only pass if the resolution is real. Cargo writes only into `target/`, which
# is ignored, so tracked mtimes are stable across an ordinary build and this does
# not produce false rejections.
#
# ON FORGERY, stated precisely because round 4 overclaimed here and a reviewer
# replayed a rejected run's PRINTED digest straight through the check. The token
# is now bound to a live file that this script creates, never printed, named per
# PID, and swept at the start of the next run. That defeats replay-from-logs and
# makes accidental attestation impossible.
#
# It does NOT defeat a builder who controls the machine: they can write both the
# variable and the file. Nothing available to a build script could stop that. A
# SIGKILLed wrapper also leaves its token behind — the trap does not run — and a
# deliberate act could reuse it before the next run sweeps it.
#
# So the artifact-level guarantee does not rest on the token at all. It rests on
# publication: the binary is built in a private staging directory and moved to
# `target/` only after the post-check passes. The guarantee is "an operator who builds through this
# script cannot be fooled by a concurrent edit", not "a hostile builder cannot
# lie about their own artifact".
#
# Usage:
#   scripts/attest_build.sh [--artifact PATH ...] -- <build command ...>
#   scripts/attest_build.sh --self-test
set -uo pipefail

ATTEST_VAR="WRFCOIN_BUILD_PROVENANCE_ATTEST"

# name+size+mtime of every tracked file, as one digest.
# NOTE the subshell `cd`. `git ls-files` emits paths relative to the repository
# root, so `stat` must run WITH THAT ROOT AS CWD. Running it in the caller's
# directory makes every stat fail, and with stderr discarded the digest becomes a
# constant hash of empty input — present, stable, and blind to every change. The
# self-test's revert case caught exactly that.
tracked_digest() {
    ( cd "$1" 2>/dev/null || exit 0
      git ls-files -z \
        | xargs -0 -r stat -c '%n %s %y' 2>/dev/null
    ) | LC_ALL=C sort | sha256sum | cut -d' ' -f1
}

sample() {
    local repo="$1"
    printf '%s\n' "$(git -C "$repo" rev-parse HEAD 2>/dev/null)"
    git -C "$repo" --no-optional-locks status --porcelain --untracked-files=no 2>/dev/null
    printf '%s\n' "$(tracked_digest "$repo")"
}

sample_digest() { sample "$1" | sha256sum | cut -d' ' -f1; }

# Run `command` between two samples; fail if the tree moved. Returns 0 only when
# the build succeeded AND the tree is provably unchanged across the whole window.
attested_run() {
    local repo="$1"; shift
    local dirty
    dirty="$(git -C "$repo" --no-optional-locks status --porcelain --untracked-files=no)"
    if [ -n "$dirty" ]; then
        echo "ATTEST_REFUSED: the tree is already dirty; an attested build must start clean" >&2
        printf '%s\n' "$dirty" >&2
        return 2
    fi

    local before after rc
    before="$(sample_digest "$repo")"
    ATTEST_DIGEST="$before" "$@"
    rc=$?
    after="$(sample_digest "$repo")"

    if [ "$before" != "$after" ]; then
        echo "ATTEST_FAILED: the working tree changed while the artifact was being produced." >&2
        echo "  The provenance claim was sampled BEFORE the build and cannot describe the" >&2
        echo "  artifact this run produced. Treat any binary from this run as UNATTESTED." >&2
        echo "  before=$before after=$after" >&2
        git -C "$repo" --no-optional-locks status --porcelain --untracked-files=no >&2
        return 3
    fi
    return "$rc"
}

self_test() {
    local tmp rc fails=0
    tmp="$(mktemp -d)"
    git -C "$tmp" init -q
    git -C "$tmp" config user.email t@example.com
    git -C "$tmp" config user.name t
    echo original > "$tmp/tracked.txt"
    git -C "$tmp" add tracked.txt
    git -C "$tmp" commit -qm init

    # CONTROL: a build that changes nothing must be accepted. Without this, a
    # verifier that rejected everything would pass the detection case below.
    attested_run "$tmp" true; rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "SELF_TEST_FAILED: an unchanged tree must be accepted (rc=$rc)" >&2
        fails=1
    fi

    # DETECTION: a build that dirties a tracked file must be rejected, even though
    # the build command itself exits 0. This is the #2882 round 4 blocker.
    attested_run "$tmp" bash -c "echo changed >> '$tmp/tracked.txt'"; rc=$?
    if [ "$rc" -ne 3 ]; then
        echo "SELF_TEST_FAILED: a mid-build tracked-file change must be rejected (rc=$rc)" >&2
        fails=1
    fi

    # DETECTION: change-then-revert, WITH NO SLEEP. Content and `git status` end
    # up identical, so only the mtime digest can see it — and only at sub-second
    # resolution. Round 4's version of this case used `sleep 1.1` to force a whole
    # second boundary, so it passed because of the sleep rather than because the
    # detector worked; with `%Y` and no sleep the revert was invisible. The sleep
    # is deliberately absent here: this case is the regression pin for that.
    git -C "$tmp" checkout -q -- tracked.txt
    attested_run "$tmp" bash -c "echo x >> '$tmp/tracked.txt'; printf 'original\n' > '$tmp/tracked.txt'"; rc=$?
    if [ "$rc" -ne 3 ]; then
        echo "SELF_TEST_FAILED: a same-instant edit-and-revert must be rejected without any sleep (rc=$rc)" >&2
        fails=1
    fi

    # REFUSAL: starting dirty is refused before anything is built.
    echo dirt >> "$tmp/tracked.txt"
    attested_run "$tmp" true; rc=$?
    if [ "$rc" -ne 2 ]; then
        echo "SELF_TEST_FAILED: a dirty starting tree must be refused (rc=$rc)" >&2
        fails=1
    fi

    # PUBLICATION IS TRANSACTIONAL. A build that succeeds and then dirties the
    # tree must leave NOTHING at the published path. Round 4 built into target/
    # and deleted afterwards, so a wrapper killed between link and cleanup left a
    # false-clean binary exactly where an operator would pick it up.
    git -C "$tmp" checkout -q -- tracked.txt
    mkdir -p "$tmp/target/.attest-staging/debug" "$tmp/target/debug"
    rm -f "$tmp/target/debug/artifact"
    attested_run "$tmp" bash -c \
        "echo built > '$tmp/target/.attest-staging/debug/artifact'; echo dirty >> '$tmp/tracked.txt'"
    rc=$?
    if [ "$rc" -eq 0 ]; then
        promote "$tmp/target/.attest-staging" "$tmp" "debug/artifact" >/dev/null 2>&1
    fi
    if [ -e "$tmp/target/debug/artifact" ]; then
        echo "SELF_TEST_FAILED: a rejected run must publish nothing (artifact present)" >&2
        fails=1
    fi
    # ...and the converse: a clean run DOES publish, or the check above passes
    # merely because promotion never works at all.
    git -C "$tmp" checkout -q -- tracked.txt
    echo built > "$tmp/target/.attest-staging/debug/artifact"
    attested_run "$tmp" true
    rc=$?
    if [ "$rc" -eq 0 ]; then
        promote "$tmp/target/.attest-staging" "$tmp" "debug/artifact" >/dev/null 2>&1
    fi
    if [ ! -e "$tmp/target/debug/artifact" ]; then
        echo "SELF_TEST_FAILED: an accepted run must publish (promotion is inert)" >&2
        fails=1
    fi

    # A digest that hashes empty input would be constant, and every rejection case
    # above would pass for the wrong reason. Assert it actually varies with content.
    local d1 d2
    git -C "$tmp" checkout -q -- tracked.txt
    d1="$(tracked_digest "$tmp")"
    echo more >> "$tmp/tracked.txt"
    d2="$(tracked_digest "$tmp")"
    if [ "$d1" = "$d2" ] || [ -z "$d1" ]; then
        echo "SELF_TEST_FAILED: tracked_digest is not sensitive to content (d1=$d1 d2=$d2)" >&2
        fails=1
    fi

    rm -rf "$tmp"
    if [ "$fails" -eq 0 ]; then
        echo "attest_build.sh self-test passed (accepts unchanged; rejects change, same-instant revert and dirty start; publication is transactional both ways; digest is live)"
        return 0
    fi
    return 1
}

# Publish `rel` out of the staging target dir only after verification succeeded.
# ORDERING, not cleanup. Round 4 built straight into `target/`, verified after,
# and deleted on failure — so killing the wrapper between link and cleanup left a
# false-clean artifact at its published path with the wrapper dead at 137.
# Deleting after the fact cannot make pre-verification publication transactional;
# only building somewhere else and moving it afterwards can.
promote() {
    local staging="$1" repo="$2" rel="$3"
    local src="$staging/$rel" dst="$repo/target/$rel"
    if [ ! -e "$src" ]; then
        echo "PROMOTE_FAILED: $rel was not produced in staging" >&2
        return 4
    fi
    mkdir -p "$(dirname "$dst")"
    cp -p "$src" "$dst.attest-tmp" && mv -f "$dst.attest-tmp" "$dst"
}

main() {
    if [ "${1:-}" = "--self-test" ]; then
        self_test
        return $?
    fi

    local publish=()
    while [ "${1:-}" != "--" ] && [ $# -gt 0 ]; do
        case "$1" in
            --publish) publish+=("$2"); shift 2 ;;
            *) echo "unknown argument: $1" >&2; return 64 ;;
        esac
    done
    [ "${1:-}" = "--" ] && shift
    if [ $# -eq 0 ]; then
        echo "usage: $0 [--publish REL_PATH_UNDER_TARGET ...] -- <build command ...>" >&2
        return 64
    fi

    local repo staging token token_file rc
    repo="$(git rev-parse --show-toplevel)" || { echo "not a git repository" >&2; return 2; }

    # Private staging target. The artifact is produced HERE and only moved to
    # target/ after the post-check passes, so an interrupted run cannot leave a
    # clean-claiming binary at the path an operator would pick up.
    staging="$repo/target/.attest-staging"
    mkdir -p "$staging"

    # Any token left behind by an earlier run is removed FIRST. A wrapper killed
    # with SIGKILL skips its trap, and round 5's own attack run left one live —
    # found by testing my own fix rather than by a reviewer. The per-PID name and
    # the sweep together mean a stale token is neither at the path a later run
    # uses nor left lying at a predictable one.
    rm -f "$staging"/.attest-token.* 2>/dev/null || true
    token="$(sample_digest "$repo")"
    token_file="$staging/.attest-token.$$"
    printf '%s' "$token" > "$token_file"
    # Deliberately NOT echoed. A reviewer replayed round 4's PRINTED digest from a
    # rejected run into a direct cargo invocation and reproduced the TOCTOU. There
    # is nothing to replay out of a log that never contained it.
    trap 'rm -f "$token_file"' EXIT RETURN

    attested_run "$repo" env \
        "$ATTEST_VAR=$token" \
        "${ATTEST_VAR}_FILE=$token_file" \
        "CARGO_TARGET_DIR=$staging" \
        "$@"
    rc=$?

    if [ "$rc" -ne 0 ]; then
        rm -f "$token_file"
        echo "  NOT PUBLISHED: nothing was moved out of staging; any binary in" >&2
        echo "  $staging is unattested and must not be used." >&2
        return "$rc"
    fi

    for rel in "${publish[@]+"${publish[@]}"}"; do
        promote "$staging" "$repo" "$rel" || return 4
        echo "  published: target/$rel"
    done
    echo "ATTESTED: tree verified identical before and after artifact production"
    return 0
}

main "$@"
