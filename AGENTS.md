# BingeSync — Agent & Contributor Guide (`AGENTS.md`)

This document provides architectural guidance, repository conventions, and workflow instructions for AI coding agents and human contributors working on **BingeSync**.

---

## 1. Project Overview & Architecture

BingeSync is a lightweight, self-hosted web service that replicates **Trakt.tv**. It captures media playback and completed watch events from **Plex Webhooks**, persists watch history in **SQLite**, and provides an interactive UI with timeline views, search, watch statistics, and per-user webhook management.

### Key Invariants
- **Multi-User Isolation**: Every user account has a distinct `user_id` and unique `webhook_token`. Scrobbles, history, timeline, stats, and active sessions are strictly scoped per-user.
- **Single-Binary Delivery**: The web frontend (`web/dist`) is embedded into the Rust binary at compile time via `rust-embed`. The Docker container runs a single minimal Debian slim image with no external runtime dependencies.
- **Dual Authentication & Role Control**:
  - **OpenID Connect (OIDC)** with PKCE (RFC 7636) for SSO (Authentik, Keycloak, Authelia, Google, Okta) with automatic `groups` scope evaluation. Users in `admin`/`admins`/`administrator`/`administrators` groups are assigned the `admin` role; others receive `user`.
  - **OIDC Back-Channel Logout 1.0**: Receives `logout_token` at `/api/v1/auth/backchannel-logout` when user access is revoked, terminated, or group membership removed in IdP, invalidating active sessions and existing JWTs immediately.
  - **Dev / Local Login**: Headless or local login for testing and standalone deployments without an external IdP.
  - Signed JWT tokens passed via `Authorization: Bearer <token>` or HTTP-only `bingesync_token` cookie.
- **Admin Management & Cascading Deletion**: Admins can manage registered users via the API and web UI. Deleting a user cascade-deletes all associated scrobbles, active sessions, and database records in an atomic transaction.
- **Plex Webhook Compatibility & Username Filtering**: Plex sends `multipart/form-data` with a `payload` JSON field, but BingeSync also accepts direct `application/json` payloads. Webhooks are accepted at `/api/v1/webhooks/plex/{token}` or `/api/v1/webhooks/plex?token={token}`. Incoming webhooks are filtered against the user's configured Plex username(s) in User Settings (or default username/display name candidates) so that playback from other users sharing the same Plex server is gracefully ignored with HTTP 200 OK (`status: "ignored", reason: "username_mismatch"`).

---

## 2. Repository Layout

```
BingeSync/
├── Cargo.toml               # Rust dependencies and package metadata
├── Cargo.lock
├── Dockerfile               # Multi-stage production container build
├── docker-compose.yml       # Production self-hosting orchestration
├── .github/
│   └── workflows/
│       ├── docker.yml       # Build, test, and publish Docker image to Docker Hub
│       └── ui-tests.yml     # Playwright E2E UI testing pipeline
├── .env.example             # Configuration reference template
├── README.md                # User-facing documentation
├── AGENTS.md                # This developer/agent reference file
├── src/
│   ├── main.rs              # Server bootstrap and TCP listener
│   ├── lib.rs               # Library root exposing modules for testing
│   ├── config.rs            # Config struct and env loader
│   ├── assets.rs            # rust-embed handler and SPA fallback router
│   ├── db/
│   │   ├── mod.rs           # SQLite pool init with WAL mode and foreign keys
│   │   └── migrations.rs    # Embedded SQL schema migrations
│   ├── models/
│   │   ├── mod.rs           # Model re-exports
│   │   ├── user.rs          # User & UserResponse models
│   │   ├── scrobble.rs      # Scrobble, TimelineDay, StatsSummary models
│   │   └── plex.rs          # Plex webhook payload structures
│   ├── auth/
│   │   ├── mod.rs           # Auth re-exports
│   │   ├── jwt.rs           # JWT creation/verification & AuthUser Axum extractor
│   │   └── oidc.rs          # OIDC discovery, PKCE auth URL, and code exchange
│   └── api/
│       ├── mod.rs           # Router assembly and middleware (CORS, extensions)
│       ├── auth.rs          # /api/v1/auth/* endpoints (login, callback, backchannel-logout)
│       ├── webhooks.rs      # /api/v1/webhooks/plex/* webhook handlers
│       ├── history.rs       # /api/v1/history (search, filters, pagination, CRUD)
│       ├── timeline.rs      # /api/v1/timeline daily grouping
│       ├── stats.rs         # /api/v1/stats watch metrics & top shows
│       ├── trakt_import.rs  # /api/v1/import/trakt JSON/CSV history & rating import
│       ├── integrations.rs  # /api/v1/integrations/* Radarr & Sonarr library check & cleanup
│       ├── admin.rs         # /api/v1/admin/users user management and cascade deletion
│       └── user.rs          # /api/v1/user/webhook & token regeneration
├── tests/
│   └── api_tests.rs         # Integration test suite (31 tests covering full API)
└── web/
    ├── package.json         # Frontend dependencies (Vite, Playwright)
    ├── vite.config.js       # Vite configuration with base: './' for subpath support
    ├── index.html           # SPA entry point with Trakt-style dark UI
    ├── playwright.config.js # Playwright E2E configuration with webServer runner
    ├── src/
    │   ├── api.js           # Fetch API client
    │   ├── main.js          # SPA logic, view management, event handling
    │   └── style.css        # Design system, dark palette, cards, micro-animations, charts
    └── tests/
        └── e2e.spec.js      # Playwright end-to-end browser tests
```

---

## 3. Technology Stack & Dependencies

| Layer | Technology | Details |
|---|---|---|
| **Backend** | Rust 1.85+ | `axum` 0.8, `tokio` 1.40, `tower-http` 0.6 |
| **Database** | SQLite 3 | `sqlx` 0.8 with WAL mode (`journal_mode=WAL`), embedded migrations |
| **Auth** | JWT & OIDC | `jsonwebtoken`, `reqwest`, `base64`, `sha2`, PKCE S256 |
| **Frontend** | Vanilla JS & CSS | Built with Vite 5, Google Fonts Inter, embedded via `rust-embed` |
| **Testing** | Cargo & Playwright | `cargo test` for backend, `@playwright/test` for browser E2E |
| **Packaging** | Docker | Multi-stage build (`node:22-alpine` -> `rust:slim` -> `debian:bookworm-slim`) |

---

## 4. Key Development Rules & Patterns

### 1. Axum 0.8 Request Extractors
In Axum 0.8:
- The body extractor (e.g. `axum::body::Bytes` or `Json<T>`) **must always be the final parameter** in a handler function signature.
- `AuthUser` implements `FromRequestParts` using native async fn in traits (`impl<S: Send + Sync> FromRequestParts<S> for AuthUser`).
- `AppState` must be supplied both via `.with_state(state)` and `.layer(Extension(state.clone()))` so `AuthUser` can extract the JWT secret from `Parts.extensions`.

### 2. Plex Webhook Handler
Plex webhooks send:
- `event`: `media.scrobble`, `media.play`, `media.pause`, `media.resume`, `media.stop`.
- `multipart/form-data` with a text field named `payload` containing JSON.
- `webhooks.rs` checks `Content-Type`: if multipart, it extracts the `payload` field using `multer`; if JSON, it deserializes directly.
- On `media.scrobble`: inserts into `scrobbles` and clears `active_sessions`.
- On `media.play`/`pause`/`resume`: updates `active_sessions`.
- On `media.stop`: clears `active_sessions`.

### 3. Database & Migrations
- Schema definitions reside in `src/db/migrations.rs`.
- When adding tables or columns, add `IF NOT EXISTS` clauses to ensure idempotency.
- Always add appropriate indexes (e.g. `(user_id, watched_at DESC)`).
- Use `sqlite::memory:` in tests for complete isolation.

### 4. Frontend Asset Embedding
- `src/assets.rs` uses `#[derive(RustEmbed)]` with `#[folder = "web/dist/"]`.
- When updating frontend code, run `npm run build` in `web/` before running `cargo build` so embedded assets are up to date.

---

## 5. Development & Testing Commands

### Run Rust Unit & Integration Tests
```bash
cargo test
```
Tests in `tests/api_tests.rs` (30 integration tests) cover:
- Dev login and `/api/v1/auth/me`
- Plex movie and TV episode scrobbling
- Multi-user isolation (User A cannot see User B's data)
- Webhook token regeneration and invalidation
- Active session playback state (`now-playing`)
- Manual scrobble creation, search, filtering, and deletion
- Multipart `form-data` Plex webhook parsing
- Subpath routing and embedded static asset delivery
- Timeline and history rich filters (genre, director, actor, rating, watch status)
- Trakt history and ratings import from multi-file ZIP archives, JSON, and CSV (with automatic chronological sorting)
- Delete by show, batch delete, and delete by date
- Admin user management, list users, and cascading data deletion
- OpenID Connect Back-Channel Logout token validation, session termination, and token invalidation
- Disk-based media poster caching and image serving with traversal protection
- Sonarr delete options (delete series & files, delete season files & unmonitor season, and delete all files & unmonitor all seasons)
- TheTVDB v4 metadata enrichment worker (genres, cast, plot overview, and directors)
- Movie TVDB metadata fallback and manual scrobble metadata refresh
- Item other watches query: movies and exact same TV episode matching with multi-user isolation
- Admin settings, log buffer, and background task management endpoints
- Stats top movies, extended metrics, and media type/time range filtering
- Tautulli playback history import, user discovery, and 3-way deduplication
- Scrobble progress tracking, configurable completion threshold, and same-day deduplication
- Rating watched items (1-10) and clearing ratings with multi-user isolation
- OpenAPI 3.0 specification and interactive Swagger UI documentation
- Plex webhook internal thumbnail extraction and metadata refresh endpoint
- User settings isolation and credential storage
- Plex webhook username and ID filtering
- Search across all history regardless of date and return all matching results
- Jellyfin webhook scrobbles for movies and TV episodes, active sessions, and username filtering
- Specific director and cinematographer history fields, filters, and crew metadata enrichment
- Option to force refresh metadata for all records resetting metadata timestamps and triggering TVDB sync
- Fix match option on history items specifying TheTVDB ID with sibling episode matching and multi-user isolation

### Run Playwright E2E UI Tests
```bash
cd web
npm install
npx playwright test
```
Playwright tests in `web/tests/e2e.spec.js` (30 browser E2E tests) cover:
- User sign-in flow, "+ Log Watch" modal, and timeline rendering
- Multi-user separation in the web UI
- Trakt history and rating import flow in the web UI (pasted JSON)
- Trakt multi-file ZIP archive import flow in the web UI (file upload, multi-file merge, and sorting)
- Timeline search, filtering, and delete by date
- Show detail modal IMDb link and delete all scrobbles for show
- Multi-select batch delete in History view
- Admin role and user management: list users, verify backchannel URL, and delete user
- Sonarr cleanup options: delete show and files, delete & unmonitor season files, and delete all files & unmonitor all seasons
- TheTVDB attribution: global footer, settings integration card, and scrobble detail modal footnote
- Item other watches: other watches table in scrobble detail modal strictly matching exact same episode
- Stats page top watched movies, extended metrics, and type/range filter tabs
- Minimised timeline search and date scrubber navigation
- Search all history option searches across all dates and returns all results
- Admin settings page, live log viewer, and background task execution
- Favicon delivery and metadata link verification
- In-progress playback badge rendering and detail modal progress display
- Tautulli history import modal, username defaulting, and deduplication
- Timeline hierarchical drill-down navigation: Year -> Month -> Dates
- Rating watched items in detail modal, update, and unrating
- Swagger UI interactive documentation at `/docs` and OpenAPI JSON endpoint
- User settings: configure personal radarr, sonarr, and tautulli with secret redaction
- Jellyfin webhooks: displays Jellyfin webhook URL, supports username filtering, and processes scrobbles
- Multilingual support: switch between English, Spanish, German, and French with dynamic UI updates and persistence
- Clickable actors, directors, and cinematographers to filter history items
- Background jobs: user cache sync displays in admin tasks and can be triggered
- Specific director and cinematographer chips on cards and detail modal tags
- Fix incorrect match on history items by specifying TheTVDB ID
- Admin background tasks: force refresh all metadata option
- Mobile responsive layout: bottom app navigation bar, stacked touch forms, and responsive modals on phone viewports

### Run the App Locally
```bash
# 1. Build frontend
cd web && npm run build && cd ..

# 2. Run backend
cargo run
```
Access at `http://localhost:8080`.

### Build & Run Docker Image
```bash
docker build -t bingesync:latest .
docker run -p 8080:8080 -v bingesync_data:/data bingesync:latest
```

---

## 6. Environment Variables Reference

| Variable | Default | Description |
|---|---|---|
| `PORT` | `8080` | Port for the HTTP server |
| `HOST` | `0.0.0.0` | Listen host IP |
| `APP_URL` | `http://localhost:8080` | Base URL for webhook generation and OIDC redirects |
| `BASE_PATH` | `""` | Optional subpath for reverse proxies (e.g. `/bingesync`) |
| `DATABASE_URL` | `sqlite:///data/bingesync.db?mode=rwc` | SQLite connection string |
| `JWT_SECRET` | *(internal dev secret)* | Secret for signing session JWTs |
| `OIDC_ENABLED` | `false` | Enable/disable OIDC provider integration |
| `OIDC_ISSUER_URL` | *(none)* | OIDC discovery endpoint (e.g. `https://auth.example.com`) |
| `OIDC_CLIENT_ID` | *(none)* | OIDC client ID |
| `OIDC_CLIENT_SECRET` | *(none)* | OIDC client secret |
| `OIDC_REDIRECT_URL` | `${APP_URL}/api/v1/auth/callback` | Callback URL |
| `DEV_LOGIN_ENABLED` | `true` | Allows local email/username login |
| `TVDB_API_KEY` | *(none)* | TheTVDB v4 API key for background metadata enrichment |
| `TVDB_PIN` | *(none)* | Optional subscriber PIN for user-subscription TheTVDB keys |
| `TVDB_BASE_URL` | `https://api4.thetvdb.com/v4` | TheTVDB v4 API base URL |
| `SCROBBLE_THRESHOLD_PERCENT` | `80` | Percentage of media playback required to mark as watched |
| `IMAGE_CACHE_DIR` | `/data/images` | Directory path on disk where media poster images are cached |
| `TAUTULLI_URL` | *(none)* | Base URL of Tautulli instance for playback history import |
| `TAUTULLI_API_KEY` | *(none)* | API key for Tautulli instance |

*Note: Radarr, Sonarr, and Tautulli integrations can also be configured per-user on the User Settings page.*
