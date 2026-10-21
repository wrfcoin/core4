#!/usr/bin/env bash
# Fail-closed, state-carrying genesis-cut composition rehearsal (#2498/#2557).
#
# Usage:
#   scripts/verify-cut-composition.sh
#
# This is intentionally not a baseline-vs-composed source sweep. It resolves
# every required cut PR to an immutable SHA, proves merged PRs are in current
# main, merges every open PR into a run-scoped detached worktree, requires each
# resulting stage to build, then carries one chain through the connected
# lifecycle in native-chain/tests/cut_composition_lifecycle_2498.rs.
#
# A missing PR, failed fetch, closed-unmerged PR, SHA race, merge conflict,
# build failure, absent test, or failed lifecycle is a non-zero result. There is
# no `--quick` arm and no "zero gates means success" arm.

set -Eeuo pipefail

if [ "$#" -ne 0 ]; then
    echo "usage: scripts/verify-cut-composition.sh" >&2
    echo "This cut gate has no skip-tests or quick mode." >&2
    exit 2
fi

REPO_SLUG=wrfcoin/core4
# The immutable gate inventory. PR numbers survive branch deletion and rename.
# Order is load-bearing for open gates: provenance first, genesis metadata
# writer before the declaration/enforcement gate.
REQUIRED_GATE_PRS=(
    2496 # block-1 registration agreement + producer tripwire
    2510 # genesis-state WAL anchor
    2508 # canonical reward provenance
    2497 # wall-clock emission curve
    2535 # block-1 registration construction/submission
    2556 # genesis protocol metadata writer
    2540 # genesis oracle declaration and block-1 enforcement
)

die() {
    echo "FAIL: $*" >&2
    exit 1
}

for required in git gh jq cargo mktemp grep tee; do
    command -v "$required" >/dev/null 2>&1 ||
        die "required command '$required' is not available"
done

repo=$(git rev-parse --show-toplevel 2>/dev/null) ||
    die "run this from a core4 checkout"
cd "$repo"

if [ -n "$(git status --porcelain --untracked-files=normal)" ]; then
    die "working tree is dirty; commit or remove local changes so HEAD names the code under test"
fi

start_oid=$(git rev-parse --verify HEAD^{commit}) ||
    die "cannot resolve starting commit"

tmp_base=${TMPDIR:-/tmp}
run_dir=$(mktemp -d "$tmp_base/wrfcoin-cut-composition.XXXXXXXX") ||
    die "cannot allocate a run directory"
case "$run_dir" in
    "$tmp_base"/wrfcoin-cut-composition.*) ;;
    *) die "mktemp returned an unexpected path: $run_dir" ;;
esac

run_id=$(basename "$run_dir")
tree=$run_dir/tree
target=$run_dir/target
test_report=$run_dir/lifecycle-test.txt
main_ref=refs/cut-composition/$run_id/main
run_refs=("$main_ref")
worktree_added=0

cleanup() {
    status=$?
    trap - EXIT INT TERM
    if [ "$worktree_added" -eq 1 ]; then
        git worktree remove --force "$tree" >/dev/null 2>&1 || true
    fi
    for ref in "${run_refs[@]}"; do
        git update-ref -d "$ref" >/dev/null 2>&1 || true
    done
    case "$run_dir" in
        "$tmp_base"/wrfcoin-cut-composition.*)
            rm -rf -- "$run_dir"
            ;;
    esac
    exit "$status"
}
trap cleanup EXIT INT TERM

printf 'run_id=%s\nstart_sha=%s\n' "$run_id" "$start_oid"

# Fetch into an invocation-unique ref. FETCH_HEAD and origin/main are shared
# process state and therefore not evidence when two rehearsals overlap.
if git fetch --quiet --no-tags origin "+refs/heads/main:$main_ref"; then
    :
else
    die "current main did not fetch"
fi
main_oid=$(git rev-parse --verify "$main_ref^{commit}") ||
    die "fetched main ref is not a commit"
printf 'main_sha=%s\n' "$main_oid"

git merge-base --is-ancestor "$main_oid" "$start_oid" ||
    die "carrier HEAD $start_oid is not based on current main $main_oid; rebase before rehearsing"

git worktree add --quiet --detach "$tree" "$start_oid" ||
    die "cannot create run-scoped detached worktree"
worktree_added=1

fetched_count=0
accounted_merged_count=0
merged_open_count=0
build_count=0

build_stage() {
    label=$1
    build_log=$run_dir/build-$build_count.log
    echo
    echo "=== BUILD $label ==="
    if env RUSTC_WRAPPER= CARGO_TARGET_DIR="$target" \
        cargo check --workspace --all-targets --message-format short 2>&1 |
        tee "$build_log"; then
        :
    else
        die "$label did not build with cargo check --workspace --all-targets"
    fi
    grep -F "Finished" "$build_log" >/dev/null ||
        die "$label build produced no Cargo completion record"
    build_count=$((build_count + 1))
}

echo
echo "=== RESOLVE REQUIRED CUT GATES ==="
for pr in "${REQUIRED_GATE_PRS[@]}"; do
    metadata=$(gh api "repos/$REPO_SLUG/pulls/$pr") ||
        die "PR #$pr metadata did not fetch"

    state=$(jq -er '.state' <<<"$metadata") ||
        die "PR #$pr metadata has no state"
    head_sha=$(jq -er '.head.sha' <<<"$metadata") ||
        die "PR #$pr metadata has no head SHA"
    head_ref=$(jq -er '.head.ref' <<<"$metadata") ||
        die "PR #$pr metadata has no head ref"
    merged_at=$(jq -r '.merged_at // ""' <<<"$metadata")
    merge_sha=$(jq -r '.merge_commit_sha // ""' <<<"$metadata")

    gate_ref=refs/cut-composition/$run_id/pr-$pr
    run_refs+=("$gate_ref")

    # Elsa's acceptance control mutates this condition to
    # `if false && git fetch ...`; the else arm MUST exit non-zero before any
    # merge, build, or test step can be credited.
    if git fetch --quiet --no-tags origin "+refs/pull/$pr/head:$gate_ref"; then
        :
    else
        die "required gate PR #$pr ($head_ref) did not fetch"
    fi
    fetched_sha=$(git rev-parse --verify "$gate_ref^{commit}") ||
        die "required gate PR #$pr fetched no commit"
    [ "$fetched_sha" = "$head_sha" ] ||
        die "PR #$pr changed during resolution: API=$head_sha fetched=$fetched_sha; rerun"
    fetched_count=$((fetched_count + 1))

    if [ "$state" = "closed" ] && [ -n "$merged_at" ]; then
        [ -n "$merge_sha" ] ||
            die "merged PR #$pr has no merge commit SHA"
        git cat-file -e "$merge_sha^{commit}" 2>/dev/null ||
            die "merged PR #$pr merge commit $merge_sha is absent from fetched main"
        git merge-base --is-ancestor "$merge_sha" "$main_oid" ||
            die "PR #$pr says merged at $merge_sha, but that commit is not in current main"
        printf '  PR #%-4s %-45s MERGED head=%s merge=%s\n' \
            "$pr" "$head_ref" "$head_sha" "$merge_sha"
        accounted_merged_count=$((accounted_merged_count + 1))
        continue
    fi

    if [ "$state" = "closed" ]; then
        die "required gate PR #$pr ($head_ref) is closed without merge"
    fi
    [ "$state" = "open" ] ||
        die "required gate PR #$pr has unsupported state '$state'"

    echo "  PR #$pr $head_ref OPEN head=$head_sha"
    if env \
        GIT_AUTHOR_NAME="WRFCoin cut composition" \
        GIT_AUTHOR_EMAIL="cut-composition@wrfcoin.invalid" \
        GIT_COMMITTER_NAME="WRFCoin cut composition" \
        GIT_COMMITTER_EMAIL="cut-composition@wrfcoin.invalid" \
        git -C "$tree" merge --quiet --no-ff --no-edit "$gate_ref"; then
        :
    else
        conflicts=$(git -C "$tree" diff --name-only --diff-filter=U | tr '\n' ' ')
        die "required gate PR #$pr failed to merge${conflicts:+; conflicts: $conflicts}"
    fi
    git -C "$tree" merge-base --is-ancestor "$head_sha" HEAD ||
        die "PR #$pr merge returned success but its exact head is absent from the composed tree"
    merged_open_count=$((merged_open_count + 1))
    build_stage "after PR #$pr ($head_sha)"
done

required_count=${#REQUIRED_GATE_PRS[@]}
[ "$fetched_count" -eq "$required_count" ] ||
    die "resolved $fetched_count of $required_count required gates"
[ $((accounted_merged_count + merged_open_count)) -eq "$required_count" ] ||
    die "only $((accounted_merged_count + merged_open_count)) of $required_count gates were accounted"

# If every gate was already merged, no per-open-gate build ran. Current main
# still has to build; zero build steps can never authorize the cut.
if [ "$build_count" -eq 0 ]; then
    build_stage "current main plus carrier"
fi

composed_oid=$(git -C "$tree" rev-parse --verify HEAD^{commit}) ||
    die "cannot resolve final composed SHA"
printf '\ncomposed_sha=%s\nresolved_gates=%s\nbuild_steps=%s\n' \
    "$composed_oid" "$fetched_count" "$build_count"

echo
echo "=== ONE STATE-CARRYING CHAIN ==="
if (
    cd "$tree"
    env RUSTC_WRAPPER= CARGO_TARGET_DIR="$target" \
        cargo test -p wrfcoin-native-chain \
        --features cut-composition-harness \
        --test cut_composition_lifecycle_2498 \
        -- --test-threads=1 --nocapture
) 2>&1 | tee "$test_report"; then
    :
else
    die "the connected genesis/WAL/reduction/peer lifecycle failed"
fi

grep -F \
    "test one_chain_survives_genesis_wal_reduction_and_peer_sync_2557 ... ok" \
    "$test_report" >/dev/null ||
    die "the named lifecycle test did not execute successfully"
grep -E \
    'test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' \
    "$test_report" >/dev/null ||
    die "the lifecycle target did not report exactly 1 passed / 0 failed"

echo
echo "CUT COMPOSITION: PASS"
echo "  required gates fetched: $fetched_count/$required_count"
echo "  merged gates in main:   $accounted_merged_count"
echo "  open gates composed:    $merged_open_count"
echo "  fatal build stages:     $build_count"
echo "  lifecycle tests:        1 passed / 0 failed"
echo "  final composed SHA:     $composed_oid"
