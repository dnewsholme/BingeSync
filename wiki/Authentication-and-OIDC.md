# Authentication & Access Control

BingeSync supports enterprise-grade OpenID Connect (OIDC) Single Sign-On with PKCE, OpenID Back-Channel Logout 1.0, role-based access control, dev login, and scoped personal API keys.

---

## OpenID Connect (SSO)

BingeSync implements RFC 7636 (Proof Key for Code Exchange by OAuth Public Clients) and OpenID Connect Core 1.0.

### Environment Setup
To enable OIDC via environment variables:

```bash
OIDC_ENABLED=true
OIDC_ISSUER_URL=https://auth.example.com/application/o/bingesync/
OIDC_CLIENT_ID=your-client-id
OIDC_CLIENT_SECRET=your-client-secret
OIDC_REDIRECT_URL=https://bingesync.example.com/api/v1/auth/callback
```

### Provider Client Settings
In your identity provider (Authentik, Keycloak, Authelia, PocketID, Google, Okta):
- **Client Type**: Confidential
- **Redirect / Callback URI**: `https://bingesync.example.com/api/v1/auth/callback`
- **Scopes**: `openid`, `profile`, `email`, `groups`
- **PKCE**: S256 (recommended / supported)

---

## OIDC Back-Channel Logout

BingeSync implements [OpenID Connect Back-Channel Logout 1.0](https://openid.net/specs/openid-connect-backchannel-1_0.html). When an employee or user is terminated, access revoked, or group membership removed in your IdP, the IdP sends a `logout_token` directly to BingeSync:

- **Back-Channel Logout URL**: `https://bingesync.example.com/api/v1/auth/backchannel-logout`
- **Supported Tokens**: Signed JWT with either subject identifier (`sub`) or session ID (`sid`).
- **Immediate Invalidation**: Upon receipt, BingeSync terminates all active playback sessions, clears refresh tokens, and immediately blocks further API requests using previously issued JWTs.

---

## Admin and User Group Mapping

Role-based access control is determined by evaluating the `groups` claim in the OIDC ID token:

### Administrator Promotion (`OIDC_ADMIN_GROUP`)
- By default, users in groups named `admin`, `admins`, `administrator`, or `administrators` (case-insensitive) are assigned the `admin` role.
- Custom admin groups can be specified via `OIDC_ADMIN_GROUP` or in **Admin Settings**:
  ```bash
  OIDC_ADMIN_GROUP="homelab-admins, devops-team"
  ```

### Standard User Access Restriction (`OIDC_USER_GROUP`)
- To prevent unauthorized users in your IdP from accessing BingeSync:
  ```bash
  OIDC_USER_GROUP="family, friends, media-viewers"
  ```
- If set, any non-admin user whose groups list does not match at least one of the configured names is rejected with `403 Forbidden` (`reason: "user_group_membership_required"`).

---

## Personal Scoped API Keys

Users can generate personal API keys to connect scripts, CLI tools, Home Assistant, or third-party automations:

### 1. Generating an API Key
1. Navigate to **User Settings** $\rightarrow$ **API Keys**.
2. Click **Generate New Key**.
3. Provide a friendly name (e.g. `Home Assistant Automation`) and choose scopes (`scrobble:write`, `history:read`, `stats:read`).
4. Copy your secret API key immediately (`bs_live_...`). BingeSync stores only a salted cryptographic SHA-256 hash in SQLite.

### 2. Using an API Key in Requests
BingeSync accepts API keys in three interchangeable ways:

```bash
# 1. Via X-Api-Key Header (Recommended)
curl -H "X-Api-Key: bs_live_xxxxxxxxxxxxxxxxxxxxxxxx" https://bingesync.example.com/api/v1/stats

# 2. Via Authorization Bearer Header
curl -H "Authorization: Bearer bs_live_xxxxxxxxxxxxxxxxxxxxxxxx" https://bingesync.example.com/api/v1/history

# 3. Via Query Parameter
curl "https://bingesync.example.com/api/v1/now-playing?api_key=bs_live_xxxxxxxxxxxxxxxxxxxxxxxx"
```

All API key requests run in the context of the user who generated them, strictly adhering to multi-user isolation.

