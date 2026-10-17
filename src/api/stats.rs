use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use chrono::{Duration, NaiveDate, Utc};
use std::collections::HashSet;

use crate::{
    api::AppState,
    auth::AuthUser,
    models::{StatsSummary, TopShowItem},
};

pub async fn get_user_stats(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<StatsSummary>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Total counts
    let total_scrobbles: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scrobbles WHERE user_id = ?"
    )
    .bind(&auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let movies_watched: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND media_type = 'movie'"
    )
    .bind(&auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let episodes_watched: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND media_type = 'episode'"
    )
    .bind(&auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let total_duration_ms: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(duration_ms) FROM scrobbles WHERE user_id = ?"
    )
    .bind(&auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(None);

    let total_minutes = total_duration_ms.unwrap_or(0) / 60_000;

    let movie_duration_ms: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(duration_ms) FROM scrobbles WHERE user_id = ? AND media_type = 'movie'"
    )
    .bind(&auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(None);
    let movie_minutes = movie_duration_ms.unwrap_or(0) / 60_000;

    let ep_duration_ms: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(duration_ms) FROM scrobbles WHERE user_id = ? AND media_type = 'episode'"
    )
    .bind(&auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(None);
    let episode_minutes = ep_duration_ms.unwrap_or(0) / 60_000;

    // 2. Top TV shows
    let top_shows_rows: Vec<(String, i64, Option<String>)> = sqlx::query_as(
        r#"
        SELECT grandparent_title, COUNT(*) as cnt, MAX(thumb_url) as thumb
        FROM scrobbles
        WHERE user_id = ? AND media_type = 'episode' AND grandparent_title IS NOT NULL
        GROUP BY grandparent_title
        ORDER BY cnt DESC
        LIMIT 5
        "#,
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let top_shows = top_shows_rows
        .into_iter()
        .map(|(show_title, count, thumb_url)| TopShowItem {
            show_title,
            count,
            thumb_url,
        })
        .collect();

    // 3. Current streak calculation (in days)
    let watch_dates_raw: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT substr(watched_at, 1, 10) as dt
        FROM scrobbles
        WHERE user_id = ?
        ORDER BY dt DESC
        LIMIT 60
        "#,
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let date_set: HashSet<NaiveDate> = watch_dates_raw
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();

    let today = Utc::now().date_naive();
    let yesterday = today - Duration::days(1);

    let mut current_streak_days = 0;
    let mut check_date = if date_set.contains(&today) {
        today
    } else if date_set.contains(&yesterday) {
        yesterday
    } else {
        today
    };

    if date_set.contains(&check_date) {
        while date_set.contains(&check_date) {
            current_streak_days += 1;
            check_date -= Duration::days(1);
        }
    }

    // 4. Daily activity (past 14 days)
    let daily_rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT substr(watched_at, 1, 10) as dt, media_type
        FROM scrobbles
        WHERE user_id = ? AND watched_at >= date('now', '-14 days')
        ORDER BY dt ASC
        "#,
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut daily_map: std::collections::BTreeMap<String, (i64, i64)> = std::collections::BTreeMap::new();
    for (dt, m_type) in daily_rows {
        let entry = daily_map.entry(dt).or_insert((0, 0));
        if m_type == "movie" {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }

    let daily_activity: Vec<crate::models::DailyActivityItem> = daily_map
        .into_iter()
        .map(|(date, (movies, episodes))| crate::models::DailyActivityItem {
            date,
            movies,
            episodes,
            total: movies + episodes,
        })
        .collect();

    // 5. Top genres
    let genre_rows: Vec<String> = sqlx::query_scalar(
        "SELECT genres FROM scrobbles WHERE user_id = ? AND genres IS NOT NULL AND genres != ''"
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut genre_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for g_str in genre_rows {
        for g in g_str.split(',') {
            let trimmed = g.trim();
            if !trimmed.is_empty() {
                *genre_counts.entry(trimmed.to_string()).or_insert(0) += 1;
            }
        }
    }
    let mut top_genres: Vec<crate::models::GenreCountItem> = genre_counts
        .into_iter()
        .map(|(genre, count)| crate::models::GenreCountItem { genre, count })
        .collect();
    top_genres.sort_by(|a, b| b.count.cmp(&a.count));
    top_genres.truncate(6);

    Ok(Json(StatsSummary {
        total_scrobbles,
        movies_watched,
        episodes_watched,
        total_minutes,
        movie_minutes,
        episode_minutes,
        current_streak_days,
        top_shows,
        daily_activity,
        top_genres,
    }))
}
