use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::{
    api::AppState,
    auth::AuthUser,
    models::{Scrobble, TimelineDay},
};

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub person: Option<String>,
    pub rating: Option<String>,
    pub watch_status: Option<String>,
    pub media_type: Option<String>,
    pub search: Option<String>,
    #[serde(default, alias = "all_results")]
    pub all: Option<bool>,
    #[serde(default)]
    pub all_dates: Option<bool>,
}

pub async fn get_timeline(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<TimelineQuery>,
) -> Result<Json<Vec<TimelineDay>>, (StatusCode, Json<serde_json::Value>)> {
    let all_results = query.all.unwrap_or(false);
    let all_dates = query.all_dates.unwrap_or(false);

    let limit = query.limit.unwrap_or(50).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0).max(0);

    let media_type_filter = query.media_type.as_deref().unwrap_or("all");
    let search_filter = query.search.as_deref().unwrap_or("").trim();
    let date_filter = if all_dates {
        ""
    } else {
        query.date.as_deref().unwrap_or("").trim()
    };
    let genre_filter = query.genre.as_deref().unwrap_or("").trim();
    let person_filter = query.person.as_deref().unwrap_or("").trim();
    let rating_filter = query.rating.as_deref().unwrap_or("all").trim();
    let watch_status_filter = query.watch_status.as_deref().unwrap_or("all").trim();

    let mut sql = "SELECT * FROM scrobbles WHERE user_id = ?".to_string();
    let mut bind_params: Vec<String> = vec![auth.user_id.clone()];

    if media_type_filter != "all" && !media_type_filter.is_empty() {
        sql.push_str(" AND media_type = ?");
        bind_params.push(media_type_filter.to_string());
    }

    if !search_filter.is_empty() {
        let pattern = format!("%{}%", search_filter);
        sql.push_str(" AND (title LIKE ? OR grandparent_title LIKE ? OR overview LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern.clone());
        bind_params.push(pattern);
    }

    if !date_filter.is_empty() {
        let pattern = format!("{}%", date_filter);
        sql.push_str(" AND (watched_at LIKE ? OR substr(watched_at, 1, 10) = ?)");
        bind_params.push(pattern);
        bind_params.push(date_filter.to_string());
    }

    if !genre_filter.is_empty() {
        let pattern = format!("%{}%", genre_filter);
        sql.push_str(" AND genres LIKE ?");
        bind_params.push(pattern);
    }

    if !person_filter.is_empty() {
        let pattern = format!("%{}%", person_filter);
        sql.push_str(" AND (director LIKE ? OR directors LIKE ? OR cinematographer LIKE ? OR actors LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern.clone());
        bind_params.push(pattern.clone());
        bind_params.push(pattern);
    }

    match rating_filter {
        "rated" => {
            sql.push_str(" AND user_rating IS NOT NULL");
        }
        "unrated" => {
            sql.push_str(" AND user_rating IS NULL");
        }
        "10" => {
            sql.push_str(" AND user_rating = 10");
        }
        "9+" | "9" => {
            sql.push_str(" AND user_rating >= 9");
        }
        "8+" | "8" => {
            sql.push_str(" AND user_rating >= 8");
        }
        "7+" | "7" => {
            sql.push_str(" AND user_rating >= 7");
        }
        "6+" | "6" => {
            sql.push_str(" AND user_rating >= 6");
        }
        "5+" | "5" => {
            sql.push_str(" AND user_rating >= 5");
        }
        _ => {}
    }

    if watch_status_filter != "all" && !watch_status_filter.is_empty() {
        sql.push_str(" AND watch_status = ?");
        bind_params.push(watch_status_filter.to_string());
    }

    if all_results {
        sql.push_str(" ORDER BY watched_at DESC");
    } else {
        sql.push_str(" ORDER BY watched_at DESC LIMIT ? OFFSET ?");
    }

    let mut fetch_query = sqlx::query_as::<_, Scrobble>(&sql);
    for p in &bind_params {
        fetch_query = fetch_query.bind(p);
    }
    if !all_results {
        fetch_query = fetch_query.bind(limit).bind(offset);
    }

    let scrobbles = fetch_query
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error fetching timeline: {}", e) })),
            )
        })?;

    // Group items by day YYYY-MM-DD preserving descending order
    let mut day_map: BTreeMap<String, Vec<Scrobble>> = BTreeMap::new();
    let mut day_order: Vec<String> = Vec::new();

    for scrobble in scrobbles {
        // Parse date prefix from watched_at (handles ISO8601 or SQLite format)
        let day_str = scrobble
            .watched_at
            .split('T')
            .next()
            .unwrap_or(&scrobble.watched_at)
            .split(' ')
            .next()
            .unwrap_or("Unknown Date")
            .to_string();

        if !day_map.contains_key(&day_str) {
            day_order.push(day_str.clone());
        }
        day_map.entry(day_str).or_default().push(scrobble);
    }

    let mut result: Vec<TimelineDay> = Vec::new();
    for day in day_order {
        if let Some(items) = day_map.remove(&day) {
            let total_count = items.len();
            let total_minutes = items
                .iter()
                .filter_map(|s| s.duration_ms)
                .map(|ms| ms / 60_000)
                .sum();

            result.push(TimelineDay {
                date: day,
                total_count,
                total_minutes,
                items,
            });
        }
    }

    Ok(Json(result))
}

fn month_name(month_num: &str) -> &'static str {
    match month_num {
        "01" => "January",
        "02" => "February",
        "03" => "March",
        "04" => "April",
        "05" => "May",
        "06" => "June",
        "07" => "July",
        "08" => "August",
        "09" => "September",
        "10" => "October",
        "11" => "November",
        "12" => "December",
        _ => "Unknown",
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TimelineMonthSummary {
    pub month: String,      // "YYYY-MM"
    pub month_num: String,  // "MM"
    pub month_name: String, // "October"
    pub count: i64,
    pub minutes: i64,
    pub poster_urls: Vec<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TimelineYearSummary {
    pub year: String,       // "YYYY"
    pub count: i64,
    pub minutes: i64,
    pub months: Vec<TimelineMonthSummary>,
    pub poster_urls: Vec<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TimelinePeriodsResponse {
    pub current_year: String,
    pub current_month: String,
    pub latest_year: Option<String>,
    pub latest_month: Option<String>,
    pub years: Vec<TimelineYearSummary>,
}

pub async fn get_timeline_periods(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<TimelinePeriodsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let now = chrono::Utc::now();
    let current_year = now.format("%Y").to_string();
    let current_month = now.format("%Y-%m").to_string();

    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"
        SELECT 
            substr(watched_at, 1, 7) as ym,
            COUNT(*) as cnt,
            COALESCE(SUM(duration_ms), 0) as dur
        FROM scrobbles
        WHERE user_id = ?
        GROUP BY ym
        ORDER BY ym DESC
        "#,
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error fetching timeline periods: {}", e) })),
        )
    })?;

    let poster_rows: Vec<(String, Option<String>)> = sqlx::query_as(
        r#"
        SELECT 
            substr(watched_at, 1, 7) as ym,
            thumb_url
        FROM scrobbles
        WHERE user_id = ? AND thumb_url IS NOT NULL AND thumb_url != '' AND thumb_url NOT LIKE '/library/%' AND thumb_url NOT LIKE '/photo/%'
        ORDER BY watched_at DESC
        "#,
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut month_posters: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (ym, thumb) in poster_rows {
        if let Some(t) = thumb {
            let list = month_posters.entry(ym).or_default();
            if list.len() < 4 && !list.contains(&t) {
                list.push(t);
            }
        }
    }

    let mut year_map: std::collections::BTreeMap<String, Vec<TimelineMonthSummary>> = std::collections::BTreeMap::new();
    let mut year_totals: std::collections::HashMap<String, (i64, i64)> = std::collections::HashMap::new();
    let mut latest_year = None;
    let mut latest_month = None;

    for (ym, cnt, dur_ms) in rows {
        if ym.len() < 7 {
            continue;
        }
        if latest_year.is_none() {
            latest_year = Some(ym[0..4].to_string());
            latest_month = Some(ym.clone());
        }
        let year_str = ym[0..4].to_string();
        let month_num = ym[5..7].to_string();
        let m_name = month_name(&month_num).to_string();
        let minutes = dur_ms / 60_000;
        let posters = month_posters.get(&ym).cloned().unwrap_or_default();

        let totals = year_totals.entry(year_str.clone()).or_insert((0, 0));
        totals.0 += cnt;
        totals.1 += minutes;

        year_map.entry(year_str).or_default().push(TimelineMonthSummary {
            month: ym,
            month_num,
            month_name: m_name,
            count: cnt,
            minutes,
            poster_urls: posters,
        });
    }

    let mut years = Vec::new();
    for (year_str, months) in year_map.into_iter().rev() {
        let (total_cnt, total_min) = year_totals.get(&year_str).copied().unwrap_or((0, 0));
        let mut year_posters = Vec::new();
        for m in &months {
            for p in &m.poster_urls {
                if year_posters.len() < 4 && !year_posters.contains(p) {
                    year_posters.push(p.clone());
                }
            }
        }
        years.push(TimelineYearSummary {
            year: year_str,
            count: total_cnt,
            minutes: total_min,
            months,
            poster_urls: year_posters,
        });
    }

    Ok(Json(TimelinePeriodsResponse {
        current_year,
        current_month,
        latest_year,
        latest_month,
        years,
    }))
}
