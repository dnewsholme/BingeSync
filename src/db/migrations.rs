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
            director TEXT,
            cinematographer TEXT,
            tvdb_id TEXT,
            tmdb_id TEXT,
            actors TEXT,
            watch_status TEXT NOT NULL DEFAULT 'completed',
            progress_percent INTEGER DEFAULT NULL,
            view_offset_ms INTEGER DEFAULT NULL,
            metadata_updated_at TEXT DEFAULT NULL,
            tags TEXT,
            notes TEXT,
            source TEXT,
            source_user TEXT,
            imported_from TEXT,
            player_name TEXT,
            device_name TEXT,
            platform TEXT,
            is_local BOOLEAN DEFAULT NULL,
            imdb_id TEXT,
            resolution TEXT,
            hdr_format TEXT,
            audio_codec TEXT,
            audio_channels TEXT,
            video_codec TEXT,
            studio TEXT,
            original_language TEXT,
            content_rating TEXT,
            is_rewatch BOOLEAN DEFAULT 0,
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
            source TEXT,
            source_user TEXT,
            player_name TEXT,
            device_name TEXT,
            platform TEXT,
            is_local BOOLEAN DEFAULT NULL,
            resolution TEXT,
            hdr_format TEXT,
            audio_codec TEXT,
            audio_channels TEXT,
            video_codec TEXT,
            content_rating TEXT,
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
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN director TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN cinematographer TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("UPDATE scrobbles SET director = directors WHERE (director IS NULL OR director = '') AND directors IS NOT NULL")
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
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN tvdb_id TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_tvdb_id ON scrobbles(tvdb_id)")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN tmdb_id TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_tmdb_id ON scrobbles(tmdb_id)")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_metadata_updated ON scrobbles(metadata_updated_at)")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN tags TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN notes TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_tags ON scrobbles(tags)")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN source TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN source_user TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN imported_from TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_user_source ON scrobbles(user_id, source)")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_user_source_user ON scrobbles(user_id, source_user)")
        .execute(pool)
        .await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_user_imported_from ON scrobbles(user_id, imported_from)")
        .execute(pool)
        .await;

    // Migrate any previous tautulli scrobbles to source = 'plex' and imported_from = 'tautulli'
    let _ = sqlx::query("UPDATE scrobbles SET source = 'plex', imported_from = 'tautulli' WHERE source = 'tautulli'")
        .execute(pool)
        .await;
    let _ = sqlx::query("UPDATE scrobbles SET imported_from = 'trakt' WHERE source = 'trakt' AND (imported_from IS NULL OR imported_from = '')")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN source TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN source_user TEXT")
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

    // User API keys for third-party integrations
    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_api_keys (
            id TEXT PRIMARY KEY NOT NULL,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            key_hash TEXT NOT NULL UNIQUE,
            key_prefix TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_used_at TEXT DEFAULT NULL
        )
        "#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_user_api_keys_user ON user_api_keys(user_id)"
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_user_api_keys_hash ON user_api_keys(key_hash)"
    )
    .execute(pool)
    .await;

    // Rich Scrobble Metadata Extensions:
    // 1. Device & Client context
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN player_name TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN device_name TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN platform TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN is_local BOOLEAN DEFAULT NULL").execute(pool).await;

    // 2. IMDb ID
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN imdb_id TEXT").execute(pool).await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_imdb_id ON scrobbles(imdb_id)").execute(pool).await;

    // 3. Media Quality & Stream Technical Specs
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN resolution TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN hdr_format TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN audio_codec TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN audio_channels TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN video_codec TEXT").execute(pool).await;

    // 4. Studio, Network, Original Language & Content Rating
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN studio TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN original_language TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN content_rating TEXT").execute(pool).await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_studio ON scrobbles(studio)").execute(pool).await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_orig_lang ON scrobbles(original_language)").execute(pool).await;

    // 6. Letterboxd / Trakt viewing flags: Rewatch
    let _ = sqlx::query("ALTER TABLE scrobbles ADD COLUMN is_rewatch BOOLEAN DEFAULT 0").execute(pool).await;
    let _ = sqlx::query("UPDATE scrobbles SET is_rewatch = 0 WHERE is_rewatch IS NULL").execute(pool).await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_user_rewatch ON scrobbles(user_id, is_rewatch)").execute(pool).await;

    // Active session quality and device fields
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN player_name TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN device_name TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN platform TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN is_local BOOLEAN DEFAULT NULL").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN resolution TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN hdr_format TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN audio_codec TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN audio_channels TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN video_codec TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE active_sessions ADD COLUMN content_rating TEXT").execute(pool).await;

    // Indexes for TheTVDB & TMDB entity IDs
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_tvdb_id ON scrobbles(tvdb_id)").execute(pool).await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_scrobbles_tmdb_id ON scrobbles(tmdb_id)").execute(pool).await;

    // Fast local extraction of tvdb_id, tmdb_id, and imdb_id from guid values where missing
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET tvdb_id = CASE
            WHEN guid LIKE 'tvdb://%' THEN SUBSTR(guid, 8)
            WHEN guid LIKE 'com.plexapp.agents.thetvdb://%?%' THEN SUBSTR(SUBSTR(guid, 30), 1, INSTR(SUBSTR(guid, 30), '?') - 1)
            WHEN guid LIKE 'com.plexapp.agents.thetvdb://%' THEN SUBSTR(guid, 30)
            ELSE tvdb_id
        END
        WHERE (tvdb_id IS NULL OR tvdb_id = '') AND media_type = 'movie' AND (guid LIKE 'tvdb://%' OR guid LIKE 'com.plexapp.agents.thetvdb://%');
        "#
    ).execute(pool).await;

    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET tvdb_id = CASE
            WHEN guid LIKE 'thetvdb.com/dereferrer/episode/%' THEN SUBSTR(guid, 32)
            WHEN guid LIKE 'tvdb://episode/%' THEN SUBSTR(guid, 16)
            ELSE tvdb_id
        END
        WHERE (tvdb_id IS NULL OR tvdb_id = '') AND media_type = 'episode' AND (guid LIKE 'thetvdb.com/dereferrer/episode/%' OR guid LIKE 'tvdb://episode/%');
        "#
    ).execute(pool).await;

    // For TV episodes, fix existing scrobbles:
    // If tvdb_id is empty, NULL, or was set to a series ID (< 1,000,000),
    // mark tvdb_id as series-{id} and reset metadata_updated_at to NULL so background worker enriches it
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET metadata_updated_at = NULL,
            tvdb_id = CASE
                WHEN tvdb_id IS NOT NULL AND tvdb_id != '' AND tvdb_id NOT LIKE 'series-%' AND tvdb_id NOT LIKE 'episode-%' AND tvdb_id NOT LIKE 'ep-%' AND CAST(tvdb_id AS INTEGER) > 0 AND CAST(tvdb_id AS INTEGER) < 1000000
                THEN 'series-' || tvdb_id
                ELSE tvdb_id
            END
        WHERE media_type = 'episode' AND (
            tvdb_id IS NULL
            OR tvdb_id = ''
            OR tvdb_id LIKE 'series-%'
            OR (CAST(tvdb_id AS INTEGER) > 0 AND CAST(tvdb_id AS INTEGER) < 1000000)
            OR guid = 'tvdb://' || tvdb_id
            OR guid LIKE 'com.plexapp.agents.thetvdb://%'
        );
        "#
    ).execute(pool).await;

    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET tmdb_id = CASE
            WHEN guid LIKE 'tmdb://%' THEN SUBSTR(guid, 8)
            WHEN guid LIKE 'com.plexapp.agents.themoviedb://%?%' THEN SUBSTR(SUBSTR(guid, 33), 1, INSTR(SUBSTR(guid, 33), '?') - 1)
            WHEN guid LIKE 'com.plexapp.agents.themoviedb://%' THEN SUBSTR(guid, 33)
            ELSE tmdb_id
        END
        WHERE (tmdb_id IS NULL OR tmdb_id = '') AND media_type = 'movie' AND (guid LIKE 'tmdb://%' OR guid LIKE 'com.plexapp.agents.themoviedb://%');
        "#
    ).execute(pool).await;

    // For TV episodes, clear any tmdb_id that was erroneously set to the individual episode's guid
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET tmdb_id = NULL, metadata_updated_at = NULL
        WHERE media_type = 'episode' AND tmdb_id IS NOT NULL AND (
            guid = 'tmdb://' || tmdb_id
            OR guid LIKE 'com.plexapp.agents.themoviedb://' || tmdb_id || '%'
        );
        "#
    ).execute(pool).await;

    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET imdb_id = CASE
            WHEN guid LIKE 'imdb://%' THEN SUBSTR(guid, 8)
            WHEN guid LIKE 'com.plexapp.agents.imdb://%?%' THEN SUBSTR(SUBSTR(guid, 27), 1, INSTR(SUBSTR(guid, 27), '?') - 1)
            WHEN guid LIKE 'com.plexapp.agents.imdb://%' THEN SUBSTR(guid, 27)
            ELSE imdb_id
        END
        WHERE (imdb_id IS NULL OR imdb_id = '') AND (guid LIKE 'imdb://%' OR guid LIKE 'com.plexapp.agents.imdb://%');
        "#
    ).execute(pool).await;

    // Retroactively propagate ratings across previous and subsequent watches of the same movie
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET user_rating = (
            SELECT s2.user_rating
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'movie'
              AND s2.user_rating IS NOT NULL
              AND (
                  (TRIM(LOWER(s2.title)) = TRIM(LOWER(scrobbles.title)) AND (scrobbles.year IS NULL OR s2.year IS NULL OR s2.year = scrobbles.year))
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
                  OR (scrobbles.imdb_id IS NOT NULL AND s2.imdb_id = scrobbles.imdb_id)
              )
            ORDER BY s2.watched_at DESC
            LIMIT 1
        )
        WHERE media_type = 'movie' AND user_rating IS NULL AND EXISTS (
            SELECT 1
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'movie'
              AND s2.user_rating IS NOT NULL
              AND (
                  (TRIM(LOWER(s2.title)) = TRIM(LOWER(scrobbles.title)) AND (scrobbles.year IS NULL OR s2.year IS NULL OR s2.year = scrobbles.year))
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
                  OR (scrobbles.imdb_id IS NOT NULL AND s2.imdb_id = scrobbles.imdb_id)
              )
        );
        "#
    ).execute(pool).await;

    // Retroactively propagate ratings across previous and subsequent watches of the same TV episode
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET user_rating = (
            SELECT s2.user_rating
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'episode'
              AND s2.user_rating IS NOT NULL
              AND (
                  (
                      scrobbles.season_number IS NOT NULL AND s2.season_number = scrobbles.season_number
                      AND scrobbles.episode_number IS NOT NULL AND s2.episode_number = scrobbles.episode_number
                      AND TRIM(LOWER(COALESCE(s2.grandparent_title, ''))) = TRIM(LOWER(COALESCE(scrobbles.grandparent_title, '')))
                  )
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tvdb_id IS NOT NULL AND s2.tvdb_id = scrobbles.tvdb_id AND scrobbles.tvdb_id NOT LIKE 'series-%')
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
              )
            ORDER BY s2.watched_at DESC
            LIMIT 1
        )
        WHERE media_type = 'episode' AND user_rating IS NULL AND EXISTS (
            SELECT 1
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'episode'
              AND s2.user_rating IS NOT NULL
              AND (
                  (
                      scrobbles.season_number IS NOT NULL AND s2.season_number = scrobbles.season_number
                      AND scrobbles.episode_number IS NOT NULL AND s2.episode_number = scrobbles.episode_number
                      AND TRIM(LOWER(COALESCE(s2.grandparent_title, ''))) = TRIM(LOWER(COALESCE(scrobbles.grandparent_title, '')))
                  )
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tvdb_id IS NOT NULL AND s2.tvdb_id = scrobbles.tvdb_id AND scrobbles.tvdb_id NOT LIKE 'series-%')
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
              )
        );
        "#
    ).execute(pool).await;

    // Retroactively propagate tags across previous and subsequent watches of the same movie
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET tags = (
            SELECT s2.tags
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'movie'
              AND s2.tags IS NOT NULL AND s2.tags != ''
              AND (
                  (TRIM(LOWER(s2.title)) = TRIM(LOWER(scrobbles.title)) AND (scrobbles.year IS NULL OR s2.year IS NULL OR s2.year = scrobbles.year))
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
                  OR (scrobbles.imdb_id IS NOT NULL AND s2.imdb_id = scrobbles.imdb_id)
              )
            ORDER BY s2.watched_at DESC
            LIMIT 1
        )
        WHERE media_type = 'movie' AND (tags IS NULL OR tags = '') AND EXISTS (
            SELECT 1
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'movie'
              AND s2.tags IS NOT NULL AND s2.tags != ''
              AND (
                  (TRIM(LOWER(s2.title)) = TRIM(LOWER(scrobbles.title)) AND (scrobbles.year IS NULL OR s2.year IS NULL OR s2.year = scrobbles.year))
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
                  OR (scrobbles.imdb_id IS NOT NULL AND s2.imdb_id = scrobbles.imdb_id)
              )
        );
        "#
    ).execute(pool).await;

    // Retroactively propagate notes across previous and subsequent watches of the same movie
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET notes = (
            SELECT s2.notes
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'movie'
              AND s2.notes IS NOT NULL AND s2.notes != ''
              AND (
                  (TRIM(LOWER(s2.title)) = TRIM(LOWER(scrobbles.title)) AND (scrobbles.year IS NULL OR s2.year IS NULL OR s2.year = scrobbles.year))
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
                  OR (scrobbles.imdb_id IS NOT NULL AND s2.imdb_id = scrobbles.imdb_id)
              )
            ORDER BY s2.watched_at DESC
            LIMIT 1
        )
        WHERE media_type = 'movie' AND (notes IS NULL OR notes = '') AND EXISTS (
            SELECT 1
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'movie'
              AND s2.notes IS NOT NULL AND s2.notes != ''
              AND (
                  (TRIM(LOWER(s2.title)) = TRIM(LOWER(scrobbles.title)) AND (scrobbles.year IS NULL OR s2.year IS NULL OR s2.year = scrobbles.year))
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
                  OR (scrobbles.imdb_id IS NOT NULL AND s2.imdb_id = scrobbles.imdb_id)
              )
        );
        "#
    ).execute(pool).await;

    // Retroactively propagate tags across previous and subsequent watches of the same TV episode
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET tags = (
            SELECT s2.tags
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'episode'
              AND s2.tags IS NOT NULL AND s2.tags != ''
              AND (
                  (
                      scrobbles.season_number IS NOT NULL AND s2.season_number = scrobbles.season_number
                      AND scrobbles.episode_number IS NOT NULL AND s2.episode_number = scrobbles.episode_number
                      AND TRIM(LOWER(COALESCE(s2.grandparent_title, ''))) = TRIM(LOWER(COALESCE(scrobbles.grandparent_title, '')))
                  )
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tvdb_id IS NOT NULL AND s2.tvdb_id = scrobbles.tvdb_id AND scrobbles.tvdb_id NOT LIKE 'series-%')
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
              )
            ORDER BY s2.watched_at DESC
            LIMIT 1
        )
        WHERE media_type = 'episode' AND (tags IS NULL OR tags = '') AND EXISTS (
            SELECT 1
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'episode'
              AND s2.tags IS NOT NULL AND s2.tags != ''
              AND (
                  (
                      scrobbles.season_number IS NOT NULL AND s2.season_number = scrobbles.season_number
                      AND scrobbles.episode_number IS NOT NULL AND s2.episode_number = scrobbles.episode_number
                      AND TRIM(LOWER(COALESCE(s2.grandparent_title, ''))) = TRIM(LOWER(COALESCE(scrobbles.grandparent_title, '')))
                  )
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tvdb_id IS NOT NULL AND s2.tvdb_id = scrobbles.tvdb_id AND scrobbles.tvdb_id NOT LIKE 'series-%')
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
              )
        );
        "#
    ).execute(pool).await;

    // Retroactively propagate notes across previous and subsequent watches of the same TV episode
    let _ = sqlx::query(
        r#"
        UPDATE scrobbles
        SET notes = (
            SELECT s2.notes
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'episode'
              AND s2.notes IS NOT NULL AND s2.notes != ''
              AND (
                  (
                      scrobbles.season_number IS NOT NULL AND s2.season_number = scrobbles.season_number
                      AND scrobbles.episode_number IS NOT NULL AND s2.episode_number = scrobbles.episode_number
                      AND TRIM(LOWER(COALESCE(s2.grandparent_title, ''))) = TRIM(LOWER(COALESCE(scrobbles.grandparent_title, '')))
                  )
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tvdb_id IS NOT NULL AND s2.tvdb_id = scrobbles.tvdb_id AND scrobbles.tvdb_id NOT LIKE 'series-%')
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
              )
            ORDER BY s2.watched_at DESC
            LIMIT 1
        )
        WHERE media_type = 'episode' AND (notes IS NULL OR notes = '') AND EXISTS (
            SELECT 1
            FROM scrobbles s2
            WHERE s2.user_id = scrobbles.user_id
              AND s2.media_type = 'episode'
              AND s2.notes IS NOT NULL AND s2.notes != ''
              AND (
                  (
                      scrobbles.season_number IS NOT NULL AND s2.season_number = scrobbles.season_number
                      AND scrobbles.episode_number IS NOT NULL AND s2.episode_number = scrobbles.episode_number
                      AND TRIM(LOWER(COALESCE(s2.grandparent_title, ''))) = TRIM(LOWER(COALESCE(scrobbles.grandparent_title, '')))
                  )
                  OR (scrobbles.guid IS NOT NULL AND s2.guid = scrobbles.guid)
                  OR (scrobbles.tvdb_id IS NOT NULL AND s2.tvdb_id = scrobbles.tvdb_id AND scrobbles.tvdb_id NOT LIKE 'series-%')
                  OR (scrobbles.tmdb_id IS NOT NULL AND s2.tmdb_id = scrobbles.tmdb_id)
              )
        );
        "#
    ).execute(pool).await;

    Ok(())
}
