#!/usr/bin/env bash
# Format specific Rust files WITHOUT the module-tree spill (epic #2102).
#
# `rustfmt path/to/main.rs` follows `mod` declarations and reformats every module
# below them. Formatting one file to fix one hunk has twice reformatted this
# repo's entire `launch_api` tree — 28 unrelated files, then 27 — both caught by
# hand. Detecting that after the fact needs token-level parsing (rustfmt reflows
# statements AND adds trailing commas, so neither `git diff -w` nor a
# whitespace-normalised compare sees it as formatting-only). Preventing it is
# trivial by comparison.
#
# This snapshots which files were already modified, runs rustfmt, then reverts
# every file that became modified but was not in your change set.
#
# Usage: scripts/fmt-file.sh <file.rs> [more.rs ...]

set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "usage: scripts/fmt-file.sh <file.rs> [...]" >&2
  exit 2
fi

# Files already dirty before we touch anything — plus the targets themselves,
# which we obviously intend to change.
before=$(mktemp)
git diff --name-only > "$before"
printf '%s\n' "$@" >> "$before"
sort -u -o "$before" "$before"

rustfmt --edition 2024 "$@" 2>/dev/null || {
  echo "fmt-file: rustfmt failed" >&2
  rm -f "$before"
  exit 1
}

after=$(mktemp)
git diff --name-only | sort -u > "$after"

mapfile -t spilled < <(comm -13 "$before" "$after")
rm -f "$before" "$after"

if [[ ${#spilled[@]} -gt 0 ]]; then
  printf '%s\n' "${spilled[@]}" | xargs -r git checkout --
  echo "fmt-file: formatted $# file(s); reverted ${#spilled[@]} collateral file(s):" >&2
  printf '  %s\n' "${spilled[@]}" >&2
else
  echo "fmt-file: formatted $# file(s); no collateral."
fi
