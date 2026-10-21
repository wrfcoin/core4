#!/usr/bin/env python3
"""Fixture tests for release/deploy gates and release-claim honesty.

Both defects were found by constructing exactly these cases, so the cases are
committed. Neither test needs a release, a signing secret, or a CI runner:

  #2300 — `scripts/verify_release_signatures.sh`
          An artifact set with NO signatures must fail. The original loop
          iterated over discovered `*.asc` files, so with none present it
          verified nothing and printed "All signatures verified successfully!".

  #2301 — `scripts/deploy_gate_smoke.sh`
          A smoke suite that exits non-zero must fail the gate. The original
          block emitted `result=pass` from both branches and suppressed every
          fallback command with `|| true`.

  #2263 — `.github/workflows/release-signing.yml`
          The release workflow must not publish unsigned SLSA-shaped metadata,
          request unused OIDC, or claim unverified reproducibility.

Run: python3 scripts/test_release_gates.py     (exit 0 = all gates enforce)

GPG note: the signature-validity cases need a working `gpg`. If it is absent,
those cases are SKIPPED and reported as skipped — never silently passed, which
would reproduce the exact bug class these tests exist to prevent.
"""

from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
VERIFY = REPO / "scripts/verify_release_signatures.sh"
SMOKE = REPO / "scripts/deploy_gate_smoke.sh"
RELEASE_WORKFLOW = REPO / ".github/workflows/release-signing.yml"

failures: list[str] = []
skipped: list[str] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    if condition:
        print(f"  PASS  {name}")
    else:
        print(f"  FAIL  {name}{(': ' + detail) if detail else ''}")
        failures.append(f"{name}{(': ' + detail) if detail else ''}")


def run(cmd: list[str], cwd: Path, env: dict | None = None) -> subprocess.CompletedProcess:
    e = dict(os.environ)
    if env:
        e.update(env)
    return subprocess.run(cmd, cwd=cwd, env=e, capture_output=True, text=True)


# ---------------------------------------------------------------------------
# #2263 — release claims must match the implemented assurance
# ---------------------------------------------------------------------------


def release_honesty_violations(text: str) -> list[str]:
    """Return claims/capabilities forbidden until their mechanisms exist.

    `provenance.json` was a blanket-forbidden substring until #2151, because
    every prior mention of it in this file was an aspirational, unsigned
    claim. #2151 made it real: `generate-provenance` (called at the JOB level
    per #312's lesson — never as a step) emits one `*.provenance.json` per
    artifact, and `sign-binaries`' "Sign SHA256SUMS" step GPG-signs a
    checksum file that covers those provenance files' own digests — the same
    key that has always signed the release binaries. So the substring is no
    longer banned outright; it is required to be co-located with real signing
    evidence (`sign sha256sums`, from that step's own name), which keeps this
    check just as strict for the actual failure mode #2263 was written for —
    a provenance mention with NO signing mechanism anywhere in the file — see
    `test_2263`'s negative control, which is built from a fixture snippet
    with no such mechanism rather than from this file's own (now honest)
    text, precisely so it keeps testing the unsigned case rather than being
    defeated by the real signing this file now legitimately contains.
    """
    lowered = text.lower()
    forbidden = {
        "SLSA claim": "slsa",
        "unused OIDC permission": "id-token:",
    }
    violations = [name for name, needle in forbidden.items() if needle in lowered]
    if "provenance.json" in lowered and "sign sha256sums" not in lowered:
        violations.append("unsigned provenance artifact")
    positive_reproducibility_claims = (
        '"reproducible":',
        "reproducible: true",
        "build reproducible",
        "build binary (reproducible)",
    )
    if any(claim in lowered for claim in positive_reproducibility_claims):
        violations.append("unverified reproducibility claim")
    return violations


def test_2263() -> None:
    print("\n#2263 — release-provenance honesty")
    text = RELEASE_WORKFLOW.read_text(encoding="utf-8")
    violations = release_honesty_violations(text)
    check(
        "workflow has no unsigned provenance, SLSA, unused OIDC, or reproducibility claim",
        not violations,
        ", ".join(violations),
    )

    # Negative controls: each misleading capability/claim must be recognized.
    # "unsigned provenance" is checked against a MINIMAL isolated fixture, not
    # `text + probe` like the others: `text` now legitimately contains real
    # signing evidence (#2151), so appending a bare provenance.json mention to
    # it would no longer look unsigned to the (correctly, narrower) check —
    # that is the honest file passing, not the control failing. The isolated
    # fixture below has no signing mechanism at all, so it still pins the
    # actual failure mode #2263 was written for.
    check(
        "negative control detects unsigned provenance",
        bool(release_honesty_violations("path: artifacts/PROVENANCE.json\n")),
    )
    probes = {
        "SLSA": "\npredicateType: https://slsa.dev/provenance/v1\n",
        "unused OIDC": "\nid-token: write\n",
        "reproducibility": '\nreproducible: true\n',
    }
    for name, probe in probes.items():
        check(
            f"negative control detects {name}",
            bool(release_honesty_violations(text + probe)),
        )

    # The downgrade must retain the narrower assurance it documents.
    check(
        "binaries, CHECKSUMS.txt and SHA256SUMS remain detached-signed",
        text.count("--armor --detach-sign") >= 3,
    )
    check(
        "release still publishes signed checksums",
        "artifacts/CHECKSUMS.txt.asc" in text,
    )
    check(
        "release publishes a signed SHA256SUMS (#2151)",
        "artifacts/SHA256SUMS.asc" in text and "--detach-sign SHA256SUMS" in text,
    )
    check(
        "provenance is generated at the JOB level, not as a step (#312)",
        "uses: ./.github/workflows/release-provenance.yml" in text,
    )
    check(
        "the just-published release is re-verified against its provenance, not only the pre-publish CI artifact",
        "gh release download" in text and "scripts/verify_provenance.sh" in text,
    )
    check(
        "release still runs expected-set signature verification",
        "bash scripts/verify_release_signatures.sh artifacts" in text,
    )


def release_node_profile_violations(text: str) -> list[str]:
    """Every official launch_api build must be the networked validator profile."""
    violations: list[str] = []
    build_lines = [
        line.strip()
        for line in text.splitlines()
        if "cargo build" in line and "--bin launch_api" in line
    ]
    if not build_lines:
        violations.append("no official launch_api build commands")
    for line in build_lines:
        if '--features "${RELEASE_NODE_FEATURES}"' not in line:
            violations.append(f"networkless official build: {line}")
    required_contracts = {
        "exact feature constant": "RELEASE_NODE_FEATURES: 'full-mining,p2p'",
        "signed build manifest": ".build-manifest.env",
        "feature provenance": "CARGO_FEATURES=${RELEASE_NODE_FEATURES}",
        "artifact P2P smoke": "scripts/verify-release-p2p-artifact.sh",
    }
    for name, needle in required_contracts.items():
        if needle not in text:
            violations.append(f"missing {name}")
    return violations


def release_checksum_violations(text: str) -> list[str]:
    """Per-binary checksums must verify with only the named binary present."""
    violations: list[str] = []
    binary_sha256_lines = [
        line.strip()
        for line in text.splitlines()
        if '> "${{ matrix.asset_name }}.sha256"' in line
    ]
    if len(binary_sha256_lines) != 2:
        violations.append(
            f"expected Linux and macOS binary SHA256 commands, got {len(binary_sha256_lines)}"
        )
    for line in binary_sha256_lines:
        before_redirect = line.split(">", 1)[0]
        if ".build-manifest.env" in before_redirect:
            violations.append(f"per-binary checksum includes manifest: {line}")

    manifest_sha256_lines = [
        line.strip()
        for line in text.splitlines()
        if '> "${{ matrix.asset_name }}.build-manifest.env.sha256"' in line
    ]
    if len(manifest_sha256_lines) != 2:
        violations.append(
            f"expected separate Linux and macOS manifest SHA256 commands, got {len(manifest_sha256_lines)}"
        )
    return violations


def documented_binary_checksum_fixture() -> tuple[bool, str]:
    """Run the guide's Step 4 after retaining only binary + per-binary SHA256."""
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        binary = root / "wrfcoin-node-linux-x86_64"
        manifest = root / "wrfcoin-node-linux-x86_64.build-manifest.env"
        binary.write_bytes(b"authentic release fixture\n")
        manifest.write_text("FORMAT_VERSION=1\n", encoding="utf-8")
        checksum = subprocess.run(
            ["sha256sum", binary.name],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        )
        (root / f"{binary.name}.sha256").write_text(
            checksum.stdout, encoding="utf-8"
        )
        manifest.unlink()
        verified = subprocess.run(
            ["sha256sum", "-c", f"{binary.name}.sha256"],
            cwd=root,
            capture_output=True,
            text=True,
        )
        return verified.returncode == 0, verified.stdout + verified.stderr


def test_3222() -> None:
    print("\n#3222 — signed node release includes P2P")
    text = RELEASE_WORKFLOW.read_text(encoding="utf-8")
    violations = release_node_profile_violations(text)
    check(
        "official node builds, provenance, and artifact smoke require P2P",
        not violations,
        ", ".join(violations),
    )
    mutated = text.replace(
        '--features "${RELEASE_NODE_FEATURES}"', "", 1
    )
    check(
        "negative control detects a networkless official node build",
        any("networkless official build" in item for item in release_node_profile_violations(mutated)),
    )
    checksum_violations = release_checksum_violations(text)
    check(
        "per-binary checksum is self-contained and manifest is checksummed separately",
        not checksum_violations,
        ", ".join(checksum_violations),
    )
    documented_ok, documented_output = documented_binary_checksum_fixture()
    check(
        "documented binary plus SHA256 verification succeeds without the manifest",
        documented_ok,
        documented_output,
    )
    checksum_mutation = text.replace(
        'sha256sum "${{ matrix.asset_name }}" > "${{ matrix.asset_name }}.sha256"',
        'sha256sum "${{ matrix.asset_name }}" "${{ matrix.asset_name }}.build-manifest.env" > "${{ matrix.asset_name }}.sha256"',
        1,
    )
    check(
        "negative control detects a manifest added to the per-binary checksum",
        any(
            "per-binary checksum includes manifest" in item
            for item in release_checksum_violations(checksum_mutation)
        ),
    )


# ---------------------------------------------------------------------------
# #2300 — release signature verification
# ---------------------------------------------------------------------------


def make_artifacts(d: Path, binaries: list[str], checksums: bool = True) -> Path:
    art = d / "artifacts"
    art.mkdir(parents=True, exist_ok=True)
    for b in binaries:
        (art / b).write_text(f"fake binary {b}\n", encoding="utf-8")
    if checksums:
        (art / "CHECKSUMS.txt").write_text("# checksums\n", encoding="utf-8")
    return art


class Gpg:
    """An ephemeral keyring, so signing tests need no repo or CI secret."""

    def __init__(self, home: Path):
        self.home = home
        self.home.mkdir(parents=True, exist_ok=True)
        self.home.chmod(stat.S_IRWXU)
        self.env = {"GNUPGHOME": str(self.home)}
        self.ok = False

    def setup(self) -> bool:
        if shutil.which("gpg") is None:
            return False
        params = self.home / "params"
        params.write_text(
            "Key-Type: RSA\nKey-Length: 2048\nName-Real: WRFCoin Gate Test\n"
            "Name-Email: gate-test@example.invalid\n%no-protection\n%commit\n",
            encoding="utf-8",
        )
        r = subprocess.run(
            ["gpg", "--batch", "--gen-key", str(params)],
            env={**os.environ, **self.env},
            capture_output=True,
            text=True,
        )
        self.ok = r.returncode == 0
        return self.ok

    def sign(self, path: Path) -> None:
        subprocess.run(
            ["gpg", "--batch", "--yes", "--armor", "--detach-sign", str(path)],
            env={**os.environ, **self.env},
            capture_output=True,
            text=True,
            check=True,
        )

    def two_packets_one_armor(self, asc: Path) -> None:
        """Rewrite `asc` so it holds TWO signature packets under a SINGLE ASCII-armor
        header (#2350). Built by dearmoring the real detached signature, concatenating
        the raw packets, and re-armoring the pair under one `BEGIN PGP SIGNATURE`
        header. `gpg --verify` accepts this and reports both good signatures, but a gate
        that counts armor headers sees a single block — the exact fail-open this fixture
        pins. Distinct from the concat-two-armor-blocks case above, which a header count
        already caught."""
        e = {**os.environ, **self.env}
        raw = subprocess.run(
            ["gpg", "--dearmor"], input=asc.read_bytes(), env=e,
            capture_output=True, check=True,
        ).stdout
        armored = subprocess.run(
            ["gpg", "--enarmor"], input=raw + raw, env=e,
            capture_output=True, check=True,
        ).stdout.decode()
        # `--enarmor` labels the block "PGP ARMORED FILE"; relabel it as a signature so
        # the artifact is a valid-looking `BEGIN PGP SIGNATURE` block.
        armored = armored.replace("ARMORED FILE", "SIGNATURE")
        asc.write_text(armored, encoding="utf-8")


def test_2300() -> None:
    print("\n#2300 — release signature verification")
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)

        # THE DEMONSTRATED CASE: artifacts present, no signatures at all.
        art = make_artifacts(d / "unsigned", ["wrfcoin-node-linux-x86_64"])
        r = run(["bash", str(VERIFY), str(art)], REPO)
        check(
            "unsigned artifact set FAILS",
            r.returncode != 0,
            f"exit {r.returncode}; the original loop exited 0 here",
        )
        check(
            "and says which artifact is unsigned",
            "wrfcoin-node-linux-x86_64" in r.stderr and "NOT SIGNED" in r.stderr,
            r.stderr.strip()[:200],
        )

        # An empty artifact directory must not pass vacuously either.
        empty = (d / "empty" / "artifacts")
        empty.mkdir(parents=True)
        r = run(["bash", str(VERIFY), str(empty)], REPO)
        check("empty artifact set FAILS", r.returncode != 0, f"exit {r.returncode}")

        gpg = Gpg(d / "gnupg")
        if not gpg.setup():
            skipped.append("#2300 gpg-dependent cases (gpg unavailable)")
            print("  SKIP  signature-validity cases — gpg unavailable")
            return

        # Partially signed: one of two binaries signed. A loop over found
        # signatures would verify the one and report success.
        art = make_artifacts(
            d / "partial", ["wrfcoin-node-linux-x86_64", "wrfcoin-node-linux-aarch64"]
        )
        gpg.sign(art / "wrfcoin-node-linux-x86_64")
        gpg.sign(art / "CHECKSUMS.txt")
        r = run(["bash", str(VERIFY), str(art)], REPO, gpg.env)
        check(
            "partially signed set FAILS (the unsigned one is named)",
            r.returncode != 0 and "wrfcoin-node-linux-aarch64" in r.stderr,
            f"exit {r.returncode}: {r.stderr.strip()[:200]}",
        )

        # Unsigned CHECKSUMS.txt.
        art = make_artifacts(d / "nochk", ["wrfcoin-node-linux-x86_64"])
        gpg.sign(art / "wrfcoin-node-linux-x86_64")
        r = run(["bash", str(VERIFY), str(art)], REPO, gpg.env)
        check(
            "unsigned CHECKSUMS.txt FAILS",
            r.returncode != 0 and "CHECKSUMS.txt" in r.stderr,
            f"exit {r.returncode}",
        )

        # Orphaned signature: .asc with no artifact.
        art = make_artifacts(d / "orphan", ["wrfcoin-node-linux-x86_64"])
        gpg.sign(art / "wrfcoin-node-linux-x86_64")
        gpg.sign(art / "CHECKSUMS.txt")
        (art / "wrfcoin-node-linux-aarch64.asc").write_text("-----BEGIN-----\n", encoding="utf-8")
        r = run(["bash", str(VERIFY), str(art)], REPO, gpg.env)
        check("orphaned signature FAILS", r.returncode != 0, f"exit {r.returncode}")

        # Duplicate signature (#2300 criterion 2): two valid detached signatures
        # concatenated into one .asc. `gpg --verify` accepts it — it succeeds on any one
        # valid signature and never counts them — so a gate that trusts gpg's exit code
        # alone lets a duplicated/tampered signature set through. The structural
        # exactly-one-block check must reject it, and name the offending artifact.
        art = make_artifacts(d / "dup", ["wrfcoin-node-linux-x86_64"])
        binary = art / "wrfcoin-node-linux-x86_64"
        gpg.sign(binary)
        gpg.sign(art / "CHECKSUMS.txt")
        one_sig = (art / "wrfcoin-node-linux-x86_64.asc").read_text(encoding="utf-8")
        # Two valid detached signatures for the same artifact, in one .asc.
        (art / "wrfcoin-node-linux-x86_64.asc").write_text(one_sig + one_sig, encoding="utf-8")
        r = run(["bash", str(VERIFY), str(art)], REPO, gpg.env)
        check(
            "duplicate signature FAILS (the artifact is named)",
            r.returncode != 0 and "wrfcoin-node-linux-x86_64.asc" in r.stderr,
            f"exit {r.returncode}: {r.stderr.strip()[:200]}",
        )

        # #2350: two valid signature PACKETS under ONE ASCII-armor header. This is the
        # case the concat-two-armor-blocks fixture above does NOT cover: there is a single
        # `BEGIN PGP SIGNATURE` header, so a gate counting headers passes it, while
        # `gpg --verify` accepts it and reports both good signatures. The gate must count
        # signature packets (`gpg --list-packets`), so this must be REJECTED and name the
        # artifact. Reverting the fix (back to a header count) turns this check RED.
        art = make_artifacts(d / "dup_packets", ["wrfcoin-node-linux-x86_64"])
        binary = art / "wrfcoin-node-linux-x86_64"
        gpg.sign(binary)
        gpg.sign(art / "CHECKSUMS.txt")
        gpg.two_packets_one_armor(art / "wrfcoin-node-linux-x86_64.asc")
        asc_text = (art / "wrfcoin-node-linux-x86_64.asc").read_text(encoding="utf-8")
        check(
            "fixture has exactly one armor header (so a header count would pass it)",
            asc_text.count("BEGIN PGP SIGNATURE") == 1,
            f"headers={asc_text.count('BEGIN PGP SIGNATURE')}",
        )
        r = run(["bash", str(VERIFY), str(art)], REPO, gpg.env)
        check(
            "two signature PACKETS under one armor header FAILS (artifact named) (#2350)",
            r.returncode != 0 and "wrfcoin-node-linux-x86_64.asc" in r.stderr,
            f"exit {r.returncode}: {r.stderr.strip()[:200]}",
        )

        # Present but cryptographically invalid (the one case the old loop DID
        # catch — it must keep working).
        art = make_artifacts(d / "invalid", ["wrfcoin-node-linux-x86_64"])
        (art / "wrfcoin-node-linux-x86_64.asc").write_text("not openpgp data\n", encoding="utf-8")
        (art / "CHECKSUMS.txt.asc").write_text("not openpgp data\n", encoding="utf-8")
        r = run(["bash", str(VERIFY), str(art)], REPO, gpg.env)
        check("invalid signature still FAILS", r.returncode != 0, f"exit {r.returncode}")

        # The happy path must PASS, or the gate is just permanently red and
        # would be disabled by the first person it blocks.
        art = make_artifacts(
            d / "good", ["wrfcoin-node-linux-x86_64", "wrfcoin-node-linux-aarch64"]
        )
        for f in ("wrfcoin-node-linux-x86_64", "wrfcoin-node-linux-aarch64", "CHECKSUMS.txt"):
            gpg.sign(art / f)
        r = run(["bash", str(VERIFY), str(art)], REPO, gpg.env)
        check(
            "fully and validly signed set PASSES",
            r.returncode == 0,
            f"exit {r.returncode}: {r.stderr.strip()[:300]}",
        )


# ---------------------------------------------------------------------------
# #2301 — deployment smoke gate
# ---------------------------------------------------------------------------


def make_project(d: Path, smoke: str | None, npm_exit: int) -> tuple[Path, dict]:
    """A project plus a stub `npm` on PATH that exits with `npm_exit`."""
    proj = d
    proj.mkdir(parents=True, exist_ok=True)
    scripts = {"test:smoke": smoke} if smoke is not None else {}
    (proj / "package.json").write_text(json.dumps({"scripts": scripts}), encoding="utf-8")

    binp = proj / "stubbin"
    binp.mkdir(exist_ok=True)
    npm = binp / "npm"
    npm.write_text(
        f'#!/usr/bin/env bash\necho "stub npm: $*" >&2\nexit {npm_exit}\n', encoding="utf-8"
    )
    npm.chmod(0o755)
    return proj, {"PATH": f"{binp}:{os.environ['PATH']}"}


def test_2301() -> None:
    print("\n#2301 — deployment smoke gate")
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)

        # THE DEMONSTRATED CASE: a smoke suite that fails.
        proj, env = make_project(d / "failing", "exit 42", npm_exit=42)
        out = proj / "gh_output"
        env = {**env, "GITHUB_OUTPUT": str(out)}
        r = run(["bash", str(SMOKE)], proj, env)
        check(
            "failing smoke suite FAILS the gate",
            r.returncode != 0,
            f"exit {r.returncode}; the original block exited 0 here",
        )
        check(
            "and is reported as a failure, not 'script not found'",
            "smoke suite FAILED" in r.stderr and "not found" not in r.stderr,
            r.stderr.strip()[:200],
        )
        check(
            "GITHUB_OUTPUT records result=fail",
            out.is_file() and "result=fail" in out.read_text(),
            out.read_text() if out.is_file() else "(no output file)",
        )

        # Missing smoke script fails closed.
        proj, env = make_project(d / "missing", None, npm_exit=0)
        out = proj / "gh_output"
        r = run(["bash", str(SMOKE)], proj, {**env, "GITHUB_OUTPUT": str(out)})
        check(
            "missing smoke script FAILS CLOSED",
            r.returncode != 0,
            f"exit {r.returncode}; the original block called this pass",
        )
        check(
            "GITHUB_OUTPUT records result=fail",
            out.is_file() and "result=fail" in out.read_text(),
            out.read_text() if out.is_file() else "(no output file)",
        )

        # ...unless explicitly and visibly exempted.
        proj, env = make_project(d / "exempt", None, npm_exit=0)
        r = run(
            ["bash", str(SMOKE)],
            proj,
            {**env, "DEPLOY_GATE_ALLOW_MISSING_SMOKE": "pre-launch service, tracked in #9999"},
        )
        check(
            "an explicit exemption is honoured and echoed",
            r.returncode == 0 and "#9999" in r.stderr,
            f"exit {r.returncode}: {r.stderr.strip()[:200]}",
        )

        # Passing smoke suite passes.
        proj, env = make_project(d / "passing", "exit 0", npm_exit=0)
        out = proj / "gh_output"
        r = run(["bash", str(SMOKE)], proj, {**env, "GITHUB_OUTPUT": str(out)})
        check(
            "passing smoke suite PASSES",
            r.returncode == 0,
            f"exit {r.returncode}: {r.stderr.strip()[:200]}",
        )
        check(
            "GITHUB_OUTPUT records result=pass",
            out.is_file() and "result=pass" in out.read_text(),
            out.read_text() if out.is_file() else "(no output file)",
        )


# ---------------------------------------------------------------------------
# #2301 demonstration 2 — the startup probe
# ---------------------------------------------------------------------------


def extract_step_run(workflow: Path, step_name: str) -> str:
    """Pull one step's `run:` block out of a workflow, so the test exercises the
    workflow's ACTUAL shell rather than a paraphrase of it."""
    import yaml

    doc = yaml.safe_load(workflow.read_text(encoding="utf-8"))
    for job in doc["jobs"].values():
        for step in job.get("steps", []):
            if step.get("name") == step_name:
                return step["run"]
    raise AssertionError(f"step {step_name!r} not found in {workflow}")


def test_2301_startup() -> None:
    print("\n#2301 demonstration 2 — startup probe")
    try:
        import yaml  # noqa: F401
    except ImportError:
        skipped.append("#2301 startup probe (pyyaml unavailable)")
        print("  SKIP  startup probe — pyyaml unavailable")
        return

    block = extract_step_run(REPO / ".github/workflows/deploy-gate.yml", "Start application server")

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        binp = d / "stubbin"
        binp.mkdir()
        # npm start fails, every health probe fails, sleep is instant so the
        # 12x5s readiness loop does not make the test take a minute.
        (binp / "npm").write_text('#!/usr/bin/env bash\necho "stub npm: $*" >&2\nexit 42\n')
        (binp / "curl").write_text('#!/usr/bin/env bash\nexit 7\n')
        (binp / "sleep").write_text("#!/usr/bin/env bash\nexit 0\n")
        for f in ("npm", "curl", "sleep"):
            (binp / f).chmod(0o755)

        out = d / "gh_output"
        out.touch()
        script = d / "startup.sh"
        script.write_text(block, encoding="utf-8")
        r = run(
            ["bash", str(script)],
            d,
            {"PATH": f"{binp}:{os.environ['PATH']}", "GITHUB_OUTPUT": str(out)},
        )
        check(
            "failed server startup FAILS the performance job",
            r.returncode != 0,
            f"exit {r.returncode}; the original block exited 0 and skipped every measurement",
        )
        check(
            "server_ready=false is still recorded",
            "server_ready=false" in out.read_text(),
            out.read_text(),
        )
        check(
            "and it is reported as an error, not a skip",
            "never measured" in r.stderr or "failed to start" in r.stderr,
            r.stderr.strip()[:200],
        )



def main() -> int:
    for script in (VERIFY, SMOKE, RELEASE_WORKFLOW):
        if not script.is_file():
            print(f"missing script: {script}", file=sys.stderr)
            return 1
    test_2263()
    test_3222()
    test_2300()
    test_2301()
    test_2301_startup()

    print()
    if skipped:
        print(f"SKIPPED ({len(skipped)}):")
        for s in skipped:
            print(f"  {s}")
    if failures:
        print(f"FAILED ({len(failures)}):", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        return 1
    print("release-gate fixtures: claims match mechanisms and demonstrated failures are rejected.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
