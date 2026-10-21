#!/usr/bin/env python3
"""Prove a release binary carries none of the dev endpoints (#2139).

Debug, trace and profiling routes must not *exist* in a release build — not be turned
off by a flag that is one misconfiguration away from on. `routes/mod.rs` registers them
inside `#[cfg(any(test, feature = "dev-endpoints"))]`, so the handler is compiled out of
every binary that does not opt in. This script is the proof that the opt-in never
happened for what ships:

    python3 scripts/check_release_endpoints.py                       # doc + build profiles
    python3 scripts/check_release_endpoints.py --build-profiles      # no profile names the feature
    python3 scripts/check_release_endpoints.py --binary target/release/launch_api
    python3 scripts/check_release_endpoints.py --binary <dev build> --expect-present
    python3 scripts/check_release_endpoints.py --self-test

HOW THE BINARY IS ENUMERATED
----------------------------
There is no route-introspection command in `launch_api` (adding one means touching the
boot path, which #2139 must not). Instead the scan reads the route *path literals*: an
Actix registration `.route("/debug/seed-account", …)` keeps that exact string in the
binary's read-only data, and a compiled-out registration leaves nothing behind. The set
of literals comes from `docs/security/RPC_SURFACE.md` — every row whose handler is
annotated `(feature: dev-endpoints)` — so the document is the single place a route is
classified, and this check follows it. `launch_api` also embeds that whole document via
`include_str!`; the scanner masks the exact contiguous document bytes before looking for
route literals so documentation cannot masquerade as a registered route.

Two things stop the scan from passing vacuously on the wrong file: the binary must be at
least `MIN_BINARY_BYTES`, and it must contain `CONTROL_ROUTE`, a path every release
registers. A cross-compiled artifact is scanned exactly the same way (bytes, not run).

FAIL CLOSED
-----------
* a dev-endpoint row classified as anything but `disabled-in-release` is an error;
* a documented route under a dev prefix (`/debug/`, `/trace/`, `/profile/`, `/pprof`,
  `/heap`) that lacks the annotation is an error — the classification cannot be skipped
  by forgetting it;
* zero annotated rows is an error (an empty proof is no proof);
* any build profile (`Dockerfile*`, `release-signing.yml`, `build-release.sh`) naming the
  feature on a `--features` list is an error, as is a `default = [...]` that includes it.

TEST-ONLY FEATURES THAT MUST NEVER SHIP
---------------------------------------
The same profile check covers every name in `SHIP_FORBIDDEN_FEATURES`, currently
`dev-endpoints` and `panic-drill` (#3224: a release binary that panics its inbound P2P
consumer on demand, used only to prove the crash-and-restart policy on a built artifact).
The binary scan also refuses a release binary that contains the drill's trigger literal.
Build files that live in other repositories — wrfcoin/infra's `infra/docker/Dockerfile.core`
builds the node image that actually ships — are checked with `--profile-file PATH`
(repeatable), which applies the same rule to that file.
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SURFACE_DOC = Path("docs/security/RPC_SURFACE.md")
CARGO_MANIFEST = Path("native-chain/Cargo.toml")
DEV_FEATURE = "dev-endpoints"
FEATURE_MARK = f"(feature: {DEV_FEATURE})"
# #3224: test-only trigger that panics the inbound P2P consumer of a release binary.
PANIC_DRILL_FEATURE = "panic-drill"
# The drill's env-var name exists in the binary only when the feature was compiled in.
PANIC_DRILL_MARKER = "WRFCOIN_PANIC_DRILL_P2P_CONSUMER_TRIGGER"
# Declared in native-chain/Cargo.toml, enabled by no build profile and no `default`.
SHIP_FORBIDDEN_FEATURES = (DEV_FEATURE, PANIC_DRILL_FEATURE)
DEV_PREFIXES = ("/debug/", "/trace/", "/profile/", "/profiling/", "/pprof", "/heap")
# Files whose `--features` lists decide what ships. The feature must appear in none.
BUILD_PROFILES = (
    Path("native-chain/Dockerfile"),
    Path("native-chain/Dockerfile.producer-key-pop"),
    Path(".github/workflows/release-signing.yml"),
    Path("scripts/build-release.sh"),
)
# Registered by every release build; proves the scan is reading real route literals.
CONTROL_ROUTE = "/readyz"
MIN_BINARY_BYTES = 1 << 20

DOC_ROW = re.compile(r"^\| `([^`]+)` \| `([^`]+)` \| `([^`]+)` \| `([^`]+)` \|")
FEATURES_FLAG = re.compile(r"--features[=\s]+[\"']?([^\"'\\\n]+)")


def dev_routes(doc_text: str) -> tuple[list[str], list[str]]:
    """Route paths the surface document marks as dev endpoints, plus doc errors."""
    paths: list[str] = []
    errors: list[str] = []
    for number, line in enumerate(doc_text.splitlines(), start=1):
        match = DOC_ROW.match(line)
        if match is None:
            continue
        method, _transport, handler, classification = match.groups()
        parts = method.split(" ", 1)
        path = parts[1] if len(parts) == 2 else method
        annotated = FEATURE_MARK in handler
        under_dev_prefix = any(path.startswith(prefix) for prefix in DEV_PREFIXES)
        if annotated and classification != "disabled-in-release":
            errors.append(
                f"{SURFACE_DOC}:{number}: {method} is {FEATURE_MARK} but classified "
                f"`{classification}`, must be `disabled-in-release`"
            )
        if under_dev_prefix and not annotated:
            errors.append(
                f"{SURFACE_DOC}:{number}: {method} is under a dev prefix but not annotated "
                f"{FEATURE_MARK} — classify it or move it"
            )
        if annotated and path not in paths:
            paths.append(path)
    if not paths:
        errors.append(f"{SURFACE_DOC}: no row is annotated {FEATURE_MARK}; an empty proof is no proof")
    return paths, errors


def scan_binary(blob: bytes, paths: list[str], doc_blob: bytes, expect_present: bool) -> list[str]:
    errors: list[str] = []
    if len(blob) < MIN_BINARY_BYTES:
        errors.append(
            f"binary is {len(blob)} bytes, below {MIN_BINARY_BYTES}: not a launch_api build"
        )
    if not doc_blob:
        errors.append(f"{SURFACE_DOC} is empty: cannot mask its embedded binary bytes")
        scan_blob = blob
    elif doc_blob not in blob:
        errors.append(
            f"embedded {SURFACE_DOC} not found: not a launch_api binary, or its include_str! "
            "contract changed — refusing to scan unmasked documentation"
        )
        scan_blob = blob
    else:
        # Mask every exact copy. Both the dev paths and CONTROL_ROUTE occur in the
        # document, so neither may count as binary evidence after include_str!.
        scan_blob = blob.replace(doc_blob, b"\0" * len(doc_blob))
    if CONTROL_ROUTE.encode() not in scan_blob:
        errors.append(
            f"control route {CONTROL_ROUTE!r} not found: not a launch_api binary, or the "
            "scan is blind — refusing to certify absence"
        )
    if errors:
        return errors
    if PANIC_DRILL_MARKER.encode() in scan_blob:
        errors.append(
            f"`{PANIC_DRILL_FEATURE}` trigger {PANIC_DRILL_MARKER!r} is present: this binary "
            "was built with the test-only panic drill and must not ship"
        )
    present = [path for path in paths if path.encode() in scan_blob]
    if expect_present:
        for path in paths:
            if path not in present:
                errors.append(f"expected dev endpoint {path} in a {DEV_FEATURE} build, not found")
    else:
        for path in present:
            errors.append(f"dev endpoint {path} is present in the release binary")
    return errors


def check_profile_file(file: Path, label: str) -> list[str]:
    """A build file's `--features` lists must name no ship-forbidden feature."""
    errors: list[str] = []
    for number, line in enumerate(file.read_text(encoding="utf-8").splitlines(), start=1):
        for features in FEATURES_FLAG.findall(line):
            names = {name.strip() for name in re.split(r"[,\s]+", features) if name.strip()}
            for feature in SHIP_FORBIDDEN_FEATURES:
                if feature in names:
                    errors.append(f"{label}:{number}: build profile enables `{feature}`")
    return errors


def check_build_profiles(root: Path, extra_profiles: tuple[Path, ...] = ()) -> list[str]:
    errors: list[str] = []
    manifest = root / CARGO_MANIFEST
    if not manifest.is_file():
        return [f"{CARGO_MANIFEST} is missing"]
    manifest_text = manifest.read_text(encoding="utf-8")
    for feature in SHIP_FORBIDDEN_FEATURES:
        if re.search(rf"^{re.escape(feature)}\s*=\s*\[", manifest_text, re.MULTILINE) is None:
            errors.append(f"{CARGO_MANIFEST}: feature `{feature}` is not declared")
    for number, line in enumerate(manifest_text.splitlines(), start=1):
        stripped = line.split("#", 1)[0]
        if not re.match(r"\s*default\s*=", stripped):
            continue
        for feature in SHIP_FORBIDDEN_FEATURES:
            if re.search(rf"[\"']{re.escape(feature)}[\"']", stripped):
                errors.append(f"{CARGO_MANIFEST}:{number}: `default` enables `{feature}`")
    for profile in BUILD_PROFILES:
        file = root / profile
        if not file.is_file():
            errors.append(f"{profile} is missing (build profile list is stale)")
            continue
        errors.extend(check_profile_file(file, str(profile)))
    for file in extra_profiles:
        if not file.is_file():
            errors.append(f"{file} is missing (--profile-file must name an existing build file)")
            continue
        found = check_profile_file(file, str(file))
        if not any(FEATURES_FLAG.search(line) for line in file.read_text(encoding="utf-8").splitlines()):
            found.append(f"{file}: no `--features` list found; cannot certify its feature set")
        errors.extend(found)
    return errors


def check(
    root: Path,
    binary: Path | None,
    expect_present: bool,
    profiles: bool,
    extra_profiles: tuple[Path, ...] = (),
) -> list[str]:
    doc = root / SURFACE_DOC
    if not doc.is_file():
        return [f"{SURFACE_DOC} is missing"]
    doc_blob = doc.read_bytes()
    doc_text = doc_blob.decode("utf-8")
    paths, errors = dev_routes(doc_text)
    if profiles:
        errors.extend(check_build_profiles(root, extra_profiles))
    if binary is not None:
        if not binary.is_file():
            errors.append(f"binary {binary} does not exist")
        else:
            errors.extend(scan_binary(binary.read_bytes(), paths, doc_blob, expect_present))
    return errors


def _fake_repo(root: Path, *, classification: str = "disabled-in-release", default_line: str = "") -> None:
    docs = root / "docs/security"
    docs.mkdir(parents=True)
    (docs / "RPC_SURFACE.md").write_text(
        "| Method | Transport | Handler path:symbol | Classification | Mutates state? | Unbounded work risk | Rate-limit class |\n"
        "| --- | --- | --- | --- | --- | --- | --- |\n"
        "| `GET /readyz` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::readyz` | `public` | no | no | global-read |\n"
        f"| `POST /debug/seed-account` | `http` | `native-chain/src/bin/launch_api/routes/debug.rs:debug::seed_account {FEATURE_MARK}` | `{classification}` | yes | no | global-write |\n",
        encoding="utf-8",
    )
    (root / "native-chain").mkdir()
    (root / "native-chain/Cargo.toml").write_text(
        f"[features]\n{default_line}p2p = []\n{DEV_FEATURE} = []\n{PANIC_DRILL_FEATURE} = [\"p2p\"]\n",
        encoding="utf-8",
    )
    (root / "native-chain/Dockerfile").write_text(
        'RUN cargo build --release --locked --features "full-mining,p2p" \\\n', encoding="utf-8"
    )
    (root / "native-chain/Dockerfile.producer-key-pop").write_text(
        "RUN cargo build --release --locked --features hsm \\\n", encoding="utf-8"
    )
    (root / ".github/workflows").mkdir(parents=True)
    (root / ".github/workflows/release-signing.yml").write_text(
        "run: cargo build --release --locked --bin launch_api\n", encoding="utf-8"
    )
    (root / "scripts").mkdir()
    (root / "scripts/build-release.sh").write_text("cargo build --release --locked\n", encoding="utf-8")


def _blob(*routes: str, size: int = MIN_BINARY_BYTES + 16) -> bytes:
    body = b"\0".join(route.encode() for route in routes)
    return body + b"\0" * (size - len(body))


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        _fake_repo(root)
        assert check(root, None, False, True) == [], check(root, None, False, True)

        # Exercise a CRLF checkout: the include_str! payload preserves the file's
        # raw bytes, so masking must use read_bytes rather than newline-normalized text.
        doc = root / SURFACE_DOC
        doc.write_bytes(doc.read_bytes().replace(b"\n", b"\r\n"))
        doc_blob = doc.read_bytes()
        release = root / "release.bin"
        release.write_bytes(_blob(doc_blob.decode("utf-8"), "/readyz", "/health"))
        assert check(root, release, False, True) == [], check(root, release, False, True)
        failures = check(root, release, True, False)
        assert any("expected dev endpoint /debug/seed-account" in f for f in failures), failures

        dev = root / "dev.bin"
        dev.write_bytes(_blob(doc_blob.decode("utf-8"), "/readyz", "/debug/seed-account"))
        failures = check(root, dev, False, False)
        assert failures == ["dev endpoint /debug/seed-account is present in the release binary"], failures
        assert check(root, dev, True, False) == [], check(root, dev, True, False)

        tiny = root / "tiny.bin"
        tiny.write_bytes(_blob("/readyz", size=64))
        assert any("below" in f for f in check(root, tiny, False, False))
        blind = root / "blind.bin"
        blind.write_bytes(_blob(doc_blob.decode("utf-8"), "/nothing"))
        assert any("control route" in f for f in check(root, blind, False, False))

        no_doc = root / "no-doc.bin"
        no_doc.write_bytes(_blob("/readyz", "/health"))
        assert any(
            "embedded docs/security/RPC_SURFACE.md not found" in f
            for f in check(root, no_doc, False, False)
        )

        # Build-profile negative controls.
        dockerfile = root / "native-chain/Dockerfile"
        dockerfile.write_text('RUN cargo build --release --features "p2p,dev-endpoints" \\\n', encoding="utf-8")
        assert any("Dockerfile:1: build profile enables" in f for f in check_build_profiles(root))
        dockerfile.write_text('RUN cargo build --release --features "p2p" \\\n', encoding="utf-8")
        manifest = root / "native-chain/Cargo.toml"
        manifest.write_text(f"[features]\ndefault = [\"{DEV_FEATURE}\"]\n{DEV_FEATURE} = []\n", encoding="utf-8")
        assert any("`default` enables" in f for f in check_build_profiles(root))
        manifest.write_text("[features]\np2p = []\n", encoding="utf-8")
        assert any("is not declared" in f for f in check_build_profiles(root))

        # #3224 panic drill: same profile rule, plus the binary trigger literal.
        manifest.write_text(
            f"[features]\np2p = []\n{DEV_FEATURE} = []\n{PANIC_DRILL_FEATURE} = [\"p2p\"]\n",
            encoding="utf-8",
        )
        assert check_build_profiles(root) == [], check_build_profiles(root)
        dockerfile.write_text('RUN cargo build --release --features "p2p,panic-drill" \\\n', encoding="utf-8")
        assert any("enables `panic-drill`" in f for f in check_build_profiles(root))
        dockerfile.write_text('RUN cargo build --release --features "p2p" \\\n', encoding="utf-8")
        manifest.write_text(
            f"[features]\ndefault = [\"{PANIC_DRILL_FEATURE}\"]\n{DEV_FEATURE} = []\n{PANIC_DRILL_FEATURE} = []\n",
            encoding="utf-8",
        )
        assert any("`default` enables `panic-drill`" in f for f in check_build_profiles(root))
        manifest.write_text(f"[features]\n{DEV_FEATURE} = []\n", encoding="utf-8")
        assert any("`panic-drill` is not declared" in f for f in check_build_profiles(root))
        manifest.write_text(
            f"[features]\np2p = []\n{DEV_FEATURE} = []\n{PANIC_DRILL_FEATURE} = [\"p2p\"]\n",
            encoding="utf-8",
        )
        infra = root / "infra-Dockerfile.core"
        infra.write_text('    cargo build --release -p wrfcoin-native-chain --features "p2p,panic-drill"\n', encoding="utf-8")
        assert any("infra-Dockerfile.core:1: build profile enables `panic-drill`" in f
                   for f in check_build_profiles(root, (infra,)))
        infra.write_text('    cargo build --release -p wrfcoin-native-chain --features "p2p,hsm"\n', encoding="utf-8")
        assert check_build_profiles(root, (infra,)) == [], check_build_profiles(root, (infra,))
        infra.write_text("    cargo build --release\n", encoding="utf-8")
        assert any("no `--features` list" in f for f in check_build_profiles(root, (infra,)))
        assert any("is missing" in f for f in check_build_profiles(root, (root / "absent",)))
        drill = root / "drill.bin"
        drill.write_bytes(_blob(doc_blob.decode("utf-8"), "/readyz", PANIC_DRILL_MARKER))
        assert any("panic drill" in f for f in check(root, drill, False, False)), check(root, drill, False, False)

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        _fake_repo(root, classification="admin")
        assert any("must be `disabled-in-release`" in f for f in check(root, None, False, False))

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        _fake_repo(root)
        doc = root / SURFACE_DOC
        doc.write_text(doc.read_text(encoding="utf-8").replace(f" {FEATURE_MARK}", ""), encoding="utf-8")
        failures = check(root, None, False, False)
        assert any("under a dev prefix but not annotated" in f for f in failures), failures
        assert any("empty proof" in f for f in failures), failures
    print("check_release_endpoints.py self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--binary", type=Path, help="built launch_api to scan for dev route literals")
    parser.add_argument(
        "--expect-present",
        action="store_true",
        help="positive control: the binary was built WITH the feature and must contain every dev route",
    )
    parser.add_argument("--build-profiles", action="store_true", help="only check the build profiles")
    parser.add_argument(
        "--profile-file",
        type=Path,
        action="append",
        default=[],
        help="an extra build file (e.g. wrfcoin/infra's infra/docker/Dockerfile.core) whose --features "
        "lists must carry no ship-forbidden feature; repeatable",
    )
    parser.add_argument("--list", action="store_true", help="print the dev routes and exit")
    parser.add_argument("--root", type=Path, default=REPO)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.list:
        paths, errors = dev_routes((args.root / SURFACE_DOC).read_text(encoding="utf-8"))
        for path in paths:
            print(path)
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1 if errors else 0
    profiles = args.build_profiles or args.binary is None or bool(args.profile_file)
    errors = check(args.root, args.binary, args.expect_present, profiles, tuple(args.profile_file))
    for error in errors:
        print(f"ERROR: {error}", file=sys.stderr)
    if errors:
        return 1
    what = []
    if profiles:
        what.append("build profiles")
        what.extend(f"profile file {path}" for path in args.profile_file)
    if args.binary is not None:
        what.append(f"{args.binary} ({'dev routes present' if args.expect_present else 'dev routes absent'})")
    print(f"check_release_endpoints.py OK: {', '.join(what)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
