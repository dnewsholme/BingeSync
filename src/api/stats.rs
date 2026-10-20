use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration, NaiveDate, Utc};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    api::AppState,
    auth::AuthUser,
    models::{
        DailyActivityItem, DayOfWeekActivity, GenreCountItem, StatsSummary, TimeOfDayActivity,
        TopMovieItem, TopPersonItem, TopShowItem,
    },
};

#[derive(Debug, Deserialize, Default)]
pub struct StatsQuery {
    pub media_type: Option<String>,
    pub time_range: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

pub async fn get_user_stats(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<StatsQuery>,
) -> Result<Json<StatsSummary>, (StatusCode, Json<serde_json::Value>)> {
    let media_type_param = query.media_type.as_deref().unwrap_or("all").to_lowercase();
    let is_movie_filter = media_type_param == "movie";
    let is_tv_filter = media_type_param == "episode" || media_type_param == "tv";

    // Build time range filter clause
    let (time_clause, time_start, time_end) = match query.time_range.as_deref().map(|s| s.to_lowercase()) {
        Some(ref r) if r == "7d" => (Some("watched_at >= datetime('now', '-7 days')".to_string()), None, None),
        Some(ref r) if r == "30d" => (Some("watched_at >= datetime('now', '-30 days')".to_string()), None, None),
        Some(ref r) if r == "90d" => (Some("watched_at >= datetime('now', '-90 days')".to_string()), None, None),
        Some(ref r) if r == "year" || r == "365d" => (Some("watched_at >= datetime('now', '-365 days')".to_string()), None, None),
        _ => {
            if let (Some(ref s), Some(ref e)) = (&query.start_date, &query.end_date) {
                (Some("watched_at >= ? AND watched_at <= ?".to_string()), Some(s.clone()), Some(e.clone()))
            } else if let Some(ref s) = query.start_date {
                (Some("watched_at >= ?".to_string()), Some(s.clone()), None)
            } else if let Some(ref e) = query.end_date {
                (Some("watched_at <= ?".to_string()), None, Some(e.clone()))
            } else {
                (None, None, None)
            }
        }
    };

    let time_clause_str = match &time_clause {
        Some(clause) => format!(" AND {}", clause),
        None => "".to_string(),
    };

    let media_clause = if is_movie_filter {
        " AND media_type = 'movie'"
    } else if is_tv_filter {
        " AND media_type = 'episode'"
    } else {
        ""
    };

    // 1. Movie metrics
    let (movies_watched, unique_movies, movie_minutes) = if is_tv_filter {
        (0, 0, 0)
    } else {
        let movies_sql = format!(
            "SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND media_type = 'movie'{}",
            time_clause_str
        );
        let mut q = sqlx::query_scalar::<_, i64>(&movies_sql).bind(&auth.user_id);
        if let Some(ref s) = time_start { q = q.bind(s); }
        if let Some(ref e) = time_end { q = q.bind(e); }
        let mw = q.fetch_one(&state.pool).await.unwrap_or(0);

        let unique_movies_sql = format!(
            "SELECT COUNT(DISTINCT title) FROM scrobbles WHERE user_id = ? AND media_type = 'movie'{}",
            time_clause_str
        );
        let mut q = sqlx::query_scalar::<_, i64>(&unique_movies_sql).bind(&auth.user_id);
        if let Some(ref s) = time_start { q = q.bind(s); }
        if let Some(ref e) = time_end { q = q.bind(e); }
        let um = q.fetch_one(&state.pool).await.unwrap_or(0);

        let movie_dur_sql = format!(
            "SELECT SUM(duration_ms) FROM scrobbles WHERE user_id = ? AND media_type = 'movie'{}",
            time_clause_str
        );
        let mut q = sqlx::query_scalar::<_, Option<i64>>(&movie_dur_sql).bind(&auth.user_id);
        if let Some(ref s) = time_start { q = q.bind(s); }
        if let Some(ref e) = time_end { q = q.bind(e); }
        let mm = q.fetch_one(&state.pool).await.unwrap_or(None).unwrap_or(0) / 60_000;

        (mw, um, mm)
    };

    // 2. TV episode metrics
    let (episodes_watched, unique_shows, episode_minutes) = if is_movie_filter {
        (0, 0, 0)
    } else {
        let episodes_sql = format!(
            "SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND media_type = 'episode'{}",
            time_clause_str
        );
        let mut q = sqlx::query_scalar::<_, i64>(&episodes_sql).bind(&auth.user_id);
        if let Some(ref s) = time_start { q = q.bind(s); }
        if let Some(ref e) = time_end { q = q.bind(e); }
        let ew = q.fetch_one(&state.pool).await.unwrap_or(0);

        let unique_shows_sql = format!(
            "SELECT COUNT(DISTINCT grandparent_title) FROM scrobbles WHERE user_id = ? AND media_type = 'episode' AND grandparent_title IS NOT NULL{}",
            time_clause_str
        );
        let mut q = sqlx::query_scalar::<_, i64>(&unique_shows_sql).bind(&auth.user_id);
        if let Some(ref s) = time_start { q = q.bind(s); }
        if let Some(ref e) = time_end { q = q.bind(e); }
        let us = q.fetch_one(&state.pool).await.unwrap_or(0);

        let ep_dur_sql = format!(
            "SELECT SUM(duration_ms) FROM scrobbles WHERE user_id = ? AND media_type = 'episode'{}",
            time_clause_str
        );
        let mut q = sqlx::query_scalar::<_, Option<i64>>(&ep_dur_sql).bind(&auth.user_id);
        if let Some(ref s) = time_start { q = q.bind(s); }
        if let Some(ref e) = time_end { q = q.bind(e); }
        let em = q.fetch_one(&state.pool).await.unwrap_or(None).unwrap_or(0) / 60_000;

        (ew, us, em)
    };

    // 3. Totals (matching active filter)
    let (total_scrobbles, total_minutes) = if is_movie_filter {
        (movies_watched, movie_minutes)
    } else if is_tv_filter {
        (episodes_watched, episode_minutes)
    } else {
        (movies_watched + episodes_watched, movie_minutes + episode_minutes)
    };

    // 4. Ratings and completion rate
    let rating_sql = format!(
        "SELECT AVG(user_rating), COUNT(CASE WHEN user_rating IS NOT NULL AND user_rating > 0 THEN 1 END), COUNT(CASE WHEN watch_status = 'completed' THEN 1 END), COUNT(*) FROM scrobbles WHERE user_id = ?{}{}",
        media_clause,
        time_clause_str
    );
    let mut q = sqlx::query_as::<_, (Option<f64>, i64, i64, i64)>(&rating_sql).bind(&auth.user_id);
    if let Some(ref s) = time_start { q = q.bind(s); }
    if let Some(ref e) = time_end { q = q.bind(e); }
    let (avg_rating, total_rated, completed_cnt, total_matching) = q.fetch_one(&state.pool).await.unwrap_or((None, 0, 0, 0));
    let average_rating = avg_rating.map(|r| (r * 10.0).round() / 10.0);
    let completion_rate_percent = if total_matching > 0 {
        ((completed_cnt as f64 / total_matching as f64) * 1000.0).round() / 10.0
    } else {
        100.0
    };

    // 5. Peak watching day in time range
    let peak_sql = format!(
        "SELECT substr(watched_at, 1, 10) as dt, COUNT(*) as cnt FROM scrobbles WHERE user_id = ?{}{} GROUP BY dt ORDER BY cnt DESC LIMIT 1",
        media_clause,
        time_clause_str
    );
    let mut q = sqlx::query_as::<_, (String, i64)>(&peak_sql).bind(&auth.user_id);
    if let Some(ref s) = time_start { q = q.bind(s); }
    if let Some(ref e) = time_end { q = q.bind(e); }
    let (peak_day, peak_day_count) = match q.fetch_optional(&state.pool).await.unwrap_or(None) {
        Some((dt, cnt)) => (Some(dt), cnt),
        None => (None, 0),
    };

    // 6. Day of week activity
    let dow_sql = format!(
        "SELECT strftime('%w', watched_at) as dow, COUNT(*) as cnt FROM scrobbles WHERE user_id = ?{}{} GROUP BY dow",
        media_clause,
        time_clause_str
    );
    let mut q = sqlx::query_as::<_, (String, i64)>(&dow_sql).bind(&auth.user_id);
    if let Some(ref s) = time_start { q = q.bind(s); }
    if let Some(ref e) = time_end { q = q.bind(e); }
    let dow_rows: Vec<(String, i64)> = q.fetch_all(&state.pool).await.unwrap_or_default();
    let mut dow_map: HashMap<u32, i64> = HashMap::new();
    for (dow_str, cnt) in dow_rows {
        if let Ok(num) = dow_str.parse::<u32>() {
            dow_map.insert(num, cnt);
        }
    }
    let days_order = [
        ("Mon", 1), ("Tue", 2), ("Wed", 3), ("Thu", 4),
        ("Fri", 5), ("Sat", 6), ("Sun", 0),
    ];
    let day_of_week_activity: Vec<DayOfWeekActivity> = days_order.iter().map(|(name, idx)| {
        DayOfWeekActivity {
            day: name.to_string(),
            day_index: *idx,
            count: *dow_map.get(idx).unwrap_or(&0),
        }
    }).collect();

    // 7. Time of day watching habits
    let tod_sql = format!(
        "SELECT strftime('%H', watched_at) as hr, COUNT(*) as cnt FROM scrobbles WHERE user_id = ?{}{} GROUP BY hr",
        media_clause,
        time_clause_str
    );
    let mut q = sqlx::query_as::<_, (String, i64)>(&tod_sql).bind(&auth.user_id);
    if let Some(ref s) = time_start { q = q.bind(s); }
    if let Some(ref e) = time_end { q = q.bind(e); }
    let hr_rows: Vec<(String, i64)> = q.fetch_all(&state.pool).await.unwrap_or_default();
    let mut morning = 0;   // 06:00 - 11:59
    let mut afternoon = 0; // 12:00 - 16:59
    let mut evening = 0;   // 17:00 - 21:59
    let mut night = 0;     // 22:00 - 05:59
    for (hr_str, cnt) in hr_rows {
        if let Ok(hr) = hr_str.parse::<u32>() {
            match hr {
                6..=11 => morning += cnt,
                12..=16 => afternoon += cnt,
                17..=21 => evening += cnt,
                _ => night += cnt,
            }
        }
    }
    let tod_total = (morning + afternoon + evening + night).max(1) as f64;
    let time_of_day_activity = vec![
        TimeOfDayActivity { period: "Morning".to_string(), count: morning, percentage: ((morning as f64 / tod_total) * 1000.0).round() / 10.0 },
        TimeOfDayActivity { period: "Afternoon".to_string(), count: afternoon, percentage: ((afternoon as f64 / tod_total) * 1000.0).round() / 10.0 },
        TimeOfDayActivity { period: "Evening".to_string(), count: evening, percentage: ((evening as f64 / tod_total) * 1000.0).round() / 10.0 },
        TimeOfDayActivity { period: "Night".to_string(), count: night, percentage: ((night as f64 / tod_total) * 1000.0).round() / 10.0 },
    ];

    // 8. Streak calculations & active days
    let dates_sql = format!(
        "SELECT DISTINCT substr(watched_at, 1, 10) as dt FROM scrobbles WHERE user_id = ?{} ORDER BY dt ASC",
        media_clause
    );
    let watch_dates_raw: Vec<String> = sqlx::query_scalar(&dates_sql)
        .bind(&auth.user_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut date_list: Vec<NaiveDate> = watch_dates_raw
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();
    date_list.sort();

    let mut longest_streak_days = 0;
    let mut current_run = 0;
    let mut prev_d: Option<NaiveDate> = None;
    for d in &date_list {
        if let Some(prev) = prev_d {
            if *d == prev + Duration::days(1) {
                current_run += 1;
            } else if *d != prev {
                current_run = 1;
            }
        } else {
            current_run = 1;
        }
        prev_d = Some(*d);
        if current_run > longest_streak_days {
            longest_streak_days = current_run;
        }
    }

    let date_set: HashSet<NaiveDate> = date_list.iter().cloned().collect();
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

    let active_days_count = date_set.len().max(1) as i64;
    let avg_daily_minutes = total_minutes / active_days_count;

    // 9. Top TV Shows (filtered by time range, empty if movie filter)
    let top_shows: Vec<TopShowItem> = if !is_movie_filter {
        let top_shows_sql = format!(
            r#"
            SELECT grandparent_title, COUNT(*) as cnt, MAX(thumb_url) as thumb
            FROM scrobbles
            WHERE user_id = ? AND media_type = 'episode' AND grandparent_title IS NOT NULL{}
            GROUP BY grandparent_title
            ORDER BY cnt DESC
            LIMIT 6
            "#,
            time_clause_str
        );
        let mut q = sqlx::query_as::<_, (String, i64, Option<String>)>(&top_shows_sql).bind(&auth.user_id);
        if let Some(ref s) = time_start { q = q.bind(s); }
        if let Some(ref e) = time_end { q = q.bind(e); }
        let rows: Vec<(String, i64, Option<String>)> = q.fetch_all(&state.pool).await.unwrap_or_default();
        rows.into_iter().map(|(show_title, count, thumb_url)| TopShowItem { show_title, count, thumb_url }).collect()
    } else {
        Vec::new()
    };

    // 10. Top Movies (filtered by time range, empty if TV filter)
    let top_movies: Vec<TopMovieItem> = if !is_tv_filter {
        let top_movies_sql = format!(
            r#"
            SELECT title, COUNT(*) as cnt, MAX(thumb_url) as thumb, MAX(year) as yr
            FROM scrobbles
            WHERE user_id = ? AND media_type = 'movie'{}
            GROUP BY title
            ORDER BY cnt DESC
            LIMIT 6
            "#,
            time_clause_str
        );
        let mut q = sqlx::query_as::<_, (String, i64, Option<String>, Option<i32>)>(&top_movies_sql).bind(&auth.user_id);
        if let Some(ref s) = time_start { q = q.bind(s); }
        if let Some(ref e) = time_end { q = q.bind(e); }
        let rows: Vec<(String, i64, Option<String>, Option<i32>)> = q.fetch_all(&state.pool).await.unwrap_or_default();
        rows.into_iter().map(|(title, count, thumb_url, year)| TopMovieItem { title, count, thumb_url, year }).collect()
    } else {
        Vec::new()
    };

    // 11. Top Genres (respects active filter & time range)
    let genres_sql = format!(
        "SELECT genres FROM scrobbles WHERE user_id = ? AND genres IS NOT NULL AND genres != ''{}{}",
        media_clause,
        time_clause_str
    );
    let mut q = sqlx::query_scalar::<_, String>(&genres_sql).bind(&auth.user_id);
    if let Some(ref s) = time_start { q = q.bind(s); }
    if let Some(ref e) = time_end { q = q.bind(e); }
    let genre_rows: Vec<String> = q.fetch_all(&state.pool).await.unwrap_or_default();
    let mut genre_counts: HashMap<String, i64> = HashMap::new();
    for g_str in genre_rows {
        for g in g_str.split(',') {
            let trimmed = g.trim();
            if !trimmed.is_empty() {
                *genre_counts.entry(trimmed.to_string()).or_insert(0) += 1;
            }
        }
    }
    let mut top_genres: Vec<GenreCountItem> = genre_counts
        .into_iter()
        .map(|(genre, count)| GenreCountItem { genre, count })
        .collect();
    top_genres.sort_by(|a, b| b.count.cmp(&a.count));
    top_genres.truncate(8);

    // 12. Top Actors & Directors (respects active filter & time range)
    let people_sql = format!(
        "SELECT actors, directors FROM scrobbles WHERE user_id = ?{}{}",
        media_clause,
        time_clause_str
    );
    let mut q = sqlx::query_as::<_, (Option<String>, Option<String>)>(&people_sql).bind(&auth.user_id);
    if let Some(ref s) = time_start { q = q.bind(s); }
    if let Some(ref e) = time_end { q = q.bind(e); }
    let people_rows: Vec<(Option<String>, Option<String>)> = q.fetch_all(&state.pool).await.unwrap_or_default();

    let mut actor_counts: HashMap<String, i64> = HashMap::new();
    let mut director_counts: HashMap<String, i64> = HashMap::new();
    for (actors_opt, directors_opt) in people_rows {
        if let Some(actors_str) = actors_opt {
            for a in actors_str.split(',') {
                let trimmed = a.trim();
                if !trimmed.is_empty() {
                    *actor_counts.entry(trimmed.to_string()).or_insert(0) += 1;
                }
            }
        }
        if let Some(directors_str) = directors_opt {
            for d in directors_str.split(',') {
                let trimmed = d.trim();
                if !trimmed.is_empty() {
                    *director_counts.entry(trimmed.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut top_actors: Vec<TopPersonItem> = actor_counts
        .into_iter()
        .map(|(name, count)| TopPersonItem { name, count })
        .collect();
    top_actors.sort_by(|a, b| b.count.cmp(&a.count));
    top_actors.truncate(6);

    let mut top_directors: Vec<TopPersonItem> = director_counts
        .into_iter()
        .map(|(name, count)| TopPersonItem { name, count })
        .collect();
    top_directors.sort_by(|a, b| b.count.cmp(&a.count));
    top_directors.truncate(6);

    // 13. Daily Watch Activity Chart Data (aligned with time range and filter)
    let days_back = match query.time_range.as_deref().map(|s| s.to_lowercase()) {
        Some(ref r) if r == "7d" => 7,
        Some(ref r) if r == "30d" => 30,
        Some(ref r) if r == "90d" => 30,
        _ => 14,
    };

    let daily_sql = format!(
        "SELECT substr(watched_at, 1, 10) as dt, media_type FROM scrobbles WHERE user_id = ? AND watched_at >= date('now', '-{} days') ORDER BY dt ASC",
        days_back
    );
    let daily_rows: Vec<(String, String)> = sqlx::query_as(&daily_sql)
        .bind(&auth.user_id)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut daily_map: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let today_naive = Utc::now().date_naive();
    for i in (0..days_back).rev() {
        let day = today_naive - Duration::days(i as i64);
        daily_map.insert(day.format("%Y-%m-%d").to_string(), (0, 0));
    }

    for (dt, m_type) in daily_rows {
        if let Some(entry) = daily_map.get_mut(&dt) {
            if m_type == "movie" {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }

    let daily_activity: Vec<DailyActivityItem> = daily_map
        .into_iter()
        .map(|(date, (movies, episodes))| {
            let (m, e) = if is_movie_filter {
                (movies, 0)
            } else if is_tv_filter {
                (0, episodes)
            } else {
                (movies, episodes)
            };
            DailyActivityItem {
                date,
                movies: m,
                episodes: e,
                total: m + e,
            }
        })
        .collect();

    // Weekly 7x24 heatmap (Day 0=Sunday .. 6=Saturday, Hour 0..23)
    let heatmap_sql = format!(
        r#"
        SELECT
            CAST(strftime('%w', watched_at) AS INTEGER) as day_idx,
            CAST(strftime('%H', watched_at) AS INTEGER) as hr,
            COUNT(*) as cnt
        FROM scrobbles
        WHERE user_id = ?{}{}
        GROUP BY day_idx, hr
        "#,
        time_clause_str,
        media_clause
    );
    let mut q = sqlx::query_as::<_, (i32, i32, i64)>(&heatmap_sql).bind(&auth.user_id);
    if let Some(ref s) = time_start { q = q.bind(s); }
    if let Some(ref e) = time_end { q = q.bind(e); }
    let heatmap_rows = q.fetch_all(&state.pool).await.unwrap_or_default();

    let mut heatmap_map: HashMap<(u32, u32), i64> = HashMap::new();
    for (d, h, cnt) in heatmap_rows {
        if (0..=6).contains(&d) && (0..=23).contains(&h) {
            heatmap_map.insert((d as u32, h as u32), cnt);
        }
    }

    let mut weekly_heatmap = Vec::with_capacity(7 * 24);
    for d in 0..7 {
        for h in 0..24 {
            let count = *heatmap_map.get(&(d, h)).unwrap_or(&0);
            weekly_heatmap.push(crate::models::HeatmapCell {
                day_index: d,
                hour: h,
                count,
            });
        }
    }

    Ok(Json(StatsSummary {
        total_scrobbles,
        movies_watched,
        episodes_watched,
        unique_movies,
        unique_shows,
        total_minutes,
        movie_minutes,
        episode_minutes,
        avg_daily_minutes,
        current_streak_days,
        longest_streak_days,
        average_rating,
        total_rated,
        completion_rate_percent,
        top_shows,
        top_movies,
        daily_activity,
        top_genres,
        top_actors,
        top_directors,
        day_of_week_activity,
        time_of_day_activity,
        peak_day,
        peak_day_count,
        weekly_heatmap,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct WrappedQuery {
    pub year: Option<i32>,
}

/// GET /api/v1/stats/wrapped
pub async fn get_user_wrapped(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<WrappedQuery>,
) -> Result<Json<crate::models::WrappedSummary>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Available years for the user
    let years: Vec<i32> = sqlx::query_scalar::<_, i32>(
        "SELECT DISTINCT CAST(strftime('%Y', watched_at) AS INTEGER) FROM scrobbles WHERE user_id = ? AND watched_at IS NOT NULL ORDER BY 1 DESC"
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let current_year = chrono::Datelike::year(&Utc::now());
    let target_year = query.year.unwrap_or(if years.is_empty() { current_year } else { years[0] });

    let start_str = format!("{:04}-01-01 00:00:00", target_year);
    let end_str = format!("{:04}-12-31 23:59:59", target_year);

    // 2. Counts and durations
    let total_scrobbles: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND watched_at >= ? AND watched_at <= ?"
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let total_movies: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND media_type = 'movie' AND watched_at >= ? AND watched_at <= ?"
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let total_episodes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND media_type = 'episode' AND watched_at >= ? AND watched_at <= ?"
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let unique_shows: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT grandparent_title) FROM scrobbles WHERE user_id = ? AND media_type = 'episode' AND watched_at >= ? AND watched_at <= ?"
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let total_dur_ms: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(duration_ms) FROM scrobbles WHERE user_id = ? AND watched_at >= ? AND watched_at <= ?"
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(None);

    let total_minutes = total_dur_ms.unwrap_or(0) / 60_000;
    let total_hours = ((total_minutes as f64 / 60.0) * 10.0).round() / 10.0;

    // 3. Busiest day
    let busiest: Option<(String, i64)> = sqlx::query_as(
        r#"
        SELECT substr(watched_at, 1, 10) as dt, COUNT(*) as cnt
        FROM scrobbles
        WHERE user_id = ? AND watched_at >= ? AND watched_at <= ?
        GROUP BY dt
        ORDER BY cnt DESC, dt DESC
        LIMIT 1
        "#
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let (busiest_day, busiest_day_count) = match busiest {
        Some((dt, cnt)) => (Some(dt), cnt),
        None => (None, 0),
    };

    // 4. Top Shows (top 5)
    let top_shows: Vec<crate::models::TopShowItem> = sqlx::query_as::<_, (String, i64, Option<String>)>(
        r#"
        SELECT grandparent_title, COUNT(*) as cnt, MAX(thumb_url) as thumb
        FROM scrobbles
        WHERE user_id = ? AND media_type = 'episode' AND grandparent_title IS NOT NULL AND watched_at >= ? AND watched_at <= ?
        GROUP BY grandparent_title
        ORDER BY cnt DESC
        LIMIT 5
        "#
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(show_title, count, thumb_url)| crate::models::TopShowItem {
        show_title,
        count,
        thumb_url,
    })
    .collect();

    // 5. Top Movies (top 5)
    let top_movies: Vec<crate::models::TopMovieItem> = sqlx::query_as::<_, (String, i64, Option<String>, Option<i32>)>(
        r#"
        SELECT title, COUNT(*) as cnt, MAX(thumb_url) as thumb, MAX(year) as yr
        FROM scrobbles
        WHERE user_id = ? AND media_type = 'movie' AND watched_at >= ? AND watched_at <= ?
        GROUP BY title
        ORDER BY cnt DESC
        LIMIT 5
        "#
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(title, count, thumb_url, year)| crate::models::TopMovieItem {
        title,
        count,
        thumb_url,
        year,
    })
    .collect();

    // 6. Longest Movie
    let longest_movie: Option<crate::models::LongestMovieItem> = sqlx::query_as::<_, (String, Option<i64>, Option<i32>, Option<String>)>(
        r#"
        SELECT title, duration_ms, year, thumb_url
        FROM scrobbles
        WHERE user_id = ? AND media_type = 'movie' AND duration_ms IS NOT NULL AND duration_ms > 0 AND watched_at >= ? AND watched_at <= ?
        ORDER BY duration_ms DESC
        LIMIT 1
        "#
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None)
    .map(|(title, dur, year, thumb_url)| crate::models::LongestMovieItem {
        title,
        duration_minutes: dur.unwrap_or(0) / 60_000,
        year,
        thumb_url,
    });

    // 7. Top Genres, Actors, Directors
    let meta_rows: Vec<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT genres, actors, directors FROM scrobbles WHERE user_id = ? AND watched_at >= ? AND watched_at <= ?"
    )
    .bind(&auth.user_id)
    .bind(&start_str)
    .bind(&end_str)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut genre_counts: HashMap<String, i64> = HashMap::new();
    let mut actor_counts: HashMap<String, i64> = HashMap::new();
    let mut director_counts: HashMap<String, i64> = HashMap::new();

    for (g_opt, a_opt, d_opt) in meta_rows {
        if let Some(g_str) = g_opt {
            for g in g_str.split(',') {
                let trimmed = g.trim();
                if !trimmed.is_empty() {
                    *genre_counts.entry(trimmed.to_string()).or_insert(0) += 1;
                }
            }
        }
        if let Some(a_str) = a_opt {
            for a in a_str.split(',') {
                let trimmed = a.trim();
                if !trimmed.is_empty() {
                    *actor_counts.entry(trimmed.to_string()).or_insert(0) += 1;
                }
            }
        }
        if let Some(d_str) = d_opt {
            for d in d_str.split(',') {
                let trimmed = d.trim();
                if !trimmed.is_empty() {
                    *director_counts.entry(trimmed.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut top_genres: Vec<crate::models::GenreCountItem> = genre_counts
        .into_iter()
        .map(|(genre, count)| crate::models::GenreCountItem { genre, count })
        .collect();
    top_genres.sort_by(|a, b| b.count.cmp(&a.count));
    top_genres.truncate(5);

    let mut top_actors: Vec<crate::models::TopPersonItem> = actor_counts
        .into_iter()
        .map(|(name, count)| crate::models::TopPersonItem { name, count })
        .collect();
    top_actors.sort_by(|a, b| b.count.cmp(&a.count));
    top_actors.truncate(5);

    let mut top_directors: Vec<crate::models::TopPersonItem> = director_counts
        .into_iter()
        .map(|(name, count)| crate::models::TopPersonItem { name, count })
        .collect();
    top_directors.sort_by(|a, b| b.count.cmp(&a.count));
    top_directors.truncate(5);

    Ok(Json(crate::models::WrappedSummary {
        year: target_year,
        total_scrobbles,
        total_hours,
        total_minutes,
        total_movies,
        total_episodes,
        unique_shows,
        busiest_day,
        busiest_day_count,
        top_shows,
        top_movies,
        top_actors,
        top_directors,
        top_genres,
        longest_movie,
        available_years: if years.is_empty() { vec![target_year] } else { years },
    }))
}
