# Setup & Configuration Guide

This guide covers deployment, environment configuration, reverse proxy setup, and initial administrator bootstrap for BingeSync.

---

## Docker Compose

The recommended way to run BingeSync is via Docker Compose:

```yaml
services:
  bingesync:
    image: bingesync:latest
    build: .
    container_name: bingesync
    restart: unless-stopped
    ports:
      - "8080:8080"
    environment:
      - HOST=0.0.0.0
      - PORT=8080
      - APP_URL=http://localhost:8080
      - DATABASE_URL=sqlite:///data/bingesync.db?mode=rwc
      - BACKUP_DIR=/backup
      - JWT_SECRET=change-this-to-a-secure-random-string
      - DEV_LOGIN_ENABLED=true
      # - OIDC_ENABLED=false
      # - METADATA_SOURCE=tvdb
    volumes:
      - bingesync_data:/data
      - bingesync_backups:/backup

volumes:
  bingesync_data:
    driver: local
  bingesync_backups:
    driver: local
```

Start the container:

```bash
docker compose up -d
```

Access the web interface at `http://localhost:8080`.

---

## Reverse Proxy and HTTPS

When deploying behind a reverse proxy (e.g. Nginx, Caddy, Traefik, NPM, or Cloudflare), set `APP_URL` to your full public HTTPS URL (e.g. `https://bingesync.example.com`).

### Nginx

```nginx
server {
    listen 443 ssl http2;
    server_name bingesync.example.com;

    ssl_certificate /path/to/fullchain.pem;
    ssl_certificate_key /path/to/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket / SSE support (for live logs)
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

### Caddy

```caddy
bingesync.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

### Subpath Deployment (`BASE_PATH`)

If hosting under a subpath (e.g. `https://myhome.net/bingesync`):
1. Set `BASE_PATH=/bingesync` in your environment.
2. Set `APP_URL=https://myhome.net/bingesync`.
3. Configure your proxy to pass the request retaining the `/bingesync` prefix:

```nginx
location /bingesync/ {
    proxy_pass http://127.0.0.1:8080/bingesync/;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

---

## Environment Variables Reference

| Variable | Default | Description |
|---|---|---|
| `PORT` | `8080` | Internal server listen port |
| `HOST` | `0.0.0.0` | Listen host IP |
| `APP_URL` | `http://localhost:8080` | Canonical public base URL for webhook links, OIDC redirects, and writeback |
| `BASE_PATH` | `""` | Optional subpath for reverse proxies (e.g. `/bingesync`) |
| `DATABASE_URL` | `sqlite:///data/bingesync.db?mode=rwc` | SQLite database connection string (automatically enables WAL mode & busy timeout) |
| `BACKUP_DIR` | `/backup` | Directory where SQLite database snapshots are created (7-day retention) |
| `JWT_SECRET` | *(built-in dev secret)* | Secret used to sign session JWTs. Must be set in production. |
| `DEV_LOGIN_ENABLED` | `true` | Enables local username/email login (useful for standalone deployments or development) |
| `OIDC_ENABLED` | `false` | Enable/disable OpenID Connect SSO integration |
| `OIDC_ISSUER_URL` | *(none)* | Discovery endpoint of your OIDC Provider (e.g. `https://auth.example.com`) |
| `OIDC_CLIENT_ID` | *(none)* | OIDC client ID registered with your provider |
| `OIDC_CLIENT_SECRET` | *(none)* | OIDC client secret |
| `OIDC_REDIRECT_URL` | `${APP_URL}/api/v1/auth/callback` | OAuth authorization code callback URL |
| `OIDC_ADMIN_GROUP` | *(none)* | Comma-separated group name(s) granting the `admin` role (e.g. `admins, sysadmins`) |
| `OIDC_USER_GROUP` | *(none)* | Comma-separated group name(s) required for standard users (restricts non-admins) |
| `METADATA_SOURCE` | `tvdb` | Default metadata enrichment provider (`tvdb` or `tmdb`) |
| `TVDB_API_KEY` | *(none)* | TheTVDB v4 API key for plot overviews, actors, genres, and directors |
| `TVDB_PIN` | *(none)* | Optional subscriber PIN for user-subscription TheTVDB keys |
| `TVDB_BASE_URL` | `https://api4.thetvdb.com/v4` | TheTVDB v4 API base URL |
| `TMDB_API_KEY` | *(none)* | The Movie Database (TMDB) API Key or Read Access Token |
| `TMDB_BASE_URL` | `https://api.themoviedb.org/3` | TMDB API base URL |
| `SCROBBLE_THRESHOLD_PERCENT` | `80` | Percentage of playback duration required to register as completed (default: 80%) |
| `IMAGE_CACHE_DIR` | `/data/images` | Local filesystem directory where poster artwork is cached |
| `TAUTULLI_URL` | *(none)* | Global base URL for Tautulli instance (can also be set per-user) |
| `TAUTULLI_API_KEY` | *(none)* | Global API key for Tautulli instance (can also be set per-user) |

---

## First-Run Setup Flow

When launching BingeSync for the first time without pre-configured users or OIDC credentials, navigating to the web UI will present a guided 3-step setup flow:

1. **Step 1: OpenID Connect Discovery**: Enter your IdP Issuer URL (e.g. `https://authentik.company.com/application/o/bingesync/`) and click **Test Discovery**. BingeSync validates the OpenID configuration document in real-time.
2. **Step 2: Client Registration**: Enter your Client ID and Client Secret, and copy the provided **Redirect URI** (`${APP_URL}/api/v1/auth/callback`) and **Back-Channel Logout URL** (`${APP_URL}/api/v1/auth/backchannel-logout`) to paste into your IdP client settings.
3. **Step 3: Review & Admin Promotion**: Specify optional admin groups. The very first user to log into the fresh system is automatically assigned the `admin` role, ensuring you are never locked out. Once completed, the setup flow locks down permanently (`403 Forbidden`).

