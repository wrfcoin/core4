#!/usr/bin/env python3
"""Assert every PR path filter still triggers on the changes it claims to gate (#2916).

WHY THIS EXISTS
---------------
A `paths:` filter that stops matching what it gates is a check that silently
disappears: still listed, still green-looking, covering nothing. It has already
happened here — #2245 blocker 2, where the determinism filter listed
sub-directories, so a PR touching only `shared-protocol/src/blockchain` (the live
consensus state code) started no job at all. That produced
`check_determinism.py --check-workflow-paths`.

That guard is hardcoded to ONE file:

    check_determinism.py:1354   WORKFLOW = ".github/workflows/determinism-lint.yml"

Measured on 585abd1df: 20 PR-triggered workflows, **18** carry a `paths:` filter,
and exactly **one** asserts anything about it. This script covers the rest. It is
the same failure class as the 19-day VOID determinism gate (#2908) and the ratchet
stranded on a dispatch-only trigger (#2888): present, configured, covering nothing.

FAIL CLOSED
-----------
A guard that quietly skips what it cannot understand is *itself* a gate covering
nothing, which would be an unhappy way to fix this particular bug. So every one of
these is an ERROR, never a skip:

  * a workflow with a `pull_request: paths:` filter and no manifest entry
  * a manifest entry naming a workflow that does not exist
  * a workflow whose `pull_request:` trigger the parser cannot read, when the file
    plainly contains one (a parser blind spot must not read as "nothing to check")
  * an entry whose `must_trigger` list is empty
  * more `unreviewed` entries than the manifest's own declared ceiling

WHAT IS DECLARED, AND WHY IT IS EXAMPLE PATHS RATHER THAN DIRECTORIES
--------------------------------------------------------------------
The original guard asks "does a glob cover this directory root". That cannot
express the filters actually in use here: four workflows gate on `**/*.rs`, which
covers every Rust file but no directory wholesale.

So the manifest declares concrete example paths instead, in both directions:

    must_trigger      changing this file MUST start the workflow
    must_not_trigger  changing this file must NOT start it   (optional)

That is a stronger primitive: it states the property a reader actually cares about,
it works for any glob shape, and `must_not_trigger` catches the opposite drift — a
filter widened until it fires on everything is also a filter that stopped meaning
anything.

The declaration lives in `scripts/workflow_path_coverage.json` rather than in the
workflows, deliberately: this change then touches no workflow file, and three PRs
(#2668, #2448, #2914) are open inside two of them.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

# Imported, not copied. A second implementation of the trigger parser would drift
# from the first, and the two would disagree about which trigger lost coverage.
from check_determinism import _trigger_paths  # noqa: E402

MANIFEST = "scripts/workflow_path_coverage.json"
WORKFLOW_DIR = ".github/workflows"


def glob_to_regex(pattern: str) -> re.Pattern[str]:
    """GitHub Actions path-filter glob semantics.

    `**` crosses directory separators, `*` does not, `?` is one non-separator
    character. Written out rather than reached for via `fnmatch`, whose `*` also
    crosses `/` and would therefore report coverage that does not exist.
    """
    out = ["^"]
    i = 0
    while i < len(pattern):
        char = pattern[i]
        if pattern.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif char == "*":
            out.append("[^/]*")
            i += 1
        elif char == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(char))
            i += 1
    out.append("$")
    return re.compile("".join(out))


def matches_any(globs: list[str], path: str) -> bool:
    return any(glob_to_regex(g).match(path) for g in globs)



def declared_filter_keys(text: str, trigger: str) -> set[str] | None:
    """Which filter keys the trigger block DECLARES, regardless of what they parse to.

    Returns `None` when the trigger is absent. Otherwise a subset of
    {"paths", "paths-ignore"} — possibly empty.

    This exists because `_trigger_paths` returns `[]` for two situations that are
    not the same thing (#2917 review):

      * the trigger declares no filter at all — legitimate, and what the
        always-on gate workflows in this repo deliberately do
      * the trigger declares a `paths:` key that yields zero usable globs —
        always a defect, because something was declared and nothing came out

    Collapsing those two produced the bug this function fixes: a workflow whose
    `paths:` parsed to nothing was dropped from the scan BEFORE either manifest
    assertion ran, so the positive `must_trigger` claims that are supposed to
    make the negative `must_not_trigger` claims admissible were never evaluated
    either. Both directions vanished together and the checker reported success —
    a guard that is correct wherever it runs, failing to run on the one input
    that matters.

    Scoped by indentation, the same way `_trigger_paths` scopes, so `push:`
    declaring `paths:` cannot be mistaken for `pull_request:` declaring one.
    """
    lines = text.splitlines()
    trig_indent: int | None = None
    keys: set[str] = set()
    for raw in lines:
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        indent = len(raw) - len(raw.lstrip())
        stripped = raw.strip()
        if trig_indent is None:
            if re.match(rf"^{re.escape(trigger)}:\s*$", stripped):
                trig_indent = indent
            continue
        if indent <= trig_indent and not stripped.startswith("-"):
            break
        for key in ("paths-ignore", "paths"):
            if re.match(rf"^{re.escape(key)}:", stripped):
                keys.add(key)
                break
    return keys if trig_indent is not None else None


def load_manifest(root: Path) -> tuple[dict, list[str]]:
    path = root / MANIFEST
    if not path.is_file():
        return {}, [f"{MANIFEST} not found — this guard cannot run, which is not a pass"]
    try:
        return json.loads(path.read_text(encoding="utf-8")), []
    except json.JSONDecodeError as exc:
        return {}, [f"{MANIFEST} is not valid JSON: {exc}"]


def pr_filtered_workflows(root: Path) -> tuple[dict[str, list[str]], list[str]]:
    """Every workflow with a `pull_request: paths:` filter, and its globs."""
    found: dict[str, list[str]] = {}
    problems: list[str] = []
    directory = root / WORKFLOW_DIR
    if not directory.is_dir():
        return {}, [f"{WORKFLOW_DIR} not found"]
    for path in sorted(directory.glob("*.y*ml")):
        rel = f"{WORKFLOW_DIR}/{path.name}"
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as exc:
            problems.append(f"{rel} could not be read ({exc}) — refusing to treat that as no filter")
            continue
        globs = _trigger_paths(text, "pull_request")
        declared = declared_filter_keys(text, "pull_request")
        declares_pr = re.search(r"^\s*pull_request:\s*$", text, re.MULTILINE) is not None

        if globs is None or declared is None:
            if declares_pr:
                problems.append(
                    f"{rel} contains a `pull_request:` key but the trigger parser returned "
                    "nothing — a parser blind spot must not read as 'no filter to check'"
                )
            continue

        if "paths-ignore" in declared:
            problems.append(
                f"{rel} filters its `pull_request:` trigger with `paths-ignore:`, which this "
                "guard does not model. It IS filtered, so treating it as unfiltered would "
                "silently exempt it. Add support or convert it to `paths:` — an unmodelled "
                "filter must not pass"
            )
            continue

        if "paths" in declared and not globs:
            problems.append(
                f"{rel} declares a `pull_request: paths:` key that parsed to ZERO usable globs. "
                "Something was declared and nothing came out — an empty or malformed list, or a "
                "form this parser cannot read. Either way the workflow triggers on nothing and "
                "every manifest assertion about it would be vacuous, so this is a defect and "
                "never a legitimate 'no filter' (#2917)"
            )
            continue

        if globs:
            # #2919: _trigger_paths now strips quotes of either kind itself, so
            # globs arrive clean here without a second, local normalisation
            # step that could drift from the shared parser.
            found[rel] = globs
    return found, problems



def anchored_ceiling(root: Path, anchor_ref: str) -> tuple[int | None, str | None]:
    """`max_unreviewed` as committed on `anchor_ref`, for ratcheting.

    A ceiling a change can raise in the same commit is not a ceiling. Comparing
    the manifest against itself — which is what this did before the #2917 review
    — makes "the ceiling only ever goes down" a sentence in a file rather than a
    gate: editing 2 to 3 stayed green. Same anchoring problem as the determinism
    baseline, and the same fix: read the value from a commit the PR cannot edit.
    """
    import subprocess

    try:
        blob = subprocess.run(
            ["git", "-C", str(root), "show", f"{anchor_ref}:{MANIFEST}"],
            capture_output=True, text=True, check=False,
        )
    except OSError as exc:
        return None, f"could not run git to read {MANIFEST} at {anchor_ref}: {exc}"
    if blob.returncode != 0:
        # The base has no manifest: this is the PR that introduces it, and there
        # is no established ceiling to ratchet against. Returning 0 here would
        # make the introducing PR fail its own ratchet. Reported loudly rather
        # than silently, because "no anchor" and "anchored and passing" must not
        # look alike — and because deleting the manifest and re-adding it with a
        # higher ceiling would land here, where a reviewer can see it.
        return None, None
    try:
        value = json.loads(blob.stdout).get("max_unreviewed")
    except json.JSONDecodeError as exc:
        return None, f"{MANIFEST} at {anchor_ref} is not valid JSON: {exc}"
    if not isinstance(value, int):
        return None, f"{MANIFEST} at {anchor_ref} has no integer max_unreviewed"
    return value, None


def check(root: Path, anchor_ref: str | None = None) -> list[str]:
    manifest, problems = load_manifest(root)
    if problems:
        return problems

    entries: dict = manifest.get("workflows", {})
    ceiling = manifest.get("max_unreviewed")
    if not isinstance(ceiling, int):
        return [f"{MANIFEST} must declare an integer `max_unreviewed` ceiling"]

    filtered, scan_problems = pr_filtered_workflows(root)
    problems.extend(scan_problems)

    for rel in sorted(entries):
        if not (root / rel).is_file():
            problems.append(
                f"{MANIFEST} has an entry for {rel}, which does not exist — a stale entry "
                "silently stops guarding a filter that may still be live"
            )

    unreviewed: list[str] = []
    for rel, globs in sorted(filtered.items()):
        entry = entries.get(rel)
        if entry is None:
            problems.append(
                f"{rel} has a `pull_request: paths:` filter but no {MANIFEST} entry. Declare "
                "what a change must trigger, or mark it `unreviewed` with a reason — an "
                "undeclared filter is exactly the drift this guard exists to catch"
            )
            continue
        if entry.get("unreviewed"):
            if not entry.get("why"):
                problems.append(f"{rel} is marked `unreviewed` without a `why`")
            unreviewed.append(rel)
            continue

        must = entry.get("must_trigger") or []
        if not must:
            problems.append(
                f"{rel} has an entry with no `must_trigger` paths — an empty assertion "
                "passes unconditionally and guards nothing"
            )
            continue
        for example in must:
            if not matches_any(globs, example):
                problems.append(
                    f"{rel}: a change to {example!r} would NOT start this workflow — no "
                    f"`pull_request: paths:` glob matches it. Globs are {globs}. A gate that "
                    "does not run cannot fail (#2245, #2916)"
                )
        for example in entry.get("must_not_trigger") or []:
            if matches_any(globs, example):
                problems.append(
                    f"{rel}: a change to {example!r} WOULD start this workflow, but the "
                    "manifest declares it out of scope. A filter widened until it matches "
                    "everything has stopped meaning anything"
                )

    if len(unreviewed) > ceiling:
        problems.append(
            f"{len(unreviewed)} workflows are marked `unreviewed` but the manifest ceiling is "
            f"{ceiling}. The ceiling only ever goes down: {', '.join(unreviewed)}"
        )

    # RATCHET. Without an anchor the ceiling is compared only against itself, so a
    # change can raise it in the same commit and stay green — which is prose, not
    # a gate (#2917 review).
    if anchor_ref:
        anchored, failure = anchored_ceiling(root, anchor_ref)
        if failure:
            problems.append(f"ratchet anchor unusable: {failure}")
        elif anchored is None:
            print(
                f"[ratchet] NO ANCHORED CEILING — {MANIFEST} does not exist at {anchor_ref}. "
                "This is the PR that introduces it; there is no established ceiling to ratchet "
                "against. If you did not expect this, the manifest was deleted and re-added."
            )
        elif ceiling > anchored:
            problems.append(
                f"max_unreviewed was raised from {anchored} to {ceiling} against {anchor_ref}. "
                "The ceiling only ever goes DOWN — raising it converts the unreviewed list into "
                "a parking space, which is the failure this ratchet exists to prevent. Pin the "
                "workflow instead, or lower the ceiling"
            )
        else:
            print(f"[ratchet] anchored to {anchor_ref}: ceiling {ceiling} <= {anchored}")
    else:
        print(
            "[ratchet] UNANCHORED — max_unreviewed compared only against itself "
            "(no PR base; the committed value is authoritative)"
        )
    return problems


def self_test() -> list[str]:
    """Prove the matcher can say NO. A matcher that always returns True would make
    every assertion above pass while guarding nothing."""
    failures = []
    cases = [
        ("consensus-engines/**", "consensus-engines/src/lib.rs", True),
        ("consensus-engines/**", "primitives/src/lib.rs", False),
        # The #2245 defect in one line: a sub-directory glob does not cover its parent.
        ("shared-protocol/src/blockchain/**", "shared-protocol/src/lib.rs", False),
        ("shared-protocol/src/**", "shared-protocol/src/blockchain/state.rs", True),
        ("**/*.rs", "tools/cli/WrfcoinCLI/src/main.rs", True),
        ("**/*.rs", "docs/governance/constitution.md", False),
        ("**/*.rs", "main.rs", True),
        # `*` must not cross a separator, or coverage is reported that does not exist.
        ("scripts/*.py", "scripts/check_determinism.py", True),
        ("scripts/*.py", "scripts/nested/thing.py", False),
        ("Cargo.lock", "Cargo.lock", True),
        ("Cargo.lock", "sub/Cargo.lock", False),
        ("**/Cargo.toml", "shared-protocol/Cargo.toml", True),
        ("**/Cargo.toml", "Cargo.toml", True),
    ]
    for pattern, path, expected in cases:
        got = matches_any([pattern], path)
        if got != expected:
            failures.append(f"SELF_TEST_FAILED {pattern!r} vs {path!r}: expected {expected}, got {got}")
    # #2919: the local `_unquote` shim is gone; this now proves the same
    # property through the shared parser this file actually depends on,
    # rather than a local copy that could drift from it.
    parsed = _trigger_paths('pull_request:\n  paths:\n    - "fuzz/**"\n', "pull_request")
    if parsed != ["fuzz/**"]:
        failures.append(
            f"SELF_TEST_FAILED double-quoted globs must be unquoted by _trigger_paths: {parsed}"
        )
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true", help="prove the matcher can reach NO")
    parser.add_argument("--root", default=".", help="repository root")
    parser.add_argument(
        "--anchor-ref",
        default=None,
        help="commit to ratchet max_unreviewed against (a PR base). Omitted on push-to-main, "
             "where there is no base and the committed value is authoritative.",
    )
    args = parser.parse_args()

    if args.self_test:
        failures = self_test()
        for line in failures:
            print(line, file=sys.stderr)
        if failures:
            return 1
        print("check_workflow_path_coverage.py self-test passed (matcher reaches both YES and NO)")
        return 0

    problems = check(Path(args.root), args.anchor_ref)
    for line in problems:
        print(f"::error::{line}" if "GITHUB_ACTIONS" in __import__("os").environ else line, file=sys.stderr)
    if problems:
        print(f"\n{len(problems)} path-coverage problem(s)", file=sys.stderr)
        return 1
    print("check_workflow_path_coverage.py: every declared filter still triggers on what it gates")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
