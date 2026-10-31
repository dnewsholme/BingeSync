# Media Server Webhooks & Synchronization

BingeSync integrates with your personal media ecosystem through incoming webhooks and automated bidirectional writeback to Plex, Jellyfin, and Emby.

---

## Plex Webhook Configuration

### 1. Adding the Webhook to Plex
1. Open Plex Web and sign in as a server administrator or Plex Pass user.
2. Click the **Wrench icon** (Settings) in the top right.
3. Under the **Manage** section in the left sidebar, click **Webhooks**.
4. Click **Add Webhook**.
5. Copy your personal Plex Webhook URL from the BingeSync Settings page:
   ```
   http://<bingesync-host>:8080/api/v1/webhooks/plex/<your_webhook_token>
   ```
6. Click **Save Changes**. Plex will now automatically post playback events (`media.play`, `media.pause`, `media.resume`, `media.stop`, `media.scrobble`) to BingeSync.

### 2. Finding your Plex Token (`X-Plex-Token`)
For bidirectional watch state sync (writeback), BingeSync requires your Plex authentication token:
1. Open Plex Web and navigate to any movie or television episode.
2. Click the three dots icon (**···**) and select **Get Info**.
3. In the info dialog, click **View XML** in the lower left corner.
4. Look at the URL in your browser's address bar. The token is the string at the end of the URL following `?X-Plex-Token=`:
   ```
   https://192-168-1-100...plex.direct:32400/library/metadata/12345?X-Plex-Token=abc123XYZ456
   ```
5. Copy that token value and paste it into the **Plex API Token** field under **User Settings** $\rightarrow$ **Connected Services**.

---

## Jellyfin Webhook Configuration

### 1. Installing the Webhook Plugin
1. Open the Jellyfin Web Dashboard as an administrator.
2. Go to **Dashboard** $\rightarrow$ **Plugins** $\rightarrow$ **Catalog**.
3. Locate the official **Webhook** plugin and install it. Restart your Jellyfin server if prompted.

### 2. Creating the Webhook Destination
1. In the Jellyfin Dashboard, go to **Plugins** $\rightarrow$ **Webhook** and select **Add Generic Destination**.
2. Set the Destination Name to `BingeSync`.
3. Set the Webhook URL to your personal Jellyfin URL from BingeSync Settings:
   ```
   http://<bingesync-host>:8080/api/v1/webhooks/jellyfin/<your_webhook_token>
   ```
4. Under **Notification Types**, check:
   - `Playback Start`
   - `Playback Progress`
   - `Playback Stop`
   - `User Data Saved`
5. Under **Item Types**, select `Movies` and `Episodes`.
6. Click **Save**.

### 3. Generating a Jellyfin API Key
For watch history sync (writeback), BingeSync requires an API key to communicate with Jellyfin's REST API:
1. Open the Jellyfin Dashboard.
2. Under **Advanced** in the left sidebar, click **API Keys**.
3. Click the **+** button to create a new key.
4. Enter `BingeSync` as the App Name and click **OK**.
5. Copy the generated API key and paste it into the **Jellyfin API Key** field in BingeSync.

---

## Emby Webhook Configuration

1. In Emby Web, click the **Gear icon** (Settings) $\rightarrow$ **Server** $\rightarrow$ **Webhooks** *(Emby Premiere required)*.
2. Click **Add Webhook** and choose **Generic Webhook**.
3. Set the Webhook URL to:
   ```
   http://<bingesync-host>:8080/api/v1/webhooks/emby/<your_webhook_token>
   ```
4. Enable **Playback Events** (`playback.start`, `playback.pause`, `playback.unpause`, `playback.stop`) and **User Data Events** (`user.data.saved`).
5. Save the webhook.

---

## Bidirectional Media Server Sync

Many home media setups run both a **Plex Media Server** and a **Jellyfin** server simultaneously (for mobile streaming, LAN playback, or backup redundancy). BingeSync solves the synchronization dilemma natively:

```
                  ┌────────────────────────┐
                  │       BingeSync        │
                  │   (SQLite Central Hub) │
                  └──────┬──────────┬──────┘
       Plex Webhook      │          │      Jellyfin Webhook
     (Media Watched)     ▼          ▼     (Media Watched)
  ┌────────────────────────┐      ┌────────────────────────┐
  │   Plex Media Server    │      │  Jellyfin Media Server │
  │   (Automatic Writeback)│◄────►│  (Automatic Writeback) │
  └────────────────────────┘      └────────────────────────┘
```

### How It Works
- **Immediate Writeback**: When a scrobble is recorded from Plex, BingeSync automatically calls Jellyfin's `/Users/{id}/PlayedItems/{itemId}` endpoint to mark the item as played. When a watch event is received from Jellyfin, BingeSync marks the item as watched on Plex (`/:/scrobble`).
- **Per-Service Controls**: In **User Settings** $\rightarrow$ **Connected Services**, check boxes allow enabling watch history sync independently:
  - `[x] Enable Watch History Sync to Plex`
  - `[x] Enable Watch History Sync to Jellyfin`
- **Scheduled Synchronization**: The background worker **`Watch History Sync (Plex & Jellyfin)`** runs on a schedule (default: every 4 hours) to verify recent scrobbles and catch up on any items watched while a server was offline or unreachable.
- **Manual Trigger**: Click **Sync Recent Watches to Media Server** under Connected Services to push your 25-100 most recent watches to your media servers immediately.

---

## Tautulli Continuous 1-Way Sync

If you maintain a Tautulli instance monitoring your Plex server:
1. In **User Settings** $\rightarrow$ **Connected Services**, enter your **Tautulli Server URL** and **Tautulli API Key**.
2. Click **Fetch Users** to retrieve all Plex users active on your server and select your username.
3. Check **Enable Continuous 1-Way Sync from Tautulli**.
4. The background worker **`Tautulli Continuous 1-Way Sync`** polls Tautulli every hour (customizable in Admin Tasks) and imports any new completed watches into BingeSync with 3-way deduplication (exact timestamp, same-day match, and Plex rating key match).

