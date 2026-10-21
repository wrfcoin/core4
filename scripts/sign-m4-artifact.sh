#!/usr/bin/env bash
# Sign the already-built artifact and provenance manifest with the approved key.
set -euo pipefail

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
usage() { printf 'usage: %s --artifact FILE --manifest FILE --fingerprint FPR --out DIR\n' "$0" >&2; exit 2; }
sha256_file() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi; }
read_manifest() {
  local key value seen_keys=''
  while IFS='=' read -r key value; do
    [[ "$key" =~ ^[A-Z][A-Z0-9_]*$ && "$value" =~ ^[A-Za-z0-9.,_-]+$ ]] || die "invalid build manifest entry"
    case " $seen_keys " in *" $key "*) die "duplicate manifest key: $key" ;; esac
    seen_keys="$seen_keys $key"
    case "$key" in
      FORMAT_VERSION) FORMAT_VERSION=$value ;;
      SOURCE_COMMIT) SOURCE_COMMIT=$value ;;
      SOURCE_ARCHIVE_SHA256) SOURCE_ARCHIVE_SHA256=$value ;;
      CARGO_LOCK_SHA256) CARGO_LOCK_SHA256=$value ;;
      RUST_TOOLCHAIN_SHA256) RUST_TOOLCHAIN_SHA256=$value ;;
      RUST_TOOLCHAIN) RUST_TOOLCHAIN=$value ;;
      BUILD_TARGET) BUILD_TARGET=$value ;;
      CARGO_PACKAGE) CARGO_PACKAGE=$value ;;
      CARGO_BIN) CARGO_BIN=$value ;;
      CARGO_FEATURES) CARGO_FEATURES=$value ;;
      ARTIFACT_NAME) ARTIFACT_NAME=$value ;;
      ARTIFACT_SHA256) ARTIFACT_SHA256=$value ;;
      RUSTC_HOST) RUSTC_HOST=$value ;;
      BUILD_COMMAND) BUILD_COMMAND=$value ;;
      *) die "unknown build manifest key: $key" ;;
    esac
  done < "$1"
  [[ "$FORMAT_VERSION" == 1 && "$SOURCE_COMMIT" =~ ^[[:xdigit:]]{40}$ && "$SOURCE_ARCHIVE_SHA256" =~ ^[[:xdigit:]]{64}$ && "$CARGO_LOCK_SHA256" =~ ^[[:xdigit:]]{64}$ && "$RUST_TOOLCHAIN_SHA256" =~ ^[[:xdigit:]]{64}$ ]] || die "build manifest provenance mismatch"
  [[ "$RUST_TOOLCHAIN" == 1.98.0 && "$BUILD_TARGET" == aarch64-apple-darwin && "$CARGO_PACKAGE" == wrfcoin-native-chain && "$CARGO_BIN" == launch_api && "$CARGO_FEATURES" == full-mining,p2p ]] || die "build manifest policy mismatch"
  [[ "$ARTIFACT_NAME" == wrfcoin-node && "$ARTIFACT_SHA256" =~ ^[[:xdigit:]]{64}$ && "$RUSTC_HOST" == aarch64-apple-darwin && "$BUILD_COMMAND" == cargo-build-release-locked ]] || die "build manifest artifact mismatch"
}
verify_signature() {
  local sig=$1 file=$2 status
  status=$(mktemp "${TMPDIR:-/tmp}/wrfcoin-gpg-status.XXXXXX")
  trap 'rm -f "$status"' RETURN
  gpg --status-fd=1 --verify "$sig" "$file" > "$status" 2>/dev/null || die "signature verification failed: $sig"
  awk -v expected="$fingerprint" '$1 == "[GNUPG:]" && $2 == "VALIDSIG" && $12 == expected {matched++} END {exit matched == 1 ? 0 : 1}' "$status" || die "signature is not from the approved primary key: $sig"
}

artifact=''; manifest=''; fingerprint=''; out=''
while (($#)); do
  case "$1" in
    --artifact) (($# >= 2)) || usage; artifact=$2; shift 2 ;;
    --manifest) (($# >= 2)) || usage; manifest=$2; shift 2 ;;
    --fingerprint) (($# >= 2)) || usage; fingerprint=$(printf '%s' "$2" | tr -d '[:space:]' | tr '[:lower:]' '[:upper:]'); shift 2 ;;
    --out) (($# >= 2)) || usage; out=$2; shift 2 ;;
    -h|--help) usage ;;
    *) usage ;;
  esac
done
[[ -n "$artifact" && -n "$manifest" && -n "$fingerprint" && -n "$out" ]] || usage
[[ "$fingerprint" =~ ^([[:xdigit:]]{40}|[[:xdigit:]]{64})$ ]] || die "fingerprint must be hexadecimal"
[[ -f "$artifact" && ! -L "$artifact" && -f "$manifest" && ! -L "$manifest" ]] || die "artifact and manifest must be regular files"
command -v gpg >/dev/null 2>&1 || die "gpg is required"
read_manifest "$manifest"
[[ "$(sha256_file "$artifact")" == "$ARTIFACT_SHA256" ]] || die "artifact checksum does not match build manifest"
gpg --fingerprint "$fingerprint" >/dev/null 2>&1 || die "approved signing key is unavailable"
mkdir -p "$out"
[[ -z "$(find "$out" -mindepth 1 -maxdepth 1 -print -quit)" ]] || die "output directory must be empty"
gpg --batch --yes --local-user "$fingerprint" --armor --detach-sign --output "$out/wrfcoin-node.asc" "$artifact"
gpg --batch --yes --local-user "$fingerprint" --armor --detach-sign --output "$out/build-manifest.env.asc" "$manifest"
verify_signature "$out/wrfcoin-node.asc" "$artifact"
verify_signature "$out/build-manifest.env.asc" "$manifest"
printf 'signatures ready\nsource_commit=%s\nartifact_sha256=%s\nsigning_fingerprint=%s\n' "$SOURCE_COMMIT" "$ARTIFACT_SHA256" "$fingerprint"
