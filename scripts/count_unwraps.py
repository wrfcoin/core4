#!/usr/bin/env python3
"""Count production `.unwrap()` occurrences under the given roots (#2269, #2278).

Replaces the raw `grep -ro '\\.unwrap()'` in ci-readiness's unwrap ratchet,
which counted textual matches anywhere — so an `.unwrap()` in a comment, a
string literal, a `///` doc example, or a `#[cfg(test)]` module inflated the
blocking baseline and failed unrelated PRs.

What this is: a pragmatic line/region filter, NOT a Rust parser.

Excluded from the count, in order of processing:
  1. line comments (`//`, `///`, `//!`) — which also removes doc examples,
     since fenced doc code lives on doc-comment lines
  2. block comments (`/* ... */`), with NESTING tracked — `/* /* */ */`
     terminates at the matching close, not the first `*/`
  3. string and char literals — the COMPLETE opener grammar (see
     RAW_STRING_PREFIXES below), not one prefix at a time:

       "..."      ordinary string     b"..."    byte string
       r#*"..."#* raw string          br#*"..."#*  raw byte string
       c"..."     C string            cr#*"..."#*  raw C string
       '...'      char literal        b'...'    byte char literal

     Escaped quotes AND escaped backslashes are handled in the quoted
     forms (`"\\"` closes). Raw openers are parsed unboundedly at ANY
     hash depth, never window-matched, and the closer must reproduce the
     opener's exact hash run, so content containing a shorter depth's
     closing sequence stays inside. Char literals include `'"'`; `'x'`,
     `'\\n'`, `'\\u{..}'` are recognized and lifetimes are left alone.
  4. `#[cfg(test)]` items and `#[test]` / `#[tokio::test]` / `#[rstest]`
     functions: from the attribute, the next top-relative `{` opens the item
     body and the region runs to its balanced close. An attribute followed by
     `;` before any `{` guards a braceless item and excludes nothing.

Counting-direction policy (#2278 round 3): where a construct cannot be
parsed reliably, this scanner must OVER-count (flag production code that
might be excludable) and never UNDER-count (blank real production code).
`#[cfg(all(test, feature = "x"))]` is a known conservative over-count.

Three under-counting defects were found on this PR, one prefix at a time —
line-vs-occurrence granularity, then a bounded 8-char lookahead on the
unbounded `r#*"` opener, then the raw C-string prefix `cr#*"` not being
recognized at all. Rather than await a fourth, the opener grammar above is
now handled as a closed SET, enumerated against the repository-pinned
toolchain rather than from memory; every form is pinned by a regression
test in scripts/test_count_unwraps.py.

Known bounds, stated so the gate's own docs can stay honest (#2278 review):
  * `#[cfg_attr(test, ...)]` and custom test macros other than the three
    listed are NOT recognized.
  * `mod tests;` pointing at a separate file is not followed — but a file
    like that under a ratcheted path would be counted as production, which
    over-counts (fails closed), not under.
  * Macro-generated `.unwrap()` calls invisible in source are not counted;
    source text inside `macro_rules!` bodies is counted as production.
  * An UNTERMINATED literal or block comment is deliberately not treated as
    one: the opener is skipped and its body is scanned as ordinary code.
    Such input is not compilable Rust, so this cannot fire on real sources;
    if it somehow did, the effect is an over-count, never a swallowed file.

Usage:
  scripts/count_unwraps.py <root> [<root> ...]      # prints one total per root
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

UNWRAP = re.compile(r"\.unwrap\(\)")
TEST_ATTR = re.compile(r"#\s*\[\s*(?:cfg\s*\(\s*test\s*\)|test\b|tokio\s*::\s*test\b|rstest\b)")
CHAR_LIT = re.compile(r"'(?:\\u\{[0-9a-fA-F_]{1,6}\}|\\.|[^'\\])'")

# Every prefix that opens a RAW string literal (`<prefix>` `#`* `"`), longest
# first so `br`/`cr` are tried before the bare `r` they contain. This is the
# complete set for the repository-pinned toolchain: each form here was
# compiled with `rustc 1.98.0`, edition 2021, and every other prefix
# permutation (`rb`, `rc`, `bc`, `cb`, `bcr`, `cbr`, `rr`) was rejected by
# that same rustc as `prefix ... is unknown`, so there is no fourth prefix
# to miss. Adding a prefix here — should a future edition stabilize one — is
# a data change, not a control-flow change.
RAW_STRING_PREFIXES = tuple(sorted(("br", "cr", "r"), key=len, reverse=True))

# First characters of the above, used to cheaply gate the scan branch. DERIVED,
# never hand-listed: a hand-maintained second copy is how a prefix gets added
# to one list and missed by the other, which is this ratchet's under-counting
# bug all over again.
RAW_PREFIX_HEADS = frozenset(p[0] for p in RAW_STRING_PREFIXES)

# Non-raw string prefixes `b"` (byte), `c"` (C) and the bare `"` need no
# special opener handling: the prefix character lexes as an identifier-ish
# byte and the quote that follows is picked up by the ordinary-string branch,
# which already implements the shared `\"` / `\\` escape rules. Likewise the
# byte-char prefix in `b'x'`, handled by CHAR_LIT. `c'x'` is NOT valid Rust
# (rustc 1.88 rejects it as an unknown prefix), so there is no C char form.


def raw_string_end(src: str, i: int, n: int) -> int | None:
    """If a raw string literal opens at `i`, return the index just past its
    closing delimiter; otherwise None.

    Both the opener and the closer are parsed unboundedly — no fixed-width
    window — because the hash run has no length limit in the grammar. An
    unterminated literal returns None (see the counting-direction policy):
    swallowing the rest of the file would be the under-counting direction.
    """
    for prefix in RAW_STRING_PREFIXES:
        if not src.startswith(prefix, i):
            continue
        j = i + len(prefix)
        hash_start = j
        while j < n and src[j] == "#":
            j += 1
        if j >= n or src[j] != '"':
            continue  # e.g. the raw identifier `r#foo`, or a plain ident
        # The closer must reproduce the opener's EXACT hash run, so content
        # containing a shorter depth's closing sequence stays inside.
        close = '"' + src[hash_start:j]
        k = src.find(close, j + 1)
        return None if k == -1 else k + len(close)
    return None


def strip_comments_and_strings(src: str) -> str:
    """Blank out comments, strings and char literals, preserving newlines and
    the position of every surviving character."""
    out = list(src)
    i, n = 0, len(src)

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""

        if c == "/" and nxt == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            blank(i, j)
            i = j
        elif c == "/" and nxt == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            if depth:
                i += 1  # unterminated — scan the body rather than swallow it
            else:
                blank(i, j)
                i = j
        elif c in RAW_PREFIX_HEADS and (
            i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")
        ):
            # Raw string opener in any of its three prefixes — `r#*"`,
            # `br#*"`, `cr#*"` — recognized as a SET and parsed with NO
            # bounded lookahead. Two under-counts on this PR came from this
            # one construct: first a fixed `src[i:i+8]` opener window (a
            # >= 8-hash opener was missed), then `cr` not being in the
            # prefix set at all. In both, the opener quote was mistaken for
            # an ordinary string, the literal's embedded quote "ended" it,
            # and an embedded `//` blanked a REAL production unwrap later on
            # the line — the dangerous direction.
            end = raw_string_end(src, i, n)
            if end is None:
                # Plain identifier starting `r`/`b`/`c`, a raw identifier
                # `r#foo`, or the non-raw `b"…"`/`c"…"`/`b'…'` forms — all
                # picked up by the string/char branches on a later iteration.
                i += 1
            else:
                blank(i, end)
                i = end
        elif c == '"' and (i == 0 or src[i - 1] != "\\"):
            j, closed = i + 1, False
            while j < n:
                if src[j] == "\\":
                    j += 2
                elif src[j] == '"':
                    j += 1
                    closed = True
                    break
                else:
                    j += 1
            if closed:
                blank(i, j)
                i = j
            else:
                i += 1  # unterminated — scan the body rather than swallow it
        elif c == "'":
            m = CHAR_LIT.match(src, i)
            if m:
                blank(i, m.end())
                i = m.end()
            else:
                i += 1  # lifetime — leave it
        else:
            i += 1

    return "".join(out)


def test_regions(clean: str) -> list[tuple[int, int]]:
    """Spans of #[cfg(test)]/#[test]-style items in comment/string-free text."""
    regions: list[tuple[int, int]] = []
    for m in TEST_ATTR.finditer(clean):
        # The item body opens at the next `{` unless a `;` (braceless item)
        # arrives first. Later attributes on the same item are inside the
        # region search span and harmless.
        j = m.end()
        while j < len(clean) and clean[j] not in "{;":
            j += 1
        if j >= len(clean) or clean[j] == ";":
            continue
        depth = 0
        k = j
        while k < len(clean):
            if clean[k] == "{":
                depth += 1
            elif clean[k] == "}":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        regions.append((m.start(), k + 1))
    return regions


def count_file(path: Path) -> int:
    try:
        src = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        return 0
    clean = strip_comments_and_strings(src)
    regions = test_regions(clean)
    total = 0
    for m in UNWRAP.finditer(clean):
        if not any(a <= m.start() < b for a, b in regions):
            total += 1
    return total


def count_root(root: Path) -> int:
    return sum(count_file(p) for p in sorted(root.rglob("*.rs")))


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    for arg in argv[1:]:
        print(count_root(Path(arg)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
