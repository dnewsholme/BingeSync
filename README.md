# 🎬 BingeSync

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![SQLite](https://img.shields.io/badge/SQLite-WAL%20mode-blue.svg)](https://www.sqlite.org/)
[![Docker](https://img.shields.io/badge/Docker-Ready-2496ED.svg)](https://www.docker.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

**BingeSync** is a self-hosted web service that replicates **Trakt.tv**. It automatically scrobbles what you watch using **Plex Webhooks**, recording a persistent history in a lightweight SQLite database and presenting your media in an interactive timeline, searchable history, and statistics dashboard.

Designed for self-hosters and families, BingeSync features **multi-user support** with dedicated per-user webhook tokens, full **OpenID Connect (OIDC)** single sign-on support, a clean **REST API**, comprehensive **unit & Playwright UI tests**, and is delivered as a lightweight **Docker image**.

---

## ✨ Features

- 🔄 **Real-Time Plex & Jellyfin Scrobbling**: Supports standard Plex (`multipart/form-data` and JSON) and Jellyfin Webhook payloads. Automatically captures playback progress, pause/resume, and completed watch events.
- 👥 **Multi-User Isolation**: Every user receives their own isolated watch history and unique Plex and Jellyfin Webhook URLs.
- 🌐 **Multilingual Support (i18n)**: Full native internationalization supporting English, Spanish (Español), German (Deutsch), and French (Français) with instant in-app switching and persistent preferences.
- 📅 **Interactive Watch Timeline**: Grouped daily records with date filtering, genre filtering, director/actor search, star rating filter, and one-click daily watch history deletion.
- 🔍 **Search & Filters**: Instant full-text search across movie and TV show titles with an option to search across all history regardless of date, plus multi-select batch deletion.
- 📺 **Show-Wide Deletion**: One-click action on show detail pages to purge all logged scrobbles for that series.
- 🌐 **IMDb Deep Links**: Direct links from scrobble cards and details modal to title pages on IMDb.
- 🧹 **Radarr & Sonarr Library Integration**: Detects whether completed movies or series exist in your Radarr/Sonarr libraries, with an optional one-click cleanup to remove and delete media files from disk after binging.
- 📊 **Binge Statistics & Visual Charts**: Comprehensive interactive charts featuring a 14-day stacked activity bar chart, top genres distribution, TV vs Movie split, top TV series, top watched movies, watch streaks, weekly pace, rating averages, peak viewing day, time-of-day/day-of-week breakdown, and dynamic media type and time range filters (7D, 30D, 90D, 1Y, All Time, Custom).
- ⭐ **Star Ratings & Reviews**: Rate movies and episodes from 1 to 10 directly in the detail modal or during manual logging, with star filters and instant unrating.
- 📥 **Trakt.tv ZIP & Export Import**: Direct import of Trakt.tv multi-file `.zip` export archives, JSON, and CSV exports. Automatically aggregates paginated history and rating files, matches ratings to scrobbles, and sorts all entries chronologically.
- 📜 **Tautulli Playback Import**: Connect directly to your Tautulli instance or paste/upload JSON/CSV exports. Choose from discovered users (defaulting to your OIDC `preferred_username`) with robust 3-way deduplication against existing records.
- 📖 **Interactive Swagger UI & OpenAPI 3.0**: Built-in dark mode Swagger documentation accessible at `/docs` or `/swagger` with full specification at `/api/v1/openapi.json`.
- ⚙️ **Admin Control Panel, Logs & Background Tasks**: Manage users, configure system API keys/thresholds in-app, inspect live system logs, and trigger background workers on demand.
- 🖼️ **Automated Disk-Based Poster Caching**: Background worker automatically searches, downloads, and caches official movie and TV posters to disk (never in SQLite) for optimal database performance and instant visual loading.
- 🎬 **TheTVDB Metadata Enrichment**: Background worker connects to TheTVDB v4 API to automatically populate genres, director credits, cast/actors, and plot overviews for scrobbles with missing metadata.
- 🔁 **Item Detail & Other Watches History**: Inspecting any scrobble in History or Timeline details displays a comprehensive table of all other watches of that exact same item (for TV shows, strictly scoped to the exact same episode).
- 🔀 **Configurable Subpath Support**: Seamlessly host behind reverse proxies on a subpath (e.g. `www.example.com/bingesync`).
- 📺 **Now Playing Banner**: Live playback beacon tracking active media and playback offset in real-time.
- 🔐 **OpenID Connect (OIDC) SSO**: Enterprise-grade authentication compatible with Authentik, Keycloak, Authelia, Okta, and Google (with PKCE RFC 7636).
- ⚡ **Lightweight Single-Binary**: Written in Rust (`axum`, `tokio`, `sqlx`) with the web frontend embedded directly into the binary via `rust-embed`.
- 🗄️ **Zero-Maintenance SQLite**: Fast, crash-safe embedded database running with Write-Ahead Logging (`WAL`) mode and automatic schema migrations.
- 🧪 **Thoroughly Tested**: 31 Rust backend integration tests and 24 Playwright browser E2E tests.

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
| `TVDB_API_KEY` | *(none)* | TheTVDB v4 API key for background metadata enrichment |
| `TVDB_PIN` | *(none)* | Optional subscriber PIN for user-subscription TheTVDB API keys |
| `TVDB_BASE_URL` | `https://api4.thetvdb.com/v4` | TheTVDB v4 API base URL |
| `SCROBBLE_THRESHOLD_PERCENT` | `80` | Percentage of media playback required to mark as watched |
| `IMAGE_CACHE_DIR` | `/data/images` | Directory path on disk where media poster images are cached |
| `TAUTULLI_URL` | *(none)* | Base URL of Tautulli instance for playback history import |
| `TAUTULLI_API_KEY` | *(none)* | API key for Tautulli instance |

---

## 🔌 Connecting Media Servers

### Plex Webhooks
1. Log in to your BingeSync dashboard.
2. Click on the **Settings** tab in the navigation bar.
3. Click **Copy URL** under **Plex Webhook URL**:
   ```
   http://<bingesync-ip>:8080/api/v1/webhooks/plex/<your_unique_token>
   ```
4. In Plex Web:
   - Click the **Wrench (Settings)** icon in the upper-right corner.
   - Under **Manage** in the left sidebar, click **Webhooks**.
   - Click **Add Webhook**.
   - Paste your BingeSync webhook URL into the URL field.
   - Click **Save Changes**.

### Jellyfin Webhooks
1. Install the official **Webhook** plugin from the Jellyfin Dashboard (Catalog > Webhook).
2. Go to **Dashboard > Plugins > Webhook** and click **Add Generic Destination**.
3. Set Webhook Name to `BingeSync`.
4. Copy and paste your unique Jellyfin Webhook URL from BingeSync Settings:
   ```
   http://<bingesync-ip>:8080/api/v1/webhooks/jellyfin/<your_unique_token>
   ```
5. Check the following Notification Type checkboxes:
   - `Playback Start`
   - `Playback Progress`
   - `Playback Stop`
   - `User Data Saved`
6. Under **Item Types**, select `Movies` and `Episodes`.
7. Click **Save**.

---

## 📡 REST API Reference

BingeSync provides a comprehensive REST API:

### Authentication
- `GET  /api/v1/auth/status` — Returns OIDC and dev login availability and the Back-Channel Logout URL.
- `GET  /api/v1/auth/login` — Initiates OIDC authorization flow.
- `GET  /api/v1/auth/callback` — Handles authorization code exchange, evaluates `groups` claim for admin role, and sets session cookie.
- `POST /api/v1/auth/dev-login` — Logs in with `{ "email": "...", "display_name": "...", "role": "..." }`.
- `GET  /api/v1/auth/me` — Returns the authenticated user profile, Plex webhook URL, and Jellyfin webhook URL.
- `POST /api/v1/auth/logout` — Clears authentication cookie.
- `POST /api/v1/auth/backchannel-logout` — OpenID Connect Back-Channel Logout 1.0 endpoint. Receives `logout_token` when user access is revoked, terminated, or group membership is removed by the IdP.

### Administration & System Control
- `GET    /api/v1/admin/users` — Lists all registered users with scrobble counts and roles (requires admin role).
- `DELETE /api/v1/admin/users/:id` — Cascading delete for a user account, including all their scrobbles and active sessions (requires admin role).
- `GET    /api/v1/admin/settings` — Get system-wide administration configuration and defaults (requires admin role).
- `POST   /api/v1/admin/settings` — Update system-wide configuration keys (requires admin role).
- `GET    /api/v1/admin/logs` — View real-time in-memory application log buffer (requires admin role).
- `GET    /api/v1/admin/tasks` — List background workers, status, last run, and next scheduled execution (requires admin role).
- `POST   /api/v1/admin/tasks/:name/run` — Trigger background worker on demand (requires admin role).

### Webhooks
- `POST /api/v1/webhooks/plex/:token` — Handles Plex webhook events (multipart or JSON).
- `POST /api/v1/webhooks/plex?token=:token` — Plex query-parameter alternative.
- `POST /api/v1/webhooks/jellyfin/:token` — Handles Jellyfin webhook events (`PlaybackStart`, `PlaybackProgress`, `PlaybackStop`, `UserDataSaved`).
- `POST /api/v1/webhooks/jellyfin?token=:token` — Jellyfin query-parameter alternative.

### History & Deletion
- `GET    /api/v1/history` — Paginated scrobble history with query parameters for `page`, `limit`, `media_type`, `search`, `date`, `genre`, `person`, `rating`, `watch_status`, and `all` (search across all history regardless of date and return all matching results).
- `POST   /api/v1/history` — Manually record a scrobble.
- `DELETE /api/v1/history/:id` — Delete a single scrobble.
- `DELETE /api/v1/history/by-show?show=:title` — Delete all scrobbles for a TV show.
- `POST   /api/v1/history/delete-batch` — Batch delete multiple scrobbles with `{ "ids": [...] }`.
- `DELETE /api/v1/history/by-date?date=:yyyy-mm-dd` — Delete all scrobbles for a specific day.
- `POST   /api/v1/history/rate` — Rate a watched item (1-10) or unrate with `{ "scrobble_id": 123, "rating": 8 }`.
- `POST   /api/v1/history/refresh-metadata` — Re-fetch and enrich metadata for a specific scrobble from TheTVDB.
- `GET    /api/v1/history/other-watches?scrobble_id=...` — Retrieve other watches for the exact same movie or TV episode.

### Timeline & Stats
- `GET /api/v1/timeline` — Grouped daily timeline entries with filters (`search`, `date`, `genre`, `person`, `rating`, `watch_status`, and `all_dates`).
- `GET /api/v1/stats` — Overall watch metrics, TV vs movie breakdown, watch streaks, weekly pace, rating averages, peak viewing day, 14-day activity, top genres, top TV shows, top watched movies, and filterable by `media_type` and `time_range`.
- `GET /api/v1/now-playing` — Active playback state and progress offset.

### Radarr & Sonarr Integrations
- `GET  /api/v1/integrations/status` — Configuration status of Radarr and Sonarr.
- `GET  /api/v1/integrations/check?title=...&media_type=...` — Check if a movie or series exists in Radarr or Sonarr.
- `POST /api/v1/integrations/radarr/delete` — Delete movie and disk files from Radarr.
- `POST /api/v1/integrations/sonarr/delete-series` — Flexible Sonarr cleanup with three action modes (`action`):
  - `"delete_series"`: Delete show and all media files from Sonarr (`{ "series_id": 123, "action": "delete_series", "delete_files": true }`).
  - `"delete_season"`: Delete episode files for a specific season and unmonitor the season (`{ "series_id": 123, "action": "delete_season", "season_number": 2 }`).
  - `"delete_all_files_unmonitor"`: Delete all episode files across all seasons and unmonitor all seasons (`{ "series_id": 123, "action": "delete_all_files_unmonitor" }`).

### Trakt & Tautulli Import
- `POST /api/v1/import/trakt` — Import watch history and ratings from Trakt.tv multi-file `.zip` export archives, JSON, or CSV export files (automatically sorted chronologically).
- `GET  /api/v1/import/tautulli/users` — Discover and list users from connected Tautulli instance.
- `POST /api/v1/import/tautulli` — Import watch history from Tautulli API, JSON, or CSV with 3-way deduplication.

### Image Cache & API Documentation
- `GET  /api/v1/images/:filename` — Serves cached media poster images from disk with immutable caching headers.
- `GET  /docs` or `/swagger` — Interactive Swagger UI documentation.
- `GET  /api/v1/openapi.json` — Full OpenAPI 3.0 specification.

### User Settings & Connected Services
- `GET  /api/v1/user/settings` — Retrieve personal settings (Plex username filter, Radarr, Sonarr, Tautulli credentials with secrets masked).
- `POST /api/v1/user/settings` — Update personal settings and Plex username filter.
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

## 📺 Metadata & Attribution

Media metadata, plot outlines, genres, director credits, and actor/cast information are enriched in the background via [TheTVDB](https://thetvdb.com).

*This product uses TheTVDB API but is not endorsed or certified by TheTVDB.*

---

## 📄 License

MIT License. See [LICENSE](LICENSE) for details.
