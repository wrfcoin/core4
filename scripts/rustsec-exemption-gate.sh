#!/usr/bin/env bash
#
# Keep the narrow #2336 RustSec exemptions true.
#
# cargo-audit examines every package retained in Cargo.lock, including optional
# dependencies whose features are not enabled. That is conservative and useful,
# but an exemption based on feature reachability must stop being an exemption
# the moment the relevant feature becomes executable.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

tree_for() {
    RUSTC_WRAPPER= cargo tree \
        --locked \
        --workspace \
        --all-features \
        -e features \
        -i "$1" 2>&1
}

set +e
HICKORY_TREE="$(tree_for hickory-proto@0.25.2)"
HICKORY_STATUS=$?
set -e

if [[ ${HICKORY_STATUS} -ne 0 ]]; then
    echo "::error::could not prove the hickory-proto exemption" >&2
    printf '%s\n' "${HICKORY_TREE}" >&2
    exit 2
fi

if grep -q '^hickory-proto v0\.25\.2$' <<<"${HICKORY_TREE}"; then
    echo "::error::hickory-proto 0.25.2 entered core4's all-features graph" >&2
    printf '%s\n' "${HICKORY_TREE}" >&2
    exit 1
fi

# The tracing-subscriber exemption (RUSTSEC-2025-0055) claims the vulnerable
# formatter features (ansi/fmt) are never compiled. `cargo tree -i` CANNOT
# check that: inverting the tree makes tracing-subscriber the root, so every
# `feature "X"` line in the output belongs to a DEPENDENT — a grep for
# `tracing-subscriber feature "..."` matches nothing and always passes
# (proven vacuous in the #2559 review by adding an arbitrary feature to the
# alternation without the check ever failing). Read the package's RESOLVED
# OWN features from `cargo metadata` instead.
resolved_features() {
    # Prints the resolved feature set (one per line) of the package whose
    # resolve-node id contains "$1". Prints nothing if the package is absent.
    RUSTC_WRAPPER= cargo metadata --locked --all-features --format-version 1 2>/dev/null |
        python3 -c '
import json, sys
needle = sys.argv[1]
meta = json.load(sys.stdin)
for node in meta.get("resolve", {}).get("nodes", []):
    if needle in node["id"]:
        for feature in sorted(node["features"]):
            print(feature)
' "$1"
}

# Self-test: prove the extraction path can see resolved features at all before
# trusting an empty result. Under --all-features the workspace root member
# wrfcoin-native-chain must resolve with its own "p2p" feature enabled; if the
# helper cannot see that, an empty tracing-subscriber result would be the same
# vacuous green this rewrite exists to remove.
if ! resolved_features 'wrfcoin-native-chain' | grep -qx 'p2p'; then
    echo "::error::resolved_features self-test failed — cannot see wrfcoin-native-chain's own 'p2p' feature; refusing to trust an empty feature set" >&2
    exit 2
fi

TRACING_FEATURES="$(resolved_features 'tracing-subscriber@0.2.25')"

if ! RUSTC_WRAPPER= cargo metadata --locked --all-features --format-version 1 2>/dev/null |
    grep -q 'tracing-subscriber@0.2.25'; then
    echo "::error::tracing-subscriber 0.2.25 is gone; remove its stale exemption and guard" >&2
    exit 1
fi

FORBIDDEN_TRACING_FEATURES='^(ansi|fmt)$'
if grep -Eq "${FORBIDDEN_TRACING_FEATURES}" <<<"${TRACING_FEATURES}"; then
    echo "::error::the vulnerable tracing-subscriber 0.2.25 formatter entered the resolved feature set" >&2
    printf 'resolved features:\n%s\n' "${TRACING_FEATURES}" >&2
    exit 1
fi

echo "rustsec-exemptions: feature-reachability assumptions hold"
