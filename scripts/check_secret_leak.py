#!/usr/bin/env python3
"""Static pass: no secret material can reach a log line, metric label, error, or panic (#2129).

'No secrets in logs, crash reports or metrics' is a review item, and a review item
only stays true if something enforces it. This is the static half; the runtime
half is `shared-protocol/tests/no_secret_in_output.rs`.

**Metric labels are the commonly missed path.** A label becomes a Prometheus
series — scraped, persisted, often shipped to a hosted dashboard — so it crosses
more trust boundaries, more permanently, than a log line, and is never rotated
away. The call site looks identical to a harmless one.

## What it examines

Sinks, matched across **whole macro invocations, not single lines** — real Rust
wraps arguments over several lines, and a line-based scan silently passes the
multiline form of a leak it rejects on one line:

  - log/tracing macros ......... log::error!, tracing::info!, error!, warn!, ...
  - formatting ................. format!, write!, writeln!, print*!, eprint*!
  - failure paths .............. panic!, unreachable!, todo!, assert*!, .expect()
  - error constructors ......... anyhow!, bail!, ensure!, .context(), .with_context()
  - metric labels .............. with_label_values, const_label*, .label()

Plus a structural check that a secret-bearing struct — brace form (secret-named
fields) or tuple form (secret-named type) — does not derive `Debug`,
`Serialize`, or `Clone`, which no amount of call-site scanning can catch.

## What the model deliberately does NOT cover (#2239 review asked for this
## boundary to be named, not assumed absent)

  - `Deref`/`AsRef<str>`/`Into<String>`/`Borrow` impls that hand out secret
    material as an innocent-looking type the caller then prints. The exposure
    happens at the caller's sink under a non-secret name; a naming-based
    scanner cannot follow the data flow.
  - `#[serde(flatten)]` of a secret-bearing struct into a serialized parent —
    the parent's derive is flagged only if the parent's own field/type names
    look secret.
  - `Hash`/`PartialEq` impls feeding a logged digest or timing side-channel:
    out of scope; a digest of a secret is treated as non-secret here.
  - `impl Display/Debug` blocks as such. Their *formatter method* sinks
    (`.write_str`, `.write_fmt`) and macro sinks (`write!`) ARE matched, so a
    leaking impl body is caught — but an impl that leaks through a helper
    function's return value is not.
  - Function-boundary flow generally: a getter returning `&self.private_key`
    that a distant caller logs. Line- and item-local only, by design — the
    runtime half (`shared-protocol/tests/no_secret_in_output.rs`) exists
    because this static half cannot chase flow.

## Triage model

A finding is real if the *value* of a secret can reach the sink. Verify by
reading the type, not the name. Fix real findings; record verified false
positives — never loosen the patterns to make the report quiet.

  - Inline:   trailing `// secret-leak-ok: <reason>` on the offending line
  - Baseline: `scripts/secret_leak_baseline.txt`, for known findings that are
              filed and out of scope for the current change

Exit status:
  0  no unbaselined findings
  1  findings
  2  usage error

Usage:
  scripts/check_secret_leak.py                 # all production Rust roots
  scripts/check_secret_leak.py <paths...>      # explicit roots
  scripts/check_secret_leak.py --no-baseline   # ignore the baseline (CI fixtures)
  scripts/check_secret_leak.py --write-baseline # regenerate the baseline
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# --------------------------------------------------------------------------
# Secret naming
# --------------------------------------------------------------------------

# Matched against *identifier segments*, not as substrings. Substring matching is
# unusable here: `priv` matches `PrivacyError` and `sk_` matches `task_id`, which
# buried the real findings under prose on the first version of this script.
SECRET_SEGMENTS = {
    "secret", "secrets", "priv", "private", "privkey", "seed", "mnemonic",
    "passphrase", "token", "jwt", "signing", "sk", "keypair", "credential", "pin",
}

IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
CAMEL_SPLIT = re.compile(r"(?<=[a-z0-9])(?=[A-Z])")


def identifier_segments(identifier: str) -> list[str]:
    """Split `myPrivateKey` / `my_private_key` into lowercase segments."""
    segments: list[str] = []
    for part in identifier.split("_"):
        segments.extend(s.lower() for s in CAMEL_SPLIT.split(part) if s)
    return segments


def mentions_secret(text: str) -> bool:
    return any(
        seg in SECRET_SEGMENTS
        for ident in IDENTIFIER.findall(text)
        for seg in identifier_segments(ident)
    )


# --------------------------------------------------------------------------
# Scan scope
# --------------------------------------------------------------------------

# Every production Rust crate root. The previous version defaulted to three
# crates and the CI job passed no paths, so it reported "clean" over a partial
# tree while `native-chain` — which contained a real bearer-token leak — was
# never looked at.
PRODUCTION_ROOTS = [
    "shared-protocol/src", "privacy/src", "p2p/src", "native-chain/src",
    "consensus-engines/src", "execution-engine/src", "evm/src",
    "zk-circuits/src", "primitives/src", "meteo-types/src", "geo-grid/src",
    "forecast-score/src", "dataset-manifest/src", "sensor-attestation/src",
    "reputation-engine/src", "keeper-proof/src", "oracle-packet/src",
    # Data availability (#2186): `choose_indices` takes the node-local
    # sampling secret, so this crate is in scope.
    "da/src",
    # Commit/dirty/timestamp provenance compiled into the shipped CLI binary and
    # shared-protocol's example targets (#2721). A workspace member that was in
    # neither list until the completeness self-test caught it.
    "build-provenance/src",
    # The shipped `wrfcoin` CLI binary. Previously omitted under a blanket
    # "developer CLI tooling" label, but it generates/loads BIP-39 mnemonics,
    # private keys, and operator tokens — which is how a real WrfKeyPair
    # Debug+Serialize derive over a private key went unseen (#2239 review).
    "tools/cli/WrfcoinCLI/src",
]

# Roots deliberately not scanned, with the reason. Printed on every run so a
# partial tree is never silently reported as clean. A root listed here is a
# DECISION; a configured root that is missing on disk is an ERROR and fails
# the run (see main) — a scanner that scans nothing must not report clean.
OMITTED_ROOTS = [
    ("tests/", "test fixtures and mocks — not production signing/logging paths"),
    ("tools/ (except tools/cli/WrfcoinCLI/src)", "RAG/MCP dev tooling; the shipped CLI IS scanned"),
    ("fuzz/", "fuzz harness"),
    ("sdks/", "generated SDK bindings"),
    ("src-tauri/", "desktop shell, excluded from the cargo workspace"),
    ("test_server/", "standalone test fixture server, not shipped"),
    ("security-demo/", "demo binary, not shipped"),
    ("dev-mocks/", "mock services"),
    ("dev-tools/", "developer tooling"),
]


def omitted_prefix(entry: str) -> str:
    """The path prefix an OMITTED_ROOTS entry covers (its first word)."""
    return entry.split(" ", 1)[0]


def workspace_member_src_roots(repo: Path) -> list[str]:
    """`<member>/src` for every cargo workspace member that has a src/ dir.

    Members are read from the root Cargo.toml (globs expanded), so a crate
    added to the workspace is seen here the moment it is added.
    """
    import tomllib

    workspace = tomllib.loads((repo / "Cargo.toml").read_text()).get("workspace", {})
    roots: list[str] = []
    for pattern in workspace.get("members", []):
        for member in sorted(repo.glob(pattern)):
            if (member / "Cargo.toml").is_file() and (member / "src").is_dir():
                roots.append((member / "src").relative_to(repo).as_posix())
    return roots


def unclassified_member_roots(repo: Path) -> list[str]:
    """Workspace `src/` roots that are neither scanned nor deliberately omitted.

    Every member must be a DECISION: in PRODUCTION_ROOTS, or under an
    OMITTED_ROOTS prefix with a reason. A crate in neither list is silently
    unscanned — how `build-provenance/src` (shipped in the CLI binary) went
    unscanned, and how `da/src`, which handles a node secret, would have
    (#3124 infra review).
    """
    prefixes = [omitted_prefix(entry) for entry, _ in OMITTED_ROOTS]
    return [
        root
        for root in workspace_member_src_roots(repo)
        if root not in PRODUCTION_ROOTS
        and not any((root + "/").startswith(prefix) for prefix in prefixes)
    ]

# --------------------------------------------------------------------------
# Sinks
# --------------------------------------------------------------------------

SINKS: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"\blog::(?:error|warn|info|debug|trace)!\s*\("), "log macro"),
    (re.compile(r"\btracing::(?:error|warn|info|debug|trace)!\s*\("), "tracing macro"),
    (re.compile(r"(?<![\w:])(?:error|warn|info|debug|trace)!\s*\("), "log/tracing macro"),
    (re.compile(r"(?<![\w:])(?:format|write|writeln)!\s*\("), "format macro"),
    (re.compile(r"(?<![\w:])(?:print|println|eprint|eprintln)!\s*\("), "stdout/stderr macro"),
    (re.compile(r"(?<![\w:])(?:panic|unreachable|todo|unimplemented)!\s*\("), "panic payload"),
    (re.compile(r"(?<![\w:])assert(?:_eq|_ne)?!\s*\("), "assertion message"),
    # Error constructors. A planted `bail!("...{signing_key}")` exited 0 before
    # these were added — an error message is an output path like any other.
    # Both the bare and path-qualified forms. The `(?<![\w:])` guard that keeps
    # `foo::error!` from matching the log pattern also blocked `anyhow::bail!`,
    # which is exactly the fixture the review planted.
    (re.compile(r"(?<![\w:])(?:anyhow|bail|ensure)!\s*\("), "error constructor"),
    (re.compile(r"\banyhow::(?:anyhow|bail|ensure)!\s*\("), "error constructor"),
    (re.compile(r"\.(?:context|with_context)\s*\("), "error context"),
    # Formatter methods: a custom Display/Debug impl that leaks via
    # `f.write_str(&self.private_key)` never spells `write!`, so the macro
    # patterns above cannot see it (#2239 review). `.write_fmt` is what
    # `write!` desugars to when called as a method. These also match
    # io::Write string writes — an output path all the same.
    (re.compile(r"\.write_str\s*\("), "formatter write"),
    (re.compile(r"\.write_fmt\s*\("), "formatter write"),
    # thiserror's `#[error("...")]` generates a Display impl: a named capture of
    # a secret field reaches every place the error is printed (#3287).
    (re.compile(r"#\[\s*error\s*\("), "error attribute"),
    (re.compile(r"\bwith_label_values\s*\("), "METRIC LABEL"),
    (re.compile(r"\bconst_label\w*\s*\("), "METRIC LABEL"),
    (re.compile(r"\.label\s*\("), "METRIC LABEL"),
]

ALLOWLIST = re.compile(r"//\s*secret-leak-ok\b")


def allowlisted_lines(raw: str, mask: bytearray) -> set[int]:
    """1-based line numbers carrying a `// secret-leak-ok` marker in a COMMENT.

    The marker is an annotation only where the lexer says it is a comment. A
    marker inside a string literal is data — `error!("{private_key} // secret-
    leak-ok")` must not be silenced by its own payload (#3287).
    """
    lines: set[int] = set()
    for m in ALLOWLIST.finditer(raw):
        if mask[m.start()] == COMMENT:
            lines.add(raw.count("\n", 0, m.start()) + 1)
    return lines
STRING_LITERAL = re.compile(r'"(?:[^"\\]|\\.)*"')
INLINE_CAPTURE = re.compile(r"\{(\w+)")

# --------------------------------------------------------------------------
# Lexing (#3284)
# --------------------------------------------------------------------------
#
# Every character of a file is classified once as CODE, LITERAL (string, raw
# string, byte string, char literal) or COMMENT, and every later decision reads
# that classification instead of re-guessing quote state locally.
#
# Why: the sink regexes used to match against text with literals intact, so the
# test string `"...format!(\"failed to deserialize..."` in p2p_events.rs was
# taken for a real `format!(` call. `macro_span` then started its quote tracking
# INSIDE a string, inverted, and never closed the span: it ran to EOF, so no
# later `format!` in that file was examined, and any `// secret-leak-ok` marker
# below it suppressed the whole EOF-length span. A planted leak at line 8000
# was caught; one at 8082 or later was not. Char literals (`'('`) and raw
# strings (`r"C:\"`, which have no escapes) broke the old tracker the same way,
# and `strip_line_comments` reset its string state on every line, so `//`
# inside a multi-line literal cut the line.

CODE, LITERAL, COMMENT = 0, 1, 2
# `r"..."`, `br"..."` and `cr"..."` (C-string, Rust 1.77) are raw: no escapes.
RAW_STRING_START = re.compile(r'[bc]?r(#*)"')


def lex_mask(text: str) -> bytearray:
    """Classify each character of Rust source as CODE, LITERAL or COMMENT."""
    n = len(text)
    mask = bytearray(n)

    def mark(a: int, b: int, kind: int) -> None:
        mask[a:b] = bytes([kind]) * (b - a)

    i = 0
    while i < n:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < n else ""
        if ch == "/" and nxt == "/":
            j = text.find("\n", i)
            j = n if j == -1 else j
            mark(i, j, COMMENT)
            i = j
            continue
        if ch == "/" and nxt == "*":
            depth, j = 0, i
            while j < n:
                if text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                    if depth == 0:
                        break
                else:
                    j += 1
            mark(i, j, COMMENT)
            i = j
            continue
        at_token_start = i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")
        if at_token_start and ch in "rbc":
            m = RAW_STRING_START.match(text, i)
            if m:
                close = '"' + m.group(1)
                j = text.find(close, m.end())
                j = n if j == -1 else j + len(close)
                mark(i, j, LITERAL)
                i = j
                continue
            if ch in "bc" and nxt in "\"'":
                mask[i] = LITERAL
                i += 1
                continue
        if ch == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            j = min(j, n)
            mark(i, j, LITERAL)
            i = j
            continue
        if ch == "'":
            if nxt == "\\":
                j = text.find("'", i + 3)
                j = n if j == -1 else j + 1
                mark(i, j, LITERAL)
                i = j
                continue
            if i + 2 < n and text[i + 2] == "'":
                mark(i, i + 3, LITERAL)
                i += 3
                continue
            # A lifetime or loop label: code.
        i += 1
    return mask


def blank_comments(text: str, mask: bytearray) -> str:
    """Replace comment characters with spaces, keeping offsets and newlines."""
    return "".join(
        " " if mask[i] == COMMENT and ch != "\n" else ch for i, ch in enumerate(text)
    )


def strip_line_comments(text: str) -> str:
    """Remove comments without touching comment-like text inside literals.

    Kept for callers of the old name; offsets are preserved (comments become
    spaces) so line numbers computed from the result match the raw source.
    """
    return blank_comments(text, lex_mask(text))


def interpolated_positions(span: str, span_mask: bytes | None = None) -> str:
    """Return only the parts of `span` that can carry a *value* to a sink.

    A secret is leaked when its **value** is interpolated, not when the word
    appears in prose. `assert!(x, "private key must round-trip")` mentions a
    private key; it does not print one. `debug!("{private_key}")` does.

    With `span_mask` (the lexer classification of `span`), literal boundaries
    are exact, including raw strings and backslash-newline continuations.
    Without it, the older regex approximation is used.
    """
    if span_mask is None:
        captures = [
            m.group(1)
            for lit in STRING_LITERAL.findall(span)
            for m in INLINE_CAPTURE.finditer(lit)
        ]
        outside = STRING_LITERAL.sub('""', span)
        return outside + " " + " ".join(captures)

    outside: list[str] = []
    literals: list[str] = []
    current: list[str] = []
    for ch, kind in zip(span, span_mask):
        if kind == LITERAL:
            current.append(ch)
            continue
        if current:
            literals.append("".join(current))
            current = []
            outside.append('""')
        outside.append(ch)
    if current:
        literals.append("".join(current))
    captures = [m.group(1) for lit in literals for m in INLINE_CAPTURE.finditer(lit)]
    return "".join(outside) + " " + " ".join(captures)


def macro_span(
    text: str, open_paren: int, mask: bytearray | None = None
) -> tuple[str, int, bool]:
    """Return `(span, end, closed)` for the `(...)` argument list at `open_paren`.

    This is what makes the scan multiline-aware. Real Rust wraps macro arguments
    across lines, and the previous line-based scan rejected a one-line leak while
    passing the identical multiline form.

    Only CODE characters count toward balance (#3284). If the list never closes,
    the span is BOUNDED: it stops at the first `}`/`]` that would close a block
    the invocation sits inside (an argument list cannot outlive its enclosing
    block), or at the end of the opening line if the file ends first. It never
    extends to EOF, so one malformed or mis-lexed invocation cannot blind the
    rest of a file. `closed` is False for a bounded span.
    """
    if mask is None:
        mask = lex_mask(text)
    depth = 0
    enclosing = 0
    n = len(text)
    for i in range(open_paren, n):
        if mask[i] != CODE:
            continue
        ch = text[i]
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return text[open_paren : i + 1], i + 1, True
        elif ch in "{[":
            enclosing += 1
        elif ch in "}]":
            enclosing -= 1
            if enclosing < 0:
                return text[open_paren:i], i, False
    line_end = text.find("\n", open_paren)
    line_end = n if line_end == -1 else line_end
    return text[open_paren:line_end], line_end, False


# --------------------------------------------------------------------------
# Structural: derived Debug/Serialize/Clone over secret-bearing structs
# --------------------------------------------------------------------------

DERIVE = re.compile(r"#\[derive\(([^)]*)\)\]")
STRUCT_DECL = re.compile(r"\b(?:pub\s+)?struct\s+(\w+)")
# A field starts a line, or follows `{`, `,` or an attribute's `]` — the last
# three cover single-line bodies like `struct X { seed: [u8; 32] }` (#3284).
# Visibility may be restricted (`pub(crate)`, `pub(in path)`). `:(?!:)` keeps a
# path segment (`std::`) inside a multi-line type from reading as a field.
FIELD = re.compile(
    r"(?:^|[{,\]])\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:r#)?(\w+)\s*:(?!:)", re.M
)

# Clone is in this set deliberately (#2266 review): cloning leaks nothing by
# itself, but every derived copy is another unzeroized secret in ordinary
# memory that something else can later print, serialize, or fail to wipe. A
# genuinely required copy gets a `// secret-leak-ok: <reason>` on the derive.
EXPOSING_DERIVES = {"Debug", "Serialize", "Clone"}


def scan_derived_exposure(
    text: str,
    raw_lines: list[str],
    mask: bytearray | None = None,
    allowed: set[int] | None = None,
) -> list[tuple[int, str, str]]:
    """Flag a struct deriving Debug/Serialize/Clone over secret material.

    No amount of call-site scanning finds this: the leak is the derive itself,
    and it fires at whatever `{:?}` or `to_json` or `.clone()` the caller
    writes later. This is the check that catches `StealthKeys` (#2259).

    Two struct forms (#2266 review — the first version handled only braces,
    so the tuple `VrfSecretKey([u8; 64])` was invisible):
      - brace structs: secret signal = any secret-named field
      - tuple structs: fields are anonymous, so the signal is the struct
        NAME itself (`VrfSecretKey`, `PrivateKey`, ...)

    The inline `// secret-leak-ok` allowlist applies here exactly as at call
    sites, checked on the derive-through-declaration lines of the RAW source
    (the text argument has comments stripped, so the earlier version silently
    ignored the annotation its own failure message recommended).
    """
    # Lexer-aware (#3287): braces and derive/struct tokens count only in CODE,
    # so `#[serde(rename = "}")]` cannot end a body early, and literal contents
    # are blanked before field extraction.
    if mask is None:
        mask = lex_mask(text)
    if allowed is None:
        allowed = allowlisted_lines(text, mask)
    code_only = "".join(
        " " if mask[i] != CODE and ch != "\n" else ch for i, ch in enumerate(text)
    )
    findings: list[tuple[int, str, str]] = []
    for m in DERIVE.finditer(text):
        if mask[m.start()] != CODE:
            continue
        derived = {d.strip() for d in m.group(1).split(",")}
        exposing = derived & EXPOSING_DERIVES
        if not exposing:
            continue
        decl = STRUCT_DECL.search(code_only, m.end())
        if not decl or decl.start() - m.end() > 200:
            continue

        # What follows the struct name decides the form.
        after = code_only[decl.end():decl.end() + 200].lstrip()
        secret_evidence: list[str] = []
        if after.startswith("("):
            # Tuple struct: no field names to inspect — the name is the signal.
            if mentions_secret(decl.group(1)):
                secret_evidence = [f"tuple struct name `{decl.group(1)}`"]
        else:
            body_start = code_only.find("{", decl.end())
            if body_start == -1:
                continue
            # Balanced brace span for the struct body, CODE braces only.
            depth = 0
            end = body_start
            for i in range(body_start, len(code_only)):
                if code_only[i] == "{":
                    depth += 1
                elif code_only[i] == "}":
                    depth -= 1
                    if depth == 0:
                        end = i
                        break
            body = code_only[body_start:end]
            secret_evidence = sorted(
                {f for f in FIELD.findall(body) if mentions_secret(f)}
            )

        if not secret_evidence:
            continue

        derive_line = text[: m.start()].count("\n") + 1
        line_no = text[: decl.start()].count("\n") + 1
        if any(ln in allowed for ln in range(derive_line, line_no + 1)):
            continue

        findings.append((
            line_no,
            f"struct {decl.group(1)} derives {'/'.join(sorted(exposing))} "
            f"over secret material: {', '.join(secret_evidence)}",
            "DERIVED EXPOSURE",
        ))
    return findings


# --------------------------------------------------------------------------
# File scan
# --------------------------------------------------------------------------

def scan_file(path: Path, rel: str) -> list[tuple[int, str, str]]:
    try:
        raw = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        return []

    raw_lines = raw.split("\n")
    mask = lex_mask(raw)
    text = blank_comments(raw, mask)
    allowed = allowlisted_lines(raw, mask)
    findings: list[tuple[int, str, str]] = []

    for pattern, sink in SINKS:
        pos = 0
        while True:
            m = pattern.search(text, pos)
            if not m:
                break
            # Sink-shaped text inside a string or char literal is data, not a
            # call (#3284). Skip it and keep scanning just past it.
            if mask[m.start()] != CODE:
                pos = m.start() + 1
                continue
            open_paren = text.find("(", m.end() - 1)
            if open_paren == -1:
                pos = m.end()
                break
            span, after, closed = macro_span(text, open_paren, mask)
            # A closed span is consumed whole (as before). A bounded one is not:
            # resume right after the sink so nothing it overlapped is skipped.
            pos = max(after, m.end()) if closed else m.end()

            line_no = text[: m.start()].count("\n") + 1
            # Allowlist applies to any line the invocation spans, so a comment on
            # the closing line of a multiline call still works.
            span_end_line = text[:after].count("\n") + 1
            if any(ln in allowed for ln in range(line_no, span_end_line + 1)):
                continue

            span_mask = bytes(mask[open_paren : open_paren + len(span)])
            if mentions_secret(interpolated_positions(span, span_mask)):
                snippet = " ".join(raw_lines[line_no - 1].strip().split())[:100]
                findings.append((line_no, snippet, sink))

    findings.extend(scan_derived_exposure(text, raw_lines, mask, allowed))
    return findings


def load_baseline(path: Path) -> dict[str, int]:
    """Read `path:sink:count` entries.

    Counts, not bare suppression: a *new* leak of the same sink in an
    already-baselined file pushes the count over the recorded number and still
    fails. Plain file+sink suppression would hide it, which would make the
    baseline a way to stop noticing rather than a way to track a backlog.
    """
    if not path.exists():
        return {}
    entries: dict[str, int] = {}
    for line in path.read_text().splitlines():
        line = line.split("#")[0].strip()
        if not line:
            continue
        rel, sink, count = line.rsplit("|", 2)
        entries[f"{rel}|{sink}"] = int(count)
    return entries


BASELINE_PATH = Path("scripts/secret_leak_baseline.txt")


def write_baseline(raw_findings: list[tuple[str, int, str, str]]) -> None:
    counts: dict[str, int] = {}
    for rel, _, _, sink in raw_findings:
        counts[f"{rel}|{sink}"] = counts.get(f"{rel}|{sink}", 0) + 1
    lines = [
        "# Known secret-leak findings, recorded so CI gates on NEW ones (#2129).",
        "# Format: <path>|<sink>|<count>. A finding beyond the recorded count fails.",
        "#",
        "# Every entry is an ACKNOWLEDGED FALSE POSITIVE, not an accepted leak: a NAME",
        "# (type/method/field/env-var-name const), a public value (hash, preimage,",
        "# signature, address, public key), a bool flag, or a test assertion / fixture /",
        "# redaction test — never a secret VALUE reaching a sink. The scanner matches the",
        "# secret-shaped token in these names; the audited code does not leak.",
        "#",
        "# The two #2259 production defects that used to be baselined here are FIXED in",
        "# code and are deliberately NOT entries: StealthKeys now has a hand-written",
        "# redacted Debug (private_spend_key/private_view_key render as <redacted>), and",
        "# the P2P forecast node_id is now blake3::keyed_hash(token) rather than the raw",
        "# bearer token. If either regresses, the scanner will surface a NEW finding.",
        "#",
        "# Regenerate: python3 scripts/check_secret_leak.py --write-baseline",
        "",
    ]
    lines += [f"{k}|{v}" for k, v in sorted(counts.items())]
    BASELINE_PATH.write_text("\n".join(lines) + "\n")


def main(argv: list[str]) -> int:
    flags = {"--no-baseline", "--write-baseline"}
    args = [a for a in argv[1:] if a not in flags]
    use_baseline = "--no-baseline" not in argv[1:]
    writing = "--write-baseline" in argv[1:]

    roots = [Path(a) for a in args] or [Path(r) for r in PRODUCTION_ROOTS]
    explicit = bool(args)

    files: list[tuple[Path, str]] = []
    missing_roots: list[str] = []
    for root in roots:
        if root.is_file() and root.suffix == ".rs":
            files.append((root, str(root)))
        elif root.is_dir():
            for f in sorted(root.rglob("*.rs")):
                files.append((f, str(f)))
        else:
            missing_roots.append(str(root))

    baseline = load_baseline(BASELINE_PATH) if use_baseline and not writing else {}

    raw_findings: list[tuple[str, int, str, str]] = []
    for path, rel in files:
        for lineno, text, sink in scan_file(path, rel):
            raw_findings.append((rel, lineno, text, sink))

    # Group by (file, sink) and allow up to the baselined count.
    seen: dict[str, int] = {}
    findings: list[tuple[str, int, str, str]] = []
    baselined = 0
    for rel, lineno, text, sink in raw_findings:
        key = f"{rel}|{sink}"
        seen[key] = seen.get(key, 0) + 1
        if seen[key] <= baseline.get(key, 0):
            baselined += 1
            continue
        findings.append((rel, lineno, text, sink))

    # Always state the scope. The previous version's failure was reporting a
    # partial tree as clean, so scope is part of the result, not a footnote.
    if not explicit:
        print(f"scope: {len(PRODUCTION_ROOTS)} production Rust roots, {len(files)} file(s)")
        for root, why in OMITTED_ROOTS:
            print(f"  omitted: {root:18s} {why}")
    if missing_roots:
        # Fail-closed (#2239 review): a configured root that is absent means
        # the scan did not cover what it claims to cover. "No findings in 0
        # files" must never exit 0 — that is a gate whose own tests pass
        # while it verifies nothing. Deliberate exclusions belong in
        # OMITTED_ROOTS with a reason, not in silence.
        print(
            f"ERROR: {len(missing_roots)} configured root(s) not found: "
            f"{', '.join(missing_roots)}",
            file=sys.stderr,
        )
        return 2
    if baselined:
        print(f"  {baselined} baselined finding(s) suppressed "
              f"(see scripts/secret_leak_baseline.txt)")

    if writing:
        write_baseline(raw_findings)
        print(f"wrote {BASELINE_PATH} from {len(raw_findings)} finding(s)")
        return 0

    metric = sum(1 for f in findings if f[3] == "METRIC LABEL")
    derived = sum(1 for f in findings if f[3] == "DERIVED EXPOSURE")
    for rel, lineno, text, sink in findings:
        print(f"{rel}:{lineno}: secret reaches a {sink}\n    {text}")

    if findings:
        extra = []
        if metric:
            extra.append(f"{metric} in METRIC LABELS (the commonly missed path)")
        if derived:
            extra.append(f"{derived} DERIVED EXPOSURE")
        suffix = f", {', '.join(extra)}" if extra else ""
        print(
            f"\n{len(findings)} potential secret-leak finding(s){suffix}.\n"
            "Fix, or record a verified false positive with a trailing "
            "`// secret-leak-ok: <reason>` comment, or baseline a filed finding in "
            "scripts/secret_leak_baseline.txt.",
            file=sys.stderr,
        )
        return 1

    print(f"check_secret_leak: no unbaselined secret-to-output patterns in {len(files)} file(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
