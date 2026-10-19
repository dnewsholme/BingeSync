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
    && rm -rf /var/lib/apt/lists/*

# Copy web/dist from frontend builder
COPY --from=frontend-builder /app/web/dist ./web/dist

# Copy Cargo configuration and sources
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# Compile optimized release binary
RUN cargo build --release

# ==========================================
# Stage 3: Minimal Runtime Image
# ==========================================
FROM debian:bookworm-slim AS runtime

# Install minimal runtime dependencies (SSL certificates, curl for healthcheck)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    sqlite3 \
    && rm -rf /var/lib/apt/lists/*

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
    DEV_LOGIN_ENABLED=true

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
