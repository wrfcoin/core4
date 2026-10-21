#!/usr/bin/env bash
# Install or verify a supplied Mac release artifact. This script never builds,
# signs, downloads, or publishes an artifact.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR="$script_dir"
readonly PEER_HOME="${WRFCOIN_SYNC_PEER_HOME:-$HOME/Library/Application Support/WRFCoinSyncPeer}"
readonly CONFIG_PATH="${WRFCOIN_SYNC_PEER_CONFIG:-$PEER_HOME/sync-peer.conf}"
readonly INSTALLED_CONFIG_PATH="$PEER_HOME/sync-peer.conf"
readonly LABEL='org.wrfcoin.sync-peer'
readonly PLIST_PATH="$HOME/Library/LaunchAgents/$LABEL.plist"
VERIFY_ONLY=false
[[ "${1:-}" != --verify-only ]] || VERIFY_ONLY=true

source "$SCRIPT_DIR/common.sh"
validate_config "$CONFIG_PATH"
for required_input in \
  WRFCOIN_ARTIFACT_PATH WRFCOIN_ARTIFACT_SHA256 \
  WRFCOIN_ARTIFACT_SIGNATURE_PATH WRFCOIN_BUILD_MANIFEST_PATH \
  WRFCOIN_BUILD_MANIFEST_SIGNATURE_PATH WRFCOIN_RELEASE_KEY_FINGERPRINT \
  WRFCOIN_SOURCE_COMMIT; do
  require_value "$required_input"
done
[[ "$WRFCOIN_SOURCE_COMMIT" =~ ^[[:xdigit:]]{40}$ ]] || die "source commit must be a 40-character SHA"
[[ -x "$WRFCOIN_ARTIFACT_PATH" ]] || die "signed install requires a raw executable artifact"
if tar -tzf "$WRFCOIN_ARTIFACT_PATH" >/dev/null 2>&1; then
  die "signed install does not accept an unsigned/archive artifact"
fi

readonly VERIFY_SCRIPT="$SCRIPT_DIR/../../scripts/verify-m4-signed-artifact.sh"
[[ -x "$VERIFY_SCRIPT" ]] || die "signed artifact verifier is missing"
"$VERIFY_SCRIPT" \
  --artifact "$WRFCOIN_ARTIFACT_PATH" \
  --artifact-sha256 "$WRFCOIN_ARTIFACT_SHA256" \
  --manifest "$WRFCOIN_BUILD_MANIFEST_PATH" \
  --artifact-signature "$WRFCOIN_ARTIFACT_SIGNATURE_PATH" \
  --manifest-signature "$WRFCOIN_BUILD_MANIFEST_SIGNATURE_PATH" \
  --fingerprint "$WRFCOIN_RELEASE_KEY_FINGERPRINT" \
  --source-commit "$WRFCOIN_SOURCE_COMMIT"
candidate="$WRFCOIN_ARTIFACT_PATH"
if "$VERIFY_ONLY"; then printf 'artifact verification passed\n'; exit 0; fi
mkdir -p "$PEER_HOME/bin" "$HOME/Library/Logs/WRFCoinSyncPeer" "$HOME/Library/LaunchAgents"
if [[ "$CONFIG_PATH" != "$INSTALLED_CONFIG_PATH" ]]; then
  install -m 600 "$CONFIG_PATH" "$INSTALLED_CONFIG_PATH"
else
  chmod 600 "$INSTALLED_CONFIG_PATH"
fi
install -m 755 "$candidate" "$PEER_HOME/bin/wrfcoin-node"
install -m 755 "$SCRIPT_DIR/sync-peer.sh" "$PEER_HOME/sync-peer.sh"
install -m 644 "$SCRIPT_DIR/common.sh" "$PEER_HOME/common.sh"
sed "s|__SYNC_PEER_HOME__|$PEER_HOME|g" "$SCRIPT_DIR/org.wrfcoin.sync-peer.plist.template" > "$PLIST_PATH"
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST_PATH"
launchctl kickstart -k "gui/$(id -u)/$LABEL"
printf 'installed and started %s\n' "$LABEL"
