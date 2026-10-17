use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path as FilePath, time::Duration};

use crate::api::AppState;

/// Serves cached media poster images directly from disk.
/// Images are stored on disk rather than in the database for optimal database performance and memory efficiency.
pub async fn get_image_handler(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, &'static str)> {
    // Sanitize filename to prevent directory traversal
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err((StatusCode::BAD_REQUEST, "Invalid image filename"));
    }

    let file_path = FilePath::new(&state.config.image_cache_dir).join(&filename);
    if !file_path.exists() {
        return Err((StatusCode::NOT_FOUND, "Image not found"));
    }

    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read image file"))?;

    let mime = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();

    Ok((
        [
            (axum::http::header::CONTENT_TYPE, mime),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    ))
}

/// Spawns the background image cache worker.
/// Automatically discovers scrobbles without thumbnails, searches for official posters,
/// downloads them to disk, and updates the scrobbles table.
pub fn start_image_cache_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Ensure cache directory exists on disk
        let _ = tokio::fs::create_dir_all(&state.config.image_cache_dir).await;

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        let mut failed_lookups: HashSet<String> = HashSet::new();

        loop {
            state.task_tracker.set_image_running("Scanning for scrobbles without thumbnails...").await;
            let processed = process_missing_images_batch(&state, &client, &mut failed_lookups).await;
            state.task_tracker.set_image_idle(processed, &format!("Processed {} poster(s)", processed)).await;

            if processed > 0 {
                // If we processed items in the batch, immediately continue after a brief pause
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }

            // If nothing was processed (either queue is empty or all failed), wait for notification or 4-hour timer
            tokio::select! {
                _ = state.image_notify.notified() => {
                    failed_lookups.clear();
                    // Small debounce to let bulk transactions settle
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                _ = tokio::time::sleep(Duration::from_secs(4 * 3600)) => {
                    // Periodic run every 4 hours: clear failed lookups to re-verify any missing artwork
                    failed_lookups.clear();
                }
            }
        }
    })
}

/// Process a single batch of missing images (useful for testing and immediate invocation).
pub async fn process_missing_images_once(state: &AppState) -> usize {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let mut failed = HashSet::new();
    process_missing_images_batch(state, &client, &mut failed).await
}

#[derive(sqlx::FromRow, Debug)]
struct MissingImageItem {
    pub media_type: String,
    pub title: String,
    pub grandparent_title: Option<String>,
    pub year: Option<i32>,
    pub guid: Option<String>,
}

async fn process_missing_images_batch(
    state: &AppState,
    client: &Client,
    failed_lookups: &mut HashSet<String>,
) -> usize {
    let rows = match sqlx::query_as::<_, MissingImageItem>(
        r#"
        SELECT media_type, title, grandparent_title, year, guid
        FROM scrobbles
        WHERE thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%'
        ORDER BY watched_at DESC
        LIMIT 25
        "#,
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Error querying scrobbles without thumbnails: {}", e);
            return 0;
        }
    };

    if rows.is_empty() {
        return 0;
    }

    let mut processed_count = 0;

    for item in rows {
        let is_episode = item.media_type == "episode";
        let display_name = if is_episode {
            item.grandparent_title.as_deref().unwrap_or(&item.title)
        } else {
            &item.title
        };

        let cache_key = if is_episode {
            format!("show:{}", display_name.trim().to_lowercase())
        } else {
            format!(
                "movie:{}:{}",
                display_name.trim().to_lowercase(),
                item.year.unwrap_or(0)
            )
        };

        if failed_lookups.contains(&cache_key) {
            continue;
        }

        // Generate deterministic filename on disk
        let mut hasher = Sha256::new();
        hasher.update(cache_key.as_bytes());
        let hash_hex = format!("{:x}", hasher.finalize());
        let short_hash = &hash_hex[..16];
        let filename = format!("{}_{}.jpg", if is_episode { "show" } else { "movie" }, short_hash);
        let dest_path = FilePath::new(&state.config.image_cache_dir).join(&filename);
        let served_url = format!("/api/v1/images/{}", filename);

        // If file already exists on disk (cached earlier or by another scrobble), update DB immediately
        if dest_path.exists() {
            update_scrobbles_thumb(state, is_episode, display_name, &served_url).await;
            processed_count += 1;
            continue;
        }

        // Extract IMDb ID if present in guid
        let imdb_id = item.guid.as_deref().and_then(|g| {
            if let Some(pos) = g.find("tt") {
                let candidate = &g[pos..];
                let end = candidate
                    .find(|c: char| !c.is_ascii_alphanumeric())
                    .unwrap_or(candidate.len());
                Some(&candidate[..end])
            } else {
                None
            }
        });

        // Search for poster image URL from public media APIs
        let poster_url_opt = if is_episode {
            fetch_show_poster(client, display_name, imdb_id).await
        } else {
            fetch_movie_poster(client, display_name, item.year).await
        };

        match poster_url_opt {
            Some(poster_url) => {
                match download_and_save_image(client, &poster_url, &dest_path).await {
                    Ok(_) => {
                        update_scrobbles_thumb(state, is_episode, display_name, &served_url).await;
                        processed_count += 1;
                        tracing::info!(
                            media = %display_name,
                            file = %filename,
                            "Downloaded and cached poster to disk"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(media = %display_name, error = %e, "Failed to download poster image");
                        failed_lookups.insert(cache_key);
                    }
                }
            }
            None => {
                tracing::debug!(media = %display_name, "No poster found online");
                failed_lookups.insert(cache_key);
            }
        }

        // Polite delay between external network requests
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    processed_count
}

async fn update_scrobbles_thumb(
    state: &AppState,
    is_episode: bool,
    display_name: &str,
    served_url: &str,
) {
    if is_episode {
        let _ = sqlx::query(
            r#"
            UPDATE scrobbles
            SET thumb_url = ?
            WHERE (grandparent_title = ? OR (media_type = 'episode' AND title = ?))
              AND (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%')
            "#,
        )
        .bind(served_url)
        .bind(display_name)
        .bind(display_name)
        .execute(&state.pool)
        .await;
    } else {
        let _ = sqlx::query(
            r#"
            UPDATE scrobbles
            SET thumb_url = ?
            WHERE media_type = 'movie' AND title = ?
              AND (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%')
            "#,
        )
        .bind(served_url)
        .bind(display_name)
        .execute(&state.pool)
        .await;
    }
}

async fn fetch_show_poster(client: &Client, title: &str, imdb_id: Option<&str>) -> Option<String> {
    // 1. Try TVmaze by IMDb ID if available
    if let Some(imdb) = imdb_id {
        if imdb.starts_with("tt") {
            let url = format!("https://api.tvmaze.com/lookup/shows?imdb={}", imdb);
            if let Ok(resp) = client.get(&url).header("User-Agent", "BingeSync/1.0").send().await {
                if resp.status().is_success() {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        if let Some(img) = data
                            .get("image")
                            .and_then(|i| i.get("medium").or_else(|| i.get("original")))
                            .and_then(|u| u.as_str())
                        {
                            return Some(img.to_string());
                        }
                    }
                }
            }
        }
    }

    // 2. Try TVmaze singlesearch by title
    let tvmaze_url = "https://api.tvmaze.com/singlesearch/shows";
    if let Ok(resp) = client
        .get(tvmaze_url)
        .query(&[("q", title)])
        .header("User-Agent", "BingeSync/1.0")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                if let Some(img) = data
                    .get("image")
                    .and_then(|i| i.get("medium").or_else(|| i.get("original")))
                    .and_then(|u| u.as_str())
                {
                    return Some(img.to_string());
                }
            }
        }
    }

    // 3. Fallback: Wikipedia search for TV show
    search_wikipedia_image(client, &format!("{} TV series", title)).await
}

async fn fetch_movie_poster(client: &Client, title: &str, year: Option<i32>) -> Option<String> {
    // 1. Try Wikipedia with title and year
    let term = if let Some(y) = year {
        format!("{} {} film", title, y)
    } else {
        format!("{} film", title)
    };

    if let Some(img) = search_wikipedia_image(client, &term).await {
        return Some(img);
    }

    // 2. Try Wikipedia with just title + film
    if year.is_some() {
        if let Some(img) = search_wikipedia_image(client, &format!("{} film", title)).await {
            return Some(img);
        }
    }

    // 3. Fallback: TVmaze singlesearch
    let tvmaze_url = "https://api.tvmaze.com/singlesearch/shows";
    if let Ok(resp) = client
        .get(tvmaze_url)
        .query(&[("q", title)])
        .header("User-Agent", "BingeSync/1.0")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                if let Some(img) = data
                    .get("image")
                    .and_then(|i| i.get("medium").or_else(|| i.get("original")))
                    .and_then(|u| u.as_str())
                {
                    return Some(img.to_string());
                }
            }
        }
    }

    None
}

async fn search_wikipedia_image(client: &Client, search_term: &str) -> Option<String> {
    let url = "https://en.wikipedia.org/w/api.php";
    let resp = client
        .get(url)
        .query(&[
            ("action", "query"),
            ("format", "json"),
            ("generator", "search"),
            ("gsrsearch", search_term),
            ("gsrlimit", "1"),
            ("prop", "pageimages"),
            ("piprop", "thumbnail"),
            ("pithumbsize", "400"),
            ("pilicense", "any"),
        ])
        .header("User-Agent", "BingeSync/1.0 (https://github.com/dnewsholme/BingeSync)")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data = resp.json::<serde_json::Value>().await.ok()?;
    let pages = data.get("query")?.get("pages")?.as_object()?;

    for (_page_id, page_data) in pages {
        if let Some(thumb) = page_data
            .get("thumbnail")
            .and_then(|t| t.get("source"))
            .and_then(|s| s.as_str())
        {
            return Some(thumb.to_string());
        }
    }

    None
}

async fn download_and_save_image(
    client: &Client,
    url: &str,
    dest_path: &FilePath,
) -> Result<(), String> {
    let resp = client
        .get(url)
        .header("User-Agent", "BingeSync/1.0 (https://github.com/dnewsholme/BingeSync)")
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP error: {}", resp.status()));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read body bytes: {}", e))?;

    if bytes.len() < 500 {
        return Err("Downloaded image too small or empty".to_string());
    }

    if let Some(parent) = dest_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    tokio::fs::write(dest_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write image file: {}", e))?;

    Ok(())
}
