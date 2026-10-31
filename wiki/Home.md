# Welcome to the BingeSync Wiki

**BingeSync** is a lightweight, self-hosted media tracker, synchronization engine, and Trakt.tv alternative. It automatically captures playback events from **Plex**, **Jellyfin**, and **Emby** webhooks into an embedded SQLite database (WAL mode) and provides an interactive web interface with rich timelines, playback analytics, duplicate watch pruning, ratings management, and bidirectional media server synchronization.

---

## Wiki Navigation & Documentation Map

### 1. [Setup & Configuration](Setup-and-Configuration)
- Recommended **Docker Compose** production deployment.
- Reverse proxy configurations (**Nginx**, **Traefik**, **Caddy**) with subpath routing support (`BASE_PATH`).
- Complete **Environment Variables** reference table.
- Guided 3-step **First-Run Setup Flow** and initial admin promotion.

### 2. [Media Server Webhooks & Sync](Media-Server-Webhooks-and-Sync)
- Setting up personal incoming webhooks for **Plex**, **Jellyfin**, and **Emby**.
- Step-by-step guides for extracting your **Plex Token (`X-Plex-Token`)** and generating **Jellyfin API Keys**.
- **Bidirectional Media Sync**: Keeping watched history in lockstep between both Plex and Jellyfin servers simultaneously.
- **Tautulli Continuous 1-Way Sync**: Background polling worker and user discovery.

### 3. [Authentication & Access Control](Authentication-and-OIDC)
- OpenID Connect (OIDC) Single Sign-On (**Authentik**, **Keycloak**, **Authelia**, **PocketID**, **Google**, **Okta**).
- **OIDC Back-Channel Logout 1.0** for instant token invalidation upon user de-provisioning.
- Custom group role evaluation (`OIDC_ADMIN_GROUP` and `OIDC_USER_GROUP`).
- Generating and scoping **Personal API Keys** (`bs_live_...`) for Home Assistant, curl, and custom scripts.

### 4. [Integrations & Power Tools](Integrations-and-Tools)
- **Radarr & Sonarr** library cross-referencing and disk space cleanup.
- Importing watch history and ratings from **Trakt.tv** (ZIP archives, JSON, CSV) and **Tautulli**.
- **Duplicate Watch Detector & Cleanup Tool** (configurable time windows, Keep Oldest / Keep Newest).
- Filtering watch history and timeline by **Rating Thresholds** (★ 8+, 9+, 10/10).
- Outbound notifications (**Discord**, **Telegram**, **ntfy**, **Gotify**, generic webhooks).

### 5. [Troubleshooting Guide](Troubleshooting-Guide)
- Solutions for common deployment hurdles:
  - SQLite lock contention and database busy resolution.
  - Webhook username/ID mismatch errors (`status: ignored`).
  - Reverse proxy SSL headers and cookie security flags.
  - Diagnosing network and server reachability via the integrated live log viewer.

---

## Community & Contributing
- **Source Code**: [GitHub Repository](https://github.com/daryl/BingeSync)
- **License**: GNU General Public License v3.0 ([GPL-3.0](https://www.gnu.org/licenses/gpl-3.0.html))

