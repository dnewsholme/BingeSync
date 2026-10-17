# 🎬 BingeSync

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![SQLite](https://img.shields.io/badge/SQLite-WAL%20mode-blue.svg)](https://www.sqlite.org/)
[![Docker](https://img.shields.io/badge/Docker-Ready-2496ED.svg)](https://www.docker.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

**BingeSync** is a self-hosted web service that replicates **Trakt.tv**. It automatically scrobbles what you watch using **Plex Webhooks**, recording a persistent history in a lightweight SQLite database and presenting your media in an interactive timeline, searchable history, and statistics dashboard.

Designed for self-hosters and families, BingeSync features **multi-user support** with dedicated per-user webhook tokens, full **OpenID Connect (OIDC)** single sign-on support, a clean **REST API**, comprehensive **unit & Playwright UI tests**, and is delivered as a lightweight **Docker image**.

---

## ✨ Features

- 🔄 **Real-Time Plex Scrobbling**: Supports standard Plex `multipart/form-data` and JSON webhook payloads. Automatically captures `media.scrobble`, `media.play`, `media.pause`, `media.resume`, and `media.stop` events.
- 👥 **Multi-User Isolation**: Every user receives their own isolated watch history and a unique Plex Webhook URL.
- 📅 **Interactive Watch Timeline**: Grouped daily records with date filtering, genre filtering, director/actor search, star rating filter, and one-click daily watch history deletion.
- 🔍 **Search & Filters**: Instant full-text search across movie and TV show titles, with multi-select batch deletion.
- 📺 **Show-Wide Deletion**: One-click action on show detail pages to purge all logged scrobbles for that series.
- 🌐 **IMDb Deep Links**: Direct links from scrobble cards and details modal to title pages on IMDb.
- 🧹 **Radarr & Sonarr Library Integration**: Detects whether completed movies or series exist in your Radarr/Sonarr libraries, with an optional one-click cleanup to remove and delete media files from disk after binging.
- 📊 **Binge Statistics & Visual Charts**: Interactive charts featuring a 14-day stacked activity bar chart, top genres distribution, TV vs Movie time split, and top TV series.
- 📥 **Trakt.tv Import**: Import past watch history from Trakt.tv JSON and CSV exports, preserving exact dates, timestamps, and 1-10 star ratings.
- 🔀 **Configurable Subpath Support**: Seamlessly host behind reverse proxies on a subpath (e.g. `www.example.com/bingesync`).
- 📺 **Now Playing Banner**: Live playback beacon tracking active media and playback offset in real-time.
- 🔐 **OpenID Connect (OIDC) SSO**: Enterprise-grade authentication compatible with Authentik, Keycloak, Authelia, Okta, and Google (with PKCE RFC 7636).
- ⚡ **Lightweight Single-Binary**: Written in Rust (`axum`, `tokio`, `sqlx`) with the web frontend embedded directly into the binary via `rust-embed`.
- 🗄️ **Zero-Maintenance SQLite**: Fast, crash-safe embedded database running with Write-Ahead Logging (`WAL`) mode and automatic schema migrations.
- ☸️ **Kubernetes Ready**: Complete manifest (`bingesync.yaml`) with Longhorn PersistentVolumeClaim integration, Deployment, Service, and Ingress.
- 🧪 **Thoroughly Tested**: 13 Rust backend integration tests and 7 Playwright browser E2E tests.

---

## 🚀 Quick Start with Docker Compose

1. Create a `docker-compose.yml` file:

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
      - BASE_PATH=
      - DATABASE_URL=sqlite:///data/bingesync.db?mode=rwc
      - JWT_SECRET=change-this-to-a-very-secure-random-secret-string
      - OIDC_ENABLED=false
      - DEV_LOGIN_ENABLED=true
    volumes:
      - bingesync_data:/data

volumes:
  bingesync_data:
    driver: local
```

2. Start the container:

```bash
docker compose up -d
```

3. Open your browser at `http://localhost:8080`.

---

## ☸️ Kubernetes Deployment

Deploy BingeSync to Kubernetes using the included [`bingesync.yaml`](bingesync.yaml):

```bash
kubectl apply -f bingesync.yaml
```

The manifest includes a permission-initializing initContainer for Longhorn storage, a single-replica deployment with SQLite WAL mode, ClusterIP service, secret, and an ingress with Traefik TLS and subpath support.

---

## 🔐 OpenID Connect (OIDC) & Administration

BingeSync supports full Single Sign-On (SSO) with PKCE (RFC 7636) and OpenID Connect Back-Channel Logout 1.0.

### IdP Configuration URLs
Configure the following URLs in your Identity Provider (Authentik, PocketID, Keycloak, Authelia, Okta):

| Setting / Field | Value | Description |
|---|---|---|
| **Redirect / Callback URI** | `${APP_URL}/api/v1/auth/callback` | OAuth2 callback where the IdP sends the user after login |
| **Post-Logout Redirect URI** | `${APP_URL}/` | URL where the user's browser is returned after logout |
| **Back-Channel Logout URL** | `${APP_URL}/api/v1/auth/backchannel-logout` | Server-to-server endpoint that receives `logout_token` when user access is revoked |

*(Note: Alias `${APP_URL}/api/v1/auth/backchannel_logout` is also supported).*

### Groups & Role Assignment
* BingeSync requests the `openid email profile groups` scopes during authorization.
* If a user belongs to an `admin`, `admins`, `administrator`, or `administrators` group in your IdP (evaluated from `groups` or `roles` claims), BingeSync automatically assigns them the **Admin** role upon login.
* Standard users receive the **User** role.

### Admin User Management & Cascading Deletion
* Users with the Admin role can access the dedicated **Users** tab in the top navigation bar.
* Admins can inspect all registered users, their join date, scrobble count, and role badge.
* Admins can permanently delete user accounts. Deleting a user initiates an atomic transaction that cascade-deletes all of their logged scrobbles, active playback sessions, and database records.
* Self-deletion is prevented to ensure active administrative access remains.

### Back-Channel Logout & Immediate Revocation
* When an IdP revokes user access (e.g. user is disabled, deleted, or removed from groups), it issues a signed JWT `logout_token` to `/api/v1/auth/backchannel-logout`.
* BingeSync verifies the token per OpenID Connect Back-Channel Logout 1.0, purges active playback sessions, and invalidates all existing user session tokens immediately (`401 Unauthorized`).

---

## 🔧 Environment Variables

All configuration options can be passed via environment variables:

| Variable | Default | Description |
|---|---|---|
| `PORT` | `8080` | Port for the HTTP server |
| `HOST` | `0.0.0.0` | Listen IP address (`0.0.0.0` for Docker) |
| `APP_URL` | `http://localhost:8080` | External base URL used to construct webhook URLs and OIDC callbacks |
| `BASE_PATH` | `""` | Optional subpath for reverse proxies (e.g. `/bingesync`) |
| `DATABASE_URL` | `sqlite:///data/bingesync.db?mode=rwc` | SQLite database URI with WAL mode |
| `JWT_SECRET` | *(built-in dev secret)* | Secret key for signing session tokens |
| `OIDC_ENABLED` | `false` | Enable OpenID Connect authentication |
| `OIDC_ISSUER_URL` | *(none)* | OIDC discovery base URL (e.g. `https://auth.example.com`) |
| `OIDC_CLIENT_ID` | *(none)* | OIDC Client ID registered with your identity provider |
| `OIDC_CLIENT_SECRET` | *(none)* | OIDC Client Secret (optional for public clients) |
| `OIDC_REDIRECT_URL` | `${APP_URL}/api/v1/auth/callback` | Callback redirect URL |
| `DEV_LOGIN_ENABLED` | `true` | Allows local email/username sign-in |
| `RADARR_URL` | *(none)* | Base URL of Radarr instance |
| `RADARR_API_KEY` | *(none)* | Radarr API key |
| `SONARR_URL` | *(none)* | Base URL of Sonarr instance |
| `SONARR_API_KEY` | *(none)* | Sonarr API key |

---

## 🔌 Connecting Plex Webhooks

1. Log in to your BingeSync dashboard.
2. Click on the **Webhook** tab in the navigation bar.
3. Click **Copy URL** to copy your unique webhook endpoint:
   ```
   http://<bingesync-ip>:8080/api/v1/webhooks/plex/<your_unique_token>
   ```
4. In Plex Web:
   - Click the **Wrench (Settings)** icon in the upper-right corner.
   - Under **Manage** in the left sidebar, click **Webhooks**.
   - Click **Add Webhook**.
   - Paste your BingeSync webhook URL into the URL field.
   - Click **Save Changes**.

---

## 📡 REST API Reference

BingeSync provides a comprehensive REST API:

### Authentication
- `GET  /api/v1/auth/status` — Returns OIDC and dev login availability and the Back-Channel Logout URL.
- `GET  /api/v1/auth/login` — Initiates OIDC authorization flow.
- `GET  /api/v1/auth/callback` — Handles authorization code exchange, evaluates `groups` claim for admin role, and sets session cookie.
- `POST /api/v1/auth/dev-login` — Logs in with `{ "email": "...", "display_name": "...", "role": "..." }`.
- `GET  /api/v1/auth/me` — Returns the authenticated user profile and webhook URL.
- `POST /api/v1/auth/logout` — Clears authentication cookie.
- `POST /api/v1/auth/backchannel-logout` — OpenID Connect Back-Channel Logout 1.0 endpoint. Receives `logout_token` when user access is revoked, terminated, or group membership is removed by the IdP.

### Administration
- `GET    /api/v1/admin/users` — Lists all registered users with scrobble counts and roles (requires admin role).
- `DELETE /api/v1/admin/users/:id` — Cascading delete for a user account, including all their scrobbles and active sessions (requires admin role).

### Webhooks
- `POST /api/v1/webhooks/plex/:token` — Handles Plex webhook events (multipart or JSON).
- `POST /api/v1/webhooks/plex?token=:token` — Query-parameter alternative.

### History & Deletion
- `GET    /api/v1/history` — Paginated scrobble history with query parameters for `page`, `limit`, `media_type`, `search`, `date`, `genre`, `person`, `rating`, `watch_status`.
- `POST   /api/v1/history` — Manually record a scrobble.
- `DELETE /api/v1/history/:id` — Delete a single scrobble.
- `DELETE /api/v1/history/by-show?show=:title` — Delete all scrobbles for a TV show.
- `POST   /api/v1/history/delete-batch` — Batch delete multiple scrobbles with `{ "ids": [...] }`.
- `DELETE /api/v1/history/by-date?date=:yyyy-mm-dd` — Delete all scrobbles for a specific day.

### Timeline & Stats
- `GET /api/v1/timeline` — Grouped daily timeline entries with filters (`search`, `date`, `genre`, `person`, `rating`, `watch_status`).
- `GET /api/v1/stats` — Overall watch metrics, TV vs movie breakdown, 14-day activity, top genres, and top TV shows.
- `GET /api/v1/now-playing` — Active playback state and progress offset.

### Radarr & Sonarr Integrations
- `GET  /api/v1/integrations/status` — Configuration status of Radarr and Sonarr.
- `GET  /api/v1/integrations/check?title=...&media_type=...` — Check if a movie or series exists in Radarr or Sonarr.
- `POST /api/v1/integrations/radarr/delete` — Delete movie and disk files from Radarr.
- `POST /api/v1/integrations/sonarr/delete-series` — Delete TV series and disk files from Sonarr.

### Trakt Import
- `POST /api/v1/import/trakt` — Import watch history and ratings from Trakt.tv JSON or CSV export files.

### User Settings
- `GET  /api/v1/user/webhook` — Retrieve current webhook token and URL.
- `POST /api/v1/user/regenerate-token` — Invalidate previous token and generate a new one.

---

## 🛠️ Local Development & Testing

### Prerequisites
- Rust 1.80+ (`cargo`)
- Node.js 20+ & `npm`

### Running the Backend
```bash
cargo run
```

### Running Rust Integration Tests
```bash
cargo test
```

### Running Playwright UI Tests
```bash
cd web
npm install
npx playwright install chromium
npm run test:e2e
```

---

## 📄 License

MIT License. See [LICENSE](LICENSE) for details.
