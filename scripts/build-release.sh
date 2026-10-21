#!/usr/bin/env bash
#
# build-release.sh — Build a reproducible Docker image for the WRFCoin native-chain
# node and generate SHA-256 checksums of the binary.
#
# Usage:
#   scripts/build-release.sh              # Build image + extract binary + checksums
#   scripts/build-release.sh --dry-run    # Print commands without executing
#   scripts/build-release.sh --help       # Show usage
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ---- Defaults ----------------------------------------------------------------

DRY_RUN=false
IMAGE_REPO="ghcr.io/wrfcoin/native-chain"
DOCKERFILE="native-chain/Dockerfile"

# ---- Parse arguments ---------------------------------------------------------

for arg in "$@"; do
    case "$arg" in
        --dry-run)
            DRY_RUN=true
            ;;
        --help|-h)
            echo "Usage: scripts/build-release.sh [--dry-run] [--help]"
            echo ""
            echo "Build a reproducible Docker image for the WRFCoin native-chain node."
            echo ""
            echo "Options:"
            echo "  --dry-run   Print commands without executing them"
            echo "  --help      Show this help message"
            echo ""
            echo "Environment variables:"
            echo "  IMAGE_REPO  Docker image repository (default: ghcr.io/wrfcoin/native-chain)"
            exit 0
            ;;
        *)
            echo "Unknown argument: $arg" >&2
            echo "Run with --help for usage." >&2
            exit 1
            ;;
    esac
done

# ---- Gather build metadata ---------------------------------------------------

cd "$REPO_ROOT"

VERSION="$(cat VERSION 2>/dev/null || echo 'dev')"
COMMIT_SHA="$(git rev-parse HEAD 2>/dev/null || echo 'unknown')"
SHORT_SHA="$(git rev-parse --short=8 HEAD 2>/dev/null || echo 'unknown')"
BUILD_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

TAG_VERSION="${IMAGE_REPO}:${VERSION}"
TAG_SHA="${IMAGE_REPO}:${VERSION}-${SHORT_SHA}"
TAG_LATEST="${IMAGE_REPO}:latest"

echo "=== WRFCoin Native-Chain Release Build ==="
echo "  Version:    ${VERSION}"
echo "  Commit:     ${COMMIT_SHA}"
echo "  Date:       ${BUILD_DATE}"
echo "  Dockerfile: ${DOCKERFILE}"
echo "  Tags:"
echo "    - ${TAG_VERSION}"
echo "    - ${TAG_SHA}"
echo "    - ${TAG_LATEST}"
echo ""

# ---- Helper ------------------------------------------------------------------

run() {
    if [ "$DRY_RUN" = true ]; then
        echo "[dry-run] $*"
    else
        echo "$ $*"
        "$@"
    fi
}

# ---- Step 1: Build Docker image ---------------------------------------------

echo "--- Building Docker image ---"
run docker build \
    -f "${DOCKERFILE}" \
    --build-arg "COMMIT_SHA=${COMMIT_SHA}" \
    --build-arg "BUILD_DATE=${BUILD_DATE}" \
    --build-arg "VERSION=${VERSION}" \
    -t "${TAG_VERSION}" \
    -t "${TAG_SHA}" \
    -t "${TAG_LATEST}" \
    .

# ---- Step 2: Extract binary and generate checksums --------------------------

echo ""
echo "--- Extracting binary ---"

OUTDIR="${REPO_ROOT}/target/release"
run mkdir -p "${OUTDIR}"

if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] docker create ${TAG_VERSION} → container_id"
    echo "[dry-run] docker cp container_id:/usr/local/bin/wrfcoin-node ${OUTDIR}/launch_api"
    echo "[dry-run] docker rm container_id"
else
    CONTAINER_ID=$(docker create "${TAG_VERSION}")
    docker cp "${CONTAINER_ID}:/usr/local/bin/wrfcoin-node" "${OUTDIR}/launch_api"
    docker rm "${CONTAINER_ID}" > /dev/null
fi

echo ""
echo "--- Generating checksums ---"

CHECKSUMS="${OUTDIR}/checksums.txt"

if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] sha256sum ${OUTDIR}/launch_api > ${CHECKSUMS}"
else
    (cd "${OUTDIR}" && sha256sum launch_api > checksums.txt)
    echo "Checksums written to ${CHECKSUMS}:"
    cat "${CHECKSUMS}"
fi

# ---- Summary -----------------------------------------------------------------

echo ""
echo "=== Build complete ==="
echo "  Image:     ${TAG_VERSION}"
echo "  Binary:    ${OUTDIR}/launch_api"
echo "  Checksums: ${CHECKSUMS}"
echo ""
echo "  Peer identity will persist at \$WRFCOIN_DATA_DIR/peer_identity.key"
echo "  inside the container at /app/data/peer_identity.key"
