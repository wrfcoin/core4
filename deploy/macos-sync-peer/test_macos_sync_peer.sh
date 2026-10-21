#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR="$script_dir"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
peer_home="$tmpdir/peer"
mkdir -p "$peer_home/bin"
cp "$SCRIPT_DIR/sync-peer.conf.example" "$tmpdir/good.conf"
sed "s|REPLACE_WITH_APPROVED_PUBLIC_MULTIADDR|/ip4/203.0.113.9/tcp/9000/p2p/12D3KooWExample|" "$tmpdir/good.conf" > "$tmpdir/edited.conf"
mv "$tmpdir/edited.conf" "$tmpdir/good.conf"
sed "s|REPLACE_WITH_ABSOLUTE_ARTIFACT_PATH|$tmpdir/node|" "$tmpdir/good.conf" > "$tmpdir/edited.conf"
mv "$tmpdir/edited.conf" "$tmpdir/good.conf"
sed 's|REPLACE_WITH_64_HEX_SHA256|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|' "$tmpdir/good.conf" > "$tmpdir/edited.conf"
mv "$tmpdir/edited.conf" "$tmpdir/good.conf"
cp "$tmpdir/good.conf" "$peer_home/sync-peer.conf"
printf '#!/usr/bin/env bash\nenv > %q\n' "$tmpdir/node.capture" > "$peer_home/bin/wrfcoin-node"
chmod +x "$peer_home/bin/wrfcoin-node"
WRFCOIN_SYNC_PEER_HOME="$peer_home" "$SCRIPT_DIR/sync-peer.sh" --validate-config >/dev/null
WRFCOIN_PRODUCER_KEY=must_not_escape WRFCOIN_SYNC_PEER_HOME="$peer_home" "$SCRIPT_DIR/sync-peer.sh"
if grep -q '^WRFCOIN_PRODUCER_KEY=' "$tmpdir/node.capture"; then
  printf 'ambient producer key escaped the clean environment\n' >&2; exit 1
fi
valid_bootstrap_count=0
invalid_bootstrap_count=0
signed_install_negative_count=0
for valid_bootstrap in \
  /ip4/203.0.113.9/tcp/9000/p2p/12D3KooWExample \
  /dns4/bootstrap.example/tcp/9000/p2p/12D3KooWExample \
  /dns6/bootstrap.example/tcp/9001/p2p/12D3KooWExample; do
  sed "s|^WRFCOIN_P2P_BOOTSTRAP=.*|WRFCOIN_P2P_BOOTSTRAP=$valid_bootstrap|" "$tmpdir/good.conf" > "$tmpdir/bootstrap.conf"
  WRFCOIN_SYNC_PEER_HOME="$peer_home" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/bootstrap.conf" "$SCRIPT_DIR/sync-peer.sh" --validate-config >/dev/null
  ((valid_bootstrap_count += 1))
done

for invalid_bootstrap in \
  /ip4/203.0.113.9/p2p/12D3KooWExample \
  /dns4/bootstrap.example/p2p/12D3KooWExample \
  /ip4/203.0.113.9/udp/9000/p2p/12D3KooWExample \
  /ip4/203.0.113.9/tcp/0/p2p/12D3KooWExample \
  /ip4/203.0.113.9/tcp/65536/p2p/12D3KooWExample \
  /ip4/203.0.113.9/tcp/9000/p2p/; do
  sed "s|^WRFCOIN_P2P_BOOTSTRAP=.*|WRFCOIN_P2P_BOOTSTRAP=$invalid_bootstrap|" "$tmpdir/good.conf" > "$tmpdir/bootstrap.conf"
  if WRFCOIN_SYNC_PEER_HOME="$peer_home" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/bootstrap.conf" "$SCRIPT_DIR/sync-peer.sh" --validate-config >/dev/null 2>&1; then
    printf 'expected bootstrap rejection: %s\n' "$invalid_bootstrap" >&2; exit 1
  fi
  ((invalid_bootstrap_count += 1))
done

printf 'WRFCOIN_PRODUCER_KEY=forbidden\n' >> "$peer_home/sync-peer.conf"
if WRFCOIN_SYNC_PEER_HOME="$peer_home" "$SCRIPT_DIR/sync-peer.sh" --validate-config >/dev/null 2>&1; then
  printf 'expected role-bearing config rejection\n' >&2; exit 1
fi
cp "$tmpdir/good.conf" "$peer_home/sync-peer.conf"
printf '%s\n' 'WRFCOIN_P2P_BOOTSTRAP=$(touch SHOULD_NOT_RUN)' >> "$peer_home/sync-peer.conf"
if WRFCOIN_SYNC_PEER_HOME="$peer_home" "$SCRIPT_DIR/sync-peer.sh" --validate-config >/dev/null 2>&1; then
  printf 'expected config-code rejection\n' >&2; exit 1
fi
bad_path="$tmpdir/."'env'
cp "$tmpdir/good.conf" "$tmpdir/bad.conf"
sed "s|WRFCOIN_ARTIFACT_PATH=.*|WRFCOIN_ARTIFACT_PATH=$bad_path|" "$tmpdir/bad.conf" > "$tmpdir/edited.conf"
mv "$tmpdir/edited.conf" "$tmpdir/bad.conf"
if WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/bad.conf" "$SCRIPT_DIR/install.sh" --verify-only >/dev/null 2>&1; then
  printf 'expected forbidden artifact-path rejection\n' >&2; exit 1
fi
printf '#!/usr/bin/env bash\nexit 0\n' > "$tmpdir/node"
chmod +x "$tmpdir/node"
node_sha="$(shasum -a 256 "$tmpdir/node" | awk '{print $1}')"
sed "s|WRFCOIN_ARTIFACT_SHA256=.*|WRFCOIN_ARTIFACT_SHA256=$node_sha|" "$tmpdir/good.conf" > "$tmpdir/arch.conf"
mkdir -p "$tmpdir/fake-bin"
printf '%s\n' '#!/usr/bin/env bash' 'case "${1:-}" in' '  -s) echo "${UNAME_SYSTEM:-Darwin}" ;;' '  -m) echo arm64 ;;' '  *) exit 1 ;;' 'esac' > "$tmpdir/fake-bin/uname"
printf '%s\n' '#!/usr/bin/env bash' 'echo "${LIPO_ARCHES:-arm64}"' > "$tmpdir/fake-bin/lipo"
printf '%s\n' '#!/usr/bin/env bash' 'test -z "${LAUNCHCTL_MARKER:-}" || touch "$LAUNCHCTL_MARKER"' 'exit 0' > "$tmpdir/fake-bin/launchctl"
chmod +x "$tmpdir/fake-bin/uname" "$tmpdir/fake-bin/lipo" "$tmpdir/fake-bin/launchctl"
unsigned_launch_marker="$tmpdir/unsigned-launch.marker"
if LAUNCHCTL_MARKER="$unsigned_launch_marker" PATH="$tmpdir/fake-bin:$PATH" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/arch.conf" "$SCRIPT_DIR/install.sh" >/dev/null 2>&1; then
  printf 'expected absent signature material rejection\n' >&2; exit 1
fi
test ! -e "$unsigned_launch_marker"
((signed_install_negative_count += 1))
if PATH="$tmpdir/fake-bin:$PATH" LIPO_ARCHES=x86_64 WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/arch.conf" "$SCRIPT_DIR/install.sh" --verify-only >/dev/null 2>&1; then
  printf 'expected non-arm64 artifact rejection\n' >&2; exit 1
fi
if UNAME_SYSTEM=Linux PATH="$tmpdir/fake-bin:$PATH" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/arch.conf" "$SCRIPT_DIR/install.sh" --verify-only >/dev/null 2>&1; then
  printf 'expected non-macOS host rejection\n' >&2; exit 1
fi
signer_fingerprint=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
wrong_fingerprint=BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB
printf '%s\n' '#!/usr/bin/env bash' 'case "$1" in' '  --fingerprint) exit 0 ;;' '  --status-fd=1) printf "[GNUPG:] VALIDSIG %s 20260802 0 0 4 0 22 8 00 %s\\n" "$GPG_TEST_SIGNING_KEY" "$GPG_TEST_PRIMARY_KEY"; exit 0 ;;' '  --list-packets) echo "signature packet: synthetic"; exit 0 ;;' '  *) exit 2 ;;' 'esac' > "$tmpdir/fake-bin/gpg"
chmod +x "$tmpdir/fake-bin/gpg"
printf 'synthetic detached signature fixture\n' > "$tmpdir/node.sig"
printf 'synthetic detached manifest signature fixture\n' > "$tmpdir/manifest.sig"
source_commit=1111111111111111111111111111111111111111
manifest="$tmpdir/build-manifest.env"
target_arch=aarch64
target_vendor=apple
target_os=darwin
build_target="$target_arch-$target_vendor-$target_os"
build_command_prefix=cargo-build
build_command_suffix=release-locked
build_command="$build_command_prefix-$build_command_suffix"
printf '%s\n' \
  'FORMAT_VERSION=1' \
  "SOURCE_COMMIT=$source_commit" \
  'SOURCE_ARCHIVE_SHA256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
  'CARGO_LOCK_SHA256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' \
  'RUST_TOOLCHAIN_SHA256=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc' \
  'RUST_TOOLCHAIN=1.98.0' \
  "BUILD_TARGET=$build_target" \
  'CARGO_PACKAGE=wrfcoin-native-chain' \
  'CARGO_BIN=launch_api' \
  'CARGO_FEATURES=full-mining,p2p' \
  'ARTIFACT_NAME=wrfcoin-node' \
  "ARTIFACT_SHA256=$node_sha" \
  'RUSTC_HOST=aarch64-apple-darwin' \
  "BUILD_COMMAND=$build_command" > "$manifest"
sed -e "s|WRFCOIN_ARTIFACT_SIGNATURE_PATH=.*|WRFCOIN_ARTIFACT_SIGNATURE_PATH=$tmpdir/node.sig|" \
  -e "s|WRFCOIN_BUILD_MANIFEST_PATH=.*|WRFCOIN_BUILD_MANIFEST_PATH=$manifest|" \
  -e "s|WRFCOIN_BUILD_MANIFEST_SIGNATURE_PATH=.*|WRFCOIN_BUILD_MANIFEST_SIGNATURE_PATH=$tmpdir/manifest.sig|" \
  -e "s|WRFCOIN_RELEASE_KEY_FINGERPRINT=.*|WRFCOIN_RELEASE_KEY_FINGERPRINT=$signer_fingerprint|" \
  -e "s|WRFCOIN_SOURCE_COMMIT=.*|WRFCOIN_SOURCE_COMMIT=$source_commit|" \
  "$tmpdir/arch.conf" > "$tmpdir/signed.conf"
GPG_TEST_SIGNING_KEY="$signer_fingerprint" GPG_TEST_PRIMARY_KEY="$signer_fingerprint" PATH="$tmpdir/fake-bin:$PATH" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/signed.conf" "$SCRIPT_DIR/install.sh" --verify-only >/dev/null
installed_home="$tmpdir/installed-peer"
launch_marker="$tmpdir/launch.marker"
GPG_TEST_SIGNING_KEY="$signer_fingerprint" GPG_TEST_PRIMARY_KEY="$signer_fingerprint" LAUNCHCTL_MARKER="$launch_marker" PATH="$tmpdir/fake-bin:$PATH" HOME="$tmpdir/home" WRFCOIN_SYNC_PEER_HOME="$installed_home" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/signed.conf" "$SCRIPT_DIR/install.sh" >/dev/null
test -e "$launch_marker"
test -r "$installed_home/common.sh"
test ! -x "$installed_home/common.sh"
WRFCOIN_SYNC_PEER_HOME="$installed_home" "$installed_home/sync-peer.sh" --validate-config >/dev/null
sed "s|WRFCOIN_RELEASE_KEY_FINGERPRINT=.*|WRFCOIN_RELEASE_KEY_FINGERPRINT=$wrong_fingerprint|" "$tmpdir/signed.conf" > "$tmpdir/wrong-signer.conf"
wrong_signer_marker="$tmpdir/wrong-signer.marker"
if GPG_TEST_SIGNING_KEY="$signer_fingerprint" GPG_TEST_PRIMARY_KEY="$signer_fingerprint" LAUNCHCTL_MARKER="$wrong_signer_marker" PATH="$tmpdir/fake-bin:$PATH" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/wrong-signer.conf" "$SCRIPT_DIR/install.sh" >/dev/null 2>&1; then
  printf 'expected wrong signer rejection\n' >&2; exit 1
fi
test ! -e "$wrong_signer_marker"
((signed_install_negative_count += 1))
printf '%s\n' '# altered' >> "$tmpdir/node"
altered_artifact_marker="$tmpdir/altered-artifact.marker"
if GPG_TEST_SIGNING_KEY="$signer_fingerprint" GPG_TEST_PRIMARY_KEY="$signer_fingerprint" LAUNCHCTL_MARKER="$altered_artifact_marker" PATH="$tmpdir/fake-bin:$PATH" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/signed.conf" "$SCRIPT_DIR/install.sh" >/dev/null 2>&1; then
  printf 'expected altered artifact rejection\n' >&2; exit 1
fi
test ! -e "$altered_artifact_marker"
((signed_install_negative_count += 1))
cp "$manifest" "$tmpdir/altered-manifest.env"
sed "s|SOURCE_COMMIT=.*|SOURCE_COMMIT=2222222222222222222222222222222222222222|" "$tmpdir/altered-manifest.env" > "$tmpdir/altered-manifest.edited"
mv "$tmpdir/altered-manifest.edited" "$tmpdir/altered-manifest.env"
sed "s|WRFCOIN_BUILD_MANIFEST_PATH=.*|WRFCOIN_BUILD_MANIFEST_PATH=$tmpdir/altered-manifest.env|" "$tmpdir/signed.conf" > "$tmpdir/altered-manifest.conf"
altered_manifest_marker="$tmpdir/altered-manifest.marker"
if GPG_TEST_SIGNING_KEY="$signer_fingerprint" GPG_TEST_PRIMARY_KEY="$signer_fingerprint" LAUNCHCTL_MARKER="$altered_manifest_marker" PATH="$tmpdir/fake-bin:$PATH" WRFCOIN_SYNC_PEER_CONFIG="$tmpdir/altered-manifest.conf" "$SCRIPT_DIR/install.sh" >/dev/null 2>&1; then
  printf 'expected altered manifest rejection\n' >&2; exit 1
fi
test ! -e "$altered_manifest_marker"
((signed_install_negative_count += 1))

# Negative control: the launcher must fail if its staged runtime dependency is
# omitted. Keep this fixture separate from the positive installer layout.
missing_common_home="$tmpdir/missing-common-peer"
mkdir -p "$missing_common_home/bin"
install -m 600 "$tmpdir/arch.conf" "$missing_common_home/sync-peer.conf"
install -m 755 "$tmpdir/node" "$missing_common_home/bin/wrfcoin-node"
install -m 755 "$SCRIPT_DIR/sync-peer.sh" "$missing_common_home/sync-peer.sh"
if WRFCOIN_SYNC_PEER_HOME="$missing_common_home" "$missing_common_home/sync-peer.sh" --validate-config >/dev/null 2>&1; then
  printf 'expected missing common.sh rejection\n' >&2; exit 1
fi
mkdir -p "$tmpdir/archive-src"
printf 'x\n' > "$tmpdir/archive-src/wrfcoin-node"
tar -czf "$tmpdir/traversal.tar.gz" --transform='s|wrfcoin-node|../escape|' -C "$tmpdir/archive-src" wrfcoin-node
if ( source "$SCRIPT_DIR/common.sh"; validate_archive "$tmpdir/traversal.tar.gz" ) >/dev/null 2>&1; then
  printf 'expected traversal archive rejection\n' >&2; exit 1
fi
grep -q 'launchctl' "$SCRIPT_DIR/install.sh"
mkdir -p "$tmpdir/mock-bin"
printf '%s\n' '#!/usr/bin/env bash' 'case "${*: -1}" in' '  */node/sync/status) echo '\''{"sync_status":"synced","peer_count":1,"chain_height":1}'\'' ;;' '  */network/info) echo '\''{"network_id":"testnet","chain_id":777,"genesis_hash":"0x805efc973fe50829ae2abf7c73bde0a5ed8511f3dfa97ff50a86615703442625"}'\'' ;;' '  *) exit 2 ;;' 'esac' > "$tmpdir/mock-bin/curl"
chmod +x "$tmpdir/mock-bin/curl"
PATH="$tmpdir/mock-bin:$PATH" "$SCRIPT_DIR/healthcheck.sh" >/dev/null
grep -q 'network identity mismatch' "$SCRIPT_DIR/healthcheck.sh"
printf 'PASS: macOS sync-peer package static and rejection checks (valid=%d, bootstrap RED controls caught=%d, signed-install RED controls caught=%d)\n' "$valid_bootstrap_count" "$invalid_bootstrap_count" "$signed_install_negative_count"
