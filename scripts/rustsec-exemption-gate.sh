#!/usr/bin/env bash
#
# Keep the RustSec ignore list honest (#2336).
#
# The #2336 exemptions were feature-reachability claims about hickory-proto
# 0.25.2 and tracing-subscriber 0.2.25, guarded here by cargo tree / cargo
# metadata checks. The libp2p 0.57 upgrade removed hickory-proto 0.25.2 from
# Cargo.lock, and tracing-subscriber 0.2.25 had already gone, so those
# exemptions and their guards are retired.
#
# With no exemptions left, this gate's job is to keep it that way: a new entry
# in .cargo/audit.toml must arrive together with a reviewed, executable guard
# in this script that proves its reachability claim. Until then, any non-empty
# ignore list fails.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - "${REPO_ROOT}/.cargo/audit.toml" <<'PY_GUARD'
import sys
import tomllib
from pathlib import Path

config = tomllib.loads(Path(sys.argv[1]).read_text())
ignore = config.get("advisories", {}).get("ignore", [])
if ignore != []:
    print(
        "::error::.cargo/audit.toml ignores "
        + ", ".join(map(str, ignore))
        + " without an executable reachability guard in scripts/rustsec-exemption-gate.sh",
        file=sys.stderr,
    )
    sys.exit(1)
print("rustsec-exemptions: no advisory exemptions")
PY_GUARD
