#!/usr/bin/env python3
"""Ban panics on attacker-controlled input paths (#2111).

A panic caused by attacker-controlled data is a security finding, not a bug — a remotely
triggered panic is a launch blocker. This lint scans the input-facing modules (the ones a
hostile peer or RPC client reaches first) for constructs that panic or allocate unboundedly
on decoded bytes, and fails on any NEW unjustified hit.

Detection classes (non-test code only):
  * `.unwrap()` / `.expect(`                          — panics on Err/None.
  * non-literal slice/array indexing `x[i]`           — panics on out-of-range (heuristic).
  * `Vec::with_capacity(n)` / `vec![_; n]`            — unbounded alloc if n is decoded.
  * `.collect()` into an owned collection             — unbounded alloc from a length field.

Escape: `// panic-ok: <justification>` on the same line or the line above; empty justification
is itself a failure. All active allows are printed.

Honest scope: the input modules already contain many such sites, so a hard fail-on-any gate
would be red. The gate runs in BASELINE mode (scripts/panic_input_baseline.json): it fails only
on NEW hits. The scanned-module list is the single editable constant SCANNED_MODULES.

The baseline's unit of identity is the **site** (#2284). An earlier revision keyed on
`path|rule|text` as a set, which meant a new panic whose source text matched anything already
frozen in the same file was free — the review of #2248 planted an attacker-controlled
`.expect(...)` in a peer-payload function reusing an existing fingerprint and the gate reported
`0 NEW` with no baseline edit at all. A fingerprint is now
`path|enclosing item|rule|normalized text|ordinal`, so identical text in a different function,
or a second identical line in the same function, is a distinct site.

Baseline growth is authorized **per fingerprint**, not by a boolean. Each fingerprint added to
the baseline must be listed in `scripts/panic_input_baseline.migrations` under a dated entry;
`--ratchet` accepts only the additions the log names. Previously any diff to that file set
`--allow-migration`, which made the escape hatch an off switch.

    python3 scripts/check_no_panic_on_input.py --self-test
    python3 scripts/check_no_panic_on_input.py --report
    python3 scripts/check_no_panic_on_input.py --baseline scripts/panic_input_baseline.json
"""

from __future__ import annotations

import argparse
import json
import re
import tempfile
from dataclasses import dataclass
from pathlib import Path

# --- Single editable constant: the attacker-controlled input surface -------------------
#
# These are DIRECTORIES (not hand-picked files) covering every trust boundary a remote
# attacker reaches — peer/gossip messages, the peer-event consumer, RF/IoT/sensor ingest,
# API request bodies, and contract calldata. Directory-level coverage means a NEW decoder
# added under any of these paths is scanned automatically; it does not have to be added to
# this list. A panic reachable from any of these paths is a security finding (#2111).
#
# Deliberately NOT "all production Rust": an `unwrap` in a pure-compute helper is not a
# remote-panic vector, and a whole-tree scan would bury the security signal under ~2000
# non-input findings. Coverage is the input surface plus a hard ratchet (below) so it
# cannot silently rot. Add a directory here when a new trust boundary appears.
SCANNED_MODULES: list[str] = [
    "p2p/src",  # all peer/gossip message + handshake + sync + DA-challenge decoders
    "shared-protocol/src/mempool",  # transaction admission from peers/RPC
    "shared-protocol/src/cross_chain",  # bridge messages / cross-chain calldata
    "native-chain/src/bin/launch_api",  # RPC handlers, websocket, AND the p2p_events peer consumer
    "execution-engine/src",  # contract calldata / VM input
    "sensor-attestation/src",  # RF/IoT sensor attestation ingest
    "oracle-packet/src",  # oracle/weather packet decoding
]

ALLOW_MARKER = "panic-ok:"

# --- allow reason quality (#2981 story 4) ---------------------------------
# An allow is a claim that a panic cannot fire. A claim that cannot be checked
# is worth nothing: it silences the gate and tells the next reader nothing they
# can verify or falsify. Empty reasons were already rejected; these two rules
# reject the next tier up — reasons that are present but say nothing.
#
# Deliberately NOT a judgement of whether the reason is TRUE. This gate cannot
# know that. It only requires the reason to be the KIND of statement that could
# be checked: enough words to name a mechanism, and not purely an assertion of
# safety with no mechanism at all.
MIN_REASON_WORDS = 4

# Matched against the whole normalized reason, so "unreachable" is rejected
# while "unreachable because the caller filters None above" is not: the point
# is a reason that is ONLY an assertion, not one that contains the word.
VACUOUS_REASON_RE = re.compile(
    r"^(?:"
    r"safe|ok|fine|n/?a|none|nope|"
    r"(?:it|this|that)?\s*(?:is|'s)?\s*(?:obviously\s+)?safe|"
    r"(?:can(?:no|')?t|will\s*not|won'?t|never)\s+(?:happen|panic|fail|occur)|"
    r"(?:not|un)\s*reachable|impossible|"
    r"checked|already\s+checked|verified|validated|"
    r"see\s+above|as\s+above|per\s+above|"
    r"by\s+design|intentional|deliberate|known|"
    r"todo.*|fixme.*|xxx.*|trust\s+me.*"
    r")[.!]?$",
    re.IGNORECASE,
)


def reason_quality_error(reason: str) -> str | None:
    """Return a failure message when an allow reason is unfalsifiable, else None."""
    normalized = " ".join(reason.split())
    if VACUOUS_REASON_RE.match(normalized):
        return (
            f"allow reason {reason!r} asserts safety without naming a mechanism. "
            f"State WHY the panic cannot fire (the bound, the guard, the caller "
            f"contract) so the claim can be falsified when it stops being true"
        )
    if len(normalized.split()) < MIN_REASON_WORDS:
        return (
            f"allow reason {reason!r} is too short to carry a mechanism "
            f"(< {MIN_REASON_WORDS} words). Name what bounds or guards the value"
        )
    return None


@dataclass(frozen=True)
class Rule:
    name: str
    pattern: re.Pattern[str]
    message: str


RULES: list[Rule] = [
    Rule("unwrap", re.compile(r"\.unwrap\(\)"), "`.unwrap()` panics on Err/None"),
    Rule("expect", re.compile(r"\.expect\("), "`.expect(...)` panics on Err/None"),
    # Indexing where the index is not a plain integer literal (heuristic).
    Rule(
        "index",
        re.compile(r"[A-Za-z0-9_\)\]]\[\s*[A-Za-z_][A-Za-z0-9_\.]*\s*\]"),
        "non-literal indexing panics on out-of-range access",
    ),
    Rule(
        "with-capacity",
        re.compile(r"\bwith_capacity\(\s*[A-Za-z_]"),
        "`with_capacity(n)` with a non-literal n can allocate unboundedly from decoded input",
    ),
    Rule(
        "vec-repeat",
        re.compile(r"vec!\[[^;\]]*;\s*[A-Za-z_]"),
        "`vec![_; n]` with a non-literal n can allocate unboundedly from decoded input",
    ),
    Rule(
        "collect",
        re.compile(r"\.collect::<Vec"),
        "`.collect::<Vec<_>>()` can allocate unboundedly from a decoded length",
    ),
]


def _allow_just(line: str) -> str | None:
    idx = line.find(ALLOW_MARKER)
    return None if idx == -1 else line[idx + len(ALLOW_MARKER):].strip()


ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?"
    r"(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?(fn|impl|mod|trait)\s+([A-Za-z0-9_<>:'\s,]*)"
)


def _is_test_path(rel: str) -> bool:
    """Files that exist only for tests, by layout rather than by in-file attribute.

    `p2p/src/tests/*.rs` is reached through a `#[cfg(test)] mod tests;` declaration in
    another file, so nothing inside them marks them as test code. Their shared helpers are
    plain `fn`s with no `#[test]` attribute — 26 such sites sat in the baseline as though a
    remote attacker could reach a test fixture. A panic in a test is a failing test, not a
    security finding.
    """
    parts = Path(rel).parts
    return "tests" in parts or Path(rel).name in ("tests.rs", "test_utils.rs")


def _normalize(text: str) -> str:
    """Whitespace-collapsed source text: reformatting must not churn the baseline."""
    return " ".join(text.split())


def _strip_literals(line: str, in_string: str | None = None) -> tuple[str, str | None]:
    """Drop comments and literal bodies, retaining string state across lines."""
    out, i, n = [], 0, len(line)
    while i < n:
        c = line[i]
        if in_string is not None:
            if c == "\\":
                i += 2
                continue
            if c == in_string:
                in_string = None
            i += 1
            continue
        if c == "/" and i + 1 < n and line[i + 1] == "/":
            break
        # Lifetimes such as `&'static str` must not open a cross-line literal.
        if c == "\"":
            in_string, i = c, i + 1
            continue
        out.append(c)
        i += 1
    return "".join(out), in_string


def _test_mask(lines: list[str]) -> list[bool]:
    """Per-line mask: inside a `#[cfg(test)]` module or `#[test]` fn?

    Tracks brace depth so the region ends. The earlier `_in_test` scanned backwards up to 400
    lines for `#[cfg(test)]` and said yes if it found one, which exempted every finding
    *after* a test module in the same file — a coverage hole in the gate's own scope
    predicate, and the same #2284 class as the fingerprint bug.
    """
    mask = [False] * len(lines)
    depth = 0
    in_string: str | None = None
    pending = False
    test_depth: int | None = None
    for i, raw in enumerate(lines):
        line, in_string = _strip_literals(raw, in_string)
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
            # Only a real item declaration may open the test region. Consuming the next
            # line with ANY brace is wrong: a `use path::{A, B};` import after a stray
            # `#[cfg(test)]` opened a bogus region at depth 0 and masked the rest of the
            # file — 6503 of 9343 lines in `launch_api/main.rs`, silently removing them
            # from the scan.
            if ITEM_RE.match(line) and opens > 0:
                test_depth = depth
                mask[i] = True
                pending = False
            elif stripped and not stripped.startswith("#["):
                # Something that is neither the attributed item nor another attribute:
                # the attribute did not introduce a test region we can track.
                pending = False
        depth += opens - closes
        if test_depth is not None and depth <= test_depth:
            test_depth = None
    return mask


def _enclosing_item(lines: list[str], i: int) -> str:
    """Nearest enclosing `fn`/`impl`/`mod`/`trait` declaration above line `i`."""
    for j in range(i, -1, -1):
        m = ITEM_RE.match(lines[j])
        if m:
            return f"{m.group(1)} {_normalize(m.group(2))}".strip()
    return "<file scope>"


@dataclass
class Finding:
    path: str
    line_no: int
    rule: str
    text: str
    item: str = "<file scope>"
    ordinal: int = 0


def scan_file(path: Path, root: Path) -> tuple[list[Finding], list[str], list[str]]:
    rel = str(path.relative_to(root))
    if _is_test_path(rel):
        return [], [], []
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    findings: list[Finding] = []
    allows: list[str] = []
    errors: list[str] = []
    in_test = _test_mask(lines)
    seen: dict[tuple[str, str, str], int] = {}
    for i, line in enumerate(lines):
        just = _allow_just(line)
        if just is not None:
            if not just:
                errors.append(f"{rel}:{i + 1}: `{ALLOW_MARKER}` requires a non-empty justification")
            else:
                quality = reason_quality_error(just)
                if quality is not None:
                    errors.append(f"{rel}:{i + 1}: {quality}")
                allows.append(f"{rel}:{i + 1}: {just}")
        if line.lstrip().startswith("//") or in_test[i]:
            continue
        here = _allow_just(line)
        above = _allow_just(lines[i - 1]) if i > 0 else None
        if here or above:
            continue
        for rule in RULES:
            if rule.pattern.search(line):
                item = _enclosing_item(lines, i)
                text = _normalize(line)
                key = (item, rule.name, text)
                ordinal = seen.get(key, 0)
                seen[key] = ordinal + 1
                findings.append(Finding(rel, i + 1, rule.name, text, item, ordinal))
    return findings, allows, errors


def scan(root: Path) -> tuple[list[Finding], list[str], list[str]]:
    f: list[Finding] = []
    a: list[str] = []
    e: list[str] = []
    for rel in SCANNED_MODULES:
        base = root / rel
        if not base.exists():
            e.append(f"scanned module does not exist: {rel}")
            continue
        targets = sorted(base.rglob("*.rs")) if base.is_dir() else [base]
        for t in targets:
            ff, aa, ee = scan_file(t, root)
            f.extend(ff)
            a.extend(aa)
            e.extend(ee)
    return f, a, e


def _fp(fi: Finding) -> str:
    """Stable per-site identity: file, enclosing item, rule, normalized text, ordinal."""
    return f"{fi.path}|{fi.item}|{fi.rule}|{fi.text}|{fi.ordinal}"


# A migration entry authorizes specific fingerprints, listed one per line as `+ <fingerprint>`.
MIGRATION_FP_RE = re.compile(r"^\+\s+(.+?)\s*$")


def parse_migrations(path: Path) -> set[str]:
    """Fingerprints explicitly authorized by the migration log.

    The escape hatch used to be a boolean the CI set whenever this file differed at all, so
    any edit to it authorized any growth. Authorization is now per fingerprint: to migrate
    debt you write the fingerprint down, in the diff, where a reviewer reads it.
    """
    if not path.is_file():
        return set()
    authorized: set[str] = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.lstrip().startswith("#"):
            continue
        m = MIGRATION_FP_RE.match(line)
        if m:
            authorized.add(m.group(1))
    return authorized


def self_test() -> None:
    cases = {
        "unwrap.rs": "pub fn f(x: Option<u8>) -> u8 { x.unwrap() }\n",
        "expect.rs": "pub fn f(x: Option<u8>) -> u8 { x.expect(\"nope\") }\n",
        "index.rs": "pub fn f(b: &[u8], i: usize) -> u8 { b[i] }\n",
        "cap.rs": "pub fn f(n: usize) -> Vec<u8> { Vec::with_capacity(n) }\n",
        "collect.rs": "pub fn f(it: impl Iterator<Item=u8>) -> Vec<u8> { it.collect::<Vec<u8>>() }\n",
    }
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        mod = root / "p2p/src"
        mod.mkdir(parents=True)
        # Point one scanned module at our temp file set by scanning the dir directly.
        global SCANNED_MODULES
        saved = SCANNED_MODULES
        SCANNED_MODULES = ["p2p/src"]
        try:
            for name, body in cases.items():
                (mod / name).write_text(body, encoding="utf-8")
            findings, _a, errors = scan(root)
            hit = {f.rule for f in findings}
            for r in ("unwrap", "expect", "index", "with-capacity", "collect"):
                assert r in hit, f"{r} not detected: {hit}"
            assert errors == [], errors

            # Justified allow suppresses; empty justification errors.
            (mod / "unwrap.rs").write_text(
                "pub fn f(x: Option<u8>) -> u8 { x.unwrap() } // panic-ok: x is a compile-time constant\n",
                encoding="utf-8",
            )
            f2, a2, _ = scan(root)
            assert not any(f.path.endswith("unwrap.rs") for f in f2), "allow must suppress"
            assert any("unwrap.rs" in x for x in a2), "allow must be recorded"

            (mod / "unwrap.rs").write_text(
                "pub fn f(x: Option<u8>) -> u8 { x.unwrap() } // panic-ok:\n", encoding="utf-8"
            )
            _f3, _a3, e3 = scan(root)
            assert any("non-empty justification" in x for x in e3), e3

            # -- reason-quality probes (#2981 story 4) -------------------------
            # The rule is only worth having if it can actually reject. Each probe
            # plants a reason that IS present and non-empty — the tier the old
            # emptiness check already passed — and requires it to be refused.
            for bad, why in (
                ("safe", "bare assertion"),
                ("cannot happen", "assertion with no mechanism"),
                ("unreachable.", "assertion with trailing punctuation"),
                ("see above", "defers instead of stating"),
                ("checked", "claims a check without naming it"),
                ("bounded by config", "under the word floor"),
            ):
                (mod / "unwrap.rs").write_text(
                    f"pub fn f(x: Option<u8>) -> u8 {{ x.unwrap() }} // panic-ok: {bad}\n",
                    encoding="utf-8",
                )
                _f, _a, e = scan(root)
                assert any("allow reason" in x for x in e), (
                    f"reason {bad!r} ({why}) must be rejected, got: {e}"
                )

            # The negative control. Without this the rule could reject everything
            # and every probe above would still pass.
            (mod / "unwrap.rs").write_text(
                "pub fn f(x: Option<u8>) -> u8 { x.unwrap() } "
                "// panic-ok: x is Some because the caller filters None at the ingest boundary\n",
                encoding="utf-8",
            )
            _f, _a, e_ok = scan(root)
            assert not any("allow reason" in x for x in e_ok), (
                f"a reason naming a mechanism must be accepted, got: {e_ok}"
            )

            # A reason that CONTAINS a vacuous word but also states a mechanism
            # must survive — the rule matches the whole reason, not substrings.
            (mod / "unwrap.rs").write_text(
                "pub fn f(x: Option<u8>) -> u8 { x.unwrap() } "
                "// panic-ok: unreachable because the length check above rejects zero first\n",
                encoding="utf-8",
            )
            _f, _a, e_sub = scan(root)
            assert not any("allow reason" in x for x in e_sub), (
                f"a mechanism-bearing reason must not be rejected for containing a word: {e_sub}"
            )
        finally:
            SCANNED_MODULES = saved

    # -- mutation probes: plant a violation INSIDE an existing baselined unit (#2284) ------
    #
    # The previous self-test asked only "is an obviously-new violation detected?", which the
    # `path|rule|text` set answered yes to while the review's probe walked straight past it.
    # These ask the question that separates the units.
    def _scan_src(src: str) -> list[Finding]:
        global SCANNED_MODULES
        saved = SCANNED_MODULES
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "p2p/src").mkdir(parents=True)
            (root / "p2p/src/x.rs").write_text(src, encoding="utf-8")
            SCANNED_MODULES = ["p2p/src"]
            try:
                return scan(root)[0]
            finally:
                SCANNED_MODULES = saved

    def _new(src: str, baseline: set[str]) -> list[Finding]:
        return [f for f in _scan_src(src) if _fp(f) not in baseline]

    grandfathered = 'pub fn handle(p: &[u8], i: usize) -> u8 {\n    p[i]\n}\n'
    base = _scan_src(grandfathered)
    assert len(base) == 1, base
    frozen = {_fp(base[0])}
    assert _new(grandfathered, frozen) == [], "the frozen site alone must pass"

    # PROBE 1 — the review's reproduction: a second attacker-controlled panic in the SAME
    # function, byte-identical to a frozen one. A `path|rule|text` set sees nothing.
    same_fn = 'pub fn handle(p: &[u8], i: usize) -> u8 {\n    p[i];\n    p[i]\n}\n'
    assert _new(same_fn, frozen), (
        "PROBE 1 FAILED: a second identical panic inside the baselined function was not NEW — "
        "the baseline unit is coarser than the violation (#2284)"
    )

    # PROBE 2 — identical text in a DIFFERENT function in the same file, which is exactly what
    # the reviewer planted in the peer-payload path.
    other_fn = 'pub fn handle(p: &[u8], i: usize) -> u8 {\n    p[i]\n}\npub fn peer(p: &[u8], i: usize) -> u8 {\n    p[i]\n}\n'
    assert _new(other_fn, frozen), (
        "PROBE 2 FAILED: identical text in another function reused a frozen fingerprint"
    )

    # PROBE 3 — reformatting must NOT churn the baseline.
    reformatted = 'pub fn unrelated() {}\npub fn handle(p: &[u8], i: usize) -> u8 {\n        p[i]\n}\n'
    assert _new(reformatted, frozen) == [], "PROBE 3 FAILED: reformatting churned the fingerprint"

    # PROBE 4 — production code AFTER a test module must still be scanned. The earlier
    # `_in_test` looked backwards 400 lines for `#[cfg(test)]`; deep inside a long test module
    # it also lost the context and wrongly scanned test code as production.
    after_tests = (
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        let v: Vec<u8> = vec![];\n"
        "        let _ = v[i];\n    }\n}\n"
        "pub fn prod_after(p: &[u8], i: usize) -> u8 {\n    p[i]\n}\n"
    )
    found = _scan_src(after_tests)
    assert len(found) == 1 and found[0].item == "fn prod_after", (
        f"PROBE 4 FAILED: expected only the post-test-module production site, got {found}"
    )

    # PROBE 5 — a continued string and a quote in a comment must not close a
    # test module early. Both forms previously desynchronized the line-local
    # literal stripper and silently scanned the tail as production.
    continued_literal = (
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n"
        "        let _s = \"open \\\\n+        close\";\n        // unmatched quote: \"\n"
        "        let v: Vec<u8> = vec![];\n        let _ = v[i];\n    }\n}\n"
        "pub fn prod_after(p: &[u8], i: usize) -> u8 { p[i] }\n"
    )
    found = _scan_src(continued_literal)
    assert len(found) == 1 and found[0].item == "fn prod_after", (
        "PROBE 5 FAILED: continued literal or comment quote unmasked test code"
    )

    # PROBE 6 — the migration escape must be bound to the fingerprint it authorizes, not to
    # "the file was touched". An unrelated entry must not launder an unrelated addition.
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        basef, headf, migf = root / "b.json", root / "h.json", root / "m.migrations"
        basef.write_text(json.dumps(["a|fn f|index|x[0]|0"]))
        headf.write_text(json.dumps(["a|fn f|index|x[0]|0", "a|fn g|index|x[0]|0"]))
        migf.write_text("# log\n2026-01-01 (#1): unrelated note, no fingerprints listed\n")
        assert ratchet(basef, headf, migf) == 1, (
            "PROBE 5 FAILED: baseline growth was accepted by a migration entry that does not "
            "name the added fingerprint — the escape hatch is an off switch"
        )
        migf.write_text("# log\n2026-01-01 (#1): authorized\n+ a|fn g|index|x[0]|0\n")
        assert ratchet(basef, headf, migf) == 0, "a named fingerprint must be authorized"

    print("check_no_panic_on_input.py self-test passed (6 mutation probes)")


def ratchet(base_path: Path, head_path: Path, migrations_path: Path) -> int:
    """Fail if the committed baseline GREW vs the base (merge-base) baseline.

    The baseline is a debt list that may only shrink. Any added fingerprint means a new
    violation was frozen instead of fixed — the bypass this gate exists to prevent.

    Growth is authorized **per fingerprint** by the migration log. The previous form took a
    boolean the CI derived from "did this file change at all", so any edit — a typo fix in a
    comment — authorized any growth. The escape hatch was an off switch.
    """
    base = set(json.loads(base_path.read_text())) if base_path.is_file() else set()
    head = set(json.loads(head_path.read_text())) if head_path.is_file() else set()
    authorized = parse_migrations(migrations_path)
    added = sorted(head - base)
    removed = len(base - head)
    print(f"[ratchet] base={len(base)} head={len(head)} added={len(added)} "
          f"removed={removed} authorized-by-migration={len(authorized)}")
    if not added:
        print("ratchet OK: baseline did not grow (debt only shrank or held).")
        return 0

    unauthorized = [fp for fp in added if fp not in authorized]
    if not unauthorized:
        print(f"ratchet OK: all {len(added)} added fingerprint(s) are named in "
              f"{migrations_path.name}.")
        return 0

    print(f"FAIL: baseline grew by {len(added)} fingerprint(s), {len(unauthorized)} of which "
          f"no migration entry authorizes. A new violation must be FIXED, not baselined. To "
          f"migrate debt intentionally, add a dated entry to {migrations_path.name} listing "
          f"each fingerprint on its own line as `+ <fingerprint>`.")
    for fp in unauthorized[:20]:
        print(f"  UNAUTHORIZED {fp}")
    if len(unauthorized) > 20:
        print(f"  ... and {len(unauthorized) - 20} more")
    return 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--report", action="store_true")
    parser.add_argument("--baseline", type=Path, default=None)
    parser.add_argument("--update-baseline", type=Path, default=None)
    parser.add_argument("--ratchet", type=Path, nargs=2, metavar=("BASE", "HEAD"),
                        help="fail if HEAD baseline grew vs BASE baseline")
    parser.add_argument("--migrations", type=Path,
                        default=Path(__file__).resolve().parent / "panic_input_baseline.migrations",
                        help="migration log naming the fingerprints whose addition is authorized")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0

    if args.ratchet is not None:
        return ratchet(args.ratchet[0], args.ratchet[1], args.migrations)

    root = Path(__file__).resolve().parents[1]
    findings, allows, errors = scan(root)

    if args.update_baseline is not None:
        fps = sorted({_fp(f) for f in findings})
        args.update_baseline.write_text(json.dumps(fps, indent=2) + "\n", encoding="utf-8")
        print(f"wrote {len(fps)} frozen panic-on-input sites to {args.update_baseline}")
        return 0

    if allows:
        print(f"Active panic-ok allows ({len(allows)}):")
        for a in allows:
            print(f"  {a}")

    if args.report:
        print(f"[report] {len(findings)} findings, {len(errors)} malformed allows (exit 0)")
        return 0

    baseline = set(json.loads(args.baseline.read_text())) if args.baseline and args.baseline.is_file() else set()
    new = [f for f in findings if _fp(f) not in baseline]
    print(f"[gate] {len(findings)} findings, {len(baseline)} frozen, {len(new)} NEW, "
          f"{len(errors)} malformed allows")
    if new or errors:
        for f in new:
            print(f"  NEW {f.path}:{f.line_no} [{f.rule}] {f.text}")
        for e in errors:
            print(f"  {e}")
        print("FAIL: new panic-on-input risk or malformed allow")
        return 1
    print("check_no_panic_on_input.py: no new panic-on-input risk in scanned modules")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
