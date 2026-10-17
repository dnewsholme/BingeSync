use sqlx::SqlitePool;

pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY NOT NULL,
            email TEXT UNIQUE NOT NULL,
            display_name TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'user',
            oidc_sub TEXT UNIQUE,
            oidc_sid TEXT,
            preferred_username TEXT,
            webhook_token TEXT UNIQUE NOT NULL,
            avatar_url TEXT,
            sessions_revoked_at INTEGER,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_users_webhook_token ON users(webhook_token);
        CREATE INDEX IF NOT EXISTS idx_users_oidc_sub ON users(oidc_sub);

        CREATE TABLE IF NOT EXISTS scrobbles (
            id TEXT PRIMARY KEY NOT NULL,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            media_type TEXT NOT NULL,
            grandparent_title TEXT,
            parent_title TEXT,
            season_number INTEGER,
            episode_number INTEGER,
            year INTEGER,
            overview TEXT,
            thumb_url TEXT,
            rating_key TEXT,
            guid TEXT,
            duration_ms INTEGER,
            user_rating INTEGER,
            genres TEXT,
            directors TEXT,
            actors TEXT,
            watch_status TEXT NOT NULL DEFAULT 'completed',
            progress_percent INTEGER DEFAULT NULL,
            view_offset_ms INTEGER DEFAULT NULL,
            metadata_updated_at TEXT DEFAULT NULL,
            watched_at TEXT NOT NULL DEFAULT (datetime('now')),
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_scrobbles_user_watched ON scrobbles(user_id, watched_at DESC);
        CREATE INDEX IF NOT EXISTS idx_scrobbles_user_media ON scrobbles(user_id, media_type);
        CREATE INDEX IF NOT EXISTS idx_scrobbles_user_title ON scrobbles(user_id, title);
        CREATE INDEX IF NOT EXISTS idx_scrobbles_user_show ON scrobbles(user_id, grandparent_title);

        CREATE TABLE IF NOT EXISTS active_sessions (
            id TEXT PRIMARY KEY NOT NULL,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            player_title TEXT,
            title TEXT NOT NULL,
            media_type TEXT NOT NULL,
            grandparent_title TEXT,
            season_number INTEGER,
            episode_number INTEGER,
            year INTEGER,
            thumb_url TEXT,
            state TEXT NOT NULL,
            view_offset_ms INTEGER NOT NULL DEFAULT 0,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_user ON active_sessions(user_id);

        CREATE TABLE IF NOT EXISTS oidc_states (
            state TEXT PRIMARY KEY NOT NULL,
            nonce TEXT NOT NULL,
            code_verifier TEXT NOT NULL,
            redirect_uri TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await?;

    // Idempotent column addition for existing databases
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'user'")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_users_role ON users(role)")
        .execute(pool)
        .await;

    let _ = sqlx::query("ALTER TABLE users ADD COLUMN oidc_sid TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_users_oidc_sid ON users(oidc_sid)")
        .execute(pool)
        .await;

    let _ = sqlx::query("ALTER TABLE users ADD COLUMN sessions_revoked_at INTEGER")
        .execute(pool)
        .await;

    let _ = sqlx::query("ALTER TABLE users ADD COLUMN preferred_username TEXT")
        .execute(pool)
        .await;

    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN user_rating INTEGER")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN genres TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN directors TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN actors TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN watch_status TEXT NOT NULL DEFAULT 'completed'")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN progress_percent INTEGER DEFAULT NULL")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN view_offset_ms INTEGER DEFAULT NULL")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN metadata_updated_at TEXT DEFAULT NULL")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_metadata_updated ON scrobbles(metadata_updated_at)")
        .execute(pool)
        .await;

    // Clean up any internal Plex relative thumb paths (e.g. /library/metadata/...) so background workers can fetch real posters
    let _ = sqlx::query("UPDATE scrobbles SET thumb_url = NULL WHERE thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%'")
        .execute(pool)
        .await;

    // Mark existing scrobbles that already have overview and genres as having metadata obtained
    let _ = sqlx::query("UPDATE scrobbles SET metadata_updated_at = datetime('now') WHERE metadata_updated_at IS NULL AND overview IS NOT NULL AND overview != '' AND genres IS NOT NULL AND genres != ''")
        .execute(pool)
        .await;

    // System Settings table for runtime admin configuration
    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS system_settings (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await;

    // User Settings table for per-user integrations (Radarr, Sonarr, Tautulli)
    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_settings (
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (user_id, key)
        )
        "#,
    )
    .execute(pool)
    .await;

    // Cache of Plex user IDs to usernames (populated from Tautulli / Plex)
    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS plex_user_cache (
            plex_id TEXT NOT NULL PRIMARY KEY,
            username TEXT NOT NULL,
            friendly_name TEXT,
            source TEXT NOT NULL DEFAULT 'tautulli',
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_plex_user_cache_username ON plex_user_cache(username)"
    )
    .execute(pool)
    .await;

    Ok(())
}
