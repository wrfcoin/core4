#!/usr/bin/env python3
"""Forbid nondeterministic constructs in consensus-critical Rust crates (#2105, #2245).

Differential replay (#2103) catches nondeterminism *after* it is written; this lint
stops it being written. It scans production Rust for constructs whose output can differ
across machines, builds or runs — floats, wall-clock reads, unseeded randomness,
unordered-map iteration, unsorted directory reads, and consensus-path env reads — and
fails on any unjustified hit that is not already frozen in the baseline.

Inline escape: put `// determinism-ok: <justification>` on the SAME line or the line
immediately ABOVE a flagged construct. An empty justification is itself a failure. Every
active allow is printed so an auditor can review them.

    python3 scripts/check_determinism.py            # scan (gate mode)
    python3 scripts/check_determinism.py --report   # scan, print findings, exit 0
    python3 scripts/check_determinism.py --self-test # fixtures only, no repo scan

Three defects found by the #2245 review shaped the current design; each is described
where it is fixed:

  1. unordered-map iteration was matched by a ONE-LINE regex, so the ordinary
     across-lines shape went undetected — see `UnorderedTracker`;
  2. the scan root set omitted `shared-protocol/src/blockchain`, the live consensus
     state code — see `SCANNED_ROOTS`;
  3. the baseline could be blindly regenerated to absorb new violations — see
     `Baseline`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

# --- Scan scope ----------------------------------------------------------------------
#
# #2245 blocker 2: the previous constant listed six hand-picked directories and omitted
# `shared-protocol/src/blockchain` — which holds the live `Block`, `Blockchain`, `State`
# and state-transition application code. A planted `SystemTime::now` there passed the
# gate with `0 NEW`. `native-chain/src/blockchain` was listed, but it is only a
# re-export.
#
# A clean scan over the wrong root set is not a clean scan, and a hand-picked list has
# to be remembered every time a module is added. So the roots are now whole crate `src`
# trees — everything is scanned by default — and anything deliberately out of scope goes
# in EXCLUDED_SUBTREES with a reason. Being wrong now requires writing down why.
SCANNED_ROOTS: list[str] = [
    "consensus-engines/src",
    "primitives/src",
    "shared-protocol/src",
    "native-chain/src",
]

# Subtrees deliberately not scanned, each with the reason it is out of scope.
# Every entry must exist (a stale exclusion is a failure, not a silent no-op) and must
# state a reason, exactly like the waiver list in `invariants::registration_audit`.
EXCLUDED_SUBTREES: list[tuple[str, str]] = []

ALLOW_MARKER = "determinism-ok:"

# Map/set types with nondeterministic iteration order. IndexMap and BTreeMap are
# deliberately absent: their iteration order IS deterministic.
UNORDERED_TYPES = ("HashMap", "HashSet", "FxHashMap", "FxHashSet", "AHashMap", "AHashSet")
_UNORDERED_ALT = "|".join(UNORDERED_TYPES)


@dataclass(frozen=True)
class Rule:
    name: str
    pattern: re.Pattern[str]
    message: str


# Line-local rules. Each flags one nondeterminism class. Patterns are deliberately
# conservative heuristics; the allow-comment escape exists for justified exceptions.
RULES: list[Rule] = [
    Rule("float-type", re.compile(r"\b(f32|f64)\b"), "floating-point type in consensus code"),
    Rule(
        "float-literal",
        re.compile(r"\b\d+\.\d+(?:f32|f64)?\b"),
        "floating-point literal in consensus code",
    ),
    Rule(
        "wall-clock",
        re.compile(r"\b(SystemTime::now|Instant::now|Utc::now)\b"),
        "wall-clock read in consensus code",
    ),
    Rule(
        "unseeded-rng",
        re.compile(r"\b(rand::random|thread_rng|OsRng)\b"),
        "unseeded RNG in consensus code",
    ),
    Rule(
        "readdir-unsorted",
        re.compile(r"\bread_dir\("),
        "read_dir without a subsequent sort (directory order is not deterministic)",
    ),
    Rule(
        "env-var",
        re.compile(r"\benv::var(_os)?\("),
        "env::var read inside a consensus path",
    ),
]

# --- Unordered-map iteration, tracked ACROSS lines ------------------------------------
#
# #2245 blocker 1. The previous rule was a single regex requiring the type name and the
# iterator call on the SAME source line:
#
#     HashMap|HashSet|... [^;\n]* \.(iter|keys|values|...)\(
#
# and the self-test asserted it using exactly that artificial one-line shape:
#
#     pub fn m(h: &HashMap<u8,u8>) { for _ in h.iter() {} }
#
# So the test and the defect came from the same misunderstanding, and the test could
# never have failed. The ordinary shape —
#
#     pub fn tally(values: &HashMap<u64, u64>) -> u64 {
#         for value in values.values() { ... }        // <- different line
#
# — was invisible. A regex over one line cannot express this: the binding and its use
# are arbitrarily far apart. It needs two passes over the file.
#
# Pass 1 records every identifier bound to an unordered type — `let` bindings, function
# parameters and struct fields. Pass 2 flags iteration over any recorded name, in method
# form (`.iter()/.keys()/.values()/...`) or bare (`for x in map`, `for x in &map`).
#
# Deliberately over-approximate, in the safe direction: names are tracked per FILE, not
# per scope, so a `values` bound to a HashMap in one function makes `values.values()` in
# another function a finding too. Over-flagging costs one `// determinism-ok:` line;
# under-flagging costs a consensus split. This is the same trade the #2278 unwrap ratchet
# settled on.

_BINDING_PATTERNS: list[re.Pattern[str]] = [
    # `let x: HashMap<..>` / `let mut x: &FxHashSet<..>`
    re.compile(
        rf"\blet\s+(?:mut\s+)?(\w+)\s*:\s*&?(?:mut\s+)?(?:[\w:]*::)?(?:{_UNORDERED_ALT})\b"
    ),
    # `let x = HashMap::new()` / `= HashMap::from(..)` / `.collect::<HashMap<_,_>>()`
    re.compile(rf"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=]*)?=\s*[^;]*\b(?:{_UNORDERED_ALT})\b"),
    # Function parameters and struct fields: `x: &HashMap<..>`, `pub x: FxHashMap<..>`
    re.compile(rf"\b(\w+)\s*:\s*&?(?:mut\s+)?(?:[\w:]*::)?(?:{_UNORDERED_ALT})\s*<"),
]

_ITER_METHODS = ("iter", "keys", "values", "iter_mut", "values_mut", "into_iter", "drain")


@dataclass
class UnorderedTracker:
    """Names in one file that are bound to an unordered map/set type."""

    names: set[str] = field(default_factory=set)

    def collect(self, lines: list[str]) -> None:
        for line in lines:
            if _is_comment(line):
                continue
            if not any(t in line for t in UNORDERED_TYPES):
                continue
            for pat in _BINDING_PATTERNS:
                for m in pat.finditer(line):
                    name = m.group(1)
                    # `let` / type keywords can be captured by the loose param pattern.
                    if name in {"let", "mut", "pub", "fn", "impl", "for", "in", "self"}:
                        continue
                    self.names.add(name)

    def iteration_hit(self, line: str) -> str | None:
        """Return the offending expression if `line` iterates a tracked name."""
        for name in self.names:
            n = re.escape(name)
            # Method form, including through `self.<field>`.
            if re.search(rf"\b(?:self\.)?{n}\s*\.\s*(?:{'|'.join(_ITER_METHODS)})\s*\(", line):
                return name
            # Bare `for x in map`, `for x in &map`, `for x in &mut map`.
            if re.search(rf"\bfor\s+[^;{{]+?\s+in\s+&?(?:mut\s+)?(?:self\.)?{n}\s*[{{.]", line):
                return name
        return None


def _is_comment(line: str) -> bool:
    return line.lstrip().startswith("//")


def _allow_justification(line: str) -> str | None:
    """Return the justification text after `determinism-ok:` on this line, or None."""
    idx = line.find(ALLOW_MARKER)
    if idx == -1:
        return None
    return line[idx + len(ALLOW_MARKER):].strip()


@dataclass
class Finding:
    path: str
    line_no: int
    rule: str
    message: str
    text: str


@dataclass
class Allow:
    path: str
    line_no: int
    justification: str


_ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?"
    r"(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?(fn|impl|mod|trait)\s+"
)


_CHAR_LIT_RE = re.compile(r"'(?:\\\\.|[^\\\\'])'")


@dataclass
class _StripState:
    """Cross-line lexical state for `_strip_literals` (#2905).

    Rust raw strings (`r"…"`, `r#"…"#`, `br##"…"##`) have no escapes, may span
    lines, and terminate only at `"` followed by the opener's exact number of
    `#`. The previous stateless per-line stripper treated their `"` as an
    ordinary string opener, so JSON braces inside a raw fixture string altered
    Rust brace depth and collapsed the `#[cfg(test)]` mask — a pre-existing
    test-only fixture (`NWS_FIXTURE` in oracle_attester.rs) was then scanned
    as production code and reported five float findings (#2905).
    """

    # Inside a raw string: the number of `#` in its delimiter. None = not inside.
    raw_hashes: int | None = None
    # Inside `/* … */`: nesting depth (Rust block comments nest). 0 = not inside.
    # Comment recognition MUST run before raw-string recognition: `r#"` is valid
    # prose inside a comment, and treating it as an opener carries raw-string
    # state across the comment's end — the #2906 review demonstrated that
    # over-masking a test module's closing brace this way hides a following
    # production float entirely (a silent-blindness path, worse than noise).
    block_depth: int = 0
    # Inside an ordinary (or byte) string literal that did not close on its line.
    # Rust strings may span lines, so a stateless stripper reads the continuation
    # line's CLOSING quote as an OPENER and hides the real code after it. That is
    # the blinding shape secret-leak's lexer fixed in #3286; rules now match on the
    # stripped text, so it would blind this lint too (#3197 follow-up).
    in_str: bool = False


# A raw-string opener: optional byte (`b`) or C-string (`c`, Rust >= 1.77) prefix,
# `r`, zero+ hashes, quote. Matched only when the preceding character cannot extend
# an identifier, so `four"` / `attr#"` tails are not misread as openers. Missing the
# `c` prefix read `cr"C:\"` as an ordinary string whose `\"` is an escape, so quote
# pairing never re-synced and every rule was blind to EOF (#3293 review).
_RAW_OPEN_RE = re.compile(r'[bc]?r(#*)"')


def _strip_literals(
    line: str, state: _StripState | None = None, comments: list[str] | None = None
) -> str:
    """Drop line comments and string/char literal bodies before counting braces.

    Only needs to be good enough that a `{` inside `"{"` does not desynchronise depth
    tracking; it is not a Rust parser. Pass one `_StripState` across successive lines
    of a file so multi-line raw strings are consumed as literal content (#2905);
    without a state, each call treats its line in isolation (single-line semantics,
    used by the self-tests).

    When `comments` is given, the text of every comment on this line (line and block,
    delimiters included) is appended to it. That is the only place a `determinism-ok:`
    marker counts: a marker inside a string literal is data, not an annotation.
    """
    st = state if state is not None else _StripState()
    out, i, n = [], 0, len(line)
    while i < n:
        if st.in_str:
            while i < n:
                if line[i] == "\\":
                    i += 2
                    continue
                if line[i] == '"':
                    i += 1
                    st.in_str = False
                    break
                i += 1
            continue
        if st.raw_hashes is not None:
            terminator = '"' + "#" * st.raw_hashes
            end = line.find(terminator, i)
            if end == -1:
                # Rest of the line is raw-string content; stay inside for the
                # next line.
                return "".join(out)
            i = end + len(terminator)
            st.raw_hashes = None
            continue
        if st.block_depth > 0:
            # Inside a block comment: everything is prose — `r#"`, quotes and
            # braces included. Only `/*` (nesting) and `*/` change state.
            open_ix = line.find("/*", i)
            close_ix = line.find("*/", i)
            if close_ix == -1 and open_ix == -1:
                if comments is not None:
                    comments.append(line[i:])
                return "".join(out)
            if open_ix != -1 and (close_ix == -1 or open_ix < close_ix):
                st.block_depth += 1
                nxt = open_ix + 2
            else:
                st.block_depth -= 1
                nxt = close_ix + 2
            if comments is not None:
                comments.append(line[i:nxt])
            i = nxt
            continue
        c = line[i]
        if c == "/" and i + 1 < n:
            if line[i + 1] == "/":
                if comments is not None:
                    comments.append(line[i:])
                break
            if line[i + 1] == "*":
                st.block_depth = 1
                if comments is not None:
                    comments.append("/*")
                i += 2
                continue
        if c in "bcr":
            m = _RAW_OPEN_RE.match(line, i)
            if m and (i == 0 or not (line[i - 1].isalnum() or line[i - 1] == "_")):
                st.raw_hashes = len(m.group(1))
                i = m.end()
                continue
        if c == "'":
            # A `'` is only a char literal if it CLOSES: 'x', '\n', '\''. Otherwise it is a
            # LIFETIME (`'static`, `'a`), and consuming to end-of-line swallows real code.
            #
            # This was a live defect, not a hypothetical. `MutexGuard<'static, ()> {` had
            # its trailing brace eaten, which desynchronised brace-depth tracking and
            # closed `mod tests` 187 lines early in launch_api/main.rs — so module-level
            # test helpers were scanned as production code and reported as findings.
            m = _CHAR_LIT_RE.match(line, i)
            if m:
                i = m.end()
                continue
            out.append(c)
            i += 1
            continue
        if c == '"':
            # Hand off to the in-string branch at the top of the loop, which consumes
            # to the closing quote or carries the open string to the next line.
            st.in_str = True
            i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def _test_mask(lines: list[str]) -> list[bool]:
    """Per-line mask: is this line inside a `#[cfg(test)]` module or `#[test]` fn?

    Tracks brace depth so the test region **ends**. The previous implementation scanned
    BACKWARDS up to 400 lines for a `#[cfg(test)]`/`mod tests` marker and answered yes if
    it found one — which exempted every hit appearing *after* a test module in the same
    file. A production `SystemTime::now()` appended below `mod tests` was therefore
    invisible to this lint entirely (#2245 review).

    This is the same defect, and the same fix, as the `unsafe` gate's scope predicate in
    `check_unsafe_justified.py` (#2106). Ported deliberately rather than reinvented,
    including the `use path::{A, B};` guard below, which that port had to learn the hard
    way: consuming the next line with ANY brace opens a bogus region at depth 0 and masks
    the rest of the file.
    """
    mask = [False] * len(lines)
    depth = 0
    pending = False
    test_depth: int | None = None
    # One lexical state across the whole file so multi-line raw strings are
    # literal content, not brace-bearing Rust syntax (#2905).
    strip_state = _StripState()
    for i, raw in enumerate(lines):
        line = _strip_literals(raw, strip_state)
        stripped = line.strip()
        if test_depth is None and (
            stripped.startswith("#[cfg(test)]")
            or stripped.startswith("#[test]")
            or re.match(r"#\[\w+(::\w+)*::test\]", stripped)
        ):
            pending = True
        opens, closes = line.count("{"), line.count("}")
        if test_depth is not None:
            mask[i] = True
        elif pending:
            if _ITEM_RE.match(line) and opens > 0:
                test_depth = depth
                mask[i] = True
                pending = False
            elif stripped and not stripped.startswith("#["):
                pending = False
        depth += opens - closes
        if test_depth is not None and depth <= test_depth:
            test_depth = None
    return mask


def scan_file(path: Path, root: Path) -> tuple[list[Finding], list[Allow], list[str]]:
    findings: list[Finding] = []
    allows: list[Allow] = []
    errors: list[str] = []
    rel = str(path.relative_to(root))
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()

    tracker = UnorderedTracker()
    tracker.collect(lines)
    in_test = _test_mask(lines)
    # Line rules match CODE, not prose (#3197 follow-up): string/char literal bodies
    # and comments are removed first, with one lexical state across the file so
    # multi-line strings, raw strings and block comments are consumed correctly.
    # Before this, `"1.0.0"`, `// 0.001 Gwei` and `"127.0.0.1"` were float findings.
    # Fingerprints still use the raw line, so existing baseline entries keep matching.
    code_state = _StripState()
    code: list[str] = []
    # Comment text per line from the same lexical pass. `determinism-ok:` counts only
    # there: `(1.5, "// determinism-ok: …")` used to suppress the real float,
    # because markers were read from the raw line (#3293 review; #3286 finding 2).
    notes: list[str] = []
    for raw in lines:
        segs: list[str] = []
        code.append(_strip_literals(raw, code_state, segs))
        notes.append("".join(segs))

    for i, line in enumerate(lines):
        # Record + validate allow markers wherever they appear.
        just = _allow_justification(notes[i])
        if just is not None:
            if not just:
                errors.append(f"{rel}:{i + 1}: `{ALLOW_MARKER}` requires a non-empty justification")
            else:
                allows.append(Allow(rel, i + 1, just))

        if _is_comment(line):
            continue
        if in_test[i]:
            continue

        here = _allow_justification(notes[i])
        above = _allow_justification(notes[i - 1]) if i > 0 else None
        allowed = bool(here or above)

        for rule in RULES:
            if rule.pattern.search(code[i]):
                if allowed:
                    continue
                findings.append(Finding(rel, i + 1, rule.name, rule.message, line.strip()))

        name = tracker.iteration_hit(line)
        if name is not None and not allowed:
            findings.append(
                Finding(
                    rel,
                    i + 1,
                    "unordered-iteration",
                    f"iteration over unordered `{name}` "
                    "(use BTreeMap/BTreeSet, or collect and sort first)",
                    line.strip(),
                )
            )

    return findings, allows, errors


def _excluded(rel_path: str, exclusions: list[tuple[str, str]]) -> bool:
    return any(rel_path == p or rel_path.startswith(p.rstrip("/") + "/") for p, _ in exclusions)


def scan(
    root: Path,
    paths: list[str],
    exclusions: list[tuple[str, str]] | None = None,
) -> tuple[list[Finding], list[Allow], list[str]]:
    exclusions = exclusions if exclusions is not None else EXCLUDED_SUBTREES
    all_findings: list[Finding] = []
    all_allows: list[Allow] = []
    all_errors: list[str] = []

    # A stale exclusion is a failure: it silently shrinks the scan and looks like a pass.
    for p, reason in exclusions:
        if not (root / p).exists():
            all_errors.append(f"excluded subtree does not exist: {p}")
        if not reason.strip():
            all_errors.append(f"excluded subtree {p} states no reason")

    for rel in paths:
        base = root / rel
        if not base.exists():
            all_errors.append(f"scanned path does not exist: {rel}")
            continue
        for f in sorted(base.rglob("*.rs")):
            if _excluded(str(f.relative_to(root)), exclusions):
                continue
            findings, allows, errors = scan_file(f, root)
            all_findings.extend(findings)
            all_allows.extend(allows)
            all_errors.extend(errors)
    return all_findings, all_allows, all_errors


def _report(findings: list[Finding], allows: list[Allow], errors: list[str]) -> None:
    if allows:
        print(f"Active determinism-ok allows ({len(allows)}):")
        for a in allows:
            print(f"  {a.path}:{a.line_no}: {a.justification}")
    if errors:
        print(f"\nMalformed allows ({len(errors)}):")
        for e in errors:
            print(f"  {e}")
    if findings:
        print(f"\nUnjustified nondeterminism findings ({len(findings)}):")
        for fi in findings:
            print(f"  {fi.path}:{fi.line_no} [{fi.rule}] {fi.message}\n      {fi.text}")


# --- Baseline with a non-growth assertion ---------------------------------------------
#
# #2245 blocker 3. `--update-baseline` used to rewrite the frozen set from whatever the
# current scan produced, and gate mode trusted every loaded fingerprint. A baseline
# regenerated with planted float/clock/RNG findings grew 726 -> 730 and the gate then
# passed, with CI permitting the baseline to change in the same PR — so review was the
# only guard.
#
# Two things are now pinned, not one:
#
#   * FINGERPRINTS, so a new violation site cannot be absorbed; and
#   * OCCURRENCE COUNTS per fingerprint, because `path|rule|text` collapses identical
#     lines within one file. Adding a second copy of an already-frozen line adds a real
#     violation but no new fingerprint, and a fingerprint-only pin would not see it.
#     The reviewed tree showed exactly this gap: 798 occurrences behind 726 fingerprints.
#
# The stored digest covers the findings map, so hand-editing counts is detected too.
# Growth is still possible — it must be, or the first genuine exception is unshippable —
# but only through `--update-baseline --authorize-additions "<reason>"`, which refuses to
# run without a reason and records what was added. A reviewer then reads a short list of
# authorised additions instead of diffing a thousand-line file.

BASELINE_VERSION = 2


def _fingerprint(f: Finding) -> str:
    """Line-independent identity so edits above a finding don't churn the baseline."""
    return f"{f.path}|{f.rule}|{f.text}"


def _occurrences(findings: list[Finding]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for f in findings:
        counts[_fingerprint(f)] = counts.get(_fingerprint(f), 0) + 1
    return dict(sorted(counts.items()))


def _digest(counts: dict[str, int], authorizations: list[dict] | None = None) -> str:
    """Digest over the findings AND the authorization records.

    Authorizations used to be outside the digest, which made them decorative: a PR could
    run `--update-baseline --authorize-additions "<reason>"` to grow the baseline, then
    delete the authorization record it had just written, and the file still verified. The
    written record of *why* the debt grew could be removed while the debt stayed — so the
    only human-readable justification in the mechanism was optional in practice (#2245
    review).

    Covering both means removing or editing an authorization invalidates the baseline, and
    the only way to restore a matching digest is to re-run `--update-baseline`, which
    writes a fresh authorization. The laundering path closes.

    What this is and is not: a tamper-EVIDENCE mechanism, not a cryptographic one. Anyone
    editing the file can also recompute the digest. The value is that doing so is a
    visible, reviewable diff rather than a silent deletion.
    """
    canonical = json.dumps(
        {"findings": counts, "authorizations": authorizations or []},
        sort_keys=True,
        separators=(",", ":"),
    )
    return "sha256:" + hashlib.sha256(canonical.encode("utf-8")).hexdigest()


@dataclass
class Baseline:
    counts: dict[str, int]
    authorizations: list[dict]

    @property
    def fingerprints(self) -> int:
        return len(self.counts)

    @property
    def occurrences(self) -> int:
        return sum(self.counts.values())


# The recovery instruction for the DIGEST-mismatch refusal (#2946).
#
# WORDING IS DELIBERATELY #2949's. That PR landed first and fixed the v1 dead end
# (#2947 item 2); its `_refusal_process_self_test` asserts the literal phrase
# "move it aside" appears in a refusal. Introducing a second phrasing here would
# both break a guard already on `main` and leave the file with two vocabularies for
# one instruction — which is a milder version of the contradiction #2947 was about.
# So this reuses "move it aside" and the `.v1.bak` shape, and adds only what the v1
# message has no reason to say: the git-restore option for an unintended edit, and
# what regeneration costs.
#
# Moving the file aside rather than editing in place is the point, not an
# inconvenience: regeneration from absent starts the `authorizations` list EMPTY. You
# cannot keep the audit trail while discarding the integrity that made it meaningful,
# and the reviewer of the next PR sees a baseline with one fresh record and asks why.
# The phrase both this file's recovery texts share, and the one #2949's
# `_refusal_process_self_test` asserts on. Defined once so a reword has to touch
# the assertion too.
RECOVERY_PHRASE = "move it aside"

RECOVERY_INSTRUCTION = (
    "Recovery: if the edit was unintended, restore the committed file with "
    "`git checkout -- <path>`. Otherwise move it aside first "
    "(e.g. `mv <path> <path>.rejected`), then regenerate with "
    "`--update-baseline <path> --authorize-additions \"<reason>\"` — run in place, "
    "--update-baseline REFUSES to overwrite a baseline that fails its own digest "
    "(#2946). Regenerating this way deliberately DISCARDS the prior `authorizations` "
    "history: an audit trail carried across a broken digest is not an audit trail."
)


def load_baseline(path: Path) -> tuple[Baseline | None, list[str]]:
    """Load and self-verify a baseline. Returns (baseline, errors)."""
    if not path.is_file():
        return None, [f"baseline file not found: {path}"]
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as e:
        return None, [f"baseline is not valid JSON: {e}"]

    if isinstance(raw, list):
        return None, [
            "baseline is in the unpinned v1 list format, which cannot detect tampering "
            "or duplicate growth (#2245 blocker 3). Recovery: move it aside first "
            "(e.g. `mv <path> <path>.v1.bak`), then regenerate with "
            "`--update-baseline <path> --authorize-additions \"<reason>\"` — run in "
            "place, --update-baseline REFUSES to overwrite a baseline it cannot load "
            "(#2944/#2947)."
        ]
    if raw.get("version") != BASELINE_VERSION:
        return None, [f"baseline version {raw.get('version')!r}, expected {BASELINE_VERSION}"]

    counts = raw.get("findings")
    if not isinstance(counts, dict):
        return None, ["baseline `findings` must be an object of fingerprint -> count"]
    counts = {str(k): int(v) for k, v in counts.items()}

    errors: list[str] = []
    stored_auths = raw.get("authorizations") or []
    actual_digest = _digest(counts, stored_auths)
    if raw.get("digest") != actual_digest:
        errors.append(
            "baseline digest does not match its own findings and authorizations — the "
            "file was edited by hand or is corrupt (stored "
            f"{raw.get('digest')}, actual {actual_digest}). Note the digest covers the "
            "`authorizations` list too: deleting the record that justified a baseline "
            "growth invalidates the file, by design."
        )
    totals = raw.get("totals") or {}
    if totals.get("fingerprints") != len(counts) or totals.get("occurrences") != sum(counts.values()):
        errors.append(
            f"baseline totals disagree with its findings (stored {totals}, actual "
            f"{{'fingerprints': {len(counts)}, 'occurrences': {sum(counts.values())}}})"
        )
    return Baseline(counts, raw.get("authorizations") or []), errors


def baseline_update_refusal(
    path: Path, prior: Baseline | None, load_errs: list[str]
) -> str | None:
    """Why `--update-baseline` must refuse to rewrite `path`, or None to proceed.

    #2946. The digest exists to detect hand editing — `load_baseline`'s own text says
    "the file was edited by hand or is corrupt". `--update-baseline` used to *repair*
    that condition instead of stopping for it: on a digest mismatch `load_baseline`
    appends to `errors` but still returns a NON-None `Baseline`, so the `prior is None`
    refusal #2944 added never fired, the rewrite proceeded, and the file was reissued
    with a fresh valid digest. Measured on main `4533aef62`: a `TAMPERED-MARKER` planted
    in `authorizations[0].reason` survived, exit 0, digest reissued. The hand edit became
    indistinguishable from a legitimate one — the tamper-evidence destroyed by the tool
    that is supposed to respect it.

    So the condition is `load_errs`, not `prior is None`. A digest mismatch is a
    STRONGER signal than a parse failure, not a weaker one, and it must refuse at least
    as hard.

    THIS IS A FUNCTION, not four lines inside `main()`, because of #2947: #2944's
    refusal lived only in `main()`, the self-test never calls `main()`, and so the one
    line that changed behaviour had no guard — revert it and every test still passed.
    A seam here lets the decision table be tested directly; `_update_refusal_self_test`
    additionally drives the real CLI, because a seam nothing wires up is the same defect
    one level down.

    A file that does not exist is NOT a refusal: bootstrapping a new baseline is the
    documented recovery path, and refusing it would leave a tampered baseline with no way
    forward.
    """
    if not path.is_file():
        return None
    if not load_errs:
        return None
    what = (
        "existing baseline could not be loaded"
        if prior is None
        else "existing baseline failed its own integrity check"
    )
    lines = [f"REFUSED: {what}:"]
    lines.extend(f"  {e}" for e in load_errs)
    lines.append(
        "\nRewriting it now would issue a fresh valid digest over the current contents, "
        "which would launder whatever edit invalidated it — the file would become "
        "indistinguishable from a legitimately generated one (#2946).\n"
    )
    # EXACTLY ONE recovery block. `load_baseline`'s v1 error carries its own recovery
    # text (#2949), because gate mode (`--baseline`) prints those errors without this
    # wrapper and would otherwise leave the operator with no way forward at all.
    # Appending unconditionally printed the instruction twice — two copies of the right
    # advice is a milder version of #2947's defect, and `_update_refusal_self_test`
    # counts `--update-baseline` invocations for exactly that reason.
    #
    # The probe is the shared phrase rather than this constant's own first line: after
    # #2949 the two texts are different strings saying the same thing, and matching on
    # my own wording would have silently stopped detecting theirs.
    body = "\n".join(lines)
    if RECOVERY_PHRASE not in body:
        body += "\n" + RECOVERY_INSTRUCTION
    return body.replace("<path>", str(path))


class AnchorError(Exception):
    """The anchor ref was requested but could not be resolved — fail closed."""


def _scan_at_ref(anchor_ref: str, cwd: Path | None = None) -> dict[str, int]:
    """Occurrences of the consensus-critical code **as committed at `anchor_ref`**.

    This is the bootstrap anchor. The base branch has no committed baseline yet, so the
    trustworthy suppression set is not any baseline FILE (the PR can edit that) but the set
    of findings that ALREADY EXIST in the base commit's CODE — content the PR cannot edit.
    A finding present in the base tree is pre-existing and suppressed; a finding the PR
    introduces is absent from the base tree and is therefore NEW.

    Extraction is via `git archive <ref> -- <root> | tar -x` into a throwaway dir, one root
    at a time so a root that does not yet exist at the base commit is skipped (everything
    under a brand-new root is legitimately new) rather than aborting the whole archive.
    """
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        present: list[str] = []
        for root in SCANNED_ROOTS:
            arch = subprocess.run(
                ["git", "archive", anchor_ref, "--", root], capture_output=True, cwd=cwd
            )
            if arch.returncode != 0:
                # Root absent at the base commit → its findings are all genuinely new.
                continue
            extract = subprocess.run(
                ["tar", "-xf", "-", "-C", str(tmp)], input=arch.stdout, capture_output=True
            )
            if extract.returncode != 0:
                raise AnchorError(
                    f"could not extract base tree for {root!r} at {anchor_ref}: "
                    f"{extract.stderr.decode('utf-8', 'replace')}"
                )
            present.append(root)
        if not present:
            return {}
        findings, _allows, _errors = scan(tmp, present, exclusions=EXCLUDED_SUBTREES)
        return _occurrences(findings)


def load_anchor_counts(
    anchor_ref: str, baseline_path: Path, cwd: Path | None = None
) -> tuple[dict[str, int] | None, str]:
    """The suppression set anchored to `anchor_ref` — content a PR cannot edit.

    Returns `(counts, note)`. When the ref has a committed baseline, `counts` is that
    baseline's frozen findings. When it does NOT (BOOTSTRAP: the baseline is being
    introduced on this branch), `counts` is the base commit's CODE scan — see `_scan_at_ref`
    — so anchoring stays ACTIVE even in the bootstrap PR and never downgrades to trusting the
    working baseline. `counts` is `None` only in the degenerate case of a bootstrap ref that
    contains neither a baseline nor any scanned code (an empty base), where there is
    genuinely nothing to suppress.

    Raises `AnchorError` if the ref itself cannot be resolved — that is a fail-closed
    condition, never a silent downgrade to unanchored. In CI this means the base commit was
    not fetched (needs `fetch-depth: 0`); we would rather the gate error than quietly let a
    PR self-authorize.

    WHY THIS EXISTS. Every earlier form anchored the baseline on a digest the PR's own run
    recomputed — so a PR could add a violation, grow its baseline to cover it, recompute the
    digest, and pass. The reviewer demonstrated exactly that (#2245). No value the PR script
    writes can anchor the PR: the anchor must come from content the PR does not control. The
    base branch is that content. A PR proposes changes to the baseline; the *committed base
    version* is fixed, and only fingerprints frozen there may suppress a finding. Baseline
    entries a PR adds are inert until they land on the base branch through a separate,
    reviewable merge.
    """
    # Resolve the ref to a concrete commit first, so "no such ref" is distinct from
    # "ref exists but has no baseline file".
    try:
        subprocess.run(
            ["git", "rev-parse", "--verify", "--quiet", f"{anchor_ref}^{{commit}}"],
            capture_output=True, text=True, check=True, cwd=cwd,
        )
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        raise AnchorError(
            f"anchor ref {anchor_ref!r} could not be resolved ({e}). In CI this usually "
            "means the base commit was not fetched — set `fetch-depth: 0` on checkout. "
            "Refusing to run unanchored: an unanchored gate lets a PR grow the baseline to "
            "cover its own violation (#2245)."
        ) from e

    # `git show <ref>:<path>` — path is repo-relative with forward slashes.
    rel = baseline_path.as_posix()
    show = subprocess.run(
        ["git", "show", f"{anchor_ref}:{rel}"], capture_output=True, text=True, cwd=cwd
    )
    if show.returncode != 0:
        # Ref resolved, but the baseline does not exist there → BOOTSTRAP. Do NOT fall back
        # to the working baseline (the PR can edit that): anchor to the base commit's CODE,
        # which the PR cannot edit. A `None` return here would let a bootstrap PR grow its
        # own baseline to cover a plant — the exact #2245 hole ("anchoring is not active in
        # this bootstrap PR"). Scanning the base code keeps the anchor load-bearing.
        counts = _scan_at_ref(anchor_ref, cwd=cwd)
        return (counts if counts else None), (
            f"BOOTSTRAP: {rel} does not exist at anchor ref {anchor_ref}; the committed "
            "baseline is introduced in this PR's diff. Anchoring the suppression set to the "
            f"base commit's CODE scan ({len(counts)} pre-existing fingerprints) — NOT the "
            "working baseline — so a plant this PR adds is NEW against the base and cannot be "
            "self-suppressed by growing the baseline. Switches to the committed baseline once "
            "it lands on the base branch."
        )
    try:
        raw = json.loads(show.stdout)
    except json.JSONDecodeError as e:
        raise AnchorError(f"anchor baseline at {anchor_ref}:{rel} is not valid JSON: {e}") from e
    counts = raw.get("findings")
    if not isinstance(counts, dict):
        raise AnchorError(f"anchor baseline at {anchor_ref}:{rel} has no `findings` object")
    return {str(k): int(v) for k, v in counts.items()}, (
        f"anchored to {rel} @ {anchor_ref} ({len(counts)} frozen fingerprints)"
    )


def _additions(current: dict[str, int], baseline: Baseline) -> tuple[list[str], list[str]]:
    """(new fingerprints, fingerprints whose occurrence count grew)."""
    new = sorted(fp for fp in current if fp not in baseline.counts)
    grown = sorted(
        fp for fp, n in current.items() if fp in baseline.counts and n > baseline.counts[fp]
    )
    return new, grown


def _dead_capacity(current: dict[str, int], baseline: Baseline) -> tuple[list[str], list[str]]:
    """(fingerprints frozen but no longer present, fingerprints frozen above their count).

    #2105. The baseline ratcheted in one direction only: `_additions` refuses growth, but
    nothing ever required it to SHRINK. A fixed violation kept its entry, and that entry
    stays a live suppression slot — so re-introducing the exact same `path|rule|text`
    later is silently absorbed. The debt number also never falls, so the file cannot show
    progress.

    Measured on the committed baseline when this was written: **8 stale fingerprints**,
    including `block.rs|unordered-iteration|let sorted: BTreeMap<_, _> =
    self.metadata.iter().collect();` — a line on the state-root path — and four signer
    `env-var` reads. Each was a re-introduction window that nothing would have reported.

    Compared against the WORKING baseline, never the anchor. The anchor is the base
    branch's file and a PR cannot edit it; checking against the anchor would fail every PR
    that FIXES something, which is precisely backwards. The working baseline is editable in
    the same change, so "you fixed it, now delete its entry" is an instruction the author
    can actually follow — the same shape as #2499's `remove/fix entries`.

    This cannot launder anything: suppression still comes from the anchor, so removing a
    live entry from the working baseline does not stop it being suppressed today, and once
    merged the entry is simply gone.
    """
    stale = sorted(fp for fp in baseline.counts if fp not in current)
    surplus = sorted(
        fp for fp, n in baseline.counts.items() if fp in current and n > current[fp]
    )
    return stale, surplus


def write_baseline(path: Path, counts: dict[str, int], prior: Baseline | None, reason: str) -> None:
    authorizations = list(prior.authorizations) if prior else []
    if prior is not None:
        new, grown = _additions(counts, prior)
        if new or grown:
            authorizations.append(
                {
                    "reason": reason,
                    "new_fingerprints": new,
                    "grown_fingerprints": [
                        f"{fp} ({prior.counts[fp]} -> {counts[fp]})" for fp in grown
                    ],
                }
            )
    else:
        authorizations.append({"reason": reason, "bootstrap_fingerprints": len(counts)})

    payload = {
        "version": BASELINE_VERSION,
        "generated_by": "scripts/check_determinism.py --update-baseline",
        "totals": {"fingerprints": len(counts), "occurrences": sum(counts.values())},
        "digest": _digest(counts, authorizations),
        "authorizations": authorizations,
        "findings": counts,
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=False) + "\n", encoding="utf-8")


# --- Self-test ------------------------------------------------------------------------


def _write(crate: Path, name: str, body: str) -> None:
    (crate / name).write_text(body, encoding="utf-8")


def self_test() -> None:
    # Realistic shapes, deliberately NOT the artificial one-line form the previous
    # self-test used. Each unordered case puts the binding and the iteration on
    # different lines, which is what the old regex could not see.
    fixtures = {
        "float.rs": "pub fn f() -> f64 { 1.5 }\n",
        "clock.rs": "pub fn t() { let _ = std::time::SystemTime::now(); }\n",
        "rng.rs": "pub fn r() { let _ = rand::random::<u64>(); }\n",
        "readdir.rs": "pub fn d() { let _ = std::fs::read_dir(\".\"); }\n",
        "env.rs": "pub fn e() { let _ = std::env::var(\"X\"); }\n",
        # --- across-lines unordered iteration, one file per shape ---
        "map_values.rs": (
            "use std::collections::HashMap;\n"
            "pub fn tally(values: &HashMap<u64, u64>) -> u64 {\n"
            "    let mut total = 0;\n"
            "    for value in values.values() {\n"
            "        total += *value;\n"
            "    }\n"
            "    total\n"
            "}\n"
        ),
        "map_keys.rs": (
            "use std::collections::HashMap;\n"
            "pub fn ids(m: &HashMap<u64, u64>) -> Vec<u64> {\n"
            "    let mut out = Vec::new();\n"
            "    for k in m.keys() {\n"
            "        out.push(*k);\n"
            "    }\n"
            "    out\n"
            "}\n"
        ),
        "map_iter.rs": (
            "use rustc_hash::FxHashMap;\n"
            "pub fn walk(cache: &FxHashMap<u64, u64>) {\n"
            "    for (k, v) in cache.iter() {\n"
            "        let _ = (k, v);\n"
            "    }\n"
            "}\n"
        ),
        "map_bare_for.rs": (
            "use std::collections::HashSet;\n"
            "pub fn each(seen: &HashSet<u64>) {\n"
            "    for s in seen {\n"
            "        let _ = s;\n"
            "    }\n"
            "}\n"
        ),
        "map_let_binding.rs": (
            "use std::collections::HashMap;\n"
            "pub fn build() -> u64 {\n"
            "    let mut scores: HashMap<u64, u64> = HashMap::new();\n"
            "    scores.insert(1, 2);\n"
            "    let mut total = 0;\n"
            "    for v in scores.values() {\n"
            "        total += *v;\n"
            "    }\n"
            "    total\n"
            "}\n"
        ),
        "map_struct_field.rs": (
            "use std::collections::HashMap;\n"
            "pub struct Ledger {\n"
            "    pub entries: HashMap<u64, u64>,\n"
            "}\n"
            "impl Ledger {\n"
            "    pub fn sum(&self) -> u64 {\n"
            "        let mut t = 0;\n"
            "        for e in self.entries.values() {\n"
            "            t += *e;\n"
            "        }\n"
            "        t\n"
            "    }\n"
            "}\n"
        ),
    }
    # Deterministic collections must NOT be flagged, or the rule is useless noise.
    negatives = {
        "btree_ok.rs": (
            "use std::collections::BTreeMap;\n"
            "pub fn tally(values: &BTreeMap<u64, u64>) -> u64 {\n"
            "    let mut total = 0;\n"
            "    for value in values.values() {\n"
            "        total += *value;\n"
            "    }\n"
            "    total\n"
            "}\n"
        ),
        "vec_ok.rs": (
            "pub fn tally(values: &Vec<u64>) -> u64 {\n"
            "    let mut total = 0;\n"
            "    for value in values.iter() {\n"
            "        total += *value;\n"
            "    }\n"
            "    total\n"
            "}\n"
        ),
    }

    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        crate = root / "consensus-engines/src"
        crate.mkdir(parents=True)
        for name, body in {**fixtures, **negatives}.items():
            _write(crate, name, body)

        findings, _allows, errors = scan(root, ["consensus-engines/src"], exclusions=[])
        rules_hit = {f.rule for f in findings}
        for expect in (
            "float-type",
            "wall-clock",
            "unseeded-rng",
            "unordered-iteration",
            "readdir-unsorted",
            "env-var",
        ):
            assert expect in rules_hit, f"self-test: rule {expect} not detected ({rules_hit})"
        assert errors == [], f"unexpected errors: {errors}"

        # EVERY across-lines shape must be caught individually — a single passing shape
        # is what let the previous self-test certify a detector that did not work.
        for name in (
            "map_values.rs",
            "map_keys.rs",
            "map_iter.rs",
            "map_bare_for.rs",
            "map_let_binding.rs",
            "map_struct_field.rs",
        ):
            hits = [
                f for f in findings if f.path.endswith(name) and f.rule == "unordered-iteration"
            ]
            assert hits, f"self-test: across-lines unordered iteration missed in {name}"
            # and the binding must be on a different line from the iteration, or the
            # fixture has regressed into the artificial one-line shape.
            body = (crate / name).read_text(encoding="utf-8").splitlines()
            decl = next(i for i, ln in enumerate(body) if any(t in ln for t in UNORDERED_TYPES))
            assert hits[0].line_no - 1 != decl, (
                f"{name}: fixture must separate the binding from the iteration, "
                "otherwise it only re-tests the one-line shape (#2245 blocker 1)"
            )

        # --- Independence: prove each assertion is CAUSED by the rule it names ------------
        #
        # The failure this PR was built around is a self-test written in the shape the buggy
        # regex matched, so it agreed with the implementation instead of checking it. Fixtures
        # alone cannot settle that: a rule for `f64` can only be exercised by a fixture
        # containing `f64`, so fixture and regex necessarily share a vocabulary.
        #
        # What CAN be settled is causality. Ablate one rule at a time and require its
        # detection to disappear. If a case still passes with its rule removed, the case was
        # being satisfied by something else and proves nothing about that rule.
        global RULES
        _saved_rules = RULES
        try:
            for target in ("float-type", "wall-clock", "unseeded-rng", "readdir-unsorted", "env-var"):
                RULES = [r for r in _saved_rules if r.name != target]
                ablated, _a, _e = scan(root, ["consensus-engines/src"], exclusions=[])
                assert target not in {f.rule for f in ablated}, (
                    f"ablation: {target} still reported with its rule removed — impossible"
                )
                restored_names = {f.rule for f in findings}
                assert target in restored_names, (
                    f"ablation: {target} was never detected with the full rule set, so the "
                    "self-test's assertion for it was vacuous"
                )
        finally:
            RULES = _saved_rules

        # The test-region mask is the other thing a fixture cannot prove on its own: swap in
        # the old backward-scanning heuristic and the production-after-tests case must break.
        _prod_after_tests = (
            "pub fn a() -> u64 { 1 }\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    #[test]\n"
            "    fn t() { let _ = 1; }\n"
            "}\n"
            "pub fn now() -> u64 {\n"
            "    std::time::SystemTime::now().elapsed().unwrap().as_secs()\n"
            "}\n"
        ).splitlines()
        mask_now = _test_mask(_prod_after_tests)
        assert not mask_now[7], (
            "the production fn below `mod tests` must NOT be masked — this is #2245's "
            "backward-scan defect and the whole reason _test_mask replaced it"
        )
        assert mask_now[4], "the #[test] fn body must still be masked, or the mask over-reports"

        # And a Rust lifetime must not eat the brace that closes a test module.
        assert "{" in _strip_literals("fn f() -> MutexGuard<'static, ()> {"), (
            "a lifetime was consumed as a char literal; brace-depth tracking will desync and "
            "close test regions early (found live in launch_api/main.rs)"
        )
        assert "{" not in _strip_literals("let c = '{';"), "a real char literal must be stripped"

        # #2905: raw strings have no escapes, may span lines, and terminate only at
        # `"` + the opener's hash count — their JSON braces are literal content, not
        # Rust syntax. The stateless stripper counted them, so a raw JSON fixture
        # collapsed the `#[cfg(test)]` mask and five test-only floats in
        # oracle_attester.rs were reported as production findings. Two raw JSON
        # strings inside one test module, followed by float literals: every test
        # line must stay masked, and production code after the module must not be.
        _raw_json_tests = (
            "pub fn prod_before() -> u64 { 1 }\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    fn parse(_s: &str) {}\n"
            "    #[test]\n"
            "    fn closes_braces_in_raw_json() {\n"
            '        parse(r#"{ "outer": { "inner": 1 } }"#);\n'
            "    }\n"
            '    const FIXTURE: &str = r##"{\n'
            '        "temperature": { "unitCode": "wmoUnit:degC", "value": 21.7 },\n'
            '        "windSpeed": { "value": 14.4 }\n'
            '    }"##;\n'
            "    #[test]\n"
            "    fn uses_fixture() { let _x: f64 = 5.0; let _ = FIXTURE; }\n"
            "}\n"
            "pub fn prod_after() -> f64 { 0.0 }\n"
        ).splitlines()
        mask_raw = _test_mask(_raw_json_tests)
        assert all(mask_raw[2:15]), (
            "every line of the test module must stay masked despite raw-JSON braces; "
            "an unmasked line means raw-string content was counted as Rust syntax and "
            "test fixtures will be reported as production nondeterminism (#2905)"
        )
        assert not mask_raw[0] and not mask_raw[15], (
            "production code around the test module must NOT be masked — the raw-string "
            "fix must not turn the mask into an over-masker (#2905)"
        )

        # #2906 review blocker: `r#"` is valid PROSE inside a Rust block comment.
        # Recognizing it there as a raw-string opener carries raw-string state
        # across the comment's end, masks the test module's closing brace, and
        # hides every production finding after it — a silent-blindness path.
        # Block comments nest, so depth is tracked. This is the reviewer's exact
        # probe shape: raw-string-like prose inside a cfg(test) block comment,
        # then an unwaived production float, which MUST be reported.
        _block_comment_raw_prose = (
            "pub fn before() -> u64 { 1 }\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            '    /* valid block-comment prose containing a raw-string example: r#" */\n'
            '    /* nested /* inner */ still comment, another opener: r##" */\n'
            "}\n"
            "pub fn production_nondeterminism_probe() -> f64 {\n"
            "    9876.5\n"
            "}\n"
        ).splitlines()
        mask_bc = _test_mask(_block_comment_raw_prose)
        assert all(mask_bc[2:6]), (
            "the whole test module, its block comments and closing brace included, "
            "must be masked"
        )
        assert not any(mask_bc[6:9]) and not mask_bc[0], (
            "production code after a test module whose block comment mentions r#\" must "
            "NOT be masked — comment prose opened raw-string state and hid a production "
            "float from the gate (#2906 review blocker)"
        )

        for name in ("btree_ok.rs", "vec_ok.rs"):
            assert not any(
                f.path.endswith(name) and f.rule == "unordered-iteration" for f in findings
            ), f"self-test: deterministic collection wrongly flagged in {name}"

        # #3197 follow-up: line rules match CODE, not prose. Literal bodies and comments
        # are stripped with one lexical state per file before any rule runs.
        #
        # Negatives: every rule token below sits inside a string, a trailing comment or
        # a block comment, which are the shapes that produced 66 false findings on main
        # (`"1.0.0"`, `"127.0.0.1"`, `// 0.001 Gwei`, `"… not f64: {e}"`).
        # Positives: real code must stay visible next to that prose, and in particular
        # code AFTER a multi-line string's closing quote. A stateless stripper reads
        # that quote as an opener and hides the rest of the line, the blinding shape
        # #3286 fixed in secret-leak's lexer. Here it would hide a production clock
        # read behind a log message.
        with tempfile.TemporaryDirectory() as d2:
            proot = Path(d2)
            pcrate = proot / "consensus-engines/src"
            pcrate.mkdir(parents=True)
            _write(pcrate, "prose_ok.rs", (
                'pub fn v() -> String { "1.0.0".to_string() } // ratio 0.5, see f64 docs\n'
                'pub fn h() -> &\'static str { "127.0.0.1" }\n'
                'pub fn e(x: &str) -> String { format!("{x} not f64 / SystemTime::now()") }\n'
                '/* multi-line block comment: 2.5 f32 thread_rng()\n'
                '   still prose: Instant::now() */\n'
                'pub fn r() -> &\'static str { r#"weights 0.25 f64"# }\n'
            ))
            _write(pcrate, "code_after_prose.rs", (
                'pub fn m() -> u64 {\n'
                '    let _msg = "a message that\n'
                'spans lines"; std::time::SystemTime::now().elapsed().unwrap().as_secs()\n'
                '}\n'
                'pub fn b() -> f64 { /* note */ 2.5 }\n'
                'pub fn c() -> (&\'static str, f32) { ("label", 1.0) }\n'
            ))
            # #3293 review probes: C-string raw literals. `cr"C:\"` ends at its quote
            # (raw strings have no escapes) and `cr#"say "hi"#` at `"#`. Real code
            # after each must stay visible, and their contents must stay prose.
            _write(pcrate, "c_raw_after.rs", (
                'pub fn p() -> usize { let _p = cr"C:\\"; 0 }\n'
                'pub fn t() -> f64 { 2.5 }\n'
                'pub fn q() -> usize { let _q = cr#"say "hi" 0.75 f32"#; 0 }\n'
                'pub fn now() -> u64 { std::time::SystemTime::now().elapsed().unwrap().as_secs() }\n'
            ))
            # #3293 review: a `determinism-ok:` marker counts only in a COMMENT. Inside
            # a string literal it is data and must not suppress anything.
            _write(pcrate, "marker_positions.rs", (
                'pub fn a() -> (f64, &\'static str) { (1.5, "// determinism-ok: not a real comment") }\n'
                'pub const NOTE: &str = "determinism-ok: also not a comment";\n'
                'pub fn b() -> f64 { 2.5 }\n'
                'pub fn c() -> f64 { /* determinism-ok: block comment is a real annotation */ 3.5 }\n'
                'pub fn d() -> (f64, &\'static str) { (4.5, "a//b") } // determinism-ok: real trailing comment after a string holding //\n'
            ))
            pf, _pa, perr = scan(proot, ["consensus-engines/src"], exclusions=[])
            assert perr == [], f"unexpected errors: {perr}"
            prose_hits = [f for f in pf if f.path.endswith("prose_ok.rs")]
            assert not prose_hits, (
                "rule tokens inside strings/comments must not be findings (#3197 follow-up): "
                + "; ".join(f"{f.line_no}:{f.rule}" for f in prose_hits)
            )
            after = {(f.line_no, f.rule) for f in pf if f.path.endswith("code_after_prose.rs")}
            assert (3, "wall-clock") in after, (
                "code after a multi-line string's closing quote was blinded: the stripper "
                f"must carry string state across lines (#3286 shape). got {sorted(after)}"
            )
            assert (5, "float-type") in after and (5, "float-literal") in after, (
                f"code after a block comment on the same line must stay visible: {sorted(after)}"
            )
            assert (6, "float-literal") in after and (6, "float-type") in after, (
                f"a real float next to a string literal must still be flagged: {sorted(after)}"
            )
            marks = {(f.line_no, f.rule) for f in pf if f.path.endswith("marker_positions.rs")}
            assert (1, "float-literal") in marks and (1, "float-type") in marks, (
                "a determinism-ok marker INSIDE A STRING suppressed a real float on its own "
                f"line; markers count only in comments (#3293 review). got {sorted(marks)}"
            )
            assert (3, "float-literal") in marks and (3, "float-type") in marks, (
                "a determinism-ok marker inside a string on the line ABOVE suppressed a real "
                f"float (#3293 review). got {sorted(marks)}"
            )
            assert not any(ln in (4, 5) for ln, _r in marks), (
                "a marker in a block comment, or in a real trailing comment after a string "
                f"that contains //, must still suppress. got {sorted(marks)}"
            )
            craw = {(f.line_no, f.rule) for f in pf if f.path.endswith("c_raw_after.rs")}
            assert (2, "float-type") in craw and (2, "float-literal") in craw, (
                'code after a C-string raw literal `cr"C:\\"` was blinded: the raw-string '
                f"opener must accept the `c` prefix (#3293 review). got {sorted(craw)}"
            )
            assert (4, "wall-clock") in craw, (
                'a clock read after `cr#"say "hi"#` was blinded: the raw-string opener must '
                f"accept the `c` prefix (#3293 review). got {sorted(craw)}"
            )
            assert not any(ln in (1, 3) for ln, _r in craw), (
                f"C-string raw literal CONTENTS must be prose, not findings: {sorted(craw)}"
            )

        # A justified allow suppresses the finding...
        _write(
            crate,
            "float.rs",
            "pub fn f() -> f64 { 1.5 } // determinism-ok: fixed-point elsewhere, display-only\n",
        )
        findings2, allows2, _ = scan(root, ["consensus-engines/src"], exclusions=[])
        assert not any(
            f.path.endswith("float.rs") and f.rule == "float-type" for f in findings2
        ), "justified allow must suppress the finding"
        assert any(a.path.endswith("float.rs") for a in allows2), "allow must be recorded"

        # ...an allow suppresses an unordered-iteration finding too...
        _write(
            crate,
            "map_values.rs",
            "use std::collections::HashMap;\n"
            "pub fn tally(values: &HashMap<u64, u64>) -> u64 {\n"
            "    let mut total = 0;\n"
            "    // determinism-ok: summing is order-independent\n"
            "    for value in values.values() {\n"
            "        total += *value;\n"
            "    }\n"
            "    total\n"
            "}\n",
        )
        f2b, _a, _e = scan(root, ["consensus-engines/src"], exclusions=[])
        assert not any(
            f.path.endswith("map_values.rs") and f.rule == "unordered-iteration" for f in f2b
        ), "an allow on the line above must suppress an unordered-iteration finding"

        # ...but an empty justification is itself an error.
        _write(crate, "float.rs", "pub fn f() -> f64 { 1.5 } // determinism-ok:\n")
        _f3, _a3, errors3 = scan(root, ["consensus-engines/src"], exclusions=[])
        assert any("non-empty justification" in e for e in errors3), errors3

        # Test-context code is not flagged.
        _write(crate, "float.rs", "#[cfg(test)]\nmod tests { pub fn f() -> f64 { 1.5 } }\n")
        f4, _a4, _e4 = scan(root, ["consensus-engines/src"], exclusions=[])
        assert not any(f.path.endswith("float.rs") for f in f4), "test code must be exempt"

        # A stale exclusion must fail rather than silently shrink the scan.
        _f5, _a5, errors5 = scan(
            root, ["consensus-engines/src"], exclusions=[("no/such/dir", "gone")]
        )
        assert any("does not exist" in e for e in errors5), errors5
        _f6, _a6, errors6 = scan(
            root, ["consensus-engines/src"], exclusions=[("consensus-engines/src", "  ")]
        )
        assert any("states no reason" in e for e in errors6), errors6

        _baseline_self_test(root, crate)

    _trigger_paths_self_test()
    _update_refusal_self_test()

    _refusal_process_self_test()

    print("check_determinism.py self-test passed")


def _refusal_process_self_test() -> None:
    """#2947: the #2944 refusal must hold at the PROCESS level.

    Every other assertion here exercises preservation, which already worked
    before #2944; the one line that changed behaviour — main() refusing to
    overwrite an existing baseline it cannot load — is unreachable from these
    in-process tests because none of them call main(). So this drives the real
    CLI as a subprocess: an unloadable (v1 list) baseline plus
    --authorize-additions must exit non-zero, must leave the file
    byte-identical, and must print a recovery instruction naming a command the
    tool will actually accept (move the file aside first — re-running
    --update-baseline in place is exactly what gets refused, so telling the
    user to do that, as the pre-#2947 text did, prescribed a dead end).
    Costs one full scan (~3s); it is the only guard on the refusal line.
    """
    with tempfile.TemporaryDirectory() as d:
        bpath = Path(d) / "baseline.json"
        v1_bytes = b'["consensus-engines/src/a.rs|wall-clock|A"]\n'
        bpath.write_bytes(v1_bytes)
        completed = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).resolve()),
                "--update-baseline",
                str(bpath),
                "--authorize-additions",
                "self-test: this rewrite must be refused, not authorized",
            ],
            capture_output=True,
            text=True,
            timeout=600,
        )
        out = completed.stdout + completed.stderr
        assert completed.returncode != 0, (
            "REFUSAL LOST: --update-baseline exited "
            f"{completed.returncode} on an unloadable baseline instead of "
            f"refusing.\n{out}"
        )
        assert "REFUSED: existing baseline could not be loaded" in out, out
        assert bpath.read_bytes() == v1_bytes, (
            "REFUSAL LOST: the unloadable baseline was rewritten on disk"
        )
        assert "move it aside" in out, (
            "v1 recovery text must tell the user to move the file aside; "
            "prescribing an in-place --update-baseline is the refused command "
            f"(#2947).\n{out}"
        )


def _baseline_self_test(root: Path, crate: Path) -> None:
    """The #2245 blocker-3 property: a baseline cannot absorb a new violation silently."""
    _write(crate, "clean.rs", "pub fn ok() -> u64 { 1 }\n")
    for junk in list(crate.glob("*.rs")):
        if junk.name != "clean.rs":
            junk.unlink()

    bpath = root / "baseline.json"
    findings, _a, _e = scan(root, ["consensus-engines/src"], exclusions=[])
    write_baseline(bpath, _occurrences(findings), None, "self-test bootstrap")
    base, errs = load_baseline(bpath)
    assert base is not None and not errs, errs
    preserved_auths = list(base.authorizations)
    assert preserved_auths, "bootstrap must create an authorization record"

    # Rewriting an existing baseline with no findings growth must preserve the
    # append-only audit history exactly; a count-only test would miss its loss.
    write_baseline(bpath, _occurrences(findings), base, "self-test: preserve history")
    preserved, preserved_errs = load_baseline(bpath)
    assert preserved is not None and not preserved_errs, preserved_errs
    assert preserved.authorizations == preserved_auths, (
        "rewriting an existing baseline must preserve authorizations exactly"
    )

    # Plant a NEW violation: it must be reported as new against the frozen baseline.
    _write(
        crate,
        "planted.rs",
        "pub fn t() {\n    let _ = std::time::SystemTime::now();\n}\n",
    )
    findings2, _a, _e = scan(root, ["consensus-engines/src"], exclusions=[])
    new, grown = _additions(_occurrences(findings2), base)
    assert new, "a planted violation must be a new fingerprint"

    # Regenerating WITHOUT authorization must be refused.
    assert requires_authorization(_occurrences(findings2), base), \
        "baseline growth must require explicit authorization (#2245 blocker 3)"

    # A DUPLICATE of an already-frozen line adds an occurrence but no fingerprint;
    # that must still count as growth, or duplicates slip through the pin.
    # The SAME line twice: identical text collapses to one fingerprint, so a
    # fingerprint-only pin would see nothing while a real violation was added.
    _write(
        crate,
        "planted.rs",
        "pub fn t() {\n"
        "    let _ = std::time::SystemTime::now();\n"
        "    let _ = std::time::SystemTime::now();\n"
        "}\n",
    )
    findings3, _a, _e = scan(root, ["consensus-engines/src"], exclusions=[])
    write_baseline(bpath, _occurrences(findings2), base, "self-test: authorize the first plant")
    base2, errs2 = load_baseline(bpath)
    assert base2 is not None and not errs2, errs2
    assert base2.authorizations[:len(preserved_auths)] == preserved_auths, (
        "baseline growth must retain the existing authorization prefix"
    )
    assert len(base2.authorizations) == len(preserved_auths) + 1, (
        "baseline growth must append exactly one authorization record"
    )
    new3, grown3 = _additions(_occurrences(findings3), base2)
    assert not new3, "the duplicate must NOT introduce a new fingerprint (that is the trap)"
    assert grown3, "a duplicate occurrence of a frozen line must still be detected as growth"

    # Tampering with the file must be detected by the digest.
    raw = json.loads(bpath.read_text(encoding="utf-8"))
    raw["findings"]["consensus-engines/src/fake.rs|wall-clock|fake"] = 1
    bpath.write_text(json.dumps(raw), encoding="utf-8")
    _b, errs3 = load_baseline(bpath)
    assert any("digest" in e for e in errs3), errs3

    # --- #2105: the baseline must SHRINK when a violation is fixed -------------------
    #
    # `_additions` refuses growth. Nothing required the reverse, so a fixed violation
    # kept its entry and that entry stayed a live suppression slot: re-introducing the
    # identical `path|rule|text` later would be absorbed silently, and the debt total
    # could never fall. The committed baseline carried 8 such entries when this was
    # written.
    write_baseline(bpath, _occurrences(findings2), None, "self-test: freeze the plant")
    base3, errs4 = load_baseline(bpath)
    assert base3 is not None and not errs4, errs4

    # Fix it: delete the planted file entirely.
    (crate / "planted.rs").unlink()
    findings4, _a, _e = scan(root, ["consensus-engines/src"], exclusions=[])
    current4 = _occurrences(findings4)

    stale, surplus = _dead_capacity(current4, base3)
    assert stale, "a frozen finding that no longer occurs must be reported as STALE"
    assert not _additions(current4, base3)[0], \
        "fixing a violation must not look like a NEW one — that would punish the fix"

    # And the surplus half: same line still present, but fewer times than frozen.
    _write(
        crate,
        "planted.rs",
        "pub fn t() {\n"
        "    let _ = std::time::SystemTime::now();\n"
        "    let _ = std::time::SystemTime::now();\n"
        "}\n",
    )
    findings5, _a, _e = scan(root, ["consensus-engines/src"], exclusions=[])
    write_baseline(bpath, _occurrences(findings5), None, "self-test: freeze two occurrences")
    base4, _e4 = load_baseline(bpath)
    assert base4 is not None
    _write(
        crate,
        "planted.rs",
        "pub fn t() {\n    let _ = std::time::SystemTime::now();\n}\n",
    )
    findings6, _a, _e = scan(root, ["consensus-engines/src"], exclusions=[])
    _stale6, surplus6 = _dead_capacity(_occurrences(findings6), base4)
    assert surplus6, \
        "a frozen count above the actual count is dead suppression capacity and must fail"

    _anchor_self_test()


def _anchor_self_test() -> None:
    """The #2245 merge blocker: the baseline must not be expandable by the PR it gates.

    A digest the PR's own run recomputes cannot anchor the PR — the reviewer grew the
    baseline, removed the authorization, recomputed the digest, and passed. The anchor is
    the baseline as committed on the base branch: git content the PR cannot edit. This test
    reproduces that attack in a throwaway git repo and requires the anchored gate to still
    see the violation.
    """
    def git(repo: Path, *a: str) -> str:
        return subprocess.run(
            ["git", *a], cwd=repo, capture_output=True, text=True, check=True
        ).stdout.strip()

    with tempfile.TemporaryDirectory() as d:
        repo = Path(d)
        git(repo, "init", "-q", "-b", "main")
        git(repo, "config", "user.email", "selftest@example.com")
        git(repo, "config", "user.name", "selftest")

        rel = Path("baseline.json")
        bpath = repo / rel

        def write_bl(counts: dict[str, int]) -> None:
            auths: list[dict] = []
            bpath.write_text(
                json.dumps(
                    {
                        "version": BASELINE_VERSION,
                        "totals": {"fingerprints": len(counts), "occurrences": sum(counts.values())},
                        "digest": _digest(counts, auths),
                        "authorizations": auths,
                        "findings": counts,
                    }
                ),
                encoding="utf-8",
            )

        # BASE: the baseline as it exists on `main`, freezing fingerprint A only.
        base_counts = {"shared-protocol/src/blockchain/a.rs|wall-clock|A": 1}
        write_bl(base_counts)
        git(repo, "add", "baseline.json")
        git(repo, "commit", "-qm", "base baseline")
        base_sha = git(repo, "rev-parse", "HEAD")

        # Sanity: the anchor loader reads exactly the base set.
        anchor, note = load_anchor_counts(base_sha, rel, cwd=repo)
        assert anchor == base_counts, (anchor, note)

        # THE ATTACK: the PR grows its WORKING baseline to also freeze B (a new violation),
        # recomputing the digest so the file self-verifies.
        grown = dict(base_counts)
        grown["shared-protocol/src/blockchain/b.rs|wall-clock|B"] = 1
        write_bl(grown)
        working, werrs = load_baseline(bpath)
        assert working is not None and not werrs, werrs  # the grown file passes its OWN digest

        # The anchor must ignore the PR's growth: B is still unfrozen from the base's view.
        anchor2, _ = load_anchor_counts(base_sha, rel, cwd=repo)
        assert "shared-protocol/src/blockchain/b.rs|wall-clock|B" not in anchor2, (
            "ANCHOR FAILED: the PR grew the base baseline. The suppression set must come "
            "from the base commit, not the working tree (#2245)."
        )
        # And a finding matching B is NEW against the anchor even though the working
        # baseline "covers" it.
        current_with_B = {"shared-protocol/src/blockchain/b.rs|wall-clock|B": 1}
        new_vs_anchor, _ = _additions(current_with_B, Baseline(anchor2, []))
        new_vs_working, _ = _additions(current_with_B, working)
        assert new_vs_anchor, "B must be NEW against the anchor"
        assert not new_vs_working, "B is (wrongly) covered by the grown working baseline"

        # BOOTSTRAP with real code (the actual #2245 bootstrap-PR shape): the base commit has
        # consensus code but NO committed baseline. Anchoring must NOT fall back to the
        # working baseline — it must scan the base CODE, which the PR cannot edit. Prove the
        # pre-existing base finding lands in the suppression set so the anchor is active.
        src = repo / "consensus-engines" / "src"
        src.mkdir(parents=True)
        (src / "base.rs").write_text(
            "pub fn t() -> u64 {\n"
            "    std::time::SystemTime::now().elapsed().unwrap().as_secs()\n"
            "}\n",
            encoding="utf-8",
        )
        git(repo, "add", "consensus-engines/src/base.rs")
        git(repo, "rm", "-q", "-f", "baseline.json")
        git(repo, "commit", "-qm", "base has code, no baseline yet")
        boot_sha = git(repo, "rev-parse", "HEAD")
        boot_counts, boot_note = load_anchor_counts(boot_sha, rel, cwd=repo)
        assert boot_counts is not None and "BOOTSTRAP" in boot_note, (boot_counts, boot_note)
        assert any(fp.startswith("consensus-engines/src/base.rs|wall-clock|") for fp in boot_counts), (
            "bootstrap anchor must be the base-CODE scan: the pre-existing wall-clock finding "
            "in the base commit must be in the suppression set, or a bootstrap PR could grow "
            "its own baseline to cover a plant (#2245 blocker: anchoring not active in bootstrap)"
        )
        # A finding that is NOT in the base code is NEW against the bootstrap anchor even if a
        # (PR-controlled) working baseline "covers" it — this is the laundering path, closed.
        planted = "consensus-engines/src/plant.rs|wall-clock|std::time::SystemTime::now()"
        grown_working = Baseline({**boot_counts, planted: 1}, [])
        new_vs_anchor, _ = _additions({planted: 1}, Baseline(boot_counts, []))
        new_vs_working, _ = _additions({planted: 1}, grown_working)
        assert new_vs_anchor, "a plant absent from the base code must be NEW against the bootstrap anchor"
        assert not new_vs_working, "the grown working baseline (wrongly) covers the plant — anchor must win"

        # An EMPTY base (no baseline AND no scanned code) yields None: nothing to suppress.
        (repo / "unrelated.txt").write_text("x", encoding="utf-8")
        git(repo, "rm", "-q", "-r", "-f", "consensus-engines")
        git(repo, "add", "unrelated.txt")
        git(repo, "commit", "-qm", "empty base: no baseline, no scanned code")
        empty_sha = git(repo, "rev-parse", "HEAD")
        boot_counts2, boot_note2 = load_anchor_counts(empty_sha, rel, cwd=repo)
        assert boot_counts2 is None and "BOOTSTRAP" in boot_note2, (boot_counts2, boot_note2)

        # UNRESOLVABLE ref must fail closed, never silently run unanchored.
        try:
            load_anchor_counts("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", rel, cwd=repo)
        except AnchorError:
            pass
        else:
            raise AssertionError("an unresolvable anchor ref must raise AnchorError (fail closed)")


def requires_authorization(current: dict[str, int], baseline: Baseline) -> bool:
    new, grown = _additions(current, baseline)
    return bool(new or grown)


# --- Workflow path-filter drift guard -------------------------------------------------
#
# #2245 blocker 2 had two halves. The scan root set was wrong, AND the workflow's
# `paths:` filter listed the same wrong directories — so a PR touching only
# `shared-protocol/src/blockchain` did not start this job at all. Fixing SCANNED_ROOTS
# without fixing the filter would leave the gate correct but never invoked, which is
# indistinguishable from a pass.
#
# Two lists in two languages cannot be derived from one another (YAML has no access to
# this module), so the next best thing is to assert they agree.
WORKFLOW = ".github/workflows/determinism-lint.yml"


def _trigger_paths(text: str, trigger: str) -> list[str] | None:
    """The `paths:` globs belonging to ONE trigger, or None if the trigger is absent.

    Scoped by indentation rather than by scanning the whole file, because the previous
    implementation collected every single-quoted list item in the document and unioned
    them. That made the two trigger blocks indistinguishable: deleting
    `shared-protocol/src/**` from `pull_request` alone — the live consensus state code,
    and the exact directory whose omission was #2245's original defect — still passed,
    because `push` retained it. A gate that cannot see which trigger lost coverage cannot
    protect PR reachability, which is the only reachability that gates a PR.

    Deliberately not `import yaml`: the workflow runs a bare `actions/setup-python` with
    no dependency install, so a PyYAML import would make this check fail to start in CI —
    trading a false pass for a different kind of not-running. The parse below is scoped to
    this one file's shape and `_self_test_workflow_paths` proves it distinguishes the two
    blocks.
    """
    lines = text.splitlines()
    trig_indent: int | None = None
    in_paths = False
    paths_indent: int | None = None
    out: list[str] = []
    for raw in lines:
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        indent = len(raw) - len(raw.lstrip())
        stripped = raw.strip()
        if trig_indent is None:
            if re.match(rf"^{re.escape(trigger)}:\s*$", stripped):
                trig_indent = indent
            continue
        # Left the trigger block entirely.
        if indent <= trig_indent and not stripped.startswith("-"):
            break
        if not in_paths:
            if stripped == "paths:":
                in_paths = True
                paths_indent = indent
            continue
        if stripped.startswith("- "):
            # #2919: single-quoted, double-quoted, and bare-token forms, in that
            # order. A double-quoted entry falling through to the bare-token
            # fallback keeps its quote characters and matches nothing —
            # measured on `fuzz-nightly.yml`, whose 10/10 `paths:` entries are
            # double-quoted.
            m = (
                re.match(r"-\s*'([^']+)'\s*$", stripped)
                or re.match(r'-\s*"([^"]+)"\s*$', stripped)
                or re.match(r"-\s*(\S+)\s*$", stripped)
            )
            if m:
                out.append(m.group(1))
        elif paths_indent is not None and indent <= paths_indent:
            in_paths = False
    return out if trig_indent is not None else None


def _update_refusal_self_test() -> None:
    """#2946 / #2947: drive the REAL CLI, not the functions around it.

    WHY THIS IS A PROCESS TEST. #2947 exists because #2944's refusal lived only inside
    `main()` and the self-test never calls `main()` — so the single line that changed
    behaviour was unguarded, and reverting it left every test green. Asserting through
    `baseline_update_refusal` alone would repeat that exactly one layer down: the seam
    could be correct while nothing called it. So each case below spawns the script the
    way CI and an operator do, and checks the two things that actually matter — the exit
    code, and whether the file on disk survived unchanged.

    Byte-identity, not "the marker is still there": a rewrite that preserved the tampered
    value but reissued the digest is the #2946 defect itself, and a value check passes on
    it. The digest is the thing being laundered, so the whole file is the observable.
    """
    script = Path(__file__).resolve()
    root = script.parents[1]
    committed = root / "scripts" / "determinism_baseline.json"
    if not committed.is_file():  # pragma: no cover - repo always ships one
        raise AssertionError(f"missing committed baseline at {committed}")
    good = committed.read_text(encoding="utf-8")

    def run(target: Path, *extra: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(script), "--update-baseline", str(target), *extra],
            cwd=root, capture_output=True, text=True,
        )

    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)

        def refuses(name: str, body: str, needle: str) -> None:
            target = tmpdir / name
            target.write_text(body, encoding="utf-8")
            before = hashlib.sha256(target.read_bytes()).hexdigest()
            done = run(target, "--authorize-additions", "self-test should never apply this")
            after = hashlib.sha256(target.read_bytes()).hexdigest()
            assert done.returncode != 0, (
                f"{name}: --update-baseline must REFUSE, got exit 0\n{done.stdout}"
            )
            assert "REFUSED" in done.stdout, f"{name}: no refusal printed\n{done.stdout}"
            assert needle in done.stdout, (
                f"{name}: refusal must explain itself; missing {needle!r}\n{done.stdout}"
            )
            assert before == after, (
                f"{name}: REFUSED but the file changed on disk ({before} -> {after}). "
                "A refusal that still rewrites the file is not a refusal."
            )
            # Refusing without a way forward just moves the wall (#2946), and the way
            # forward must not be a command this tool rejects (#2947).
            # The shared phrase, not my own suffix: #2949's v1 message says
            # `.v1.bak` and this file's digest message says `.rejected`. Asserting a
            # particular suffix would pass only on whichever text I happened to write.
            assert "mv " in done.stdout and RECOVERY_PHRASE in done.stdout, (
                f"{name}: refusal names no move-aside recovery path\n{done.stdout}"
            )
            # ABSENCE, not just presence. The first version of this assertion only
            # required the good instruction to be THERE, and a mutation restoring the
            # old self-contradictory v1 text survived it untouched: the bad advice was
            # additional text, and "the good string is present" is true either way. The
            # property that actually matters is that the output prescribes exactly ONE
            # command, the move-aside one — so count the invocations instead.
            # Count PRESCRIBED COMMANDS, not mentions. Both recovery texts name the
            # flag twice: once as a backticked command to run, and once as prose
            # explaining that running it in place is what gets refused. A raw
            # `.count("--update-baseline")` scored that legitimate shape as 2 and
            # failed — the assertion firing on the explanation rather than on a
            # second instruction. The convention both texts follow is that a command
            # to run is backticked; prose about the flag is not.
            invocations = done.stdout.count("`--update-baseline")
            assert invocations == 1, (
                f"{name}: refusal prescribes {invocations} `--update-baseline` "
                "commands, expected exactly 1 (the move-aside recovery). A second one "
                "is an instruction to re-run the command that was just refused — "
                f"#2947's defect, re-added.\n{done.stdout}"
            )

        # (a) #2946: a HAND-EDITED baseline. The value is preserved and the digest is
        #     stale — precisely the state --update-baseline used to launder.
        tampered = json.loads(good)
        tampered["authorizations"][0]["reason"] = "TAMPERED-MARKER"
        refuses("tampered.json", json.dumps(tampered, indent=2), "digest does not match")

        # (b) #2946 second shape: edit the FINDINGS rather than the audit record, so the
        #     suppression list itself is what was tampered with.
        grown = json.loads(good)
        grown["findings"]["consensus-engines/src/planted.rs|wall-clock|planted"] = 1
        refuses("grown.json", json.dumps(grown, indent=2), "digest does not match")

        # (c) #2947 item 1: the #2944 refusal (unloadable baseline) had NO process-level
        #     guard at all. It has one now, in the same place as its sibling.
        refuses("broken.json", "{ this is not json", "not valid JSON")

        # (d) #2947 item 2: a v1 baseline must not be told to run a command that is
        #     refused. `refuses` already requires the move-aside text, so this case
        #     fails if the old self-contradictory instruction comes back.
        refuses("v1.json", json.dumps(["some/path.rs|wall-clock|x"]), "v1 list format")

        # (e) THE OTHER DIRECTION. An untouched baseline must still update cleanly —
        #     a refusal that fires on everything is not a gate, it is an outage.
        #
        #     Decoupled from live-tree cleanliness (#3197). This case used to rewrite
        #     the COMMITTED baseline against the LIVE tree with no authorization, so
        #     any unbaselined finding on main failed the *detector self-test* on every
        #     branch. That is how #3107's red spread to every later PR, where it was
        #     ignored while more findings landed. Tree drift is the gate step's job,
        #     not the self-test's. So first bring the copy in line with whatever tree
        #     this runs on, through the tool's own authorized write, which leaves a
        #     digest-valid, untampered file. Then require the property itself: an
        #     unauthorized update of an untampered baseline that adds nothing exits 0.
        clean = tmpdir / "clean.json"
        clean.write_text(good, encoding="utf-8")
        synced = run(clean, "--authorize-additions", "self-test: sync to the tree under test")
        assert synced.returncode == 0, (
            f"authorized sync of the committed baseline failed: exit {synced.returncode}\n"
            f"{synced.stdout}{synced.stderr}"
        )
        done = run(clean)
        assert done.returncode == 0, (
            f"an untampered baseline must still update: exit {done.returncode}\n"
            f"{done.stdout}{done.stderr}"
        )
        reloaded, errs = load_baseline(clean)
        assert reloaded is not None and not errs, f"rewrite must self-verify: {errs}"

        # (f) The recovery path the refusals PRESCRIBE must actually work, or the error
        #     text is the #2947 defect wearing new words. Move aside, rebuild, verify.
        moved = tmpdir / "recover.json"
        moved.write_text(json.dumps(tampered, indent=2), encoding="utf-8")
        moved.rename(tmpdir / "recover.json.rejected")
        done = run(moved, "--authorize-additions", "self-test: prescribed recovery")
        assert done.returncode == 0, (
            f"the recovery path named in the refusal must work: exit {done.returncode}\n"
            f"{done.stdout}{done.stderr}"
        )
        rebuilt, errs = load_baseline(moved)
        assert rebuilt is not None and not errs, f"recovered baseline must verify: {errs}"
        assert not any(
            a.get("reason") == "TAMPERED-MARKER" for a in rebuilt.authorizations
        ), "regeneration must NOT carry the tampered audit record across"

    # Decision-table coverage of the seam itself. Cheap, and it pins the boundary the
    # process cases cannot enumerate: absent file proceeds, clean load proceeds,
    # non-empty errors refuse whether or not a Baseline came back.
    absent = Path(tmp) / "gone.json"
    assert baseline_update_refusal(absent, None, ["file not found"]) is None, (
        "a missing baseline must be allowed to bootstrap, or a tampered file has no "
        "recovery path at all"
    )
    with tempfile.NamedTemporaryFile(suffix=".json") as handle:
        here = Path(handle.name)
        assert baseline_update_refusal(here, None, []) is None
        assert baseline_update_refusal(here, None, ["unloadable"]) is not None
        # The #2946 case in one line: a NON-None Baseline with errors must still refuse.
        loaded = Baseline({}, [])
        assert baseline_update_refusal(here, loaded, ["digest mismatch"]) is not None, (
            "a baseline that loaded but failed its digest must refuse — returning a "
            "Baseline alongside errors is exactly how #2946 slipped through"
        )


def _trigger_paths_self_test() -> None:
    """#2919: `_trigger_paths` must strip quotes of EITHER kind, not just single.

    Pinned to the PARSER via synthetic YAML, not to `fuzz-nightly.yml`'s current
    contents — a test that only passes once someone re-quotes that file is not a
    guard on the parser, and would go silent the moment the file is re-quoted or
    deleted while the defect itself is still live for the next double-quoted
    workflow.

    Measured consequence, not assumed: an entry that keeps its quote characters
    (`'"consensus-engines/src/**"'`) matches nothing. Whether that is safe or
    catastrophic depends entirely on which direction the assertion consuming it
    runs — a positive "this root must be matched" claim fails closed on it, but
    a `must_not_trigger` absence claim passes vacuously, because a list that can
    never match satisfies "did not match" for the wrong reason. Fixing the
    shared parser closes both.
    """

    def block(entries: list[str]) -> str:
        lines = ["pull_request:", "  paths:"]
        lines.extend(f"    - {entry}" for entry in entries)
        return "\n".join(lines) + "\n"

    single = block(["'a/**'", "'b/**'"])
    double = block(['"a/**"', '"b/**"'])
    bare = block(["a/**", "b/**"])
    mixed = block(["'a/**'", '"b/**"', "c/**"])

    expected = ["a/**", "b/**"]
    got_single = _trigger_paths(single, "pull_request")
    got_double = _trigger_paths(double, "pull_request")
    got_bare = _trigger_paths(bare, "pull_request")
    got_mixed = _trigger_paths(mixed, "pull_request")

    # Forms that already work today must not change — a parser fix that
    # silently alters correct existing behavior is its own regression.
    assert got_single == expected, f"single-quoted regressed: {got_single}"
    assert got_bare == expected, f"bare-token regressed: {got_bare}"

    # The actual defect: double-quoted globs must come back clean, not as
    # '"a/**"' with the quote characters still attached.
    assert got_double == expected, f"double-quoted globs still carry quote characters: {got_double}"

    # Mixed list, one of each form in the same `paths:` block.
    assert got_mixed == ["a/**", "b/**", "c/**"], f"mixed-quoting list not fully normalised: {got_mixed}"


def _covers(globs: list[str], scanned: str) -> bool:
    return any(
        g == scanned or g.rstrip("*").rstrip("/") in ("", scanned)
        or scanned.startswith(g.rstrip("*").rstrip("/") + "/")
        or scanned == g.rstrip("*").rstrip("/")
        for g in globs
    )


def check_workflow_paths(root: Path) -> list[str]:
    """Every scanned root must be covered by the workflow's `paths:` globs.

    Checked PER TRIGGER. `pull_request` is the one that decides whether a PR touching a
    scanned directory starts this job at all, so it is required; `push` is checked too
    when present, because main-branch coverage is the other half of the claim.
    """
    wf = root / WORKFLOW
    if not wf.is_file():
        return [f"{WORKFLOW} not found"]
    text = wf.read_text(encoding="utf-8")

    problems: list[str] = []
    pr_globs = _trigger_paths(text, "pull_request")
    if pr_globs is None:
        return [
            f"{WORKFLOW} has no `pull_request:` trigger — this lint cannot gate a PR at all"
        ]
    triggers: list[tuple[str, list[str]]] = [("pull_request", pr_globs)]
    push_globs = _trigger_paths(text, "push")
    if push_globs is not None:
        triggers.append(("push", push_globs))

    for name, globs in triggers:
        for scanned in SCANNED_ROOTS:
            if not _covers(globs, scanned):
                problems.append(
                    f"SCANNED_ROOTS contains {scanned!r} but no `paths:` glob under the "
                    f"`{name}:` trigger in {WORKFLOW} matches it — a {name} touching only "
                    "that directory would not start this job, and a gate that never runs "
                    "cannot fail (#2245 blocker 2)"
                )
        for required in ("scripts/check_determinism.py", "scripts/determinism_baseline.json"):
            if required not in globs:
                problems.append(
                    f"{WORKFLOW} `{name}:` must trigger on {required}: the detector and its "
                    "baseline can otherwise be changed without the gate that depends on "
                    "them running"
                )
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--check-workflow-paths", action="store_true",
                        help="assert the workflow triggers on every scanned root")
    parser.add_argument("--report", action="store_true",
                        help="print findings and always exit 0 (baseline survey)")
    parser.add_argument("--baseline", type=Path, default=None,
                        help="frozen-findings JSON; only findings NOT in it fail the gate")
    parser.add_argument("--anchor-ref", type=str, default=os.environ.get("DETERMINISM_ANCHOR_REF"),
                        help="git ref (e.g. the PR base SHA) whose committed baseline is the "
                             "trusted anchor. Only fingerprints frozen THERE suppress a "
                             "finding; entries the PR adds to its own baseline are inert. This "
                             "is what stops a PR growing the baseline to cover its own "
                             "violation (#2245). Falls back to env DETERMINISM_ANCHOR_REF.")
    parser.add_argument("--update-baseline", type=Path, default=None,
                        help="rewrite the baseline JSON from the current scan")
    parser.add_argument("--authorize-additions", type=str, default=None, metavar="REASON",
                        help="required by --update-baseline when the rewrite would ADD "
                             "findings; the reason is recorded in the baseline file")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0

    if args.check_workflow_paths:
        problems = check_workflow_paths(Path(__file__).resolve().parents[1])
        if problems:
            print('FAIL: workflow path filter does not cover the scan roots:')
            for p in problems:
                print(f'  {p}')
            return 1
        print(f'workflow path filter covers all {len(SCANNED_ROOTS)} scanned roots')
        return 0

    root = Path(__file__).resolve().parents[1]
    findings, allows, errors = scan(root, SCANNED_ROOTS)
    current = _occurrences(findings)

    if args.update_baseline is not None:
        prior, load_errs = load_baseline(args.update_baseline)
        refusal = baseline_update_refusal(args.update_baseline, prior, load_errs)
        if refusal is not None:
            print(refusal)
            return 1
        if prior is not None and requires_authorization(current, prior):
            new, grown = _additions(current, prior)
            if not args.authorize_additions:
                print(
                    f"REFUSED: this rewrite would ADD {len(new)} new fingerprint(s) and grow "
                    f"{len(grown)} existing one(s).\n"
                    "A baseline that silently absorbs new violations is not a gate (#2245 "
                    "blocker 3). Re-run with:\n"
                    "  --authorize-additions \"<why each addition is acceptable>\"\n"
                )
                for fp in new[:20]:
                    print(f"  NEW    {fp}")
                for fp in grown[:20]:
                    print(f"  GREW   {fp} ({prior.counts[fp]} -> {current[fp]})")
                if len(new) + len(grown) > 40:
                    print(f"  ... and {len(new) + len(grown) - 40} more")
                return 1
        write_baseline(args.update_baseline, current, prior,
                       args.authorize_additions or "no additions")
        # Re-read so the printed digest is the STORED one (over findings + authorizations),
        # not `_digest(current)` with no authorizations — those differ, which the #2245
        # review flagged as a confusing mismatch.
        written = json.loads(args.update_baseline.read_text(encoding="utf-8"))
        print(f"wrote baseline: {len(current)} fingerprints, {sum(current.values())} "
              f"occurrences, digest {written.get('digest')}")
        print("NOTE: this digest is an integrity checksum of the committed file, not a "
              "security anchor. Growing the baseline in a PR does NOT satisfy the gate — "
              "the gate anchors to the baseline on the base branch (--anchor-ref).")
        return 0

    _report(findings, allows, errors)

    if args.report:
        print(f"\n[report mode] {len(findings)} findings "
              f"({len(current)} distinct), {len(errors)} malformed allows (exit 0)")
        return 0

    # Gate mode. A frozen baseline suppresses pre-existing findings so the gate fails
    # ONLY on newly introduced nondeterminism; malformed allows always fail.
    new_fps: list[str] = []
    grown_fps: list[str] = []
    unanchored_entries: list[str] = []
    baseline: Baseline | None = None
    if args.baseline:
        baseline, base_errors = load_baseline(args.baseline)
        if baseline is None:
            print("FAIL: baseline unusable:")
            for e in base_errors:
                print(f"  {e}")
            return 1
        # The working-baseline digest is still checked (it catches hand-edits of the
        # committed file), but it is NOT the security anchor — see load_anchor_counts.
        errors.extend(base_errors)

        # Resolve the suppression set. With --anchor-ref, it is the baseline frozen on the
        # base branch (which the PR cannot edit); entries the PR added to its own baseline
        # do not suppress. Without it (local dev / push-to-main), the working baseline is
        # authoritative, printed as a loud caveat.
        suppress = baseline.counts
        anchor_note = "UNANCHORED — working baseline trusted (local/dev or push-to-main)"
        if args.anchor_ref:
            try:
                anchor_counts, anchor_note = load_anchor_counts(args.anchor_ref, args.baseline)
            except AnchorError as e:
                print(f"FAIL: {e}")
                return 1
            if anchor_counts is not None:
                suppress = anchor_counts
                # Entries present in the PR's baseline but NOT in the anchor are inert;
                # surface them so a reviewer sees the PR is proposing baseline growth.
                unanchored_entries = sorted(set(baseline.counts) - set(anchor_counts))

        anchor_baseline = Baseline(suppress, [])
        new_fps, grown_fps = _additions(current, anchor_baseline)
        print(f"\n[gate] {len(findings)} findings ({len(current)} distinct), "
              f"suppression set {anchor_baseline.fingerprints} fingerprints / "
              f"{anchor_baseline.occurrences} occurrences, {len(new_fps)} NEW, "
              f"{len(grown_fps)} GROWN")
        print(f"[gate] {anchor_note}")
        if unanchored_entries:
            print(f"[gate] {len(unanchored_entries)} baseline entr(y/ies) added in this "
                  "change are INERT until merged to the base branch — they do not suppress "
                  "findings:")
            for fp in unanchored_entries[:20]:
                print(f"  UNANCHORED {fp}")
            if len(unanchored_entries) > 20:
                print(f"  ... and {len(unanchored_entries) - 20} more")
    else:
        new_fps = sorted(current)

    # #2105: the baseline must SHRINK as violations are fixed. Measured against the
    # working baseline, which the change can edit — never the anchor, which it cannot.
    if baseline is not None:
        stale_fps, surplus_fps = _dead_capacity(current, baseline)
    else:
        stale_fps, surplus_fps = [], []

    bad = len(new_fps) + len(grown_fps) + len(stale_fps) + len(surplus_fps) + len(errors)
    if bad:
        if new_fps:
            print(f"\nFAIL: {len(new_fps)} NEW unjustified nondeterminism finding(s):")
            by_fp = {_fingerprint(f): f for f in findings}
            for fp in new_fps:
                f = by_fp[fp]
                print(f"  {f.path}:{f.line_no} [{f.rule}] {f.text}")
        if grown_fps:
            print(f"\nFAIL: {len(grown_fps)} frozen finding(s) gained occurrences "
                  f"(duplicate lines are still new violations):")
            for fp in grown_fps:
                print(f"  {fp}")
        if stale_fps:
            print(f"\nFAIL: {len(stale_fps)} frozen finding(s) NO LONGER OCCUR — the "
                  "baseline must shrink when a violation is fixed, or the entry stays a "
                  "live suppression slot for re-introducing it. Remove them:")
            for fp in stale_fps:
                print(f"  STALE {fp}")
            print("  fix: python3 scripts/check_determinism.py --update-baseline "
                  "scripts/determinism_baseline.json")
        if surplus_fps:
            print(f"\nFAIL: {len(surplus_fps)} frozen finding(s) are frozen ABOVE their "
                  "current occurrence count — the surplus is dead suppression capacity:")
            for fp in surplus_fps:
                print(f"  SURPLUS {fp} (frozen {baseline.counts[fp]}, actual {current[fp]})")
            print("  fix: python3 scripts/check_determinism.py --update-baseline "
                  "scripts/determinism_baseline.json")
        if errors:
            print(f"FAIL: {len(errors)} baseline/allow error(s)")
            for e in errors:
                print(f"  {e}")
        return 1
    print("check_determinism.py: no new unjustified nondeterminism in scanned crates")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
