# Integrations & Power Tools

BingeSync includes powerful built-in utility tools for media management, watch history imports, duplicate pruning, and external scrobble notifications.

---

## Radarr and Sonarr Cleanup

BingeSync cross-references your completed watch history with your **Radarr** and **Sonarr** libraries to help reclaim disk space:

### 1. Configuration
In **User Settings** $\rightarrow$ **Connected Services**:
- **Radarr**: Enter your Server URL (e.g. `http://192.168.1.100:7878`) and API Key.
- **Sonarr**: Enter your Server URL (e.g. `http://192.168.1.100:8989`) and API Key.

### 2. Finding Watched Candidates
1. Navigate to the **Cleanup** view in the main navigation.
2. BingeSync queries Radarr and Sonarr to find movies and series where all monitored items have been watched.
3. Review reclaimable storage (e.g. `45.2 GB reclaimable`), season watch completion rates, and file paths.
4. Execute granular cleanup options:
   - **Radarr**: Delete Movie and File.
   - **Sonarr**: Delete Show & Files, Delete Season Files & Unmonitor Season, or Delete All Files & Unmonitor All Seasons.

---

## Trakt and Tautulli History Import

### Trakt.tv Export Import
BingeSync supports importing your complete Trakt.tv watch history and ratings without losing historical timestamps:
1. Download your data export from Trakt (Settings $\rightarrow$ Export).
2. In BingeSync, click **+ Log Watch** $\rightarrow$ **Import History** $\rightarrow$ **Trakt.tv**.
3. Either:
   - Upload the official multi-file **Trakt ZIP archive** (automatically parses `watched_movies.csv`, `watched_shows.csv`, `ratings_movies.csv`, and `ratings_episodes.csv`), or
   - Paste raw JSON/CSV history into the text box.
4. BingeSync automatically sorts records chronologically, establishes ratings, and triggers background poster caching.

### Tautulli History Import
1. In the Import modal, switch to the **Tautulli** tab.
2. Enter your Plex username (defaults to your current BingeSync account name).
3. Paste a Tautulli history JSON dump or fetch directly via the continuous sync worker.

---

## Duplicate Watch Detector & Cleanup

Playback can occasionally be logged twice (e.g. if multiple webhooks are triggered simultaneously by a client restart):

1. In the **History** tab, click **Duplicates** (or navigate to **Cleanup**).
2. Choose a detection time window: **1h**, **6h**, **12h**, **24h**, **48h**, or **72h**.
3. BingeSync identifies identical media titles watched within that timeframe.
4. Clean up in one click:
   - **Keep Oldest**: Preserves the first watch timestamp and prunes duplicates.
   - **Keep Newest**: Preserves the latest timestamp.
   - **Selective Deletion**: Delete individual duplicates manually with the trash icon.

---

## Rating Threshold Filtering

Easily inspect your highest-rated movies and TV episodes across your entire watch history:

1. In the **History** or **Timeline** view, click the **Rating** filter pill.
2. Select any minimum rating threshold:
   - `★ 10/10 Masterpieces`
   - `★ 9+ Stars`
   - `★ 8+ Stars` down to `★ 1+ Star`
   - `Rated Only` or `Unrated`
3. Combine with media type pills (`Movies` or `Episodes`), genres, directors, or custom tags to find exactly what you are looking for.

---

## Outbound Webhooks

Get real-time push notifications whenever a watch event completes:

1. In **User Settings** $\rightarrow$ **Preferences**, locate **Outbound Webhooks**.
2. Select your provider:
   - **Discord**: Discord webhook URL with rich embedded media cards.
   - **Telegram**: Chat notification via bot token and chat ID.
   - **ntfy**: Push notifications to private or public ntfy topics.
   - **Gotify**: Push notifications to self-hosted Gotify servers.
   - **Generic JSON**: Raw JSON payload posted via HTTP POST for Home Assistant or Node-RED.
3. Click **Send Test Notification** to verify connectivity before saving.

