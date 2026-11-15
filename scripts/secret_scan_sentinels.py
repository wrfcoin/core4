"""Sentinel findings for the secret-scanning gate (#2292, #2315).

WHY THIS FILE EXISTS

The gate compares `current - acknowledged` and fails on the difference. That
comparison CANNOT detect the defect it was hardened against. If the hermetic
`--no-verify` flag is ever dropped, detect-secrets silently drops the three
templated Slack webhooks from the scan output; they then vanish from `current`
while remaining in the baseline, the one-way difference stays empty, and the
gate exits green while scanning strictly less. Measured with verification
re-enabled: baseline 2457, current 2454, unacknowledged 0.

So the gate also needs a POSITIVE assertion: these hashes must be PRESENT in
the scan output. That assertion lives in `.github/workflows/secret-scanning.yml`
immediately before the unacknowledged comparison, and is pinned by
`scripts/test_secret_scan_hermetic.py`.

WHY A SEPARATE FILE

Both the gate and the regression test need this list. Two copies is how the two
drift, and a drifted sentinel list silently stops sentinelling.

WHY PYTHON RATHER THAN JSON

This started as JSON and the gate immediately caught it: a 40-char hex string
is a `Hex High Entropy String` finding, so the sentinel file added three
unacknowledged findings and failed the very gate it serves. JSON has no comment
syntax, so `pragma: allowlist secret` cannot be attached. Python can carry the
pragmas, and needs no baseline entry.

WHY THE FIXTURE IS GENERATED, NOT COMMITTED

The public tree must not carry a literal Slack-webhook-shaped string: GitHub
push protection rejects it and secret-scanning alerts watchers, templated or
not. So the fixture is assembled from split fragments at run time into a
temporary directory, scanned with the gate's own flags, and discarded. It is
never tracked, so `.secrets.baseline` needs no entry for it and the main scan
never sees it.

WHAT THESE ARE

SHA-1 digests, as detect-secrets records them, of three TEMPLATED placeholder
webhooks (`T00000000` / `B0000000x` / `XXXX...`). They are not credentials.
They are used as sentinels precisely because they are inert, permanent, and are
the exact findings the verification filter removes.
"""

import json
import subprocess
import tempfile
from pathlib import Path

# Description of the original in-tree location (not shipped in the public tree).
SENTINEL_PATH = "<generated at run time by render_fixture()>"

SENTINEL_TYPE = "Slack Token"

HASHED_SECRETS = [
    "83ed050863fae46380e1664a3173dbdb0fb68d36",  # pragma: allowlist secret
    "2fa1482a0e3f25ab830f4d7db846c19f777e74e7",  # pragma: allowlist secret
    "3aadf42805a20beba0579130a9afed57a2a98be2",  # pragma: allowlist secret
]


def render_fixture() -> str:
    """Assemble the three templated webhooks from fragments.

    No fragment matches a webhook pattern on its own, so this file stays inert
    to push protection and to the repository scan.
    """
    host = "https://hooks." + "slack" + ".com/" + "services/"
    team = "T" + "0" * 8
    pad = "X" * 24
    return "".join(
        f"{label}: {host}{team}/B{n:08d}/{pad}\n"
        for label, n in (("a", 0), ("b", 1), ("c", 2))
    )


def scan_generated(flags, generator=render_fixture) -> set:
    """Scan a freshly generated fixture; return the hashed secrets found."""
    with tempfile.TemporaryDirectory() as td:
        (Path(td) / "sentinel-fixture.md").write_text(generator(), encoding="utf-8")
        r = subprocess.run(
            ["detect-secrets", "scan", *flags, "sentinel-fixture.md"],
            capture_output=True,
            text=True,
            cwd=td,
        )
    if r.returncode != 0:
        return set()
    return {
        f["hashed_secret"]
        for fs in json.loads(r.stdout or "{}").get("results", {}).values()
        for f in fs
    }


def absent_sentinels(flags, generator=render_fixture) -> list:
    return sorted(set(HASHED_SECRETS) - scan_generated(flags, generator))
