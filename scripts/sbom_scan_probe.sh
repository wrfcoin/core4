#!/usr/bin/env bash
# Committed negative control for the SBOM critical-vulnerability gate (#2317).
#
# The #2317 audit found: "a fake grype that printed a critical-vulnerability
# finding and exited 1 was swallowed by the exact workflow command's `|| true`,
# producing overall exit 0" — and the scan step also had `continue-on-error:
# true`. This probe proves the FIXED scan command is fail-closed: a critical
# finding (grype exit 1) fails the gate. It uses a fake `grype` on PATH, so it
# needs no network and no real scanner and runs in well under a second.
#
#   scripts/sbom_scan_probe.sh
#
# Exit 0 = the gate FAILS on a critical finding and PASSES on a clean scan.
# Exit 1 = a critical finding did not fail the gate (the gate is not fail-closed),
#          or the control's own setup is wrong.
#
# Reverting the fix — re-adding `|| true` / `continue-on-error` around the scan —
# turns step 2 below RED, which is the point: this file is the committed proof
# that a critical scanner result cannot be swallowed.

set -uo pipefail

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Fake grype: prints a findings table and exits 1 (exactly as `--fail-on critical`
# does on a critical finding) when FAKE_GRYPE_RC=1, else exits 0.
cat > "$TMP/grype" <<'SH'
#!/usr/bin/env bash
echo "NAME     INSTALLED  VULNERABILITY   SEVERITY"
if [[ "${FAKE_GRYPE_RC:-0}" -ne 0 ]]; then
  echo "openssl  1.0.0      CVE-2999-0001   Critical"
fi
exit "${FAKE_GRYPE_RC:-0}"
SH
chmod +x "$TMP/grype"
export PATH="$TMP:$PATH"

# The EXACT shape of the fixed workflow scan step (.github/workflows/sbom-generate.yml):
#   set -o pipefail
#   grype sbom:... --output table --fail-on critical | tee report.txt
fixed_scan() {
  set -o pipefail
  grype sbom:./sbom.cdx.json --output table --fail-on critical | tee "$TMP/report.txt" >/dev/null
}

# The OLD, swallowing shape the audit found green — kept so the control is
# self-evident: re-introducing `|| true` lets the same critical finding pass.
swallowing_scan() {
  set -o pipefail
  grype sbom:./sbom.cdx.json --output table --fail-on critical | tee "$TMP/report.txt" >/dev/null || true
}

fail() { echo "PROBE FAILED: $*" >&2; exit 1; }

# 1. Positive control: a clean scan (no critical) must PASS.
if ! FAKE_GRYPE_RC=0 fixed_scan; then
  fail "a clean scan (grype exit 0) did not pass"
fi
echo "OK: clean scan passes (exit 0)."

# 2. Negative control: a critical finding (grype exit 1) must FAIL the fixed gate.
if FAKE_GRYPE_RC=1 fixed_scan; then
  fail "a critical finding did NOT fail the gate — the scan is not fail-closed (#2317)"
fi
echo "OK: a critical finding fails the fixed gate (non-zero exit)."

# 3. Demonstrate the exact regression this guards against: the swallowing form
#    lets the same critical finding pass. If the workflow ever regresses to
#    `|| true` / continue-on-error, the gate is decorative again.
if ! FAKE_GRYPE_RC=1 swallowing_scan; then
  fail "the swallowing form unexpectedly failed — the control's demonstration is set up wrong"
fi
echo "OK: confirmed the old '|| true' form would swallow the critical finding (the bug this guards against)."

echo "PROBE PASSED: the SBOM critical-scan gate is fail-closed — a critical finding fails it."
