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

WHAT THESE ARE

SHA-1 digests, as detect-secrets records them, of three TEMPLATED placeholder
webhooks (`T00000000` / `B0000000x` / `XXXX...`). They are not credentials.
They are used as sentinels precisely because they are inert, permanent, and are
the exact findings the verification filter removes.
"""

# The file the sentinels live in. Asserted to exist by the regression test.
SENTINEL_PATH = ".taskmaster/docs/infrastructure-hardening/ids-ips-configuration.md"

SENTINEL_TYPE = "Slack Token"

HASHED_SECRETS = [
    "83ed050863fae46380e1664a3173dbdb0fb68d36",  # pragma: allowlist secret
    "2fa1482a0e3f25ab830f4d7db846c19f777e74e7",  # pragma: allowlist secret
    "3aadf42805a20beba0579130a9afed57a2a98be2",  # pragma: allowlist secret
]
