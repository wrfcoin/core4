#!/usr/bin/env bash
# Build only on the pinned native Apple Silicon host; never downloads or signs.
set -euo pipefail

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
usage() { printf 'usage: %s --bundle DIR --out DIR\n' "$0" >&2; exit 2; }
sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi
}
require_file() { [[ -f "$1" && ! -L "$1" ]] || die "required regular file missing: $1"; }
read_manifest() {
  local key value seen_keys=''
  while IFS='=' read -r key value; do
    [[ "$key" =~ ^[A-Z][A-Z0-9_]*$ && "$value" =~ ^[A-Za-z0-9.,_-]+$ ]] || die "invalid manifest entry"
    case " $seen_keys " in *" $key "*) die "duplicate manifest key: $key" ;; esac
    seen_keys="$seen_keys $key"
    case "$key" in
      FORMAT_VERSION) FORMAT_VERSION=$value ;;
      SOURCE_COMMIT) SOURCE_COMMIT=$value ;;
      SOURCE_ARCHIVE) SOURCE_ARCHIVE=$value ;;
      SOURCE_ARCHIVE_SHA256) SOURCE_ARCHIVE_SHA256=$value ;;
      CARGO_LOCK_SHA256) CARGO_LOCK_SHA256=$value ;;
      RUST_TOOLCHAIN_SHA256) RUST_TOOLCHAIN_SHA256=$value ;;
      RUST_TOOLCHAIN) RUST_TOOLCHAIN=$value ;;
      BUILD_TARGET) BUILD_TARGET=$value ;;
      CARGO_PACKAGE) CARGO_PACKAGE=$value ;;
      CARGO_BIN) CARGO_BIN=$value ;;
      CARGO_FEATURES) CARGO_FEATURES=$value ;;
      ARTIFACT_NAME) ARTIFACT_NAME=$value ;;
      *) die "unknown manifest key: $key" ;;
    esac
  done < "$1"
  [[ "$FORMAT_VERSION" == 1 && "$SOURCE_COMMIT" =~ ^[[:xdigit:]]{40}$ ]] || die "invalid source manifest"
  [[ "$SOURCE_ARCHIVE" == source.tar && "$RUST_TOOLCHAIN" == 1.98.0 ]] || die "source manifest policy mismatch"
  [[ "$BUILD_TARGET" == aarch64-apple-darwin && "$CARGO_PACKAGE" == wrfcoin-native-chain ]] || die "native build policy mismatch"
  [[ "$CARGO_BIN" == launch_api && "$CARGO_FEATURES" == full-mining,p2p && "$ARTIFACT_NAME" == wrfcoin-node ]] || die "artifact policy mismatch"
}
verify_digest() { [[ "$(sha256_file "$1")" == "$2" ]] || die "checksum mismatch: $1"; }

bundle=''
out=''
while (($#)); do
  case "$1" in
    --bundle) (($# >= 2)) || usage; bundle=$2; shift 2 ;;
    --out) (($# >= 2)) || usage; out=$2; shift 2 ;;
    -h|--help) usage ;;
    *) usage ;;
  esac
done
[[ -n "$bundle" && -n "$out" ]] || usage
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || die "native build must run on Darwin arm64"
command -v cargo >/dev/null 2>&1 || die "cargo is required"
command -v rustc >/dev/null 2>&1 || die "rustc is required"
command -v rustup >/dev/null 2>&1 || die "rustup is required to prove the pinned toolchain"
command -v tar >/dev/null 2>&1 || die "tar is required"
command -v lipo >/dev/null 2>&1 || die "lipo is required"
[[ -d "$bundle" ]] || die "bundle directory does not exist"
manifest="$bundle/source-manifest.env"
manifest_checksum="$bundle/source-manifest.env.sha256"
archive="$bundle/source.tar"
require_file "$manifest"; require_file "$manifest_checksum"; require_file "$archive"
expected_manifest_sha=$(awk 'NF == 2 && $2 == "source-manifest.env" {print $1; exit}' "$manifest_checksum")
[[ "$expected_manifest_sha" =~ ^[[:xdigit:]]{64}$ ]] || die "invalid manifest checksum file"
verify_digest "$manifest" "$expected_manifest_sha"
read_manifest "$manifest"
verify_digest "$archive" "$SOURCE_ARCHIVE_SHA256"

build_root=$(mktemp -d "${TMPDIR:-/tmp}/wrfcoin-m4-build.XXXXXX")
trap 'rm -rf "$build_root"' EXIT
tar -tf "$archive" | while IFS= read -r member; do
  case "$member" in /*|../*|*/../*|*/..|*'/.'|*'//'*|'') die "unsafe archive member: $member" ;; esac
done
tar -xf "$archive" -C "$build_root"
source_root="$build_root/wrfcoin-source"
[[ -d "$source_root" && ! -L "$source_root" ]] || die "archive root missing"
[[ -z "$(find "$source_root" \( -type l -o -type p -o -type s -o -type b -o -type c \) -print -quit)" ]] || die "archive contains a non-regular entry"
require_file "$source_root/Cargo.lock"; require_file "$source_root/rust-toolchain.toml"
verify_digest "$source_root/Cargo.lock" "$CARGO_LOCK_SHA256"
verify_digest "$source_root/rust-toolchain.toml" "$RUST_TOOLCHAIN_SHA256"
channel=$(awk -F'"' '$1 ~ /^[[:space:]]*channel[[:space:]]*=/ {print $2; exit}' "$source_root/rust-toolchain.toml")
[[ "$channel" == "$RUST_TOOLCHAIN" ]] || die "toolchain manifest mismatch"
[[ "$(rustc --version)" == rustc\ 1.98.0\ * ]] || die "rustc 1.98.0 is required"
host=$(rustc -vV | awk '$1 == "host:" {print $2; exit}')
[[ "$host" == aarch64-apple-darwin ]] || die "rustc host must be aarch64-apple-darwin"
rustup target list --installed | awk '$1 == "aarch64-apple-darwin" {found=1} END {exit found ? 0 : 1}' || die "aarch64-apple-darwin target is not installed"

mkdir -p "$out"
[[ -z "$(find "$out" -mindepth 1 -maxdepth 1 -print -quit)" ]] || die "output directory must be empty"
home_dir=${HOME:?HOME is required}
cargo_home=${CARGO_HOME:-$home_dir/.cargo}
rustup_home=${RUSTUP_HOME:-$home_dir/.rustup}
(cd "$source_root" && env -i HOME="$home_dir" PATH="$PATH" CARGO_HOME="$cargo_home" RUSTUP_HOME="$rustup_home" CARGO_TERM_COLOR=never SOURCE_DATE_EPOCH=0 cargo build --release --locked --target "$BUILD_TARGET" --bin "$CARGO_BIN" --features "$CARGO_FEATURES")
candidate="$source_root/target/$BUILD_TARGET/release/launch_api"
require_file "$candidate"
[[ -x "$candidate" ]] || die "built artifact is not executable"
archs=$(lipo -archs "$candidate")
case " $archs " in *' arm64 '*) ;; *) die "artifact is not arm64: $archs" ;; esac
cp "$candidate" "$out/$ARTIFACT_NAME"
chmod 755 "$out/$ARTIFACT_NAME"
artifact_sha=$(sha256_file "$out/$ARTIFACT_NAME")
cat > "$out/build-manifest.env" <<EOF
FORMAT_VERSION=1
SOURCE_COMMIT=$SOURCE_COMMIT
SOURCE_ARCHIVE_SHA256=$SOURCE_ARCHIVE_SHA256
CARGO_LOCK_SHA256=$CARGO_LOCK_SHA256
RUST_TOOLCHAIN_SHA256=$RUST_TOOLCHAIN_SHA256
RUST_TOOLCHAIN=$RUST_TOOLCHAIN
BUILD_TARGET=$BUILD_TARGET
CARGO_PACKAGE=$CARGO_PACKAGE
CARGO_BIN=$CARGO_BIN
CARGO_FEATURES=$CARGO_FEATURES
ARTIFACT_NAME=$ARTIFACT_NAME
ARTIFACT_SHA256=$artifact_sha
RUSTC_HOST=$host
BUILD_COMMAND=cargo-build-release-locked
EOF
printf '%s  build-manifest.env\n' "$(sha256_file "$out/build-manifest.env")" > "$out/build-manifest.env.sha256"
printf 'native artifact ready\nsource_commit=%s\nartifact_sha256=%s\n' "$SOURCE_COMMIT" "$artifact_sha"
