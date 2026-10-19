# BingeSync

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![SQLite](https://img.shields.io/badge/SQLite-WAL%20mode-blue.svg)](https://www.sqlite.org/)
[![Docker](https://img.shields.io/badge/Docker-Ready-2496ED.svg)](https://www.docker.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

BingeSync is a self-hosted media tracker and Trakt.tv alternative. It automatically records playback from Plex and Jellyfin webhooks into an embedded SQLite database and provides a web interface with timelines, playback statistics, and search.

## Features

- **Plex and Jellyfin Webhooks**: Captures start, pause, resume, progress, and completed scrobbles.
- **Multi-User Isolation**: Distinct watch histories, settings, and webhook endpoints for each user.
- **Timeline and History**: Daily grouped watch logs with search, multi-select deletion, and filters by genre, director, actor, rating, and date.
- **Playback Statistics**: Watch pace, streaks, hourly distribution, top movies, and top shows across customizable timeframes.
- **Metadata Enrichment**: Background enrichment of genres, cast, crew, and overviews using either TheTVDB or The Movie Database (TMDB), with local poster caching.
- **Library Integrations**: Cross-reference watched items against Radarr and Sonarr libraries to identify cleanup candidates and delete media files.
- **History Imports**: Import history and ratings from Trakt.tv export files (ZIP, JSON, CSV) or connected Tautulli servers with automatic deduplication.
- **Authentication**: OpenID Connect (OIDC) SSO with PKCE, Back-Channel Logout 1.0, role mapping, and local dev login.
- **Single Binary**: Rust backend with an embedded web UI and zero external runtime dependencies.
- **API Documentation**: Built-in interactive Swagger UI available at `/docs` with OpenAPI 3.0 specification at `/api/v1/openapi.json`.

## Quick Start

### Docker Compose

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
      - JWT_SECRET=replace-with-a-random-secret
      - DEV_LOGIN_ENABLED=true
    volumes:
      - bingesync_data:/data

volumes:
  bingesync_data:
    driver: local
```

Start the service:

```bash
docker compose up -d
```

The web interface will be available at `http://localhost:8080`.

## Webhook Configuration

### Plex

1. Open Plex Web and go to **Settings** > **Webhooks** under the **Manage** section.
2. Click **Add Webhook**.
3. Copy your personal Plex Webhook URL from the BingeSync Settings page:
   ```
   http://<host>:8080/api/v1/webhooks/plex/<token>
   ```
4. Save changes. Plex will post scrobble and playback events directly to BingeSync.

### Jellyfin

1. Install the official **Webhook** plugin from the Jellyfin Dashboard (Catalog > Webhook).
2. Go to **Dashboard** > **Plugins** > **Webhook** and select **Add Generic Destination**.
3. Set the destination name to `BingeSync` and paste your Jellyfin Webhook URL from BingeSync Settings:
   ```
   http://<host>:8080/api/v1/webhooks/jellyfin/<token>
   ```
4. Enable notification types for `Playback Start`, `Playback Progress`, `Playback Stop`, and `User Data Saved`.
5. Select `Movies` and `Episodes` under Item Types, then save.

## Metadata Providers

BingeSync can enrich scrobbles with overview text, genres, directors, cinematographers, actors, and official posters.

You can configure your provider in the **Admin Settings** tab or via environment variables:

- **TheTVDB (v4)**: Set `METADATA_SOURCE=tvdb` and provide `TVDB_API_KEY` (and optional `TVDB_PIN`).
- **The Movie Database (TMDB)**: Set `METADATA_SOURCE=tmdb` and provide `TMDB_API_KEY` (API key or Read Access Token).

If metadata credentials are not set, BingeSync scrobbles with basic title information and Plex/Jellyfin internal thumbnails.

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `PORT` | `8080` | Server listen port |
| `HOST` | `0.0.0.0` | Server listen address |
| `APP_URL` | `http://localhost:8080` | Public base URL used for webhook links and OIDC redirects |
| `BASE_PATH` | `""` | Optional subpath when running behind a reverse proxy (e.g. `/bingesync`) |
| `DATABASE_URL` | `sqlite:///data/bingesync.db?mode=rwc` | SQLite database path with WAL mode |
| `JWT_SECRET` | *(built-in dev secret)* | Secret key for signing session tokens |
| `DEV_LOGIN_ENABLED` | `true` | Allows local email/username sign-in |
| `OIDC_ENABLED` | `false` | Enable OpenID Connect authentication |
| `OIDC_ISSUER_URL` | *(none)* | OIDC discovery issuer URL (e.g. `https://auth.example.com`) |
| `OIDC_CLIENT_ID` | *(none)* | OIDC client identifier |
| `OIDC_CLIENT_SECRET` | *(none)* | OIDC client secret |
| `OIDC_REDIRECT_URL` | `${APP_URL}/api/v1/auth/callback` | OAuth redirect URL |
| `METADATA_SOURCE` | `tvdb` | Active metadata enrichment provider (`tvdb` or `tmdb`) |
| `TVDB_API_KEY` | *(none)* | TheTVDB v4 API key |
| `TVDB_PIN` | *(none)* | TheTVDB v4 subscriber PIN (optional) |
| `TVDB_BASE_URL` | `https://api4.thetvdb.com/v4` | TheTVDB v4 API endpoint |
| `TMDB_API_KEY` | *(none)* | TMDB API key or v4 bearer token |
| `TMDB_BASE_URL` | `https://api.themoviedb.org/3` | TMDB API endpoint |
| `SCROBBLE_THRESHOLD_PERCENT` | `80` | Percentage of media playback required to log a watch |
| `IMAGE_CACHE_DIR` | `/data/images` | Disk directory for caching poster artwork |
| `TAUTULLI_URL` | *(none)* | Base URL of Tautulli server for history imports |
| `TAUTULLI_API_KEY` | *(none)* | API key for Tautulli |

Per-user connections for Radarr, Sonarr, and Tautulli can also be managed directly within User Settings.

## Authentication and Administration

### OpenID Connect (SSO)
- Set `OIDC_ENABLED=true` and configure your provider's issuer URL, client ID, and secret.
- Set the callback URL in your provider to `${APP_URL}/api/v1/auth/callback`.
- Users belonging to `admin`, `admins`, `administrator`, or `administrators` groups in your IdP are automatically granted admin permissions.
- Back-Channel Logout is supported at `/api/v1/auth/backchannel-logout` to terminate sessions immediately upon IdP revocation.

### Admin Controls
- **User Management**: Inspect users, scrobble totals, and remove accounts with cascading data cleanup.
- **System Settings**: Switch metadata providers, update API keys, and adjust scrobble completion thresholds.
- **Background Tasks**: Monitor task execution and trigger metadata, image cache, or user sync jobs on demand.
- **Log Viewer**: Live application log buffer accessible from the admin panel.

## REST API Overview

Interactive Swagger documentation is available at `/docs` (or `/swagger`). The raw specification is served at `/api/v1/openapi.json`.

Key endpoints:
- `GET /api/v1/history` - List, search, and filter watch history.
- `POST /api/v1/history` - Manually record a scrobble.
- `DELETE /api/v1/history/:id` - Delete an individual scrobble.
- `DELETE /api/v1/history/by-show` - Delete all scrobbles for a TV show.
- `GET /api/v1/timeline` - Daily grouped watch entries.
- `GET /api/v1/stats` - Watch metrics and charts summary.
- `GET /api/v1/now-playing` - Real-time playback status.
- `POST /api/v1/import/trakt` - Import Trakt export files.
- `POST /api/v1/import/tautulli` - Import playback records from Tautulli.
- `GET /api/v1/integrations/cleanup/candidates` - Find watched items remaining on Radarr or Sonarr.
- `POST /api/v1/admin/settings` - Update system configuration.

## Development and Testing

### Build and Run

```bash
# 1. Build frontend
cd web && npm run build && cd ..

# 2. Run backend
cargo run
```

### Running Tests

```bash
# Backend integration tests (40 tests)
cargo test

# Frontend browser E2E tests (32 tests)
cd web
npm install
npx playwright test
```

## Attributions

- Metadata, plot outlines, and cast information can be enriched via [TheTVDB](https://thetvdb.com). This product uses TheTVDB API but is not endorsed or certified by TheTVDB.
- Metadata, plot outlines, and cast information can be enriched via [The Movie Database (TMDB)](https://www.themoviedb.org). This product uses the TMDB API but is not endorsed or certified by TMDB.

## License

MIT License. See [LICENSE](LICENSE) for details.
