use sqlx::SqlitePool;

pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY NOT NULL,
            email TEXT UNIQUE NOT NULL,
            display_name TEXT NOT NULL,
            oidc_sub TEXT UNIQUE,
            webhook_token TEXT UNIQUE NOT NULL,
            avatar_url TEXT,
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

    // Indexes on added columns
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_user_status ON scrobbles(user_id, watch_status)")
        .execute(pool)
        .await;

    Ok(())
}
