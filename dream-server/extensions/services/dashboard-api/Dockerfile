# Dream Server Dashboard API — Rust/Axum
# Lightweight system status backend for the Dashboard UI
#
# Multi-stage build: compile in a full Rust image, copy the binary into a
# minimal runtime image.  Final image is ~30 MB (vs ~350 MB for Python).

# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.86-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency builds: copy manifests first, create stub sources, build deps
COPY Cargo.toml Cargo.lock ./
COPY crates/dream-common/Cargo.toml crates/dream-common/Cargo.toml
COPY crates/dashboard-api/Cargo.toml crates/dashboard-api/Cargo.toml
COPY crates/dream-scripts/Cargo.toml crates/dream-scripts/Cargo.toml

RUN mkdir -p crates/dream-common/src crates/dashboard-api/src crates/dream-scripts/src \
    && echo "pub mod error; pub mod manifest; pub mod models;" > crates/dream-common/src/lib.rs \
    && echo "fn main() {}" > crates/dashboard-api/src/main.rs \
    && echo "fn main() {}" > crates/dream-scripts/src/main.rs \
    && echo "" > crates/dream-scripts/src/lib.rs \
    && cargo build --release --workspace 2>/dev/null || true

# Copy actual source and build for real
COPY crates/ crates/
# Touch sources so cargo sees them as newer than the stubs
RUN find crates -name "*.rs" -exec touch {} + \
    && cargo build --release --workspace

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim

LABEL org.opencontainers.image.source="https://github.com/Light-Heart-Labs/DreamServer"
LABEL org.opencontainers.image.description="Dream Server Dashboard API (Rust)"

# Runtime deps: curl for healthcheck, ca-certificates for HTTPS
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Non-root user (matches Python image)
RUN useradd -m -u 1000 dreamer

# Create /data directory for API key generation (B1 fix parity)
RUN mkdir -p /data && chown dreamer:dreamer /data

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /build/target/release/dashboard-api /app/dashboard-api
COPY --from=builder /build/target/release/dream-scripts /app/dream-scripts

USER dreamer

ENV DASHBOARD_API_PORT=3002

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:${DASHBOARD_API_PORT}/health || exit 1

EXPOSE ${DASHBOARD_API_PORT}

CMD ["/app/dashboard-api"]
