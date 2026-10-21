#!/usr/bin/env python3
"""Flag conversions of secret-bearing TYPES into leak-prone forms (#2128).

A `Zeroize` on a struct whose secret was already copied into a `String`, a
`Vec<u8>` returned by a public function, a `format!`/`Debug` derivation, or a
serde `Serialize` impl is theatre — the copy outlives the zeroization. This
script is the static half of #2128's zeroization audit: it scans Rust sources
for those copies on types whose names look secret-bearing (the `KeyHandle`
family), rooted at the crates that define them.

## Scope, and the boundary with #2239's `check_secret_leak.py`

This is deliberately NOT the general secret-to-output leak gate, and it does
not own the #2259 acceptance test. Two complementary scanners exist:

  - `check_secret_leak.py` (#2239) — a secret VALUE reaching an output sink
    (log / metric / error / panic), and derived `Debug`/`Serialize`/`Clone`
    exposure, across all production roots incl. `native-chain` and the shipped
    CLI. It is the gate for the two #2259 defects (bearer token → node_id log;
    `StealthKeys` derived `Debug`/`Serialize`), and its default scan flags both
    on a pre-fix tree. Bearer-token and `StealthKeys` detection, and the
    `native-chain` root, live THERE.
  - this file (#2238) — a copy of a zeroizing TYPE outliving its own
    zeroization. Its remit is the `KeyHandle`-family lifecycle introduced by
    #2128, rooted where those types are defined (`shared-protocol/src`,
    `privacy/src`).

The split is drawn this way so the same semantic (e.g. "recognize bearer
tokens") is not implemented divergently in two scripts. #2259 coverage is
#2239's; this script must not be widened to duplicate it.

It is intentionally conservative about *what* is a secret type (name-based) and
loud about *what it does* to one, because a false negative here is a leaked key
and a false positive is a one-line allowlist comment.

Exit status:
  0  no findings (or all allowlisted)
  1  findings
  2  usage error

Allowlist a line with a trailing `// secret-copy-ok: <reason>` comment.

Usage:
  scripts/check_secret_copies.py [paths...]      # default: shared-protocol/src privacy/src
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Types whose names indicate they hold secret material. Matched as whole words.
SECRET_TYPE_HINTS = [
    "KeyHandle",
    "PrivateKey",
    "SecretKey",
    "SigningKey",
    "Mnemonic",
    "Seed",
    "Passphrase",
]

# Identifier fragments that indicate a secret-bearing *value* (for the
# format/log heuristics).
SECRET_VALUE_HINTS = [
    "secret",
    "privkey",
    "private_key",
    "signing_key",
    "mnemonic",
    "passphrase",
    "seed",
]

ALLOWLIST = re.compile(r"//\s*secret-copy-ok\b")

# Any call that hands out raw secret bytes. Copying the result into an owned
# value defeats the owner's zeroization: the copy outlives the handle and no
# `ZeroizeOnDrop` covers it.
# Only accessors that are unambiguously secret-bearing BY NAME. `as_bytes` /
# `to_bytes` were tried and removed: they are the ordinary accessor on hashes,
# public keys and curve points, so including them reported public data as
# leaking. Copies off a secret-*named* value are already covered by the
# name-based rules below; this list exists for the case those rules cannot see,
# where the binding is renamed to something innocuous.
SECRET_ACCESSORS = ["expose_secret", "secret_bytes", "expose_secret_bytes"]

# A secret type appearing in a returned-by-public-fn owned position, or being
# fed to a stringify/serialize sink.
SINK_PATTERNS = [
    # `impl Serialize for <SecretType>` — serializing secret material.
    (re.compile(r"impl\s+Serialize\s+for\s+(" + "|".join(SECRET_TYPE_HINTS) + r")\b"),
     "Serialize impl on a secret type"),
    # `#[derive(... Debug ...)]` immediately preceding a secret struct — a
    # derived Debug prints fields.
    # (handled specially below because it needs two-line context)
    # `pub fn ... -> String` / `-> Vec<u8>` in a fn whose name looks secret.
    (re.compile(r"pub\s+fn\s+\w*(" + "|".join(SECRET_VALUE_HINTS) +
                r")\w*\s*\([^)]*\)\s*->\s*(String|Vec<u8>)\b"),
     "public fn returns owned secret bytes/String"),
    # `format!`/`println!`/log macros interpolating a secret-named value.
    (re.compile(r"(format!|write!|writeln!|println!|eprintln!|panic!|"
                r"log::(?:error|warn|info|debug|trace)!|"
                r"tracing::(?:error|warn|info|debug|trace)!)\s*\([^;]*\b(" +
                "|".join(SECRET_VALUE_HINTS) + r")\b"),
     "secret-named value interpolated into a format/log/panic macro"),
    # `.to_string()` / `String::from` / `.to_vec()` on a secret-named value.
    (re.compile(r"\b(" + "|".join(SECRET_VALUE_HINTS) +
                r")\w*\.(to_string|to_vec|clone)\s*\(\)"),
     "owned copy taken of a secret-named value"),
    # An owned copy taken directly off a secret accessor. This is the exact
    # expression the review supplied — `handle.expose_secret().to_vec()` — which
    # the name-based rules above miss entirely, because the binding it lands in
    # can be called anything.
    (re.compile(r"\.(?:" + "|".join(SECRET_ACCESSORS) +
                r")\s*\(\s*\)\s*\.\s*(?:to_vec|to_owned|to_string|clone)\s*\("),
     "owned copy taken directly off a secret accessor"),
]

# Sinks that a *tainted* binding must not reach. Kept separate from the
# name-based rules because taint is tracked by data flow, not by identifier.
TAINT_SINKS = re.compile(
    r"(?:format!|write!|writeln!|print!|println!|eprint!|eprintln!|panic!|"
    r"log::(?:error|warn|info|debug|trace)!|tracing::(?:error|warn|info|debug|trace)!|"
    r"(?<![\w:])(?:error|warn|info|debug|trace)!|"
    r"with_label_values|\.context\(|bail!|anyhow!)"
)

# `let <name> = <expr containing a secret accessor>`
TAINT_BINDING = re.compile(
    r"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?=\s*[^;]*\.(?:"
    + "|".join(SECRET_ACCESSORS)
    + r")\s*\(\s*\)"
)

# A derived Debug/Display on a secret struct: `#[derive(...Debug...)]` on the
# line(s) before `struct <SecretType>`.
DERIVE_DEBUG = re.compile(r"#\[derive\([^)]*\bDebug\b[^)]*\)\]")
SECRET_STRUCT = re.compile(r"\bstruct\s+(" + "|".join(SECRET_TYPE_HINTS) + r")\b")


def scan_file(path: Path) -> list[tuple[int, str, str]]:
    findings: list[tuple[int, str, str]] = []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (UnicodeDecodeError, OSError):
        return findings

    pending_derive_debug = False
    # Bindings that hold secret material regardless of what they are called.
    # The review's case renamed the value to `copy`, which every name-based rule
    # is blind to by construction — so track the data flow instead of the name.
    tainted: set[str] = set()

    for idx, line in enumerate(lines, start=1):
        if ALLOWLIST.search(line):
            pending_derive_debug = False
            continue

        # A new function body resets taint; bindings do not escape it.
        if re.match(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s", line):
            tainted.clear()

        bind = TAINT_BINDING.search(line)
        if bind:
            tainted.add(bind.group(1))

        if tainted and TAINT_SINKS.search(line):
            for name in sorted(tainted):
                if re.search(r"[\{\(,\s]" + re.escape(name) + r"[\}\)\.,:\s]", line):
                    findings.append((
                        idx,
                        line.strip(),
                        f"secret-derived binding `{name}` reaches an output sink",
                    ))
                    break

        # Two-line derive(Debug) + secret struct heuristic.
        if DERIVE_DEBUG.search(line):
            pending_derive_debug = True
        elif pending_derive_debug:
            m = SECRET_STRUCT.search(line)
            if m:
                findings.append(
                    (idx, line.strip(),
                     f"derived Debug on secret struct {m.group(1)} (prints fields)"))
            # A derive attaches to the next item; stop looking after one non-attr line.
            if not line.lstrip().startswith("#["):
                pending_derive_debug = False

        for pattern, why in SINK_PATTERNS:
            if pattern.search(line):
                findings.append((idx, line.strip(), why))

    return findings


def default_roots() -> list[Path]:
    return [Path("shared-protocol/src"), Path("privacy/src")]


def main(argv: list[str]) -> int:
    roots = [Path(a) for a in argv[1:]] or default_roots()
    files: list[Path] = []
    for root in roots:
        if root.is_file() and root.suffix == ".rs":
            files.append(root)
        elif root.is_dir():
            files.extend(sorted(root.rglob("*.rs")))
        else:
            print(f"warning: {root} is not a Rust file or directory", file=sys.stderr)

    total = 0
    for f in files:
        for lineno, text, why in scan_file(f):
            total += 1
            print(f"{f}:{lineno}: {why}\n    {text}")

    if total:
        print(f"\n{total} potential secret-copy finding(s). "
              f"Fix, or allowlist a false positive with a trailing "
              f"`// secret-copy-ok: <reason>` comment.", file=sys.stderr)
        return 1

    print(f"check_secret_copies: no secret-copy patterns found in {len(files)} file(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
