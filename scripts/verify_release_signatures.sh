#!/usr/bin/env bash
#
# Verify that every artifact a release is supposed to produce carries a valid
# detached GPG signature (#2300).
#
# WHY THIS IS NOT A LOOP OVER *.asc
#
# The previous verification step in .github/workflows/release-signing.yml was:
#
#     for sig in *.asc; do
#       base_file="${sig%.asc}"
#       if [ -f "$base_file" ]; then gpg --verify "$sig" "$base_file" || FAILED=true; fi
#     done
#
# With no signatures present, Bash leaves `*.asc` literal, `*` is not a file, the
# body never runs, and the step prints "All signatures verified successfully!" and
# exits 0. A signature gate that reports success when NOTHING is signed is worse
# than no gate, because a release manager reads the green and ships.
#
# `shopt -s nullglob` alone does not fix this. It turns the bogus single iteration
# into zero iterations — still exit 0. The defect is not the glob; it is that the
# loop is driven by what was FOUND rather than by what was EXPECTED. A loop over
# discovered signatures can never notice a missing one.
#
# So this script derives the expected set from what the release actually builds,
# and asserts a signature for each. The expected set mirrors
# release-signing.yml's own "Sign all binaries" and "Create combined checksum
# file" steps:
#
#   * every `wrfcoin-node-*` artifact that is not itself a
#     .sha256/.sha512/.sig/.asc side-file, and
#   * CHECKSUMS.txt
#
# Structure is checked before crypto, deliberately: "no signature exists" and
# "the signature is invalid" are different failures and a release manager needs
# to be told which. It also means the structural half is testable without keys.
#
# Usage:  verify_release_signatures.sh [artifacts-dir]
# Exit:   0 all expected artifacts present and validly signed
#         1 anything missing, orphaned, duplicated or cryptographically invalid

set -uo pipefail

DIR="${1:-artifacts}"
FAILED=0

fail() {
    echo "  ERROR: $*" >&2
    FAILED=1
}

if [ ! -d "$DIR" ]; then
    echo "::error::artifact directory '$DIR' does not exist" >&2
    exit 1
fi

cd "$DIR" || exit 1

echo "Verifying release signatures in $(pwd)"

# --- Expected set -------------------------------------------------------------
# Built from the artifacts themselves, not from the signatures, so a missing
# signature is visible. `nullglob` so a non-matching pattern yields nothing
# rather than the literal pattern.
shopt -s nullglob

EXPECTED=()
for f in wrfcoin-node-*; do
    [ -f "$f" ] || continue
    case "$f" in
        *.sha256 | *.sha512 | *.sig | *.asc) continue ;;
    esac
    EXPECTED+=("$f")
done

# A release with no binaries is not a release. Without this, an empty directory
# would satisfy "every expected artifact is signed" vacuously — the same
# emptiness bug one level up.
if [ ${#EXPECTED[@]} -eq 0 ]; then
    echo "::error::no release binaries (wrfcoin-node-*) found in $DIR — refusing to report success over an empty artifact set" >&2
    exit 1
fi

# The signed checksum file is part of the contract, not optional.
if [ -f CHECKSUMS.txt ]; then
    EXPECTED+=("CHECKSUMS.txt")
else
    fail "CHECKSUMS.txt is missing; the release must publish a signed checksum file"
fi

echo "Expected signed artifacts (${#EXPECTED[@]}):"
for f in "${EXPECTED[@]}"; do echo "    $f"; done

# --- Structural checks --------------------------------------------------------
echo ""
echo "Checking every expected artifact has exactly one signature..."
for f in "${EXPECTED[@]}"; do
    if [ ! -f "$f.asc" ]; then
        fail "$f is NOT SIGNED (expected $f.asc)"
    elif [ ! -s "$f.asc" ]; then
        fail "$f.asc is empty"
    else
        # Exactly ONE detached signature per artifact (#2300 criterion 2). A duplicate or
        # extra signature still passes `gpg --verify`, which succeeds on any one valid
        # signature and never counts them. So "duplicated", which this gate's contract
        # promises to reject, has to be checked structurally, before crypto. The release
        # signs each artifact once with one key.
        #
        # Count actual OpenPGP signature PACKETS, not ASCII-armor headers (#2350). Two
        # valid signature packets can live under a SINGLE `BEGIN PGP SIGNATURE` header
        # (dearmor two detached sigs, concatenate the packets, re-armor as one block);
        # `gpg --verify` accepts that and reports both good, while `grep -c` on the header
        # still returns 1 — so counting headers is fail-open for a duplicated signature.
        # `gpg --list-packets` parses the OpenPGP structure and emits one
        # `:signature packet:` record per signature, catching both the multi-header and
        # the multi-packet-single-header representations.
        sig_packets=$(gpg --list-packets "$f.asc" 2>/dev/null | grep -c ':signature packet:' || true)
        if [ "${sig_packets:-0}" -ne 1 ]; then
            fail "$f.asc contains ${sig_packets:-0} signature packet(s); exactly one is required (duplicate or malformed signature)"
        fi
    fi
done

# Orphaned signatures: an .asc whose base artifact is absent means either the
# artifact was dropped or the signature belongs to something unpublished.
echo "Checking for orphaned or unexpected signatures..."
for sig in *.asc; do
    base="${sig%.asc}"
    if [ ! -f "$base" ]; then
        fail "orphaned signature $sig has no corresponding artifact '$base'"
        continue
    fi
    is_expected=0
    for f in "${EXPECTED[@]}"; do
        [ "$f" = "$base" ] && is_expected=1 && break
    done
    if [ "$is_expected" -eq 0 ]; then
        fail "unexpected signature $sig — '$base' is not in the expected artifact set"
    fi
done

if [ "$FAILED" -ne 0 ]; then
    echo ""
    echo "::error::release artifact set failed structural signature checks — see errors above" >&2
    exit 1
fi

# --- Cryptographic verification ----------------------------------------------
echo ""
echo "Verifying signatures with GPG..."
for f in "${EXPECTED[@]}"; do
    if gpg --verify "$f.asc" "$f" >/dev/null 2>&1; then
        echo "  OK    $f"
    else
        fail "$f.asc is NOT a valid signature for $f"
    fi
done

if [ "$FAILED" -ne 0 ]; then
    echo ""
    echo "::error::one or more signatures failed GPG verification" >&2
    exit 1
fi

echo ""
echo "All ${#EXPECTED[@]} expected artifacts are present and validly signed."
