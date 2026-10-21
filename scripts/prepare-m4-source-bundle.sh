#!/usr/bin/env bash
# Prepare an auditable, credential-free source bundle for the native M4 build.
set -euo pipefail

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
usage() {
  printf 'usage: %s --source-dir DIR --commit SHA --out DIR\n' "$0" >&2
  exit 2
}
sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

source_dir=''
commit=''
out=''
while (($#)); do
  case "$1" in
    --source-dir) (($# >= 2)) || usage; source_dir=$2; shift 2 ;;
    --commit) (($# >= 2)) || usage; commit=$2; shift 2 ;;
    --out) (($# >= 2)) || usage; out=$2; shift 2 ;;
    -h|--help) usage ;;
    *) usage ;;
  esac
done
[[ -n "$source_dir" && -n "$commit" && -n "$out" ]] || usage
[[ "$commit" =~ ^[[:xdigit:]]{40}$ ]] || die "commit must be a 40-character SHA"
[[ -d "$source_dir" ]] || die "source directory does not exist"
command -v git >/dev/null 2>&1 || die "git is required"

root=$(git -C "$source_dir" rev-parse --show-toplevel 2>/dev/null) || die "source directory is not a Git worktree"
actual=$(git -C "$root" rev-parse HEAD) || die "cannot resolve source HEAD"
[[ "$actual" == "$commit" ]] || die "source HEAD $actual does not equal requested commit $commit"
[[ -z "$(git -C "$root" status --porcelain=v1 --untracked-files=all)" ]] || die "source worktree is dirty"
git -C "$root" diff --exit-code >/dev/null || die "source worktree has unstaged changes"
git -C "$root" diff --cached --exit-code >/dev/null || die "source worktree has staged changes"

[[ -f "$root/Cargo.lock" && -f "$root/rust-toolchain.toml" ]] || die "Cargo.lock and rust-toolchain.toml are required"
[[ -f "$root/native-chain/Cargo.toml" ]] || die "native-chain/Cargo.toml is required"
toolchain=$(awk -F'"' '$1 ~ /^[[:space:]]*channel[[:space:]]*=/ {print $2; exit}' "$root/rust-toolchain.toml")
[[ "$toolchain" == '1.98.0' ]] || die "unsupported Rust toolchain: ${toolchain:-missing}"

mkdir -p "$out"
[[ -z "$(find "$out" -mindepth 1 -maxdepth 1 -print -quit)" ]] || die "output directory must be empty"
umask 077
git -C "$root" archive --format=tar --prefix=wrfcoin-source/ "$commit" > "$out/source.tar"

source_sha=$(sha256_file "$out/source.tar")
lock_sha=$(sha256_file "$root/Cargo.lock")
toolchain_sha=$(sha256_file "$root/rust-toolchain.toml")
cat > "$out/source-manifest.env" <<EOF
FORMAT_VERSION=1
SOURCE_COMMIT=$commit
SOURCE_ARCHIVE=source.tar
SOURCE_ARCHIVE_SHA256=$source_sha
CARGO_LOCK_SHA256=$lock_sha
RUST_TOOLCHAIN_SHA256=$toolchain_sha
RUST_TOOLCHAIN=1.98.0
BUILD_TARGET=aarch64-apple-darwin
CARGO_PACKAGE=wrfcoin-native-chain
CARGO_BIN=launch_api
CARGO_FEATURES=full-mining,p2p
ARTIFACT_NAME=wrfcoin-node
EOF
manifest_sha=$(sha256_file "$out/source-manifest.env")
printf '%s  source-manifest.env\n' "$manifest_sha" > "$out/source-manifest.env.sha256"
printf 'source bundle ready\ncommit=%s\nsource_archive_sha256=%s\nmanifest_sha256=%s\n' "$commit" "$source_sha" "$manifest_sha"
