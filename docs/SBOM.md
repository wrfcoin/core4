# Software Bill of Materials (SBOM) Documentation

## Overview

WRFCoin maintains automated Software Bill of Materials (SBOM) generation for all shipped artifacts. SBOMs provide transparency into the software supply chain by listing all components, dependencies, and their metadata.

## SBOM Formats

We generate SBOMs in **CycloneDX 1.5** format for both ecosystems:

| Ecosystem | Tool | Output File |
|-----------|------|-------------|
| Node.js | `@cyclonedx/cyclonedx-npm` | `sbom/npm-sbom.cdx.json` |
| Rust | `cargo-sbom` | `sbom/rust-sbom.cdx.json` |

## Manual Generation

### Node.js SBOM

```bash
# Install cyclonedx-npm
npm install -g @cyclonedx/cyclonedx-npm

# Generate SBOM
npx @cyclonedx/cyclonedx-npm \
  --ignore-npm-errors \
  --output-format json \
  --output-file sbom/npm-sbom.cdx.json
```

### Rust SBOM

```bash
# Install cargo-sbom
cargo install cargo-sbom

# Generate SBOM (from blockchain-core directory)
cd blockchain-core
cargo sbom --output-format cyclone_dx_json_1_5 > ../sbom/rust-sbom.cdx.json
```

## CI/CD Integration

SBOMs are automatically generated via GitHub Actions:

- **Workflow**: `.github/workflows/sbom-generate.yml`
- **Triggers** (as wired — #2317):
  - **Release-candidate tag push (`v*.*.*`)** — the **pre-publication gate**. The
    `release: published` event fires only *after* a release is public, so it can
    never prevent a bad release. The authoritative gate is the tag: it runs the
    full validate + critical-scan before a GitHub release is cut. **Publish the
    release only once this gate is green.**
  - **Published release** — attaches the SBOMs to the release (re-running the same
    validate + scan first, so a bad SBOM is never attached). This event *attaches*;
    it does not *gate* publication.
  - Weekly schedule (Wed 05:00 UTC)
  - Manual dispatch via Actions UI (with a `scan_vulnerabilities` toggle)

  There is **no branch `push` trigger** — routine dependency changes are caught on
  the weekly schedule; release safety is caught on the tag gate.

### Fail-closed at the publishing boundary

All validation and scanning run **inside `generate-sbom`, before** the SBOMs are
uploaded or attached. Nothing is published until every gate passes:

1. **SBOM Generation**: Creates SBOMs for both npm and Cargo dependencies
2. **Fail-closed inventory validation**: `scripts/validate_sbom.py` rejects a
   missing / empty / non-JSON / wrong-format / empty inventory (and the
   `components`-as-a-string bypass) with a non-zero exit.
3. **Schema Validation (before publish)**: `cyclonedx validate --fail-on-errors`
   runs in `generate-sbom` *before* upload/attach (authoritative; without the flag
   the CLI returns 0 on schema errors).
4. **Critical-scanner negative control**: `scripts/sbom_scan_probe.sh` proves a
   critical scanner result fails the gate (a fake grype exit 1 is not swallowed)
   before the real scans run.
5. **Vulnerability Scanning**: [Grype](https://github.com/anchore/grype) with
   `--fail-on critical` under `pipefail` — a **critical finding fails the job**
   (no `continue-on-error`, no `|| true`).
6. **Publish only after gates pass**: artifact upload (90-day retention) and
   release attachment happen last, so a bad SBOM is never published.
7. **GitHub Security Integration**: SARIF reports uploaded to GitHub Security tab
   (best-effort; SARIF upload does not gate the build).
8. **Redundant re-validation**: `verify-sbom` re-checks the *published* artifact
   (round-trip integrity), independent of the pre-publish gate above.

## Viewing SBOMs

### From GitHub Actions

1. Go to Actions tab
2. Select "SBOM Generation" workflow
3. Download artifacts from completed run

### From Releases

SBOMs are attached to each release as:
- `npm-sbom.cdx.json`
- `rust-sbom.cdx.json`
- `combined-sbom-metadata.json`
- `vulnerability-summary.md`

### Local Files

After generation, SBOMs are available in the `sbom/` directory:

```bash
ls -la sbom/
# npm-sbom.cdx.json      - Node.js dependencies
# rust-sbom.cdx.json     - Rust dependencies
```

## SBOM Contents

### Component Information

Each SBOM contains:
- Package name and version
- Package URL (purl)
- License information
- Dependency relationships
- Hashes (when available)

### Example Entry

```json
{
  "type": "library",
  "name": "express",
  "version": "4.18.2",
  "purl": "pkg:npm/express@4.18.2",
  "licenses": [
    {
      "license": {
        "id": "MIT"
      }
    }
  ]
}
```

## Vulnerability Scanning

### Grype Integration

SBOMs are scanned using Grype against multiple vulnerability databases:
- National Vulnerability Database (NVD)
- GitHub Security Advisories
- OSV (Open Source Vulnerabilities)

### Scan Output

```bash
# Scan npm SBOM
grype sbom:./sbom/npm-sbom.cdx.json --output table

# Scan Rust SBOM
grype sbom:./sbom/rust-sbom.cdx.json --output table

# Fail on critical vulnerabilities
grype sbom:./sbom/npm-sbom.cdx.json --fail-on critical
```

### Severity Levels

| Level | Action |
|-------|--------|
| Critical | Build fails, immediate remediation required |
| High | Review required, remediate within sprint |
| Medium | Track and plan remediation |
| Low | Monitor for updates |

## SBOM Verification

### Validate Format

```bash
# Install CycloneDX CLI
wget -qO- https://github.com/CycloneDX/cyclonedx-cli/releases/download/v0.27.1/cyclonedx-linux-x64 > cyclonedx
chmod +x cyclonedx

# Validate SBOM
./cyclonedx validate --input-file sbom/npm-sbom.cdx.json --input-format json
```

### Verify Component Count

```bash
# Count npm components
jq '.components | length' sbom/npm-sbom.cdx.json

# Count Rust components
jq '.components | length' sbom/rust-sbom.cdx.json
```

## Existing SBOM Data

GitHub automatically generates an SPDX SBOM available at:
- `.taskmaster/research/dependency-security/wrfcoin_core4_4d3db7.json`

This contains 7,870 packages in SPDX 2.3 format with license analysis:
- MIT: 5,470 packages (69%)
- Apache-2.0: 486 packages (6%)
- ISC: 469 packages (6%)
- BSD-3-Clause: 224 packages (3%)
- Unknown: 868 packages (11%)

## Best Practices

1. **Review SBOMs before releases**: Check for new vulnerabilities
2. **Monitor vulnerability reports**: Address critical/high issues promptly
3. **Keep dependencies updated**: Regular dependency updates reduce risk
4. **Archive historical SBOMs**: Maintain SBOMs for compliance audits
5. **Integrate with procurement**: Share SBOMs with customers on request

## Compliance

SBOMs support compliance with:
- Executive Order 14028 (US Government)
- NTIA SBOM Minimum Elements
- EU Cyber Resilience Act (proposed)
- Industry security frameworks (SOC2, ISO 27001)

## Resources

- [CycloneDX Specification](https://cyclonedx.org/specification/overview/)
- [SPDX Specification](https://spdx.dev/specifications/)
- [Grype Documentation](https://github.com/anchore/grype)
- [NTIA SBOM Resources](https://ntia.gov/page/software-bill-materials)
