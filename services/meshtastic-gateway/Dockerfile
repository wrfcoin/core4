# Meshtastic Gateway Service Dockerfile
# WRFCoin LoRa-Meshtastic Integration
#
# Multi-stage build: install deps in builder, copy to minimal runtime image.
# Base image pinned to sha256 digest to prevent supply-chain drift.

# ── Stage 1: dependency installer ────────────────────────────────────────────
FROM python:3.11-slim@sha256:9358444059ed78e2975ada2c189f1c1a3144a5dab6f35bff8c981afb38946634 AS builder

WORKDIR /install

# Install runtime dependencies only (exclude dev/test tools)
COPY requirements.txt .
RUN pip install --no-cache-dir --prefix=/install/deps -r requirements.txt

# ── Stage 2: minimal runtime image ───────────────────────────────────────────
FROM python:3.11-slim@sha256:9358444059ed78e2975ada2c189f1c1a3144a5dab6f35bff8c981afb38946634

# Security: Run as non-root user
RUN groupadd -r gateway && useradd -r -g gateway gateway

# Install curl (required for HEALTHCHECK only — no build tools)
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy pre-built Python packages from builder stage
COPY --from=builder /install/deps /usr/local

# Copy application code
COPY src/ ./src/

# Set ownership
RUN chown -R gateway:gateway /app

# Switch to non-root user
USER gateway

# Environment variables with defaults
ENV MESHTASTIC_GW_HOST=0.0.0.0
ENV MESHTASTIC_GW_PORT=8001
ENV MESHTASTIC_GW_LOG_LEVEL=INFO
ENV MESHTASTIC_GW_MESHTASTIC_MODE=mock

# Expose port
EXPOSE 8001

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8001/health || exit 1

# Run the application
CMD ["python", "-m", "uvicorn", "src.gateway.main:app", "--host", "0.0.0.0", "--port", "8001"]
