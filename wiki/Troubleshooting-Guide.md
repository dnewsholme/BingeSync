# Troubleshooting Guide

This guide helps you diagnose and resolve common setup, networking, database, and webhook issues in BingeSync.

---

## Common Issues

### 1. Webhook received but playback not recorded (`status: ignored`)
- **Symptom**: The webhook URL responds with `HTTP 200 OK` and JSON `{"status": "ignored", "reason": "username_mismatch"}`.
- **Cause**: Plex or Jellyfin sent a webhook for a user account that does not match your configured allowed usernames in BingeSync.
- **Solution**:
  1. Open **User Settings** $\rightarrow$ **Connected Services**.
  2. Under **Plex Account Filtering** or **Jellyfin Account Filtering**, add your exact Plex username, email prefix, or user ID (e.g. `your_plex_name, 3078822f-..., 1`).
  3. Comma-separated lists are supported. Check the **Application Logs** under Admin Settings to see the exact username and ID that the media server sent in the payload.

### 2. Can't log in over HTTPS (`Invalid Session` or cookie not sent)
- **Symptom**: Logging in via OIDC or dev-login redirects back to the login page without creating an active session.
- **Cause**: Browser cookie security flag mismatch. When running behind an HTTPS reverse proxy without `X-Forwarded-Proto: https`, the server thinks it is on HTTP and sets cookies without the `Secure` flag, or vice versa.
- **Solution**:
  - In your reverse proxy (Nginx, Traefik, Caddy), ensure `X-Forwarded-Proto $scheme;` and `X-Forwarded-For` headers are forwarded.
  - Set `APP_URL=https://bingesync.example.com` in your environment.

### 3. Database Locks and WAL Mode (`(code: 5) database is locked`)
- **Symptom**: Error logs show `(code: 5) database is locked` or `SQLITE_BUSY`.
- **Cause**: Concurrent background workers (metadata enrichment, poster caching, database maintenance) executing write queries simultaneously without a busy timeout.
- **Solution**:
  - BingeSync automatically enables SQLite WAL mode (`journal_mode=WAL`) and a **15-second busy timeout** (`busy_timeout(15s)`) on all database connection pools.
  - Ensure the `/data` directory is mounted on a local filesystem. **Avoid mounting SQLite on networked NFS or SMB shares**, as network file locks do not reliably support POSIX advisory locks required by SQLite WAL mode.

### 4. Poster Images Not Loading (`404 Not Found` or Broken Images)
- **Symptom**: Scrobble cards show generic movie/episode placeholder icons instead of official posters.
- **Cause**: Poster artwork is being downloaded asynchronously by the background image cache worker.
- **Solution**:
  1. Navigate to **Admin Settings** $\rightarrow$ **Background Tasks**.
  2. Verify that **Media Poster & Thumbnail Caching** is running or click **Run Now**.
  3. Click **Force Refresh All Posters** if you have moved or cleared your `/data/images` volume.
  4. Ensure `IMAGE_CACHE_DIR` has write permissions in your Docker container.

### 5. Media Server Writeback Failure
- **Symptom**: Watches scrobbled in BingeSync are not marked as played on Plex or Jellyfin.
- **Cause**: Invalid token/API key, incorrect server URL, or item not found in media library.
- **Solution**:
  1. Go to **User Settings** $\rightarrow$ **Connected Services**.
  2. Click **Test Plex Connection** and **Test Jellyfin Connection**.
  3. Verify that the URL is directly reachable from the BingeSync container (e.g. use local IP `http://192.168.1.100:32400` rather than `http://localhost:32400`).
  4. Check the **Application Logs** filtered to `writeback` to inspect the exact REST response returned by your media server.

---

## Log Inspection and Diagnostics

BingeSync includes an integrated, high-resolution application log viewer:

1. Navigate to **Admin Settings** $\rightarrow$ **Application Logs**.
2. Filter by minimum log level (**INFO**, **WARN**, **ERROR**).
3. Search by component keyword:
   - `plex`: All incoming Plex webhooks, username matches, and session updates.
   - `jellyfin` / `emby`: Incoming Jellyfin and Emby webhook events.
   - `writeback`: Media server sync writeback attempts, matches, and error codes.
   - `tautulli`: Background continuous sync polling and deduplication results.
   - `metadata` / `tvdb` / `tmdb`: Artwork search and overview enrichment.
   - `backup`: Snapshot creation and 7-day retention pruning.
4. From the **Background Tasks** panel, clicking **View Logs** on any task card automatically routes to the log viewer pre-filtered for that specific background job.

