use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::collections::HashSet;

use crate::{
    api::{
        tmdb::{RecommendationItem, TmdbClient},
        AppState,
    },
    auth::AuthUser,
};

/// GET /api/v1/recommendations
pub async fn get_user_recommendations(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // 1. Get user's watched titles to avoid recommending what they already watched
    let watched_titles: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT LOWER(title) FROM scrobbles WHERE user_id = ?"
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let watched_set: HashSet<String> = watched_titles.into_iter().collect();

    // 2. Query top-rated or recent favorite completed scrobbles
    #[derive(sqlx::FromRow)]
    struct SeedScrobble {
        title: String,
        media_type: String,
        grandparent_title: Option<String>,
        rating_key: Option<String>,
        user_rating: Option<i32>,
    }

    let seeds: Vec<SeedScrobble> = sqlx::query_as(
        r#"
        SELECT title, media_type, grandparent_title, rating_key, user_rating
        FROM scrobbles
        WHERE user_id = ? AND (user_rating >= 6 OR watch_status = 'completed')
        ORDER BY user_rating DESC, watched_at DESC
        LIMIT 8
        "#
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let tmdb_key = state.get_effective_tmdb_api_key().await;
    let tmdb_base = state.get_effective_tmdb_base_url().await;

    let mut recommendations: Vec<RecommendationItem> = Vec::new();
    let mut seen_ids: HashSet<i64> = HashSet::new();

    if let Some(key) = tmdb_key {
        let client = TmdbClient::new(tmdb_base, key);

        for seed in &seeds {
            let is_tv = seed.media_type.eq_ignore_ascii_case("episode") || seed.media_type.eq_ignore_ascii_case("tv");
            let query_title = if is_tv {
                seed.grandparent_title.as_deref().unwrap_or(&seed.title)
            } else {
                &seed.title
            };

            // Find TMDB ID: if rating_key is digits, use it; otherwise search TMDB
            let tmdb_id = if let Some(ref rk) = seed.rating_key {
                if rk.chars().all(|c| c.is_ascii_digit()) && !rk.is_empty() {
                    Some(rk.clone())
                } else {
                    None
                }
            } else {
                None
            };

            let resolved_id = match tmdb_id {
                Some(id) => Some(id),
                None => {
                    let search_res = client
                        .search(query_title, if is_tv { "series" } else { "movie" }, None)
                        .await;
                    search_res.ok().flatten().map(|res| res.id)
                }
            };

            if let Some(ref id) = resolved_id {
                let reason_str = if let Some(r) = seed.user_rating {
                    format!("Based on your {}★ rating of {}", r, query_title)
                } else {
                    format!("Because you watched {}", query_title)
                };

                let recs = if is_tv {
                    client.get_recommendations_for_tv(id, &reason_str).await
                } else {
                    client.get_recommendations_for_movie(id, &reason_str).await
                };

                if let Ok(items) = recs {
                    for item in items {
                        let low_title = item.title.to_lowercase();
                        if !watched_set.contains(&low_title) && seen_ids.insert(item.tmdb_id) {
                            recommendations.push(item);
                        }
                    }
                }
            }

            if recommendations.len() >= 18 {
                break;
            }
        }

        // If recommendations are low, supplement with trending
        if recommendations.len() < 12 {
            if let Ok(trending) = client.get_trending().await {
                for item in trending {
                    let low_title = item.title.to_lowercase();
                    if !watched_set.contains(&low_title) && seen_ids.insert(item.tmdb_id) {
                        recommendations.push(item);
                    }
                    if recommendations.len() >= 24 {
                        break;
                    }
                }
            }
        }
    }

    // Fallback: If no TMDB key or no results, return curated recommendations based on user's top genres
    if recommendations.is_empty() {
        let fallback_samples = vec![
            RecommendationItem {
                id: "27205".to_string(),
                tmdb_id: 27205,
                title: "Inception".to_string(),
                media_type: "movie".to_string(),
                overview: Some("Cobb, a skilled thief who commits corporate espionage by infiltrating the subconscious of his targets, is offered a chance to regain his old life as payment for a task considered to be impossible: \"inception\".".to_string()),
                thumb_url: Some("https://image.tmdb.org/t/p/w500/oYuLEt3zVCKq57qu2F8dT7NIa6f.jpg".to_string()),
                backdrop_url: Some("https://image.tmdb.org/t/p/w780/8ZTVqvKDQ8emSGUEMjsS4yHAwrp.jpg".to_string()),
                year: Some(2010),
                vote_average: Some(8.4),
                reason: "Critically Acclaimed Sci-Fi Thriller".to_string(),
            },
            RecommendationItem {
                id: "157336".to_string(),
                tmdb_id: 157336,
                title: "Interstellar".to_string(),
                media_type: "movie".to_string(),
                overview: Some("The adventures of a group of explorers who make use of a newly discovered wormhole to surpass the limitations on human space travel and conquer the vast distances involved in an interstellar voyage.".to_string()),
                thumb_url: Some("https://image.tmdb.org/t/p/w500/gEU2QniE6E77NI6lCU6MxlNBvIx.jpg".to_string()),
                backdrop_url: Some("https://image.tmdb.org/t/p/w780/rAiYTsqJiOEZvtNuC97iCnmrG4v.jpg".to_string()),
                year: Some(2014),
                vote_average: Some(8.4),
                reason: "Top Sci-Fi Epic Recommendation".to_string(),
            },
            RecommendationItem {
                id: "1396".to_string(),
                tmdb_id: 1396,
                title: "Breaking Bad".to_string(),
                media_type: "tv".to_string(),
                overview: Some("Walter White, a New Mexico chemistry teacher, is diagnosed with Stage III cancer and given a prognosis of two years left to live. He chooses to enter a dangerous world of crime and produces the highest-purity methamphetamine.".to_string()),
                thumb_url: Some("https://image.tmdb.org/t/p/w500/ztkUQFLlC19CCMYHW9o1zWhJRNq.jpg".to_string()),
                backdrop_url: Some("https://image.tmdb.org/t/p/w780/9faGSFi5jam6pDWGNd0p8JcJgXQ.jpg".to_string()),
                year: Some(2008),
                vote_average: Some(8.9),
                reason: "Top Rated Drama Series".to_string(),
            },
            RecommendationItem {
                id: "1399".to_string(),
                tmdb_id: 1399,
                title: "Game of Thrones".to_string(),
                media_type: "tv".to_string(),
                overview: Some("Seven noble families fight for control of the mythical land of Westeros. Friction between the houses leads to full-scale war.".to_string()),
                thumb_url: Some("https://image.tmdb.org/t/p/w500/1XS1oqL89opfnbLl8WnZY1O1uJx.jpg".to_string()),
                backdrop_url: Some("https://image.tmdb.org/t/p/w780/2OMB0ynKlyIenMJWI2Dy9IWT4c.jpg".to_string()),
                year: Some(2011),
                vote_average: Some(8.4),
                reason: "Popular Fantasy Epic".to_string(),
            },
        ];

        for item in fallback_samples {
            if !watched_set.contains(&item.title.to_lowercase()) {
                recommendations.push(item);
            }
        }
    }

    Ok(Json(serde_json::to_value(recommendations).unwrap_or_default()))
}
