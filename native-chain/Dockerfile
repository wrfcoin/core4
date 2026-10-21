# Dockerfile for WRFCoin Native Chain Node (launch_api)
# Multi-stage build for a minimal runtime image

# Build arguments for provenance tracking
ARG COMMIT_SHA=unknown
ARG BUILD_DATE=unknown
ARG VERSION=dev

# =============================================================================
# Stage 1: Builder
# =============================================================================
FROM rust:1.98.0-slim AS builder

ARG COMMIT_SHA
ARG BUILD_DATE
ARG VERSION

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    git \
    make \
    g++ \
    && rm -rf /var/lib/apt/lists/*

# Copy the entire workspace to get all members
COPY . .

# Build the launch_api binary with cargo registry and build caches.
# --locked ensures Cargo.lock is respected for reproducible dependency resolution.
#
# The wrfcoin operator CLI builds in the same invocation (gate 9, #2804):
# packaging it here — instead of hand-copying a workstation binary onto the node container —
# is what makes the deployed CLI's provenance the image digest rather than an
# operator's memory. Same layer so the two binaries can never come from
# different source trees.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked \
        -p wrfcoin-native-chain --bin launch_api \
        --features "full-mining,p2p" \
    && cargo build --release --locked \
        -p wrfcoin_cli --bin wrfcoin \
    && cp /build/target/release/launch_api /build/launch_api \
    && cp /build/target/release/wrfcoin /build/wrfcoin

# =============================================================================
# Stage 2: Runtime
# =============================================================================
FROM debian:bookworm-slim AS runtime

ARG COMMIT_SHA
ARG BUILD_DATE
ARG VERSION

# OCI image labels for provenance
LABEL org.opencontainers.image.title="wrfcoin-native-chain"
LABEL org.opencontainers.image.description="WRFCoin blockchain node (launch_api)"
LABEL org.opencontainers.image.version="${VERSION}"
LABEL org.opencontainers.image.revision="${COMMIT_SHA}"
LABEL org.opencontainers.image.created="${BUILD_DATE}"
LABEL org.opencontainers.image.source="https://github.com/wrfcoin/core4"
LABEL org.opencontainers.image.vendor="WRFCoin"

WORKDIR /app

# Install runtime dependencies (SSL and curl for health checks)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binaries from the builder stage. The operator CLI ships
# in the same image (gate 9): `docker exec <node> wrfcoin ...` on the node container, or
# copy it out with `docker cp` — either way its provenance is this image's
# digest, not a hand-installed workstation build.
COPY --from=builder /build/launch_api /usr/local/bin/wrfcoin-node
COPY --from=builder /build/wrfcoin /usr/local/bin/wrfcoin

# Create data and logs directories
RUN mkdir -p /app/data /app/logs /app/config

# Create a non-root user for security
RUN groupadd -g 1001 wrfcoin && \
    useradd -m -u 1001 -g wrfcoin wrfcoin && \
    chown -R wrfcoin:wrfcoin /app

USER wrfcoin

# Default environment variables
ENV WRFCOIN_API_HOST=0.0.0.0
ENV WRFCOIN_API_PORT=8556
ENV WRFCOIN_DATA_DIR=/app/data
ENV RUST_LOG=info

# Expose API and P2P ports
EXPOSE 8556
EXPOSE 9000

# Health check using the metrics endpoint
HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8556/metrics || exit 1

ENTRYPOINT ["/usr/local/bin/wrfcoin-node"]
