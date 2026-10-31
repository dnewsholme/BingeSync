# ==========================================
# Stage 1: Build Web Frontend Assets
# ==========================================
FROM node:22-alpine AS frontend-builder
WORKDIR /app/web

# Copy package definitions and install dependencies
COPY web/package.json ./
RUN npm install

# Copy web source and build production bundle
COPY web/ ./
RUN npm run build

# ==========================================
# Stage 2: Build Rust Backend Binary
# ==========================================
FROM rust:slim AS backend-builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    curl \
    build-essential \
    clang \
    && rm -rf /var/lib/apt/lists/*

# Install latest SQLite 3.5.x (3.54.1) with full feature support
RUN curl -sSL https://www.sqlite.org/2026/sqlite-autoconf-3540100.tar.gz | tar -xz \
    && cd sqlite-autoconf-3540100 \
    && CFLAGS="-DSQLITE_ENABLE_COLUMN_METADATA=1 -DSQLITE_ENABLE_UNLOCK_NOTIFY=1 -DSQLITE_ENABLE_FTS5=1 -DSQLITE_ENABLE_RTREE=1 -DSQLITE_ENABLE_DBSTAT_VTAB=1" ./configure --prefix=/usr --disable-static \
    && make -j$(nproc) \
    && make install \
    && ldconfig \
    && cd .. \
    && rm -rf sqlite-autoconf-3540100

# Copy web/dist from frontend builder
COPY --from=frontend-builder /app/web/dist ./web/dist

# Copy Cargo configuration and sources
COPY Cargo.toml Cargo.lock build.rs ./
COPY src/ ./src/

ARG APP_VERSION=0.1.0
ENV BINGESYNC_VERSION=$APP_VERSION

# Compile optimized release binary
RUN cargo build --release

# ==========================================
# Stage 3: Minimal Runtime Image
# ==========================================
FROM debian:trixie-slim AS runtime

# Install minimal runtime dependencies (SSL certificates, curl for healthcheck)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy SQLite 3.5.x libraries and CLI from backend-builder
COPY --from=backend-builder /usr/lib/libsqlite3.so* /usr/lib/
COPY --from=backend-builder /usr/bin/sqlite3 /usr/bin/sqlite3
RUN ldconfig

# Create application user and data storage directory
RUN useradd -m -u 1000 -U -s /bin/sh bingesync && \
    mkdir -p /data /backup && \
    chown -R bingesync:bingesync /data /backup

WORKDIR /app

# Copy binary from builder
COPY --from=backend-builder /app/target/release/bingesync /usr/local/bin/bingesync

# Set permissions
RUN chmod +x /usr/local/bin/bingesync

# Default Environment Variables
ENV HOST=0.0.0.0 \
    PORT=8080 \
    DATABASE_URL=sqlite:///data/bingesync.db?mode=rwc \
    BACKUP_DIR=/backup \
    APP_URL=http://localhost:8080 \
    OIDC_ENABLED=false \
    DEV_LOGIN_ENABLED=true \
    DOCKER_BASE_IMAGE="debian:trixie-slim"

# Storage volumes for SQLite database and database backups
VOLUME ["/data", "/backup"]

# Expose default HTTP port
EXPOSE 8080

# Switch to non-root user
USER bingesync

# Healthcheck
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8080/api/v1/auth/status || exit 1

ENTRYPOINT ["/usr/local/bin/bingesync"]
