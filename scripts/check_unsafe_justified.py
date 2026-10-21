#!/usr/bin/env python3
"""Require every `unsafe` block/fn to carry a `// SAFETY:` justification (#2106).

Scans the UB-prone crates (primitives, shared-protocol, consensus-engines — the last
contains the `ffi.rs` FFI surface) and, for every `unsafe` in non-test code, requires a
`// SAFETY:` comment on the line above (or on the same line). Emits a full inventory of
every unsafe site with its justification (or MISSING) so CI can archive it as an artifact.

Honest scope: the tree already has unjustified `unsafe` sites, so a hard fail-on-any gate
would be red. The gate runs in BASELINE mode (scripts/unsafe_baseline.json): it fails only
on NEW unjustified `unsafe`. --inventory prints every site; --self-test proves detection.

The baseline's unit of identity is the **site**, not a count and not a shape (#2284).

Two earlier forms both failed the same probe — *introduce a violation inside an existing
baselined unit* — because their unit was coarser than the thing they claimed to detect:

* a set of `path|text` shapes accepted any number of bare `unsafe {` in a baselined file;
* a count per `path|text` accepted a **remove-and-replace swap**: justify the grandfathered
  block, add an unjustified one elsewhere, count unchanged, gate green. Counts detect *more*
  blocks, not *different* ones.

A site fingerprint is `path|enclosing item|normalized text|ordinal`:

* **enclosing item** — the nearest enclosing `fn`/`impl`/`mod` declaration. This is what makes
  a swap visible: a replacement block lives in a different item, so it is a different
  fingerprint even when the source text is identical.
* **normalized text** — whitespace-collapsed, so reformatting does not churn the baseline.
* **ordinal** — index among identical `(item, text)` pairs, so a second identical `unsafe` in
  the *same* function is still a distinct site.

No line numbers: those churn on every edit above the site, which is what pushed the earlier
revisions toward coarser units in the first place.

Baseline entries carry a justification, and the justification must reference an issue
(`#1234`). `--update-baseline` writes placeholders that deliberately **fail** the gate, so
regenerating the baseline in the same change as a violation cannot launder it — the
reviewer's exact reproduction. Grandfathering becomes a recorded decision visible in the
diff rather than a silent count bump.

    python3 scripts/check_unsafe_justified.py --self-test
    python3 scripts/check_unsafe_justified.py --inventory
    python3 scripts/check_unsafe_justified.py --baseline scripts/unsafe_baseline.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import tempfile
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

SCANNED_PATHS: list[str] = ["primitives/src", "shared-protocol/src", "consensus-engines/src"]

# `unsafe {`, `unsafe fn`, `unsafe impl`, `unsafe extern`, `unsafe trait`.
# The earlier revision omitted the `impl`/`extern` forms (review of #2247).
UNSAFE_RE = re.compile(r"\bunsafe\s+(\{|fn\b|impl\b|extern\b|trait\b)")
SAFETY_RE = re.compile(r"//\s*SAFETY:")
# The nearest enclosing declaration, used as the stable part of a site's identity.
ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?"
    r"(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?(fn|impl|mod|trait)\s+([A-Za-z0-9_<>:'\s,]*)"
)
# A grandfathering justification must point at a tracked decision.
ISSUE_RE = re.compile(r"#\d+")
BASELINE_PLACEHOLDER = "TODO: justify this grandfathered site and cite the tracking issue (#NNNN)"


@dataclass
class UnsafeSite:
    path: str
    line_no: int
    justified: bool
    text: str
    item: str = "<file scope>"
    ordinal: int = 0
    body: str = ""          # normalized body of the unsafe construct
    body_digest: str = ""   # short hash of `body`, carried in the fingerprint


_CHAR_LIT_RE = re.compile(r"'(?:\\.|[^\\'])'")


def _strip_literals(line: str) -> str:
    """Remove line comments and string/char literal bodies before counting braces.

    Only needs to be good enough that a `{` inside `"{"` does not desynchronise the depth
    tracking; it is not a Rust parser.

    A `'` is a char literal ONLY if it closes (`'x'`, `'\n'`, `'\''`). Otherwise it is a
    Rust LIFETIME (`'static`, `'a`, `Formatter<'_>`), and treating it as a string opener
    consumes to end-of-line and eats real braces. That was a live defect: a
    `Formatter<'_>) -> fmt::Result {` line lost its `{`, desynchronising brace-depth
    tracking and closing a `#[cfg(test)]` region early — so production `unsafe` below it
    was scanned as test code and exempted. Found via the sibling determinism lint (#2245).
    """
    out, i, n = [], 0, len(line)
    while i < n:
        c = line[i]
        if c == "/" and i + 1 < n and line[i + 1] == "/":
            break
        if c == "'":
            m = _CHAR_LIT_RE.match(line, i)
            if m:
                i = m.end()
                continue
            out.append(c)
            i += 1
            continue
        if c == '"':
            i += 1
            while i < n:
                if line[i] == "\\":
                    i += 2
                    continue
                if line[i] == '"':
                    i += 1
                    break
                i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def _strip_comment_only(line: str) -> str:
    """Cut a `//` line comment but KEEP string literal bodies.

    `_strip_literals` erases literal bodies, which is right for brace counting and wrong for
    reading a `cfg` predicate: it turns `feature = "hsm"` into `feature = `, collapsing
    distinct atoms into one. Attribute detection needs the values intact.

    Comments must still go. `_strip_literals` breaking at `//` is load-bearing today: it is
    what stops a commented-out `// #[cfg(test)]` sitting above a `mod foo {` from opening a
    bogus test region and exempting the rest of the file. That protection is preserved here.
    """
    i, n = 0, len(line)
    while i < n:
        c = line[i]
        if c == "/" and i + 1 < n and line[i + 1] == "/":
            return line[:i]
        if c == '"':
            i += 1
            while i < n:
                if line[i] == "\\":
                    i += 2
                    continue
                if line[i] == '"':
                    i += 1
                    break
                i += 1
            continue
        if c == "'":
            m = _CHAR_LIT_RE.match(line, i)
            if m:
                i = m.end()
                continue
            i += 1
            continue
        i += 1
    return line


def _cfg_predicate(stripped: str) -> str | None:
    """The inside of a leading `#[cfg(...)]`, paren-balanced, or None.

    Balanced scanning rather than a regex: `cfg(all(test, debug_assertions))` has nested
    parens, so a non-greedy `\\(.*?\\)` stops at the wrong one and a greedy one runs past the
    attribute on a `#[cfg(test)] mod tests {` single-line form.
    """
    m = re.match(r"^#!?\[\s*cfg\s*\(", stripped)
    if not m:
        return None
    open_idx = stripped.index("(", m.end() - 1)
    depth = 0
    for j in range(open_idx, len(stripped)):
        if stripped[j] == "(":
            depth += 1
        elif stripped[j] == ")":
            depth -= 1
            if depth == 0:
                return stripped[open_idx + 1 : j]
    return None


def _parse_cfg(pred: str) -> tuple | None:
    """`all`/`any`/`not`/atom AST for a cfg predicate, or None if not understood."""
    toks = re.findall(r'"[^"]*"|[A-Za-z_][A-Za-z0-9_]*|[(),=]', pred)
    pos = 0

    def parse() -> tuple | None:
        nonlocal pos
        if pos >= len(toks):
            return None
        t = toks[pos]
        if t in ("all", "any", "not") and pos + 1 < len(toks) and toks[pos + 1] == "(":
            op, args = t, []
            pos += 2
            while pos < len(toks) and toks[pos] != ")":
                a = parse()
                if a is None:
                    return None
                args.append(a)
                if pos < len(toks) and toks[pos] == ",":
                    pos += 1
            if pos >= len(toks):
                return None
            pos += 1
            if op == "not" and len(args) != 1:
                return None
            return (op, args)
        if not re.match(r"^[A-Za-z_]", t):
            return None
        name = t
        pos += 1
        if pos < len(toks) and toks[pos] == "=":
            pos += 1
            if pos >= len(toks):
                return None
            name = f"{name}={toks[pos]}"
            pos += 1
        return ("atom", name)

    ast = parse()
    return ast if ast is not None and pos == len(toks) else None


def _cfg_eval(ast: tuple, env: dict[str, bool]) -> bool:
    kind, val = ast
    if kind == "atom":
        return env.get(val, False)
    if kind == "all":
        return all(_cfg_eval(a, env) for a in val)
    if kind == "any":
        return any(_cfg_eval(a, env) for a in val)
    return not _cfg_eval(val[0], env)


def _cfg_is_test_only(pred: str) -> bool:
    """Does this cfg predicate hold ONLY when `test` is set?

    The distinction the gate depends on, and the reason this is not a string match:

    * `test`, `all(test, debug_assertions)`, `all(test, feature = "x")` — cannot compile
      without `test`, so the code is unreachable in a production build and is exempt;
    * `any(test, feature = "x")` — compiles in a NON-test build whenever `x` is on, so
      exempting it would hide production `unsafe`. Not exempt;
    * `not(test)` — production only. Not exempt.

    Decided by refutation, not pattern: fix `test = false`, then search for any assignment of
    the remaining atoms that still satisfies the predicate. One witness means a production
    build can reach the code, so it is not test-only. No witness means it is. Unknown atoms
    are free variables; anything unparseable or implausibly wide returns False, so the
    failure direction is "scan it as production", never "exempt it".
    """
    ast = _parse_cfg(pred)
    if ast is None:
        return False
    atoms: set[str] = set()

    def collect(node: tuple) -> None:
        kind, val = node
        if kind == "atom":
            atoms.add(val)
        else:
            for a in val:
                collect(a)

    collect(ast)
    others = sorted(atoms - {"test"})
    if len(others) > 12:
        return False
    for bits in range(1 << len(others)):
        env = {"test": False}
        for j, name in enumerate(others):
            env[name] = bool(bits >> j & 1)
        if _cfg_eval(ast, env):
            return False
    return True


def _opens_test_region(attr_line: str) -> bool:
    """Is this attribute line a test gate — `#[test]`, `#[*::test]`, or a test-only `cfg`?"""
    if attr_line.startswith("#[test]") or re.match(r"#\[\w+(::\w+)*::test\]", attr_line):
        return True
    pred = _cfg_predicate(attr_line)
    return pred is not None and _cfg_is_test_only(pred)


def _test_mask(lines: list[str]) -> list[bool]:
    """Per-line mask: is this line inside a test-only `cfg` module or a `#[test]` fn?

    Tracks brace depth so the region **ends**. The earlier revision scanned backwards up to
    400 lines for `#[cfg(test)]` and answered yes if it found one — which exempted every
    `unsafe` appearing *after* a test module in the same file. `consensus-engines/src/ffi.rs`
    has `mod tests` at line 411 of 529, so production code appended below it was invisible to
    the gate entirely: a third instance of #2284, in the gate's own scope predicate.

    The region opener was a literal `#[cfg(test)]` string match, which does not recognise any
    other test-only form. `shared-protocol/src/crypto/signer.rs` gates its tests with
    `#[cfg(all(test, debug_assertions))]`, so that module never opened a region: `#[test]`
    fns inside still matched individually, but a plain helper between them —
    `with_valid_rotation_env` — was scanned as production and its two `unsafe` blocks were
    reported as new violations. That is a false positive in the gate, not debt in the tree
    (#2429). Openers are now decided by `_cfg_is_test_only`, which distinguishes `all(test,
    ..)` from `any(test, ..)` and `not(test)` instead of matching text.
    """
    mask = [False] * len(lines)
    depth = 0
    pending = False       # saw a #[cfg(test)] / #[test] attribute, awaiting its item
    test_depth: int | None = None
    for i, raw in enumerate(lines):
        line = _strip_literals(raw)
        stripped = line.strip()
        # Attribute detection reads the comment-stripped line with literals INTACT, so a
        # `feature = "x"` value survives to be parsed; brace tracking below still uses the
        # fully literal-stripped form.
        if test_depth is None and _opens_test_region(_strip_comment_only(raw).strip()):
            pending = True
        opens, closes = line.count("{"), line.count("}")
        if test_depth is not None:
            mask[i] = True
        elif pending:
            # Only a real item declaration may open the test region. Consuming the next
            # line with ANY brace is wrong: a `use path::{A, B};` import following a stray
            # `#[cfg(test)]` opened a bogus region at depth 0 and masked the rest of the
            # file. Found while porting this mask to the panic-input gate (#2111), where it
            # hid 6503 of 9343 lines in `launch_api/main.rs`.
            if ITEM_RE.match(line) and opens > 0:
                test_depth = depth
                mask[i] = True
                pending = False
            elif stripped and not stripped.startswith("#["):
                pending = False
        depth += opens - closes
        if test_depth is not None and depth <= test_depth:
            test_depth = None
    return mask


def _normalize(text: str) -> str:
    """Whitespace-collapsed source text: reformatting must not churn the baseline."""
    return " ".join(text.split())


def _block_body(lines: list[str], i: int) -> str:
    """Normalized source of the unsafe construct starting at line `i`.

    This is what binds a baseline reason to the *operation* it justifies. Without it the
    fingerprint is only `path|item|opening line|ordinal`, so the body of a grandfathered
    block can be replaced wholesale while the opener, enclosing item and ordinal stay
    identical — and the gate reports `0 NEW`. Demonstrated on this branch: swapping
    `CString::from_raw(s)` for `core::ptr::read_volatile(&0u8)` inside the grandfathered
    FFI block passed a baseline whose reason is specifically about reclaiming an FFI
    CString pointer (review of #2247).

    Stability, and its stated bound: the body is taken through `_strip_literals`, so
    comments and string/char literal *contents* do not churn the digest, and through
    `_normalize`, so reformatting does not either. The consequence is deliberate and worth
    naming — a change confined entirely to a string literal or a comment will not be seen
    as a new site. Safety-relevant operations are not string contents, so this trades a
    negligible blind spot for a baseline that does not churn on every rustfmt run.

    Constructs without a brace body on the opening line (`unsafe fn foo();`,
    `unsafe impl Send for T {}` written across lines, a trailing `unsafe extern "C"`) fall
    back to the opening line alone, so they are no worse identified than before.
    """
    depth = 0
    started = False
    parts: list[str] = []
    for j in range(i, min(i + 400, len(lines))):
        stripped = _strip_literals(lines[j])
        parts.append(stripped)
        for c in stripped:
            if c == "{":
                depth += 1
                started = True
            elif c == "}":
                depth -= 1
        if started and depth <= 0:
            break
    return _normalize(" ".join(parts))


def _enclosing_item(lines: list[str], i: int) -> str:
    """Nearest enclosing `fn`/`impl`/`mod`/`trait` declaration above line `i`.

    This is the part of a site's identity that a remove-and-replace swap cannot preserve:
    a replacement block introduced elsewhere sits under a different declaration, so it is a
    different site even when its source text is byte-identical to the grandfathered one.
    """
    for j in range(i, -1, -1):
        m = ITEM_RE.match(lines[j])
        if m:
            kind, name = m.group(1), _normalize(m.group(2))
            return f"{kind} {name}".strip()
    return "<file scope>"


def scan_file(path: Path, root: Path) -> list[UnsafeSite]:
    rel = str(path.relative_to(root))
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    sites: list[UnsafeSite] = []
    seen: Counter = Counter()
    in_test = _test_mask(lines)
    for i, line in enumerate(lines):
        if line.lstrip().startswith("//"):
            continue
        if not UNSAFE_RE.search(line):
            continue
        if in_test[i]:
            continue
        # Justified if SAFETY is on this line or within the 2 lines above (allowing a blank).
        window = [line] + [lines[i - k] for k in range(1, 3) if i - k >= 0]
        justified = any(SAFETY_RE.search(w) for w in window)
        item = _enclosing_item(lines, i)
        text = _normalize(line)
        body = _block_body(lines, i)
        digest = hashlib.sha256(body.encode("utf-8")).hexdigest()[:12]
        # Ordinal disambiguates a second identical `unsafe` inside the SAME item — the
        # "violation inside an existing baselined unit" probe (#2284).
        key = (item, text)
        ordinal = seen[key]
        seen[key] += 1
        sites.append(UnsafeSite(rel, i + 1, justified, text, item, ordinal, body, digest))
    return sites


def scan(root: Path) -> list[UnsafeSite]:
    out: list[UnsafeSite] = []
    for rel in SCANNED_PATHS:
        base = root / rel
        if not base.exists():
            continue
        for f in sorted(base.rglob("*.rs")):
            out.extend(scan_file(f, root))
    return out


def _fp(s: UnsafeSite) -> str:
    """Stable per-site identity: file, enclosing item, normalized text, ordinal, body.

    The trailing `body:<digest>` is what ties a grandfathering reason to the operation it
    actually justifies. It is a short hash rather than the body itself so the baseline
    stays human-reviewable — the readable prefix still says which file, which function and
    which opening line, and the digest only answers "is this still the same code?".
    """
    return f"{s.path}|{s.item}|{s.text}|{s.ordinal}|body:{s.body_digest}"


def _load_baseline(path: Path) -> dict[str, str]:
    """Load `{fingerprint: justification}`.

    The two earlier forms — a JSON list of shapes and a `{shape: count}` map — are rejected
    outright rather than tolerated. Silently reading an old baseline under new semantics
    would re-open the bypass it was rewritten to close.
    """
    if not (path and path.is_file()):
        return {}
    data = json.loads(path.read_text())
    if isinstance(data, list) or any(not isinstance(v, str) for v in data.values()):
        raise SystemExit(
            f"{path}: legacy baseline format (shape list or count map). The baseline unit is "
            f"now the site, with a justification per entry — regenerate with "
            f"--update-baseline and justify each entry."
        )
    return data


def _unjustified_reasons(baseline: dict[str, str]) -> list[str]:
    """Baseline entries whose justification is missing or does not cite an issue.

    This is what stops `--update-baseline` from laundering a violation introduced in the
    same change: the regenerated entries are placeholders, and placeholders fail.
    """
    return [fp for fp, why in sorted(baseline.items()) if not ISSUE_RE.search(why or "")]


def self_test() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        crate = root / "primitives/src"
        crate.mkdir(parents=True)
        (crate / "a.rs").write_text(
            "pub fn a() { unsafe { core::ptr::null::<u8>(); } }\n", encoding="utf-8"
        )
        (crate / "b.rs").write_text(
            "// SAFETY: null pointer is never dereferenced\n"
            "pub fn b() { unsafe { core::ptr::null::<u8>(); } }\n",
            encoding="utf-8",
        )
        (crate / "c.rs").write_text(
            "#[cfg(test)]\nmod tests { pub fn c() { unsafe { core::ptr::null::<u8>(); } } }\n",
            encoding="utf-8",
        )
        sites = scan(root)
        by = {s.path: s for s in sites}
        assert "primitives/src/a.rs" in by and not by["primitives/src/a.rs"].justified, sites
        assert "primitives/src/b.rs" in by and by["primitives/src/b.rs"].justified, sites
        assert "primitives/src/c.rs" not in by, "test-context unsafe must be exempt"

    # -- cfg-predicate probes (#2429) -------------------------------------------------------
    #
    # The opener used to be a literal `#[cfg(test)]` match, so `#[cfg(all(test,
    # debug_assertions))]` opened no region and plain helpers inside it were scanned as
    # production. Widening the opener is only safe if it widens to *test-only* predicates and
    # no further, so these probes assert BOTH directions: the exempt ones and — more
    # importantly — the ones that must keep failing, because that is where a bypass would be.
    def _cfg_sites(attr: str) -> list[UnsafeSite]:
        """`unsafe` inside a plain (non-`#[test]`) helper under `attr`."""
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "primitives/src").mkdir(parents=True)
            (root / "primitives/src/g.rs").write_text(
                f"{attr}\nmod region {{\n"
                "    fn helper() { unsafe { core::ptr::null::<u8>(); } }\n"
                "}\n",
                encoding="utf-8",
            )
            return scan(root)

    for attr in (
        "#[cfg(test)]",
        "#[cfg(all(test, debug_assertions))]",
        '#[cfg(all(test, feature = "hsm"))]',
        "#[cfg(all(debug_assertions, all(test, unix)))]",
    ):
        assert _cfg_sites(attr) == [], f"test-only cfg must be exempt: {attr}"

    # The bypass directions. Each of these predicates CAN hold in a build without `test`, so
    # the code is reachable in production and must still be scanned. An `any(test, ..)` read
    # as test-only would silently exempt real `unsafe` behind any feature flag — strictly
    # worse than the false positive this change fixes.
    for attr in (
        '#[cfg(any(test, feature = "hsm"))]',
        "#[cfg(not(test))]",
        "#[cfg(all(not(test), unix))]",
        '#[cfg(feature = "test-util")]',
        "#[cfg(any(all(test, unix), windows))]",
    ):
        assert len(_cfg_sites(attr)) == 1, f"non-test-only cfg must still be scanned: {attr}"

    # A commented-out attribute must not open a region: that would exempt the rest of the
    # file. `_strip_comment_only` keeps this closed now that detection no longer runs on the
    # fully literal-stripped line.
    assert len(_cfg_sites("// #[cfg(test)]")) == 1, "a commented cfg must not open a region"

    # -- mutation probes: plant a violation INSIDE an existing baselined unit (#2284) ------
    #
    # Every earlier revision of this gate passed its own tests and still had a bypass,
    # because its tests only asked "does an obviously-new violation fail?". These ask the
    # question that actually distinguishes the units: does a violation that looks, to the
    # baseline, exactly like something already frozen still fail?
    def _sites(src: str) -> list[UnsafeSite]:
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "primitives/src").mkdir(parents=True)
            (root / "primitives/src/m.rs").write_text(src, encoding="utf-8")
            return scan(root)

    def _gate(sites: list[UnsafeSite], baseline: dict[str, str]) -> list[str]:
        """Fingerprints the gate would report as NEW."""
        missing = {_fp(s): s for s in sites if not s.justified}
        return sorted(fp for fp in missing if fp not in baseline)

    grandfathered = "pub fn one() {\n    unsafe { core::ptr::null::<u8>(); }\n}\n"
    base_sites = _sites(grandfathered)
    assert len(base_sites) == 1, base_sites
    frozen = {_fp(base_sites[0]): "grandfathered pending audit (#2106)"}
    assert _gate(base_sites, frozen) == [], "the frozen site alone must pass"

    # PROBE 1 — a second identical `unsafe` in the SAME function. Same file, same enclosing
    # item, byte-identical text: indistinguishable to any shape- or count-per-text baseline.
    same_fn = (
        "pub fn one() {\n"
        "    unsafe { core::ptr::null::<u8>(); }\n"
        "    unsafe { core::ptr::null::<u8>(); }\n"
        "}\n"
    )
    assert _gate(_sites(same_fn), frozen), (
        "PROBE 1 FAILED: a second identical unsafe inside the baselined function was not "
        "reported NEW — the baseline unit is coarser than the violation (#2284)"
    )

    # PROBE 2 — the constant-count replacement swap from the #2247 review: justify the
    # grandfathered block, introduce an unjustified one elsewhere. Total unjustified count is
    # unchanged, so a count baseline sees nothing.
    swapped = (
        "pub fn one() {\n"
        "    // SAFETY: audited, null is never dereferenced\n"
        "    unsafe { core::ptr::null::<u8>(); }\n"
        "}\n"
        "pub fn two() {\n"
        "    unsafe { core::ptr::null::<u8>(); }\n"
        "}\n"
    )
    swapped_sites = _sites(swapped)
    assert sum(1 for s in swapped_sites if not s.justified) == 1, "count is unchanged by design"
    assert _gate(swapped_sites, frozen), (
        "PROBE 2 FAILED: remove-and-replace at constant count was not reported NEW — counts "
        "detect more blocks, not different blocks (#2247 review)"
    )

    # PROBE 3 — reformatting must NOT churn the baseline, or the gate is unusable and gets
    # weakened back. Re-indent and add an unrelated function above.
    reformatted = (
        "pub fn unrelated() {}\n"
        "pub fn one() {\n"
        "        unsafe {  core::ptr::null::<u8>();  }\n"
        "}\n"
    )
    assert _gate(_sites(reformatted), frozen) == [], (
        "PROBE 3 FAILED: reformatting churned the fingerprint"
    )

    # PROBE 3b — the body swap from the #2247 review. The OPENING LINE, enclosing function
    # and ordinal are byte-identical; only the operation inside changes. That is the real
    # ffi.rs shape (`unsafe {` on its own line), and it is why a one-line probe here would
    # be vacuous: a single-line block puts the operation *in* the opening line, so the
    # fingerprint would differ even without binding the body. Verified by removing the body
    # binding and confirming this probe then fails.
    multiline_orig = (
        "pub fn one() {\n"
        "    unsafe {\n"
        "        let _ = core::ptr::null::<u8>();\n"
        "    }\n"
        "}\n"
    )
    multiline_swapped = (
        "pub fn one() {\n"
        "    unsafe {\n"
        "        let _ = core::ptr::read_volatile(&0u8);\n"
        "    }\n"
        "}\n"
    )
    orig_sites = _sites(multiline_orig)
    swapped_body_sites = _sites(multiline_swapped)
    assert len(orig_sites) == 1 and len(swapped_body_sites) == 1
    a, b = orig_sites[0], swapped_body_sites[0]
    # The pre-fix fingerprint components are identical — that is the whole point.
    assert (a.path, a.item, a.text, a.ordinal) == (b.path, b.item, b.text, b.ordinal), (
        "PROBE 3b is set up wrong: the opening line must be identical for this to test "
        "anything about the body"
    )
    frozen_multiline = {_fp(a): "grandfathered pending audit (#2106)"}
    assert _gate(orig_sites, frozen_multiline) == [], "the frozen multiline site must pass"
    assert _gate(swapped_body_sites, frozen_multiline), (
        "PROBE 3b FAILED: replacing the body of a grandfathered unsafe block was not "
        "reported NEW — the fingerprint does not bind the operation the reason justifies "
        "(#2247 review)"
    )

    # PROBE 4 — `--update-baseline` must not be able to launder a violation. Regenerating
    # over the planted violation produces a placeholder entry, and placeholders fail.
    regenerated = {
        _fp(s): frozen.get(_fp(s), BASELINE_PLACEHOLDER)
        for s in _sites(same_fn)
        if not s.justified
    }
    assert _gate(_sites(same_fn), regenerated) == [], "regeneration does silence the NEW check"
    assert _unjustified_reasons(regenerated), (
        "PROBE 4 FAILED: a regenerated baseline had no placeholder, so --update-baseline in "
        "the same change as a violation would still turn the gate green (#2247 review)"
    )

    # PROBE 5 — production code AFTER a test module must still be scanned. The earlier
    # `_in_test` scanned backwards for `#[cfg(test)]` and answered yes if it found one within
    # 400 lines, so everything below a test module was exempt. `ffi.rs` has `mod tests` at
    # line 411 of 529: an unsafe appended below it was invisible to the gate entirely. This
    # is the bypass that made probes 1 and 2 pass for the wrong reason when first run live.
    after_tests = (
        "pub fn prod_before() {}\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    #[test]\n"
        "    fn t() {\n"
        "        unsafe { core::ptr::null::<u8>(); }\n"
        "    }\n"
        "}\n"
        "pub fn prod_after() {\n"
        "    unsafe { core::ptr::null::<u8>(); }\n"
        "}\n"
    )
    found = _sites(after_tests)
    assert len(found) == 1, f"PROBE 5 FAILED: expected exactly the post-test-module site, got {found}"
    assert found[0].item == "fn prod_after", found[0]
    assert not found[0].justified

    # PROBE 6 — a Rust LIFETIME must not be parsed as a char literal. PROBE 5 relies on
    # brace-depth tracking to close the `mod tests` region; if a line inside that region
    # carries a lifetime with a trailing brace (`Formatter<'_>) -> Result {`), the earlier
    # `_strip_literals` consumed the `'` as a string opener and ate the `{`, desyncing the
    # depth and closing the region early — so a production `unsafe` below it was exempted.
    # This is the sibling of the bug the determinism lint hit in launch_api/main.rs (#2245).
    lifetime_close = (
        "pub fn prod_before() {}\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    struct W;\n"
        "    impl std::fmt::Display for W {\n"
        "        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n"
        "            write!(f, \"x\")\n"
        "        }\n"
        "    }\n"
        "}\n"
        "pub fn prod_after_lifetime() {\n"
        "    unsafe { core::ptr::null::<u8>(); }\n"
        "}\n"
    )
    found_lt = _sites(lifetime_close)
    assert len(found_lt) == 1, (
        f"PROBE 6 FAILED: expected exactly the post-test-module site, got {found_lt}. A "
        "lifetime inside the test module was likely parsed as a char literal, eating a "
        "brace and closing the region early."
    )
    assert found_lt[0].item == "fn prod_after_lifetime", found_lt[0]
    # Direct check on the primitive: the lifetime brace survives stripping.
    assert "{" in _strip_literals("fn f() -> MutexGuard<'static, ()> {"), (
        "a lifetime was consumed as a char literal; brace-depth tracking will desync"
    )
    assert "{" not in _strip_literals("let c = '{';"), "a real char literal must be stripped"

    # PROBE 7 — the ratchet (#2247 review, round 92). The bypass a current-tree-only gate
    # cannot see: add a new unjustified `unsafe` AND a baseline entry that cites a real issue,
    # in the same change. The fingerprint is now "in the baseline" and the citation is real, so
    # every check above passes. Only comparing against the base ref exposes it — the frozen
    # site does not exist there.
    def _grown(base_src: str, cur_src: str, baseline: dict[str, str]) -> list[str]:
        """Replicates main()'s ratchet: baseline entries that freeze a current site absent on
        the base ref. Two separate temp roots stand in for `origin/main` and the PR head."""
        with tempfile.TemporaryDirectory() as bd, tempfile.TemporaryDirectory() as cd:
            for root, src in ((Path(bd), base_src), (Path(cd), cur_src)):
                (root / "primitives/src").mkdir(parents=True)
                (root / "primitives/src/m.rs").write_text(src, encoding="utf-8")
            base_fps = {_fp(s) for s in scan(Path(bd))}
            current_fps = {_fp(s) for s in scan(Path(cd))}
            return sorted(fp for fp in baseline if fp in current_fps and fp not in base_fps)

    # Base ref (main): one grandfathered site in `one()`.
    base_tree = "pub fn one() {\n    unsafe { core::ptr::null::<u8>(); }\n}\n"
    base_only_sites = _sites(base_tree)
    frozen_base = {_fp(base_only_sites[0]): "grandfathered pending audit (#2106)"}
    # The clean case: the same tree, same baseline, nothing grew.
    assert _grown(base_tree, base_tree, frozen_base) == [], (
        "PROBE 7 setup wrong: an unchanged tree must not report growth"
    )
    # The laundering case: this change ADDS an unjustified block in `two()` and freezes it in
    # the baseline with a genuine issue citation. new_sites is empty (it is in the baseline) and
    # the citation is valid, so only the ratchet can catch it.
    laundered_tree = (
        "pub fn one() {\n"
        "    unsafe { core::ptr::null::<u8>(); }\n"
        "}\n"
        "pub fn two() {\n"
        "    unsafe { core::ptr::null::<u8>(); }\n"
        "}\n"
    )
    two_site = next(s for s in _sites(laundered_tree) if s.item == "fn two")
    laundered_baseline = dict(frozen_base)
    laundered_baseline[_fp(two_site)] = "grandfathered, see #2106"  # real citation, still a lie
    # It slips past every current-tree check…
    assert _gate(_sites(laundered_tree), laundered_baseline) == [], (
        "the laundered entry must pass the NEW-site check — that is why the ratchet is needed"
    )
    assert not _unjustified_reasons(laundered_baseline), "the laundered entry cites an issue"
    # …and only the ratchet catches it:
    assert _grown(base_tree, laundered_tree, laundered_baseline) == [_fp(two_site)], (
        "PROBE 7 FAILED: a baseline entry freezing a site introduced in this change (absent on "
        "the base ref) was not reported as illegal growth — same-change baseline laundering is "
        "still open (#2247 review, round 92)"
    )

    # And a justification that does not cite an issue is not a justification.
    assert _unjustified_reasons({"x": "because"}), "a reason must cite an issue"
    assert not _unjustified_reasons({"x": "audited in #2106"})

    print("check_unsafe_justified.py self-test passed (8 mutation probes)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--inventory", action="store_true", help="print every unsafe site, exit 0")
    parser.add_argument("--baseline", type=Path, default=None)
    parser.add_argument("--update-baseline", type=Path, default=None)
    parser.add_argument(
        "--base-root",
        type=Path,
        default=None,
        help="a checkout of the PR base ref (e.g. main). Enables the grandfather RATCHET: a "
        "baseline entry is legitimate only if the site it freezes already exists on the base "
        "ref. This closes same-change baseline growth — adding a new `unsafe` and a baseline "
        "entry citing an issue in the SAME change — which no current-tree-only check can see.",
    )
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0

    root = Path(__file__).resolve().parents[1]
    sites = scan(root)
    missing = [s for s in sites if not s.justified]

    if args.update_baseline is not None:
        existing = {}
        if args.update_baseline.is_file():
            try:
                existing = _load_baseline(args.update_baseline)
            except SystemExit:
                existing = {}
        # Keep justifications already written; new sites get a placeholder that FAILS the
        # gate. Regenerating the baseline in the same change as a violation therefore cannot
        # turn it green — someone has to write down why, in the diff.
        ordered = {
            _fp(s): existing.get(_fp(s), BASELINE_PLACEHOLDER) for s in sorted(missing, key=_fp)
        }
        args.update_baseline.write_text(json.dumps(ordered, indent=2) + "\n", encoding="utf-8")
        placeholders = len(_unjustified_reasons(ordered))
        print(f"wrote {len(ordered)} grandfathered unsafe sites to {args.update_baseline}")
        if placeholders:
            print(f"  {placeholders} entr{'y' if placeholders == 1 else 'ies'} need a "
                  f"justification citing an issue — the gate fails until they have one.")
        return 0

    if args.inventory:
        print(f"Unsafe inventory ({len(sites)} sites, {len(missing)} without // SAFETY:):")
        for s in sites:
            tag = "OK  " if s.justified else "MISS"
            print(f"  [{tag}] {s.path}:{s.line_no}  {s.text}")
        return 0

    baseline = _load_baseline(args.baseline) if args.baseline else {}
    by_fp = {_fp(s): s for s in missing}
    new_sites = [s for fp, s in sorted(by_fp.items()) if fp not in baseline]
    placeholders = _unjustified_reasons(baseline)
    stale = sorted(fp for fp in baseline if fp not in by_fp)

    # The RATCHET (#2247 review, round 92). Without a base ref, a baseline entry with a real
    # issue citation is indistinguishable from a legitimate grandfathering: the developer adds
    # a new `unsafe` and, instead of a `// SAFETY:` comment, adds a baseline entry citing an
    # issue in the SAME change — and the gate is green, because the fingerprint is now "in the
    # baseline" and the citation is real. No current-tree-only check can see this: the site
    # looks exactly like debt that predates the policy.
    #
    # The base ref is the discriminator. A baseline entry is legitimate ONLY if the site it
    # freezes already exists on the base ref (it is pre-existing debt). A baseline entry that
    # freezes a site present now but ABSENT on the base ref was introduced in this change and
    # is being laundered. The baseline may shrink freely (debt paid down) but may not grow to
    # cover a site this change created.
    grown: list[str] = []
    if args.base_root is not None:
        current_fps = {_fp(s) for s in sites}
        base_fps = {_fp(s) for s in scan(args.base_root)}
        # Only entries that freeze a CURRENT site (not stale ones already reported below) and
        # that are absent on the base ref: present-now-and-new-here is the laundering shape.
        grown = sorted(fp for fp in baseline if fp in current_fps and fp not in base_fps)

    print(f"[gate] {len(sites)} unsafe sites, {len(missing)} unjustified, "
          f"{len(baseline)} grandfathered, {len(new_sites)} NEW"
          + (f", {len(grown)} GROWN" if args.base_root is not None else ""))

    failed = False
    if new_sites:
        failed = True
        print("FAIL: new `unsafe` without a // SAFETY: comment:")
        for s in new_sites:
            print(f"  {s.path}:{s.line_no}  [{s.item}]  `{s.text}`")
    if grown:
        failed = True
        print("FAIL: the grandfather baseline grew to cover `unsafe` this change introduced.")
        print("  These sites do not exist on the base ref, so freezing them is laundering a new")
        print("  violation, not recording pre-existing debt. Add a // SAFETY: comment instead:")
        for fp in grown:
            print(f"  {fp}")
    if placeholders:
        failed = True
        print("FAIL: grandfathered sites without a justification citing an issue (#NNNN):")
        for fp in placeholders:
            print(f"  {fp}")
        print("  Grandfathering is a decision, not a default — record why, in the diff.")
    if stale:
        # Not a failure: debt paid down is the point. Reported so a shrinking baseline is
        # visible, and so a swap cannot hide as "one left, one arrived" — the arrival is
        # already a NEW site above, independently of this.
        print(f"note: {len(stale)} baselined site(s) no longer present (debt retired):")
        for fp in stale:
            print(f"  {fp}")
    if failed:
        return 1
    print("check_unsafe_justified.py: no new unjustified unsafe")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
