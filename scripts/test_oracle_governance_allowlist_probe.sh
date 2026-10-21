#!/usr/bin/env bash
# Local disposable proof for the O2b process-environment control and its
# documentation binding. It never contacts a node or Docker daemon.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
probe="$repo_root/scripts/verify-oracle-governance-allowlist.sh"
doc_check="$repo_root/scripts/check_oracle_governance_ceremony_doc.py"
document="$repo_root/docs/security/ORACLE_KEY_CEREMONY.md"
expected="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/proc/4242"
printf 'PATH=/usr/bin\0WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=%s\0' "$expected" > "$tmp/proc/4242/environ"

"$probe" --expected-pubkey "$expected" --pid 4242 --proc-root "$tmp/proc"

# Each condition named by O2b must have its own executable fail-closed proof.
# Mismatch: the same process fixture has a different allowlist.
printf 'WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=%064d\0' 7 > "$tmp/proc/4242/environ"
if "$probe" --expected-pubkey "$expected" --pid 4242 --proc-root "$tmp/proc"; then
  echo "ERROR: mismatched process allowlist unexpectedly passed" >&2
  exit 1
fi

# Missing: the process has no governance allowlist entry at all.
printf 'PATH=/usr/bin\0' > "$tmp/proc/4242/environ"
if "$probe" --expected-pubkey "$expected" --pid 4242 --proc-root "$tmp/proc"; then
  echo "ERROR: missing process allowlist unexpectedly passed" >&2
  exit 1
fi

# Duplicate: an expected entry plus an attacker-controlled second entry is not
# an exact singleton allowlist. This catches a weakened key-count predicate.
printf 'WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=%s\0WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=%064d\0' \
  "$expected" 7 > "$tmp/proc/4242/environ"
if "$probe" --expected-pubkey "$expected" --pid 4242 --proc-root "$tmp/proc"; then
  echo "ERROR: duplicate process allowlist unexpectedly passed" >&2
  exit 1
fi

# Unreadable/absent: without the process environment, no positive deployment
# assertion is possible. The probe must stop rather than infer success.
rm "$tmp/proc/4242/environ"
if "$probe" --expected-pubkey "$expected" --pid 4242 --proc-root "$tmp/proc"; then
  echo "ERROR: absent process environment unexpectedly passed" >&2
  exit 1
fi

# A NUL-safe parser must treat an embedded newline as part of one mismatching
# value, not as a fake record boundary. Rust retains interior newlines here,
# so accepting this would record PASS while every registration is refused.
printf 'WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=%s\nEXTRA=junk\0' "$expected" > "$tmp/proc/4242/environ"
if "$probe" --expected-pubkey "$expected" --pid 4242 --proc-root "$tmp/proc"; then
  echo "ERROR: newline-contaminated process allowlist unexpectedly passed" >&2
  exit 1
fi

# Restore the known-good fixture before documentation controls.
printf 'PATH=/usr/bin\0WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=%s\0' "$expected" > "$tmp/proc/4242/environ"

"$doc_check" --document "$document"

# Semantic RED control: weaken equality to containment in a disposable copy.
mutant="$tmp/ORACLE_KEY_CEREMONY.md"
cp "$document" "$mutant"
sed -z -i 's/exactly[[:space:]]*equal/contains/' "$mutant"
if "$doc_check" --document "$mutant"; then
  echo "ERROR: semantic mutation unexpectedly passed" >&2
  exit 1
fi

# Specificity control: an unrelated O5 wording edit must not make O2b fail.
specificity="$tmp/ORACLE_KEY_CEREMONY-specificity.md"
cp "$document" "$specificity"
sed -i 's/real point of no return/irreversible admission point/' "$specificity"
"$doc_check" --document "$specificity"

echo "PASS: O2b probe and semantic RED control verified"
