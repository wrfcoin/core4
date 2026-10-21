#!/usr/bin/env python3
"""Regression suite for count_unwraps.py — the unwrap ratchet's counter (#2278).

Two classes, both demanded by the #2278 review, both of which regressed (or
would have) silently without a committed test:

  1. SAME-LINE GROWTH (the #2284 universal probe — a violation inside an
     already-baselined unit): a second real `.unwrap()` added to a line that
     already carries one must raise the count by exactly one, so the gate's
     `actual > baseline` comparison fails. The pre-#2278 line-counting gate
     passed this silently.

  2. NON-PRODUCTION FALSE POSITIVES: an `.unwrap()` in each of the four
     non-production categories — line comment, doc example, string literal,
     `#[cfg(test)]` / `#[test]` code — must NOT move the count. The raw-grep
     counter failed all four (reproduced in the #2278 review: 323 -> 327).

  3. UNDER-COUNTS (the serious direction — #2278 rounds 2 and 3): an
     adversarial literal must never blank a REAL production unwrap near it.
     Two instances were found one prefix at a time — a bounded 8-char
     lookahead missing `r#*"` openers with >= 8 hashes, then the raw
     C-string prefix `cr#*"` not being recognized at all. So this suite
     stops chasing prefixes and pins Rust's COMPLETE string/char opener
     grammar as verified against the pinned rustc 1.98.0 (edition 2021):

         "…"  b"…"  c"…"  r#*"…"#*  br#*"…"#*  cr#*"…"#*  '…'  b'…'

     Every raw form is probed at both shallow and >= 8 hash depth, each
     carrying an embedded quote and `//` — the exact shape that blanks a
     line. Every other prefix permutation (`rb`, `rc`, `bc`, `cb`, `bcr`,
     `cbr`, `rr`, and `c'x'`) is rejected by that same rustc as an unknown
     prefix, so the set above is closed and there is no fourth prefix
     awaiting a fourth review round. Shorter-depth close sequences, escaped
     backslashes, char-literal quotes and nested block comments are pinned
     alongside.

Run: python3 scripts/test_count_unwraps.py   (exit 0 = counter healthy)
The ratchet workflow step runs this BEFORE comparing against the baseline, so
a regression in the counter itself fails the gate rather than mis-counting.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from count_unwraps import count_root  # noqa: E402

PRODUCTION_ONE = """
fn produce(v: Option<u32>) -> u32 {
    v.unwrap()
}
"""

# Identical, plus a SECOND unwrap on the already-counted line.
PRODUCTION_SAME_LINE_TWO = """
fn produce(v: Option<Option<u32>>) -> u32 {
    v.unwrap().unwrap()
}
"""

# One production unwrap, plus one in each non-production category. The
# unwrap-bearing values here are inert text, not real panics.
PRODUCTION_PLUS_NONPRODUCTION = """
fn produce(v: Option<u32>) -> u32 {
    // a comment mentioning value.unwrap() must not count
    /* nor a block comment: value.unwrap() */
    let _s = "string literal with value.unwrap() inside";
    let _r = r#"raw string with value.unwrap() inside"#;
    v.unwrap()
}

/// Doc example — fenced code in doc comments must not count:
/// ```
/// let x = Some(1).unwrap();
/// ```
fn documented() {}

#[cfg(test)]
mod tests {
    #[test]
    fn test_only_unwrap_is_fine() {
        let x: Option<u32> = Some(1);
        assert_eq!(x.unwrap(), 1);
    }
}

#[tokio::test]
async fn free_test_fn() {
    let y: Option<u32> = Some(2);
    y.unwrap();
}
"""


# Class 3 (#2278 rounds 2 and 3): UNDER-counting — the serious direction.
# Each case here contains exactly ONE real production unwrap that must count 1
# no matter what the adjacent literal contains. The adversarial shape is
# always the same: a literal carrying an embedded quote and an embedded `//`.
# If the scanner fails to recognize the opener, the embedded quote ends a
# phantom ordinary string and the `//` blanks the rest of the line, erasing
# the real unwrap that follows.


def _literal_case(literal: str) -> str:
    """One production unwrap, immediately after an adversarial literal."""
    return (
        "fn produce(v: Option<u32>) -> u32 {\n"
        f"    let _s = {literal}; let w = v.unwrap();\n"
        "    w\n"
        "}\n"
    )


def _raw_cases() -> dict[str, str]:
    """Every RAW opener prefix valid in rustc 1.98 / edition 2021, at hash
    depths 0, 2, 8 and 64.

    Depth >= 1 embeds a literal `"` — the payload that fooled the pre-fix
    scanner. Depth 0 cannot (the quote would close the literal), so it
    carries only the `//`. Round 2 missed depth >= 8 for `r`; round 3 missed
    every depth for `cr`. Enumerating prefix x depth closes the family.
    """
    cases: dict[str, str] = {}
    for prefix in ("r", "br", "cr"):
        for depth in (0, 2, 8, 64):
            h = "#" * depth
            body = 'embedded " quote and // slashes' if depth else "only // slashes"
            cases[f"{prefix}_raw_string_{depth}_hash"] = _literal_case(
                f'{prefix}{h}"{body}"{h}'
            )
    return cases


UNDERCOUNT_CASES = {
    **_raw_cases(),
    # Non-raw prefixed forms: the prefix character must not derail the
    # ordinary-string branch that handles the escapes.
    "byte_string_escaped_quote": _literal_case(r'b"escaped \" quote and // slashes"'),
    "c_string_escaped_quote": _literal_case(r'c"escaped \" quote and // slashes"'),
    "byte_char_quote": _literal_case(r"""b'"'"""),
    "raw_string_containing_shorter_close": r'''
fn produce(v: Option<u32>) -> u32 {
    let _s = r##"contains "# a shorter close and // slashes"##; let w = v.unwrap();
    w
}
''',
    "escaped_backslash_ends_string": r'''
fn produce(v: Option<u32>) -> u32 {
    let _s = "\\"; let w = v.unwrap(); // the "\\" string ENDS at the second quote
    w
}
''',
    "char_literal_quote": r'''
fn produce(v: Option<u32>) -> u32 {
    let _c = '"'; let w = v.unwrap();
    w
}
''',
    "nested_block_comment": r'''
fn produce(v: Option<u32>) -> u32 {
    /* outer /* inner value.unwrap() */ still comment */ let w = v.unwrap();
    w
}
''',
}

# MALFORMED INPUT, pinned in the documented direction. None of these compile,
# so they cannot occur in a ratcheted path — but if an unterminated construct
# ever did appear, blanking to end-of-file would silently zero out every
# unwrap after it. The policy says over-count instead: the opener is skipped
# and its body is scanned as ordinary code, so the unwrap below still counts.
MALFORMED_CASES = {
    "unterminated_raw_c_string": r'''
fn produce(v: Option<u32>) -> u32 {
    let _s = cr########"never closed;
    let w = v.unwrap();
    w
}
''',
    "unterminated_ordinary_string": r'''
fn produce(v: Option<u32>) -> u32 {
    let _s = "never closed;
    let w = v.unwrap();
    w
}
''',
    "unterminated_block_comment": r'''
fn produce(v: Option<u32>) -> u32 {
    /* never closed
    let w = v.unwrap();
    w
}
''',
}


def write_root(tmp: Path, name: str, content: str) -> Path:
    root = tmp / name
    root.mkdir()
    (root / "lib.rs").write_text(content)
    return root


def main() -> int:
    failures: list[str] = []
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        one = count_root(write_root(tmp, "one", PRODUCTION_ONE))
        two = count_root(write_root(tmp, "two", PRODUCTION_SAME_LINE_TWO))
        mixed = count_root(write_root(tmp, "mixed", PRODUCTION_PLUS_NONPRODUCTION))

        if one != 1:
            failures.append(f"single production unwrap counted as {one}, want 1")
        if two != one + 1:
            failures.append(
                f"SAME-LINE GROWTH REGRESSION: two unwraps on one line counted as "
                f"{two}, want {one + 1} — a second unwrap on an already-counted "
                f"line would pass the gate (#2284)"
            )
        if mixed != 1:
            failures.append(
                f"NON-PRODUCTION FALSE POSITIVE: file with 1 production unwrap plus "
                f"comment/doc-example/string/test unwraps counted as {mixed}, want 1"
            )

        for name, content in UNDERCOUNT_CASES.items():
            got = count_root(write_root(tmp, name, content))
            if got != 1:
                failures.append(
                    f"UNDER-COUNT REGRESSION [{name}]: real production unwrap "
                    f"adjacent to an adversarial literal counted {got}, want 1 — "
                    f"a blocking ratchet that silently misses production code is "
                    f"worse than no ratchet"
                )

        for name, content in MALFORMED_CASES.items():
            got = count_root(write_root(tmp, name, content))
            if got != 1:
                failures.append(
                    f"MALFORMED-INPUT REGRESSION [{name}]: an unterminated "
                    f"construct swallowed the code after it (counted {got}, "
                    f"want 1) — uncompilable input must over-count, never "
                    f"blank real production code"
                )

    if failures:
        print("count_unwraps self-test FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        return 1
    print(
        "count_unwraps self-test: same-line growth counts (+1); all four "
        "non-production categories (line comment, block comment/doc example, "
        "string literals, cfg(test)/#[test]/#[tokio::test]) are excluded; "
        f"{len(UNDERCOUNT_CASES)} adversarial-literal cases spanning Rust's "
        "complete opener grammar (ordinary/byte/C strings, r|br|cr raw "
        "strings at 0/2/8/64 hashes, char and byte-char literals, nested "
        "block comments) each count their one real production unwrap; and "
        f"{len(MALFORMED_CASES)} uncompilable-input cases over-count rather "
        "than swallow the file. No under-counting."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
