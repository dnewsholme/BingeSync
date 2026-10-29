# BingeSync

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![SQLite](https://img.shields.io/badge/SQLite-WAL%20mode-blue.svg)](https://www.sqlite.org/)
[![Docker](https://img.shields.io/badge/Docker-Ready-2496ED.svg)](https://www.docker.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

BingeSync is a lightweight, self-hosted media tracker and Trakt.tv alternative. It automatically captures playback from **Plex**, **Jellyfin**, and **Emby** webhooks into an embedded SQLite database with WAL mode and delivers a modern, high-performance web interface with interactive timelines, playback analytics, duplicate cleanup, and ratings management.

---

## Features

- **Plex, Jellyfin & Emby Webhooks**: Captures playback start, pause, resume, progress, and completed scrobbles with configurable playback completion thresholds and username/UUID filtering.
- **Multi-User Isolation**: Strictly scoped watch histories, user settings, active sessions, and personal webhook tokens for every account.
- **Rating Threshold & Star History**: Filter watch history and timeline by minimum star rating thresholds (10/10, 9+ stars, 8+ stars down to 1+ star, rated, or unrated). Seamlessly combine ratings with media type filters to view all movies or TV shows above a specific rating.
- **Duplicate Watch Detector & Cleanup Tool**: Scan scrobbles for duplicate watches within configurable time windows (1h, 6h, 12h, 24h, 48h, 72h) and automatically prune redundant entries using *Keep Oldest* or *Keep Newest* strategies, or delete selectively.
- **Custom Tags & Personal Review Notes**: Annotate scrobbles with searchable tags (e.g. `rewatch`, `imax`, `favorites`) and personal notes, with one-click filter chips and export support.
- **Timeline & History**: Daily grouped watch logs with date scrubber, search across all history, batch selection and deletion, and rich filters for genres, directors, cinematographers, actors, and missing metadata.
- **Viewing Heatmap & Annual Review (Binge Wrapped)**: 7x24 weekly hourly viewing density heatmap matrix, top movies/shows charts, streaks, and an interactive annual "Binge Wrapped" review modal.
- **Outbound Webhooks**: Dispatch automated scrobble notifications to Discord, Telegram, ntfy, Gotify, or generic JSON webhook endpoints.
- **Metadata Enrichment & Poster Caching**: Background enrichment of plot overviews, genres, cast, crew, and directors using either **The Movie Database (TMDB)** or **TheTVDB (v4)**, with disk-based poster caching and title match search.
- **Library Integrations & Cleanup**: Cross-reference watched items against Radarr and Sonarr libraries to identify deletion candidates, calculate reclaimable disk space, and trigger media removal.
- **Trakt.tv & Tautulli Imports**: Import watch history and ratings from Trakt export files (multi-file ZIP, JSON, CSV) or connected Tautulli instances with automatic deduplication.
- **Backups & User Data Export**: Automated and on-demand SQLite database backups with 7-day retention and pre-restore snapshots, plus one-click user data export to a clean ZIP archive (scrobbles CSV, JSON, and ratings).
- **Authentication & Back-Channel Logout**: OpenID Connect (OIDC) SSO with PKCE, OpenID Back-Channel Logout 1.0 for instant session termination upon access revocation, group role mapping, and local dev login.
- **UI Customization & Multilingual Support**: Trakt-inspired responsive dark UI with accent theme switcher (Red, Amber, Purple, Cyan, Emerald, Rose, Slate/Black) and multi-language localization (English, Spanish, German, French).
- **Single Binary Delivery**: Rust backend with embedded web assets (`rust-embed`) and zero external runtime dependencies.
- **Interactive API Documentation**: Embedded OpenAPI 3.0 specification served at `/api/v1/openapi.json` and interactive Swagger UI at `/docs`.

---

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
      - bingesync_backups:/backup

volumes:
  bingesync_data:
    driver: local
  bingesync_backups:
    driver: local
```

Start the service:

```bash
docker compose up -d
```

Access the web interface at `http://localhost:8080`.

---

## Webhook Configuration

### Plex

1. Open Plex Web and navigate to **Settings** > **Webhooks** under the **Manage** section.
2. Click **Add Webhook**.
3. Copy your personal Plex Webhook URL from the BingeSync Settings page:
   ```
   http://<host>:8080/api/v1/webhooks/plex/<token>
   ```
4. Save changes. Plex will automatically post scrobbles and playback status to BingeSync.

### Jellyfin

1. Install the official **Webhook** plugin from the Jellyfin Dashboard (Catalog > Webhook).
2. Go to **Dashboard** > **Plugins** > **Webhook** and select **Add Generic Destination**.
3. Set the destination name to `BingeSync` and paste your Jellyfin Webhook URL:
   ```
   http://<host>:8080/api/v1/webhooks/jellyfin/<token>
   ```
4. Enable notification types for `Playback Start`, `Playback Progress`, `Playback Stop`, and `User Data Saved`.
5. Select `Movies` and `Episodes` under Item Types, then save.

### Emby

1. Open the Emby Server Dashboard and go to **Webhooks** (Emby Premiere required).
2. Click **Add Webhook** and select `Generic Webhook`.
3. Set the Webhook URL to:
   ```
   http://<host>:8080/api/v1/webhooks/emby/<token>
   ```
4. Check `Playback Events` (`playback.start`, `playback.pause`, `playback.unpause`, `playback.stop`) and `User Data Events` (`user.data.saved`), then save.

---

## Viewing History Above a Rating Threshold

BingeSync allows you to filter your watch history and timeline by rating threshold:

1. Navigate to the **History** tab.
2. Click the **Rating Filter** dropdown (e.g. `★ 8+ Stars`, `★ 9+ Stars`, or `★ 10/10`).
3. Combine with the **Movies** or **Episodes** filter pills to inspect all movies or TV shows rated above your chosen threshold.
4. You can also filter by `Rated Only` or `Unrated`, or search titles, cast, directors, or custom tags while the rating threshold is active.

---

## Duplicate Watch Detector & Cleanup Tool

If playback sessions were recorded multiple times (e.g., from multiple webhook destinations or concurrent devices):

1. Open the **History** tab and click **Duplicates** in the filter bar (or navigate to **Cleanup**).
2. Select your desired time window (1 hour, 6 hours, 12 hours, 24 hours, 48 hours, or 72 hours).
3. The detector will group identical media watched within that interval.
4. Click **Keep Oldest** or **Keep Newest** to prune redundant scrobbles instantly, or selectively remove individual duplicate entries.

---

## Metadata Providers

BingeSync enriches scrobbles with overview text, genres, directors, cinematographers, actors, and official artwork.

Configure your provider in **Admin Settings** or via environment variables:

- **The Movie Database (TMDB)**: Set `METADATA_SOURCE=tmdb` and provide `TMDB_API_KEY` (API key or Read Access Token).
- **TheTVDB (v4)**: Set `METADATA_SOURCE=tvdb` and provide `TVDB_API_KEY` (and optional `TVDB_PIN`).

If metadata credentials are not set, BingeSync scrobbles with basic title information and media server internal artwork.

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `PORT` | `8080` | Server listen port |
| `HOST` | `0.0.0.0` | Server listen address |
| `APP_URL` | `http://localhost:8080` | Public base URL used for webhook links and OIDC redirects |
| `BASE_PATH` | `""` | Optional subpath when running behind a reverse proxy (e.g. `/bingesync`) |
| `DATABASE_URL` | `sqlite:///data/bingesync.db?mode=rwc` | SQLite database path with WAL mode |
| `BACKUP_DIR` | `/backup` | Storage directory for database backups (7-day retention) |
| `JWT_SECRET` | *(built-in dev secret)* | Secret key for signing session tokens |
| `DEV_LOGIN_ENABLED` | `true` | Allows local email/username sign-in |
| `OIDC_ENABLED` | `false` | Enable OpenID Connect authentication |
| `OIDC_ISSUER_URL` | *(none)* | OIDC discovery issuer URL (e.g. `https://auth.example.com`) |
| `OIDC_CLIENT_ID` | *(none)* | OIDC client identifier |
| `OIDC_CLIENT_SECRET` | *(none)* | OIDC client secret |
| `OIDC_REDIRECT_URL` | `${APP_URL}/api/v1/auth/callback` | OAuth redirect URL |
| `OIDC_ADMIN_GROUP` | *(none)* | Custom group name(s) (comma-separated) granting the admin role |
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

Per-user integrations for Radarr, Sonarr, Tautulli, Overseerr, and outbound webhooks can also be configured directly on the User Settings page.

---

## Authentication and Administration

### OpenID Connect (SSO)
- Set `OIDC_ENABLED=true` and configure your provider's issuer URL, client ID, and secret.
- Set the callback URL in your provider to `${APP_URL}/api/v1/auth/callback`.
- Users belonging to `admin`, `admins`, `administrator`, `administrators`, or any group configured in `OIDC_ADMIN_GROUP` (or Admin Settings) are automatically granted admin permissions.
- Back-Channel Logout is supported at `/api/v1/auth/backchannel-logout` to terminate sessions immediately upon IdP revocation.

### Admin Controls
- **User Management**: Inspect users, scrobble totals, and remove accounts with cascading data cleanup.
- **Database Backups**: Create point-in-time SQLite snapshots, view backup history, and restore with automatic pre-restore safety snapshots.
- **System Settings**: Switch metadata providers, update API keys, and adjust scrobble completion thresholds.
- **Background Tasks**: Monitor task execution and trigger metadata, image cache, or user sync jobs on demand.
- **Live Log Viewer**: Streaming application log buffer accessible from the admin panel.

---

## REST API Overview

Interactive Swagger documentation is available at `/docs` (or `/swagger`). The raw specification is served at `/api/v1/openapi.json`.

Key endpoints:
- `GET /api/v1/history` - List, search, and filter watch history (with `rating`, `media_type`, `genre`, `person`, `tag`, and `missing_metadata` filters).
- `POST /api/v1/history` - Manually record a scrobble.
- `DELETE /api/v1/history/:id` - Delete an individual scrobble.
- `DELETE /api/v1/history/by-show` - Delete all scrobbles for a TV show.
- `GET /api/v1/history/duplicates` - Scan for duplicate scrobbles within a time window.
- `POST /api/v1/history/duplicates/clean` - Prune redundant duplicate scrobbles.
- `PUT /api/v1/history/:id/rate` - Set star rating (1-10) or clear rating.
- `PUT /api/v1/history/:id/meta` - Update custom tags and personal review notes.
- `GET /api/v1/timeline` - Daily grouped watch entries.
- `GET /api/v1/stats` - Watch metrics and charts summary.
- `GET /api/v1/stats/heatmap` - 7x24 weekly hourly viewing density matrix.
- `GET /api/v1/stats/wrapped` - Annual viewing metrics.
- `GET /api/v1/now-playing` - Real-time playback status.
- `POST /api/v1/import/trakt` - Import Trakt export files.
- `POST /api/v1/import/tautulli` - Import playback records from Tautulli.
- `GET /api/v1/user/export` - Export watch history ZIP archive (CSV, JSON, ratings).
- `GET /api/v1/integrations/cleanup/candidates` - Find watched items remaining on Radarr or Sonarr.
- `POST /api/v1/admin/backups` - Create a database backup.
- `POST /api/v1/admin/settings` - Update system configuration.

---

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
# Backend integration tests (55 tests)
cargo test

# Frontend browser E2E tests (41 tests)
cd web
npm install
npx playwright test
```

---

## Attributions

- Metadata, plot outlines, and cast information can be enriched via [TheTVDB](https://thetvdb.com). This product uses TheTVDB API but is not endorsed or certified by TheTVDB.
- Metadata, plot outlines, and cast information can be enriched via [The Movie Database (TMDB)](https://www.themoviedb.org). This product uses the TMDB API but is not endorsed or certified by TMDB.

---

## License

MIT License. See [LICENSE](LICENSE) for details.
