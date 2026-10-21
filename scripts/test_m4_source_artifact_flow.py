#!/usr/bin/env python3
"""Credential-free regression checks for the M4 handoff contract."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = [
    ROOT / "scripts/prepare-m4-source-bundle.sh",
    ROOT / "scripts/build-m4-artifact.sh",
    ROOT / "scripts/sign-m4-artifact.sh",
    ROOT / "scripts/verify-m4-signed-artifact.sh",
]


def main() -> None:
    for script in SCRIPTS:
        subprocess.run(["bash", "-n", str(script)], check=True)
        text = script.read_text()
        assert "set -euo pipefail" in text
        assert "curl |" not in text and "| sh" not in text
    prepare = SCRIPTS[0].read_text()
    assert "status --porcelain=v1 --untracked-files=all" in prepare
    assert " archive --format=tar" in prepare and "SOURCE_ARCHIVE_SHA256" in prepare
    assert "Cargo.lock" in prepare and "rust-toolchain.toml" in prepare
    build = SCRIPTS[1].read_text()
    for token in ("Darwin", "arm64", "--locked", "aarch64-apple-darwin", "1.98.0", "full-mining,p2p", "env -i"):
        assert token in build
    assert "duplicate manifest key" in build
    for token in ("gpg", "--detach-sign", "VALIDSIG", "fingerprint"):
        assert token in SCRIPTS[2].read_text()
    verify = SCRIPTS[3].read_text()
    for token in ("Darwin", "arm64", "lipo", "ARTIFACT_SHA256", "VALIDSIG", "--source-commit", "--artifact-sha256"):
        assert token in verify
    installer = (ROOT / "deploy/macos-sync-peer/install.sh").read_text()
    for token in ("WRFCOIN_BUILD_MANIFEST_PATH", "WRFCOIN_BUILD_MANIFEST_SIGNATURE_PATH", "WRFCOIN_SOURCE_COMMIT", "VERIFY_SCRIPT", "--artifact-sha256"):
        assert token in installer
    package_test = (ROOT / "deploy/macos-sync-peer/test_macos_sync_peer.sh").read_text()
    for token in ("absent signature material rejection", "wrong signer rejection", "altered artifact rejection", "altered manifest rejection"):
        assert token in package_test
    runbook = (ROOT / "docs/security/M4_SOURCE_ARTIFACT_HANDOFF.md").read_text()
    for token in ("Tailscale", "scp", "source commit", "Cargo.lock", "YubiKey", "--verify-only", "healthcheck.sh"):
        assert token in runbook
    print("M4 source/artifact flow static checks passed")


if __name__ == "__main__":
    main()
