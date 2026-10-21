#!/usr/bin/env bash
# Verify the signed native artifact on the destination M4 before installation.
set -euo pipefail

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
usage() { printf 'usage: %s --artifact FILE --artifact-sha256 SHA --manifest FILE --artifact-signature FILE --manifest-signature FILE --fingerprint FPR --source-commit SHA\n' "$0" >&2; exit 2; }
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
      ARTIFACT_NAME) ARTIFACT_NAME=$value ;;
      ARTIFACT_SHA256) ARTIFACT_SHA256=$value ;;
      BUILD_TARGET) BUILD_TARGET=$value ;;
      CARGO_PACKAGE) CARGO_PACKAGE=$value ;;
      CARGO_BIN) CARGO_BIN=$value ;;
      CARGO_FEATURES) CARGO_FEATURES=$value ;;
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
  local sig=$1 file=$2 status packets
  status=$(mktemp "${TMPDIR:-/tmp}/wrfcoin-gpg-status.XXXXXX")
  trap 'rm -f "$status"' RETURN
  gpg --status-fd=1 --verify "$sig" "$file" > "$status" 2>/dev/null || die "signature verification failed: $sig"
  awk -v expected="$fingerprint" '$1 == "[GNUPG:]" && $2 == "VALIDSIG" && $12 == expected {matched++} END {exit matched == 1 ? 0 : 1}' "$status" || die "signature is not from approved primary key: $sig"
  packets=$(gpg --list-packets "$sig" 2>/dev/null | awk '/signature packet:/ {count++} END {print count+0}')
  [[ "$packets" == 1 ]] || die "signature must contain exactly one packet: $sig"
}

artifact=''; artifact_sha256=''; manifest=''; artifact_signature=''; manifest_signature=''; fingerprint=''; source_commit=''
while (($#)); do
  case "$1" in
    --artifact) (($# >= 2)) || usage; artifact=$2; shift 2 ;;
    --artifact-sha256) (($# >= 2)) || usage; artifact_sha256=$2; shift 2 ;;
    --manifest) (($# >= 2)) || usage; manifest=$2; shift 2 ;;
    --artifact-signature) (($# >= 2)) || usage; artifact_signature=$2; shift 2 ;;
    --manifest-signature) (($# >= 2)) || usage; manifest_signature=$2; shift 2 ;;
    --fingerprint) (($# >= 2)) || usage; fingerprint=$(printf '%s' "$2" | tr -d '[:space:]' | tr '[:lower:]' '[:upper:]'); shift 2 ;;
    --source-commit) (($# >= 2)) || usage; source_commit=$2; shift 2 ;;
    -h|--help) usage ;;
    *) usage ;;
  esac
done
[[ -n "$artifact" && -n "$artifact_sha256" && -n "$manifest" && -n "$artifact_signature" && -n "$manifest_signature" && -n "$fingerprint" && -n "$source_commit" ]] || usage
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || die "verification must run on Darwin arm64"
[[ "$fingerprint" =~ ^([[:xdigit:]]{40}|[[:xdigit:]]{64})$ ]] || die "fingerprint must be hexadecimal"
[[ "$source_commit" =~ ^[[:xdigit:]]{40}$ ]] || die "source commit must be a 40-character SHA"
[[ "$artifact_sha256" =~ ^[[:xdigit:]]{64}$ ]] || die "artifact checksum must be 64 hexadecimal characters"
for file in "$artifact" "$manifest" "$artifact_signature" "$manifest_signature"; do [[ -f "$file" && ! -L "$file" ]] || die "required regular file missing: $file"; done
command -v gpg >/dev/null 2>&1 || die "gpg is required"
command -v lipo >/dev/null 2>&1 || die "lipo is required"
read_manifest "$manifest"
[[ "$SOURCE_COMMIT" == "$source_commit" ]] || die "source commit mismatch"
[[ "$ARTIFACT_SHA256" == "$artifact_sha256" ]] || die "configured artifact checksum does not match build manifest"
[[ "$(sha256_file "$artifact")" == "$artifact_sha256" ]] || die "artifact checksum mismatch"
gpg --fingerprint "$fingerprint" >/dev/null 2>&1 || die "approved release key is unavailable"
verify_signature "$artifact_signature" "$artifact"
verify_signature "$manifest_signature" "$manifest"
archs=$(lipo -archs "$artifact")
case " $archs " in *' arm64 '*) ;; *) die "artifact is not arm64: $archs" ;; esac
printf 'signed native artifact verified\nsource_commit=%s\nartifact_sha256=%s\n' "$SOURCE_COMMIT" "$ARTIFACT_SHA256"
