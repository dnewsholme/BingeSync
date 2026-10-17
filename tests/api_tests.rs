use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use bingesync::{
    api::{create_api_router, AppState},
    auth::{create_jwt, OidcClient},
    config::Config,
    db::init_db,
    models::{HistoryResponse, Scrobble, StatsSummary, TimelineDay, UserResponse},
};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

async fn setup_test_app() -> (axum::Router, sqlx::SqlitePool, Arc<Config>) {
    let pool = init_db("sqlite::memory:").await.expect("Failed to create in-memory sqlite db");
    let mut config = Config::from_env();
    config.jwt_secret = "test-secret-key-12345".to_string();
    config.app_url = "http://localhost:8080".to_string();
    config.dev_login_enabled = true;
    let config = Arc::new(config);

    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
    };

    let router = create_api_router(state);
    (router, pool, config)
}

#[tokio::test]
async fn test_dev_login_and_me() {
    let (app, _pool, _config) = setup_test_app().await;

    // 1. Dev login
    let login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/dev-login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "email": "tester@bingesync.local",
                "display_name": "Test User"
            })
            .to_string(),
        ))
        .unwrap();

    let res = app.clone().oneshot(login_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let token = login_data["token"].as_str().unwrap().to_string();
    let webhook_token = login_data["user"]["webhook_token"].as_str().unwrap().to_string();
    assert!(!token.is_empty());
    assert!(!webhook_token.is_empty());

    // 2. Fetch /api/v1/auth/me using Bearer token
    let me_req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(me_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let user_resp: UserResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(user_resp.email, "tester@bingesync.local");
    assert_eq!(user_resp.display_name, "Test User");
    assert_eq!(user_resp.webhook_token, webhook_token);
}

#[tokio::test]
async fn test_plex_webhook_scrobble_movie_and_episode() {
    let (app, pool, config) = setup_test_app().await;

    // Create user 1
    let user1 = bingesync::api::auth::get_or_create_user(
        &pool,
        "alice@bingesync.local",
        "Alice",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    // 1. Send Movie Scrobble to Alice's Webhook
    let movie_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "owner": true,
        "Account": { "title": "alice_plex" },
        "Server": { "title": "Home Server" },
        "Metadata": {
            "type": "movie",
            "title": "Inception",
            "year": 2010,
            "duration": 8880000,
            "summary": "A thief who steals corporate secrets through dream-sharing technology.",
            "thumb": "https://example.com/inception.jpg"
        }
    });

    let webhook_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user1.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(movie_payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(webhook_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // 2. Send TV Episode Scrobble to Alice's Webhook
    let episode_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "owner": true,
        "Account": { "title": "alice_plex" },
        "Metadata": {
            "type": "episode",
            "title": "Ozymandias",
            "grandparentTitle": "Breaking Bad",
            "parentTitle": "Season 5",
            "index": 14,
            "parentIndex": 5,
            "year": 2013,
            "duration": 2820000,
            "summary": "Everyone copes with the aftermath.",
            "grandparentThumb": "https://example.com/bb.jpg"
        }
    });

    let webhook_req2 = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user1.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(episode_payload.to_string()))
        .unwrap();

    let res2 = app.clone().oneshot(webhook_req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::CREATED);

    // 3. Verify history for Alice
    let token = create_jwt(&user1.id, &user1.email, &config.jwt_secret).unwrap();
    let history_req = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let res3 = app.clone().oneshot(history_req).await.unwrap();
    assert_eq!(res3.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(res3.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(hist.total, 2);
    assert_eq!(hist.items.len(), 2);

    // 4. Verify Alice's Stats
    let stats_req = Request::builder()
        .method("GET")
        .uri("/api/v1/stats")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let res4 = app.clone().oneshot(stats_req).await.unwrap();
    let body_bytes = axum::body::to_bytes(res4.into_body(), usize::MAX).await.unwrap();
    let stats: StatsSummary = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(stats.total_scrobbles, 2);
    assert_eq!(stats.movies_watched, 1);
    assert_eq!(stats.episodes_watched, 1);
    assert_eq!(stats.top_shows.len(), 1);
    assert_eq!(stats.top_shows[0].show_title, "Breaking Bad");
    assert_eq!(stats.top_shows[0].count, 1);

    // 5. Verify Timeline grouping
    let timeline_req = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let res5 = app.oneshot(timeline_req).await.unwrap();
    let body_bytes = axum::body::to_bytes(res5.into_body(), usize::MAX).await.unwrap();
    let timeline: Vec<TimelineDay> = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].total_count, 2);
}

#[tokio::test]
async fn test_multi_user_isolation() {
    let (app, pool, config) = setup_test_app().await;

    // Create User A and User B
    let user_a = bingesync::api::auth::get_or_create_user(
        &pool,
        "usera@test.local",
        "User A",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let user_b = bingesync::api::auth::get_or_create_user(
        &pool,
        "userb@test.local",
        "User B",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    assert_ne!(user_a.webhook_token, user_b.webhook_token);

    // Scrobble to User A
    let scrobble_a = json!({
        "event": "media.scrobble",
        "Metadata": {
            "type": "movie",
            "title": "Interstellar",
            "year": 2014,
            "duration": 10140000
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user_a.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(scrobble_a.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Verify User A has 1 scrobble
    let token_a = create_jwt(&user_a.id, &user_a.email, &config.jwt_secret).unwrap();
    let req_a = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();
    let res_a = app.clone().oneshot(req_a).await.unwrap();
    let body_a = axum::body::to_bytes(res_a.into_body(), usize::MAX).await.unwrap();
    let hist_a: HistoryResponse = serde_json::from_slice(&body_a).unwrap();
    assert_eq!(hist_a.total, 1);
    assert_eq!(hist_a.items[0].title, "Interstellar");

    // Verify User B has ZERO scrobbles (complete isolation)
    let token_b = create_jwt(&user_b.id, &user_b.email, &config.jwt_secret).unwrap();
    let req_b = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();
    let res_b = app.oneshot(req_b).await.unwrap();
    let body_b = axum::body::to_bytes(res_b.into_body(), usize::MAX).await.unwrap();
    let hist_b: HistoryResponse = serde_json::from_slice(&body_b).unwrap();
    assert_eq!(hist_b.total, 0);
    assert_eq!(hist_b.items.len(), 0);
}

#[tokio::test]
async fn test_webhook_token_regeneration() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "regen@test.local",
        "Regen User",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let old_token = user.webhook_token.clone();
    let auth_token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // Regenerate token
    let regen_req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/regenerate-token")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(regen_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let regen_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let new_token = regen_data["webhook_token"].as_str().unwrap().to_string();
    assert_ne!(old_token, new_token);

    // Old webhook token must now fail with 401 Unauthorized
    let old_webhook_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", old_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "event": "media.scrobble", "Metadata": { "title": "Old" } }).to_string()))
        .unwrap();

    let res_old = app.clone().oneshot(old_webhook_req).await.unwrap();
    assert_eq!(res_old.status(), StatusCode::UNAUTHORIZED);

    // New webhook token must succeed
    let new_webhook_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", new_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "event": "media.scrobble", "Metadata": { "title": "New" } }).to_string()))
        .unwrap();

    let res_new = app.oneshot(new_webhook_req).await.unwrap();
    assert_eq!(res_new.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_active_session_playback_state() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "player@test.local",
        "Player User",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let auth_token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // 1. media.play event
    let play_payload = json!({
        "event": "media.play",
        "Player": { "title": "Apple TV 4K" },
        "Metadata": {
            "type": "movie",
            "title": "Dune: Part Two",
            "year": 2024,
            "duration": 9960000,
            "viewOffset": 120000
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(play_payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Query now-playing
    let now_playing_req = Request::builder()
        .method("GET")
        .uri("/api/v1/now-playing")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(now_playing_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["now_playing"]["title"], "Dune: Part Two");
    assert_eq!(data["now_playing"]["state"], "playing");
    assert_eq!(data["now_playing"]["player_title"], "Apple TV 4K");

    // 2. media.stop event
    let stop_payload = json!({
        "event": "media.stop",
        "Metadata": {
            "type": "movie",
            "title": "Dune: Part Two"
        }
    });

    let stop_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(stop_payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(stop_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Query now-playing again -> should be null
    let now_playing_req2 = Request::builder()
        .method("GET")
        .uri("/api/v1/now-playing")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res = app.oneshot(now_playing_req2).await.unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(data["now_playing"].is_null());
}

#[tokio::test]
async fn test_manual_scrobble_filter_search_and_delete() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "manual@test.local",
        "Manual Scrobbler",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let auth_token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // 1. Manually create movie scrobble
    let create_movie = json!({
        "title": "The Dark Knight",
        "media_type": "movie",
        "year": 2008,
        "duration_ms": 9120000,
        "overview": "Batman raises the stakes in his war on crime."
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(create_movie.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let created_movie: Scrobble = serde_json::from_slice(&body).unwrap();

    // 2. Manually create episode scrobble
    let create_ep = json!({
        "title": "Pilot",
        "media_type": "episode",
        "grandparent_title": "Severance",
        "parent_title": "Season 1",
        "season_number": 1,
        "episode_number": 1,
        "year": 2022,
        "duration_ms": 3300000
    });

    let req_ep = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(create_ep.to_string()))
        .unwrap();

    let res_ep = app.clone().oneshot(req_ep).await.unwrap();
    assert_eq!(res_ep.status(), StatusCode::CREATED);

    // 3. Filter by media_type=movie
    let req_filter = Request::builder()
        .method("GET")
        .uri("/api/v1/history?media_type=movie")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res_f = app.clone().oneshot(req_filter).await.unwrap();
    let body_f = axum::body::to_bytes(res_f.into_body(), usize::MAX).await.unwrap();
    let hist_f: HistoryResponse = serde_json::from_slice(&body_f).unwrap();
    assert_eq!(hist_f.total, 1);
    assert_eq!(hist_f.items[0].title, "The Dark Knight");

    // 4. Search query
    let req_search = Request::builder()
        .method("GET")
        .uri("/api/v1/history?search=Severance")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res_s = app.clone().oneshot(req_search).await.unwrap();
    let body_s = axum::body::to_bytes(res_s.into_body(), usize::MAX).await.unwrap();
    let hist_s: HistoryResponse = serde_json::from_slice(&body_s).unwrap();
    assert_eq!(hist_s.total, 1);
    assert_eq!(hist_s.items[0].grandparent_title.as_deref(), Some("Severance"));

    // 5. Delete movie scrobble
    let del_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/history/{}", created_movie.id))
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res_d = app.clone().oneshot(del_req).await.unwrap();
    assert_eq!(res_d.status(), StatusCode::OK);

    // 6. Verify total history is now 1
    let req_all = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res_a = app.oneshot(req_all).await.unwrap();
    let body_a = axum::body::to_bytes(res_a.into_body(), usize::MAX).await.unwrap();
    let hist_a: HistoryResponse = serde_json::from_slice(&body_a).unwrap();
    assert_eq!(hist_a.total, 1);
    assert_eq!(hist_a.items[0].title, "Pilot");
}

#[tokio::test]
async fn test_multipart_form_data_plex_webhook() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "plexmultipart@test.local",
        "Multipart Plex",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let inner_json = json!({
        "event": "media.scrobble",
        "Metadata": {
            "type": "movie",
            "title": "Gladiator II",
            "year": 2024,
            "duration": 8880000
        }
    }).to_string();

    let boundary = "---------------------------974767299852498929531610575";
    let body_str = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"payload\"\r\n\r\n{inner_json}\r\n--{boundary}--\r\n"
    );

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(Body::from(body_str))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Verify scrobble recorded
    let token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();
    let hist_req = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let res_h = app.oneshot(hist_req).await.unwrap();
    assert_eq!(res_h.status(), StatusCode::OK);
    let body_h = axum::body::to_bytes(res_h.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&body_h).unwrap();
    assert_eq!(hist.total, 1);
    assert_eq!(hist.items[0].title, "Gladiator II");
}

#[tokio::test]
async fn test_trakt_import_json_and_csv_preserves_timestamp_and_rating() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "traktuser@test.local",
        "Trakt User",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let auth_token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // 1. Trakt JSON payload with movie and TV episode, preserving exact dates and ratings
    let trakt_json = json!([
        {
            "id": 101,
            "watched_at": "2021-06-15T21:35:45Z",
            "type": "movie",
            "movie": {
                "title": "Blade Runner 2049",
                "year": 2017
            },
            "rating": 9
        },
        {
            "id": 102,
            "watched_at": "2020-03-20T14:10:00Z",
            "type": "episode",
            "episode": {
                "season": 4,
                "number": 10,
                "title": "Face Off"
            },
            "show": {
                "title": "Breaking Bad",
                "year": 2008
            },
            "rating": 10
        }
    ]);

    let req_json = Request::builder()
        .method("POST")
        .uri("/api/v1/import/trakt")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(trakt_json.to_string()))
        .unwrap();

    let res_json = app.clone().oneshot(req_json).await.unwrap();
    assert_eq!(res_json.status(), StatusCode::OK);

    let body_res = axum::body::to_bytes(res_json.into_body(), usize::MAX).await.unwrap();
    let import_summary: serde_json::Value = serde_json::from_slice(&body_res).unwrap();
    assert_eq!(import_summary["imported"], 2);

    // 2. Trakt CSV payload with exact date and rating
    let trakt_csv = "Title,Year,Watched At,Rating\nArrival,2016,2019-11-10 20:45:00,8\n";

    let req_csv = Request::builder()
        .method("POST")
        .uri("/api/v1/import/trakt")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .header(header::CONTENT_TYPE, "text/csv")
        .body(Body::from(trakt_csv))
        .unwrap();

    let res_csv = app.clone().oneshot(req_csv).await.unwrap();
    assert_eq!(res_csv.status(), StatusCode::OK);

    // 3. Verify history entries in database: verify date, time, and rating
    let list_req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?limit=10")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res_l = app.clone().oneshot(list_req).await.unwrap();
    let body_l = axum::body::to_bytes(res_l.into_body(), usize::MAX).await.unwrap();
    let history: HistoryResponse = serde_json::from_slice(&body_l).unwrap();

    assert_eq!(history.total, 3);

    // Find Blade Runner 2049
    let movie = history.items.iter().find(|i| i.title == "Blade Runner 2049").unwrap();
    assert_eq!(movie.media_type, "movie");
    assert_eq!(movie.year, Some(2017));
    assert_eq!(movie.user_rating, Some(9));
    assert_eq!(movie.watched_at, "2021-06-15T21:35:45+00:00");

    // Find Face Off episode
    let episode = history.items.iter().find(|i| i.title == "Face Off").unwrap();
    assert_eq!(episode.media_type, "episode");
    assert_eq!(episode.grandparent_title.as_deref(), Some("Breaking Bad"));
    assert_eq!(episode.season_number, Some(4));
    assert_eq!(episode.episode_number, Some(10));
    assert_eq!(episode.user_rating, Some(10));
    assert_eq!(episode.watched_at, "2020-03-20T14:10:00+00:00");

    // Find Arrival
    let arrival = history.items.iter().find(|i| i.title == "Arrival").unwrap();
    assert_eq!(arrival.user_rating, Some(8));
    assert_eq!(arrival.watched_at, "2019-11-10T20:45:00+00:00");

    // 4. Test re-import with updated rating -> should update rating without duplicating record
    let update_rating_json = json!([
        {
            "watched_at": "2021-06-15T21:35:45Z",
            "type": "movie",
            "movie": { "title": "Blade Runner 2049" },
            "rating": 10
        }
    ]);

    let req_update = Request::builder()
        .method("POST")
        .uri("/api/v1/import/trakt")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(update_rating_json.to_string()))
        .unwrap();

    let res_u = app.clone().oneshot(req_update).await.unwrap();
    let body_u = axum::body::to_bytes(res_u.into_body(), usize::MAX).await.unwrap();
    let update_summary: serde_json::Value = serde_json::from_slice(&body_u).unwrap();
    assert_eq!(update_summary["updated"], 1);
    assert_eq!(update_summary["imported"], 0);

    // Verify rating updated to 10
    let check_req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?search=Blade+Runner")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res_c = app.oneshot(check_req).await.unwrap();
    let body_c = axum::body::to_bytes(res_c.into_body(), usize::MAX).await.unwrap();
    let c_hist: HistoryResponse = serde_json::from_slice(&body_c).unwrap();
    assert_eq!(c_hist.total, 1);
    assert_eq!(c_hist.items[0].user_rating, Some(10));
}

#[tokio::test]
async fn test_trakt_import_zip_multi_file_and_automatic_sorting() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "zipuser@test.local",
        "Zip User",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let auth_token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // 1. Build an in-memory ZIP archive mimicking Trakt's export structure:
    // - ratings-movies-1.json (ratings for movies)
    // - ratings-episodes-1.json (ratings for episodes)
    // - watched-history-1.json (scrobbles: 2024 and 2022)
    // - watched-history-2.json (scrobbles: 2023 and 2020)
    let ratings_movies = json!([
        {
            "rated_at": "2024-01-01T00:00:00.000Z",
            "rating": 9,
            "type": "movie",
            "movie": {
                "title": "Dune: Part Two",
                "year": 2024,
                "ids": { "imdb": "tt15239678", "tmdb": 693134 }
            }
        }
    ]);

    let ratings_episodes = json!([
        {
            "rated_at": "2024-01-02T00:00:00.000Z",
            "rating": 10,
            "type": "episode",
            "episode": {
                "title": "Ozymandias",
                "season": 5,
                "number": 14,
                "ids": { "imdb": "tt2301451", "tmdb": 63098 }
            },
            "show": {
                "title": "Breaking Bad",
                "year": 2008,
                "ids": { "imdb": "tt0903747" }
            }
        }
    ]);

    let history_1 = json!([
        {
            "id": 2001,
            "watched_at": "2024-03-15T21:00:00.000Z",
            "action": "watch",
            "type": "movie",
            "movie": {
                "title": "Dune: Part Two",
                "year": 2024,
                "ids": { "imdb": "tt15239678", "tmdb": 693134 }
            }
        },
        {
            "id": 2002,
            "watched_at": "2022-06-10T19:30:00.000Z",
            "action": "watch",
            "type": "movie",
            "movie": {
                "title": "Top Gun: Maverick",
                "year": 2022,
                "ids": { "imdb": "tt1745960" }
            }
        }
    ]);

    let history_2 = json!([
        {
            "id": 2003,
            "watched_at": "2023-11-05T20:15:00.000Z",
            "action": "scrobble",
            "type": "episode",
            "episode": {
                "title": "Ozymandias",
                "season": 5,
                "number": 14,
                "ids": { "imdb": "tt2301451" }
            },
            "show": {
                "title": "Breaking Bad",
                "year": 2008,
                "ids": { "imdb": "tt0903747" }
            }
        },
        {
            "id": 2004,
            "watched_at": "2020-01-01T12:00:00.000Z",
            "action": "watch",
            "type": "movie",
            "movie": {
                "title": "Parasite",
                "year": 2019,
                "ids": { "imdb": "tt6751668" }
            }
        }
    ]);

    use std::io::Write;
    let mut zip_bytes = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut zip_bytes);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip_writer.start_file("ratings-movies-1.json", options).unwrap();
        zip_writer.write_all(ratings_movies.to_string().as_bytes()).unwrap();

        zip_writer.start_file("ratings-episodes-1.json", options).unwrap();
        zip_writer.write_all(ratings_episodes.to_string().as_bytes()).unwrap();

        zip_writer.start_file("watched-history-1.json", options).unwrap();
        zip_writer.write_all(history_1.to_string().as_bytes()).unwrap();

        zip_writer.start_file("watched-history-2.json", options).unwrap();
        zip_writer.write_all(history_2.to_string().as_bytes()).unwrap();

        zip_writer.finish().unwrap();
    }

    // 2. Upload zip binary to /api/v1/import/trakt
    let req_zip = Request::builder()
        .method("POST")
        .uri("/api/v1/import/trakt")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .header(header::CONTENT_TYPE, "application/zip")
        .body(Body::from(zip_bytes.clone()))
        .unwrap();

    let res_zip = app.clone().oneshot(req_zip).await.unwrap();
    assert_eq!(res_zip.status(), StatusCode::OK);

    let body_res = axum::body::to_bytes(res_zip.into_body(), usize::MAX).await.unwrap();
    let import_summary: serde_json::Value = serde_json::from_slice(&body_res).unwrap();
    assert_eq!(import_summary["imported"], 4);
    assert_eq!(import_summary["skipped"], 0);

    // 3. Verify history entries, sorting, and rating propagation
    let list_req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?limit=10")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .body(Body::empty())
        .unwrap();

    let res_l = app.clone().oneshot(list_req).await.unwrap();
    let body_l = axum::body::to_bytes(res_l.into_body(), usize::MAX).await.unwrap();
    let history: HistoryResponse = serde_json::from_slice(&body_l).unwrap();

    assert_eq!(history.total, 4);
    // History should be returned sorted DESC: 2024, 2023, 2022, 2020
    assert_eq!(history.items[0].title, "Dune: Part Two");
    assert_eq!(history.items[0].user_rating, Some(9));
    assert_eq!(history.items[0].guid.as_deref(), Some("imdb://tt15239678"));

    assert_eq!(history.items[1].title, "Ozymandias");
    assert_eq!(history.items[1].user_rating, Some(10));
    assert_eq!(history.items[1].grandparent_title.as_deref(), Some("Breaking Bad"));

    assert_eq!(history.items[2].title, "Top Gun: Maverick");
    assert_eq!(history.items[2].user_rating, None);

    assert_eq!(history.items[3].title, "Parasite");
    assert_eq!(history.items[3].user_rating, None);

    // 4. Test re-import of same zip -> skips all 4
    let req_reimport = Request::builder()
        .method("POST")
        .uri("/api/v1/import/trakt")
        .header(header::AUTHORIZATION, format!("Bearer {}", auth_token))
        .header(header::CONTENT_TYPE, "application/zip")
        .body(Body::from(zip_bytes))
        .unwrap();

    let res_re = app.clone().oneshot(req_reimport).await.unwrap();
    assert_eq!(res_re.status(), StatusCode::OK);
    let body_re = axum::body::to_bytes(res_re.into_body(), usize::MAX).await.unwrap();
    let re_summary: serde_json::Value = serde_json::from_slice(&body_re).unwrap();
    assert_eq!(re_summary["imported"], 0);
    assert_eq!(re_summary["skipped"], 4);

    // 5. If sample trakt-export-dbizsley.zip is present locally, test real full import
    if std::path::Path::new("trakt-export-dbizsley.zip").exists() {
        let real_user = bingesync::api::auth::get_or_create_user(
            &pool,
            "realexport@test.local",
            "Real Export User",
            None,
            None,
            false,
        )
        .await
        .unwrap();
        let real_token = create_jwt(&real_user.id, &real_user.email, &config.jwt_secret).unwrap();

        let real_zip_bytes = std::fs::read("trakt-export-dbizsley.zip").unwrap();
        let real_req = Request::builder()
            .method("POST")
            .uri("/api/v1/import/trakt")
            .header(header::AUTHORIZATION, format!("Bearer {}", real_token))
            .header(header::CONTENT_TYPE, "application/zip")
            .body(Body::from(real_zip_bytes))
            .unwrap();

        let real_res = app.clone().oneshot(real_req).await.unwrap();
        assert_eq!(real_res.status(), StatusCode::OK);
        let real_body = axum::body::to_bytes(real_res.into_body(), usize::MAX).await.unwrap();
        let real_summary: serde_json::Value = serde_json::from_slice(&real_body).unwrap();
        assert_eq!(real_summary["status"], "success");
        assert_eq!(real_summary["imported"], 4792);
        assert_eq!(real_summary["skipped"], 5);
        assert_eq!(real_summary["total_processed"], 4797);

        // Verify timeline works smoothly with all 4797 items
        let timeline_req = Request::builder()
            .method("GET")
            .uri("/api/v1/timeline?limit=10")
            .header(header::AUTHORIZATION, format!("Bearer {}", real_token))
            .body(Body::empty())
            .unwrap();
        let timeline_res = app.clone().oneshot(timeline_req).await.unwrap();
        assert_eq!(timeline_res.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn test_delete_by_show_delete_batch_and_delete_by_date() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "deleter@bingesync.local",
        "Deleter",
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // Create 3 episodes of "Stranger Things" on 2024-01-01
    for ep in 1..=3 {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/history")
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "title": format!("Chapter {}", ep),
                    "media_type": "episode",
                    "grandparent_title": "Stranger Things",
                    "episode_number": ep,
                    "season_number": 1,
                    "watched_at": format!("2024-01-01T1{:02}:00:00Z", ep)
                })
                .to_string(),
            ))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    // Create 2 movies on 2024-02-15
    let mut movie_ids = Vec::new();
    for title in &["Dune", "Dune: Part Two"] {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/history")
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "title": title,
                    "media_type": "movie",
                    "year": 2024,
                    "watched_at": "2024-02-15T20:00:00Z"
                })
                .to_string(),
            ))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let created: Scrobble = serde_json::from_slice(&b).unwrap();
        movie_ids.push(created.id);
    }

    // Total should be 5
    let hist_req = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(hist_req).await.unwrap();
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&b).unwrap();
    assert_eq!(hist.total, 5);

    // 1. Test DELETE /api/v1/history/by-show?show=Stranger+Things
    let del_show_req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/history/by-show?show=Stranger+Things")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(del_show_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let del_resp: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(del_resp["deleted_count"], 3);

    // 2. Test POST /api/v1/history/delete-batch (delete one movie)
    let del_batch_req = Request::builder()
        .method("POST")
        .uri("/api/v1/history/delete-batch")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "ids": [movie_ids[0]] }).to_string()))
        .unwrap();
    let res = app.clone().oneshot(del_batch_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let del_batch_resp: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(del_batch_resp["deleted_count"], 1);

    // 3. Test DELETE /api/v1/history/by-date?date=2024-02-15 (delete remaining movie)
    let del_date_req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/history/by-date?date=2024-02-15")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(del_date_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let del_date_resp: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(del_date_resp["deleted_count"], 1);

    // History should now be 0
    let hist_final_req = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(hist_final_req).await.unwrap();
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let final_hist: HistoryResponse = serde_json::from_slice(&b).unwrap();
    assert_eq!(final_hist.total, 0);
}

#[tokio::test]
async fn test_timeline_and_history_rich_filters() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "filtertester@bingesync.local",
        "FilterTester",
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // Create item 1: Nolan movie, Sci-Fi, rating 10, completed, 2024-06-10
    let req1 = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "title": "Interstellar",
                "media_type": "movie",
                "year": 2014,
                "genres": "Sci-Fi, Adventure, Drama",
                "directors": "Christopher Nolan",
                "actors": "Matthew McConaughey, Anne Hathaway",
                "user_rating": 10,
                "watch_status": "completed",
                "watched_at": "2024-06-10T21:00:00Z"
            })
            .to_string(),
        ))
        .unwrap();
    let res1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(res1.status(), StatusCode::CREATED);

    // Create item 2: Comedy, rating 6, in_progress, 2024-07-20
    let req2 = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "title": "Superbad",
                "media_type": "movie",
                "year": 2007,
                "genres": "Comedy",
                "directors": "Greg Mottola",
                "actors": "Jonah Hill, Michael Cera",
                "user_rating": 6,
                "watch_status": "in_progress",
                "watched_at": "2024-07-20T19:30:00Z"
            })
            .to_string(),
        ))
        .unwrap();
    let res2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::CREATED);

    // 1. Filter timeline by genre "Sci-Fi"
    let req_genre = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline?genre=Sci-Fi")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req_genre).await.unwrap();
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let days: Vec<TimelineDay> = serde_json::from_slice(&b).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].items[0].title, "Interstellar");

    // 2. Filter timeline by person "Nolan"
    let req_person = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline?person=Nolan")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req_person).await.unwrap();
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let days: Vec<TimelineDay> = serde_json::from_slice(&b).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].items[0].title, "Interstellar");

    // 3. Filter timeline by person "Cera"
    let req_actor = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline?person=Cera")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req_actor).await.unwrap();
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let days: Vec<TimelineDay> = serde_json::from_slice(&b).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].items[0].title, "Superbad");

    // 4. Filter timeline by date "2024-06"
    let req_date = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline?date=2024-06")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req_date).await.unwrap();
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let days: Vec<TimelineDay> = serde_json::from_slice(&b).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].date, "2024-06-10");

    // 5. Filter timeline by rating "9+" -> should only return Interstellar (rating 10)
    let req_rate = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline?rating=9+")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req_rate).await.unwrap();
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let days: Vec<TimelineDay> = serde_json::from_slice(&b).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].items[0].title, "Interstellar");

    // 6. Filter history by watch_status "in_progress"
    let req_status = Request::builder()
        .method("GET")
        .uri("/api/v1/history?watch_status=in_progress")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req_status).await.unwrap();
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&b).unwrap();
    assert_eq!(hist.total, 1);
    assert_eq!(hist.items[0].title, "Superbad");

    // 7. Test Integrations status endpoint
    let req_arr = Request::builder()
        .method("GET")
        .uri("/api/v1/integrations/status")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req_arr).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_subpath_routing_and_assets() {
    let pool = init_db("sqlite::memory:").await.unwrap();
    let mut config = Config::from_env();
    config.base_path = "/bingesync".to_string();
    let config = Arc::new(config);
    let state = AppState {
        pool,
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
    };
    let app = create_api_router(state).fallback(bingesync::assets::static_handler);
    let base = config.base_path.trim_end_matches('/').to_string();
    let redirect_target = format!("{}/", base);
    let app = axum::Router::new()
        .route(
            &base,
            axum::routing::get({
                let redirect_target = redirect_target.clone();
                move || async move { axum::response::Redirect::permanent(&redirect_target) }
            }),
        )
        .nest(&base, app.clone())
        .merge(app);

    // Request /bingesync (should redirect to /bingesync/)
    let req = Request::builder().uri("/bingesync").body(Body::empty()).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    println!("Status for /bingesync: {}", res.status());
    assert_eq!(res.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(res.headers().get("location").unwrap(), "/bingesync/");

    // Request /bingesync/ (should serve index.html with 200 OK)
    let req2 = Request::builder().uri("/bingesync/").body(Body::empty()).unwrap();
    let res2 = app.clone().oneshot(req2).await.unwrap();
    println!("Status for /bingesync/: {}", res2.status());
    assert_eq!(res2.status(), StatusCode::OK);

    // Request /bingesync/assets/index-DTQhH7_5.css
    let req_asset = Request::builder().uri("/bingesync/assets/index-DTQhH7_5.css").body(Body::empty()).unwrap();
    let res_asset = app.clone().oneshot(req_asset).await.unwrap();
    println!("Status for /bingesync/assets/index-DTQhH7_5.css: {}", res_asset.status());
    assert_eq!(res_asset.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_admin_user_management_and_cascading_deletion() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create an admin user and a standard user
    let admin_user = bingesync::api::auth::get_or_create_user(
        &pool,
        "superadmin@test.local",
        "Super Admin",
        None,
        None,
        true, // is_admin
    )
    .await
    .unwrap();
    assert_eq!(admin_user.role, "admin");

    let standard_user = bingesync::api::auth::get_or_create_user(
        &pool,
        "standard@test.local",
        "Standard User",
        None,
        None,
        false, // not admin
    )
    .await
    .unwrap();
    assert_eq!(standard_user.role, "user");

    let admin_token = create_jwt(&admin_user.id, &admin_user.email, &config.jwt_secret).unwrap();
    let standard_token = create_jwt(&standard_user.id, &standard_user.email, &config.jwt_secret).unwrap();

    // 2. Standard user scrobbles an item
    let scrobble_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", standard_user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "event": "media.scrobble",
            "Metadata": {
                "type": "movie",
                "title": "Inception",
                "year": 2010
            }
        }).to_string()))
        .unwrap();
    let res_scrobble = app.clone().oneshot(scrobble_req).await.unwrap();
    assert_eq!(res_scrobble.status(), StatusCode::CREATED);

    // Standard user has an active session
    let play_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", standard_user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "event": "media.play",
            "Player": { "title": "Living Room TV" },
            "Metadata": { "type": "movie", "title": "Interstellar", "duration": 10000 },
            "viewOffset": 2000
        }).to_string()))
        .unwrap();
    let res_play = app.clone().oneshot(play_req).await.unwrap();
    assert_eq!(res_play.status(), StatusCode::OK);

    // 3. Non-admin attempting to list users receives 403 Forbidden
    let non_admin_list_req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/users")
        .header(header::AUTHORIZATION, format!("Bearer {}", standard_token))
        .body(Body::empty())
        .unwrap();
    let res_forbidden = app.clone().oneshot(non_admin_list_req).await.unwrap();
    assert_eq!(res_forbidden.status(), StatusCode::FORBIDDEN);

    // 4. Admin listing users receives 200 OK with scrobble counts
    let admin_list_req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/users")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();
    let res_users = app.clone().oneshot(admin_list_req).await.unwrap();
    assert_eq!(res_users.status(), StatusCode::OK);

    let body = axum::body::to_bytes(res_users.into_body(), usize::MAX).await.unwrap();
    let users_list: Vec<bingesync::models::AdminUserItem> = serde_json::from_slice(&body).unwrap();
    assert!(users_list.len() >= 2);

    let found_standard = users_list.iter().find(|u| u.id == standard_user.id).unwrap();
    assert_eq!(found_standard.email, "standard@test.local");
    assert_eq!(found_standard.role, "user");
    // Standard user has 1 completed scrobble ("Inception") + 1 in-progress started scrobble ("Interstellar")
    assert_eq!(found_standard.scrobble_count, 2);

    // 5. Admin cannot delete self (400 Bad Request)
    let self_delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/admin/users/{}", admin_user.id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();
    let res_self_del = app.clone().oneshot(self_delete_req).await.unwrap();
    assert_eq!(res_self_del.status(), StatusCode::BAD_REQUEST);

    // 6. Admin deletes standard user (200 OK)
    let delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/admin/users/{}", standard_user.id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();
    let res_del = app.clone().oneshot(delete_req).await.unwrap();
    assert_eq!(res_del.status(), StatusCode::OK);

    // 7. Verify all data associated with deleted user is completely gone
    let scrobble_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&standard_user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scrobble_count.0, 0);

    let session_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM active_sessions WHERE user_id = ?")
        .bind(&standard_user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(session_count.0, 0);

    let user_exists: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
        .bind(&standard_user.id)
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(user_exists.is_none());
}

#[tokio::test]
async fn test_oidc_backchannel_logout() {
    let pool = init_db("sqlite::memory:").await.unwrap();
    let mut config = Config::from_env();
    config.jwt_secret = "test-secret-key-12345".to_string();
    config.app_url = "http://localhost:8080".to_string();
    config.oidc_enabled = true;
    config.oidc_issuer_url = Some("https://auth.example.com".to_string());
    config.oidc_client_id = Some("bingesync-client".to_string());
    config.oidc_client_secret = Some("idp-client-secret-999".to_string());
    let config = Arc::new(config);

    let oidc_client = Arc::new(OidcClient::new(
        "https://auth.example.com".to_string(),
        "bingesync-client".to_string(),
        Some("idp-client-secret-999".to_string()),
        "http://localhost:8080/api/v1/auth/callback".to_string(),
    ));

    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: Some(oidc_client),
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
    };

    let app = create_api_router(state);

    // 1. Create a user with oidc_sub and oidc_sid
    let user = bingesync::api::auth::get_or_create_user_with_sid(
        &pool,
        "revoked_user@example.com",
        "Revoked User",
        Some("pocketid_sub_12345"),
        Some("pocketid_sid_abcde"),
        None,
        Some("revoked_user"),
        false,
    )
    .await
    .unwrap();

    // 2. Issue a JWT for this user
    let user_token = bingesync::auth::create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // Verify token works on /api/v1/auth/me
    let me_req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();
    let res_me = app.clone().oneshot(me_req).await.unwrap();
    assert_eq!(res_me.status(), StatusCode::OK);

    // 3. Create a valid OpenID Connect Back-Channel Logout token
    let now = chrono::Utc::now().timestamp();
    let valid_claims = json!({
        "iss": "https://auth.example.com",
        "sub": "pocketid_sub_12345",
        "aud": "bingesync-client",
        "iat": now,
        "jti": uuid::Uuid::new_v4().to_string(),
        "events": {
            "http://schemas.openid.net/event/backchannel-logout": {}
        }
    });

    let logout_token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &valid_claims,
        &jsonwebtoken::EncodingKey::from_secret("idp-client-secret-999".as_bytes()),
    )
    .unwrap();

    // 4. Send POST /api/v1/auth/backchannel-logout with form-urlencoded body
    let logout_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/backchannel-logout")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("logout_token={}", logout_token)))
        .unwrap();

    let res_logout = app.clone().oneshot(logout_req).await.unwrap();
    assert_eq!(res_logout.status(), StatusCode::OK);
    assert_eq!(
        res_logout.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache, no-store"
    );
    assert_eq!(
        res_logout.headers().get(header::PRAGMA).unwrap(),
        "no-cache"
    );

    let body_bytes = axum::body::to_bytes(res_logout.into_body(), usize::MAX).await.unwrap();
    let logout_res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(logout_res_json["status"], "ok");
    assert_eq!(logout_res_json["revoked"], true);

    // 5. Verify the user's previous token is now rejected with 401 Unauthorized
    let me_req_after = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();
    let res_me_after = app.clone().oneshot(me_req_after).await.unwrap();
    assert_eq!(res_me_after.status(), StatusCode::UNAUTHORIZED);

    // 6. Test invalid logout token (spec rule: nonce must NOT be present)
    let nonce_claims = json!({
        "iss": "https://auth.example.com",
        "sub": "pocketid_sub_12345",
        "aud": "bingesync-client",
        "iat": now,
        "nonce": "forbidden_nonce",
        "events": {
            "http://schemas.openid.net/event/backchannel-logout": {}
        }
    });
    let nonce_token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &nonce_claims,
        &jsonwebtoken::EncodingKey::from_secret("idp-client-secret-999".as_bytes()),
    )
    .unwrap();

    let invalid_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/backchannel-logout")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("logout_token={}", nonce_token)))
        .unwrap();
    let res_invalid = app.clone().oneshot(invalid_req).await.unwrap();
    assert_eq!(res_invalid.status(), StatusCode::BAD_REQUEST);

    // 7. Test invalid logout token (missing backchannel-logout event claim)
    let no_event_claims = json!({
        "iss": "https://auth.example.com",
        "sub": "pocketid_sub_12345",
        "aud": "bingesync-client",
        "iat": now
    });
    let no_event_token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &no_event_claims,
        &jsonwebtoken::EncodingKey::from_secret("idp-client-secret-999".as_bytes()),
    )
    .unwrap();

    let no_event_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/backchannel-logout")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "logout_token": no_event_token }).to_string()))
        .unwrap();
    let res_no_event = app.clone().oneshot(no_event_req).await.unwrap();
    assert_eq!(res_no_event.status(), StatusCode::BAD_REQUEST);

    // 8. Test alias route /api/v1/auth/backchannel_logout with sid session logout
    let sid_claims = json!({
        "iss": "https://auth.example.com",
        "sid": "pocketid_sid_abcde",
        "aud": "bingesync-client",
        "iat": now,
        "events": {
            "http://schemas.openid.net/event/backchannel-logout": {}
        }
    });
    let sid_token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &sid_claims,
        &jsonwebtoken::EncodingKey::from_secret("idp-client-secret-999".as_bytes()),
    )
    .unwrap();

    let sid_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/backchannel_logout")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("logout_token={}", sid_token)))
        .unwrap();
    let res_sid = app.clone().oneshot(sid_req).await.unwrap();
    assert_eq!(res_sid.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_image_cache_serving_and_background_worker() {
    let pool = init_db("sqlite::memory:").await.unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut config = Config::from_env();
    config.image_cache_dir = temp_dir.path().to_string_lossy().to_string();
    let config = Arc::new(config);

    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
    };

    let app = create_api_router(state.clone());

    // 1. Write a test image to the cache directory on disk
    let test_img_path = temp_dir.path().join("test_poster.jpg");
    let test_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46]; // JPEG header bytes
    tokio::fs::write(&test_img_path, &test_bytes).await.unwrap();

    // 2. Request GET /api/v1/images/test_poster.jpg
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/images/test_poster.jpg")
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );
    assert_eq!(
        res.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=31536000, immutable"
    );
    let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.to_vec(), test_bytes);

    // 3. Test 404 on nonexistent image
    let not_found_req = Request::builder()
        .method("GET")
        .uri("/api/v1/images/nonexistent_image.jpg")
        .body(Body::empty())
        .unwrap();
    let not_found_res = app.clone().oneshot(not_found_req).await.unwrap();
    assert_eq!(not_found_res.status(), StatusCode::NOT_FOUND);

    // 4. Test directory traversal security rejection
    let traversal_req = Request::builder()
        .method("GET")
        .uri("/api/v1/images/..%2F..%2Fetc%2Fpasswd")
        .body(Body::empty())
        .unwrap();
    let traversal_res = app.clone().oneshot(traversal_req).await.unwrap();
    assert!(
        traversal_res.status() == StatusCode::BAD_REQUEST
            || traversal_res.status() == StatusCode::NOT_FOUND
    );

    // 5. Test background image worker processing scrobbles without thumbnails
    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "imgtest@test.local",
        "Image Tester",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    // Insert scrobble with thumb_url = None
    let scrobble_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO scrobbles (
            id, user_id, title, media_type, grandparent_title, parent_title,
            season_number, episode_number, year, overview, thumb_url,
            duration_ms, user_rating, watch_status, watched_at, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&scrobble_id)
    .bind(&user.id)
    .bind("Pilot")
    .bind("episode")
    .bind("Breaking Bad")
    .bind("Season 1")
    .bind(1)
    .bind(1)
    .bind(2008)
    .bind("A high school chemistry teacher diagnosed with lung cancer turns to manufacturing methamphetamine.")
    .bind(None::<String>) // thumb_url is initially None
    .bind(3480000_i64)
    .bind(10)
    .bind("completed")
    .bind("2024-01-01T20:00:00Z")
    .bind("2024-01-01T20:00:00Z")
    .execute(&pool)
    .await
    .unwrap();

    // Run process_missing_images_once
    let processed = bingesync::api::images::process_missing_images_once(&state).await;
    // If online, it will download and cache the poster to disk
    if processed > 0 {
        let scrobble: Scrobble = sqlx::query_as("SELECT * FROM scrobbles WHERE id = ?")
            .bind(&scrobble_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(scrobble.thumb_url.is_some());
        let thumb_url = scrobble.thumb_url.unwrap();
        assert!(thumb_url.starts_with("/api/v1/images/show_"));

        // Verify the file was written to disk
        let filename = thumb_url.strip_prefix("/api/v1/images/").unwrap();
        let cached_path = temp_dir.path().join(filename);
        assert!(cached_path.exists());

        // Verify requesting the cached image returns 200 OK
        let img_req = Request::builder()
            .method("GET")
            .uri(&thumb_url)
            .body(Body::empty())
            .unwrap();
        let img_res = app.oneshot(img_req).await.unwrap();
        assert_eq!(img_res.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn test_sonarr_delete_options() {
    let pool = init_db("sqlite::memory:").await.unwrap();

    // Spin up a mock Sonarr server
    let mock_sonarr_router = axum::Router::new()
        .route(
            "/api/v3/episodefile",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!([
                    { "id": 101, "seriesId": 42, "seasonNumber": 1 },
                    { "id": 102, "seriesId": 42, "seasonNumber": 1 },
                    { "id": 201, "seriesId": 42, "seasonNumber": 2 }
                ]))
            }),
        )
        .route(
            "/api/v3/episodefile/{id}",
            axum::routing::delete(|| async { StatusCode::OK }),
        )
        .route(
            "/api/v3/series/{id}",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({
                    "id": 42,
                    "title": "Breaking Bad",
                    "monitored": true,
                    "seasons": [
                        { "seasonNumber": 1, "monitored": true },
                        { "seasonNumber": 2, "monitored": true }
                    ]
                }))
            })
            .put(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                axum::Json(body)
            })
            .delete(|| async { StatusCode::OK }),
        )
        .route(
            "/api/v3/episode",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!([
                    { "id": 1001, "seasonNumber": 1 },
                    { "id": 1002, "seasonNumber": 1 }
                ]))
            }),
        )
        .route(
            "/api/v3/episode/monitor",
            axum::routing::put(|| async { StatusCode::OK }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, mock_sonarr_router).await.unwrap();
    });

    let config = Arc::new(Config::from_env());
    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
    };
    let app = create_api_router(state);

    // Dev login to obtain valid token
    let login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/dev-login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "email": "sonarr_tester@bingesync.local",
                "display_name": "Sonarr Tester"
            })
            .to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(login_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let user_id = login_data["user"]["id"].as_str().unwrap();
    let token = login_data["token"].as_str().unwrap().to_string();

    bingesync::api::user_settings::set_user_setting(&pool, user_id, "sonarr_url", &format!("http://{}", addr)).await.unwrap();
    bingesync::api::user_settings::set_user_setting(&pool, user_id, "sonarr_api_key", "test_api_key").await.unwrap();

    // 1. Test Option 1: delete_series (delete show and files)
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/integrations/sonarr/delete-series")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "series_id": 42,
                "action": "delete_series",
                "delete_files": true
            })
            .to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let b = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(json["action"], "delete_series");
    assert_eq!(json["status"], "deleted");

    // 2. Test Option 2: delete_season (just delete and unmonitor season files)
    let req2 = Request::builder()
        .method("POST")
        .uri("/api/v1/integrations/sonarr/delete-series")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "series_id": 42,
                "action": "delete_season",
                "season_number": 1
            })
            .to_string(),
        ))
        .unwrap();
    let res2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let b2 = axum::body::to_bytes(res2.into_body(), usize::MAX).await.unwrap();
    let json2: serde_json::Value = serde_json::from_slice(&b2).unwrap();
    assert_eq!(json2["action"], "delete_season");
    assert_eq!(json2["season_number"], 1);
    assert_eq!(json2["deleted_files_count"], 2);

    // 3. Test Option 3: delete_all_files_unmonitor (delete all files and unmonitor all seasons)
    let req3 = Request::builder()
        .method("POST")
        .uri("/api/v1/integrations/sonarr/delete-series")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "series_id": 42,
                "action": "delete_all_files_unmonitor"
            })
            .to_string(),
        ))
        .unwrap();
    let res3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(res3.status(), StatusCode::OK);
    let b3 = axum::body::to_bytes(res3.into_body(), usize::MAX).await.unwrap();
    let json3: serde_json::Value = serde_json::from_slice(&b3).unwrap();
    assert_eq!(json3["action"], "delete_all_files_unmonitor");
    assert_eq!(json3["deleted_files_count"], 3);

    // 4. Test missing season_number validation for delete_season
    let req4 = Request::builder()
        .method("POST")
        .uri("/api/v1/integrations/sonarr/delete-series")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "series_id": 42,
                "action": "delete_season"
            })
            .to_string(),
        ))
        .unwrap();
    let res4 = app.clone().oneshot(req4).await.unwrap();
    assert_eq!(res4.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_tvdb_metadata_enrichment_worker() {
    let pool = init_db("sqlite::memory:").await.unwrap();

    // 1. Mock TVDB v4 API server
    let mock_tvdb_router = axum::Router::new()
        .route(
            "/login",
            axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                if body.get("apikey").and_then(|v| v.as_str()) == Some("test_tvdb_key") {
                    (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "status": "success",
                            "data": { "token": "mock_tvdb_bearer_token" }
                        })),
                    )
                } else {
                    (
                        StatusCode::UNAUTHORIZED,
                        axum::Json(serde_json::json!({ "error": "Invalid API key" })),
                    )
                }
            }),
        )
        .route(
            "/search",
            axum::routing::get(|axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                let query = params.get("query").map(|s| s.as_str()).unwrap_or("");
                if query.eq_ignore_ascii_case("Breaking Bad") {
                    axum::Json(serde_json::json!({
                        "status": "success",
                        "data": [
                            {
                                "tvdb_id": "121361",
                                "name": "Breaking Bad",
                                "overview": "A chemistry teacher with cancer starts making meth.",
                                "image_url": "https://artworks.thetvdb.com/banners/posters/121361-1.jpg"
                            }
                        ]
                    }))
                } else {
                    axum::Json(serde_json::json!({ "status": "success", "data": [] }))
                }
            }),
        )
        .route(
            "/series/{id}/extended",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({
                    "status": "success",
                    "data": {
                        "id": 121361,
                        "name": "Breaking Bad",
                        "overview": "A chemistry teacher diagnosed with inoperable lung cancer turns to manufacturing methamphetamine.",
                        "genres": [
                            { "id": 1, "name": "Crime" },
                            { "id": 2, "name": "Drama" },
                            { "id": 3, "name": "Thriller" }
                        ],
                        "characters": [
                            { "personName": "Bryan Cranston", "peopleType": "Actor" },
                            { "personName": "Aaron Paul", "peopleType": "Actor" },
                            { "personName": "Vince Gilligan", "peopleType": "Director" }
                        ],
                        "image": "https://artworks.thetvdb.com/banners/posters/121361-1.jpg"
                    }
                }))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, mock_tvdb_router).await.unwrap();
    });

    let mut config = Config::from_env();
    config.tvdb_api_key = Some("test_tvdb_key".to_string());
    config.tvdb_base_url = format!("http://{}", addr);
    let config = Arc::new(config);
    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
    };

    // 2. Create test user and insert a scrobble with missing overview, genres, actors, directors
    let user_id = "test_user_tvdb";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token) VALUES (?, ?, ?, ?)")
        .bind(user_id)
        .bind("tvdb_tester@bingesync.local")
        .bind("TVDB Tester")
        .bind("token_tvdb_123")
        .execute(&pool)
        .await
        .unwrap();

    let scrobble_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO scrobbles (
            id, user_id, title, media_type, grandparent_title, parent_title,
            season_number, episode_number, year, overview, thumb_url,
            duration_ms, user_rating, watch_status, watched_at, created_at
        ) VALUES (
            ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?
        )
        "#
    )
    .bind(&scrobble_id)
    .bind(user_id)
    .bind("Pilot")
    .bind("episode")
    .bind("Breaking Bad")
    .bind("Season 1")
    .bind(1)
    .bind(1)
    .bind(2008)
    .bind(None::<String>) // overview is empty
    .bind(None::<String>) // thumb is empty
    .bind(3480000_i64)
    .bind(10)
    .bind("completed")
    .bind("2024-01-01T20:00:00Z")
    .bind("2024-01-01T20:00:00Z")
    .execute(&pool)
    .await
    .unwrap();

    // 3. Run TVDB metadata worker once
    let processed = bingesync::api::tvdb::process_missing_metadata_once(&state).await;
    assert_eq!(processed, 1);

    // 4. Verify scrobble now has genres, actors, directors, and overview populated from TVDB!
    let scrobble: Scrobble = sqlx::query_as("SELECT * FROM scrobbles WHERE id = ?")
        .bind(&scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(scrobble.genres.as_deref(), Some("Crime, Drama, Thriller"));
    assert_eq!(scrobble.actors.as_deref(), Some("Bryan Cranston, Aaron Paul"));
    assert_eq!(scrobble.directors.as_deref(), Some("Vince Gilligan"));
    assert!(scrobble.overview.is_some());
    assert!(scrobble.overview.unwrap().contains("chemistry teacher"));
    assert!(scrobble.metadata_updated_at.is_some());

    // 5. Subsequent runs must NOT re-fetch already enriched metadata
    let processed_second_run = bingesync::api::tvdb::process_missing_metadata_once(&state).await;
    assert_eq!(processed_second_run, 0);
}

#[tokio::test]
async fn test_item_other_watches_movies_and_exact_episodes() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create User A and User B
    let user_a_id = "user_watch_a";
    let token_a = "token_watch_a";
    let jwt_a = create_jwt(user_a_id, "user_a@example.com", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("user_a@example.com")
        .bind("User A")
        .bind(token_a)
        .execute(&pool)
        .await
        .unwrap();

    let user_b_id = "user_watch_b";
    let token_b = "token_watch_b";
    let jwt_b = create_jwt(user_b_id, "user_b@example.com", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("user_b@example.com")
        .bind("User B")
        .bind(token_b)
        .execute(&pool)
        .await
        .unwrap();

    // Helper to insert scrobbles
    async fn insert_scrobble(
        pool: &sqlx::SqlitePool,
        user_id: &str,
        title: &str,
        media_type: &str,
        grandparent_title: Option<&str>,
        season_number: Option<i32>,
        episode_number: Option<i32>,
        year: Option<i32>,
        watched_at: &str,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO scrobbles (
                id, user_id, title, media_type, grandparent_title, season_number, episode_number,
                year, watched_at, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(user_id)
        .bind(title)
        .bind(media_type)
        .bind(grandparent_title)
        .bind(season_number)
        .bind(episode_number)
        .bind(year)
        .bind(watched_at)
        .bind(watched_at)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    // 2. Insert watches for User A
    // Inception (3 watches)
    let incep_1 = insert_scrobble(&pool, user_a_id, "Inception", "movie", None, None, None, Some(2010), "2023-01-01T20:00:00Z").await;
    let incep_2 = insert_scrobble(&pool, user_a_id, "Inception", "movie", None, None, None, Some(2010), "2023-06-15T21:00:00Z").await;
    let incep_3 = insert_scrobble(&pool, user_a_id, "Inception", "movie", None, None, None, Some(2010), "2024-02-10T19:30:00Z").await;

    // Interstellar (1 watch)
    let inter_1 = insert_scrobble(&pool, user_a_id, "Interstellar", "movie", None, None, None, Some(2014), "2023-03-01T20:00:00Z").await;

    // Breaking Bad S01E01 (2 watches)
    let bb_s1e1_1 = insert_scrobble(&pool, user_a_id, "Pilot", "episode", Some("Breaking Bad"), Some(1), Some(1), Some(2008), "2023-04-01T20:00:00Z").await;
    let bb_s1e1_2 = insert_scrobble(&pool, user_a_id, "Pilot", "episode", Some("Breaking Bad"), Some(1), Some(1), Some(2008), "2023-08-01T20:00:00Z").await;

    // Breaking Bad S01E02 (1 watch) - Different episode!
    let bb_s1e2_1 = insert_scrobble(&pool, user_a_id, "Cat's in the Bag...", "episode", Some("Breaking Bad"), Some(1), Some(2), Some(2008), "2023-04-02T20:00:00Z").await;

    // Better Call Saul S01E01 (1 watch) - Different show!
    let _bcs_s1e1_1 = insert_scrobble(&pool, user_a_id, "Uno", "episode", Some("Better Call Saul"), Some(1), Some(1), Some(2015), "2023-05-01T20:00:00Z").await;

    // 3. Insert watches for User B (must NOT be visible to User A)
    let _user_b_bb = insert_scrobble(&pool, user_b_id, "Pilot", "episode", Some("Breaking Bad"), Some(1), Some(1), Some(2008), "2023-09-01T20:00:00Z").await;
    let _user_b_incep = insert_scrobble(&pool, user_b_id, "Inception", "movie", None, None, None, Some(2010), "2023-09-02T20:00:00Z").await;

    // 4. Test User A querying Inception watch 1:
    let req = Request::builder()
        .uri(format!("/api/v1/history/{}/watches", incep_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", jwt_a))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: bingesync::api::history::ItemWatchesResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(res.current_id, incep_1);
    assert_eq!(res.total_watches, 3);
    assert_eq!(res.other_watches.len(), 2);
    // Ordered by watched_at DESC -> incep_3 (2024) then incep_2 (2023-06)
    assert_eq!(res.other_watches[0].id, incep_3);
    assert_eq!(res.other_watches[1].id, incep_2);

    // 5. Test User A querying Breaking Bad S01E01 watch 1:
    // Must ONLY return S01E01 watch 2 (NOT S01E02, NOT other shows, NOT User B)
    let req = Request::builder()
        .uri(format!("/api/v1/history/{}/watches", bb_s1e1_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", jwt_a))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: bingesync::api::history::ItemWatchesResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(res.current_id, bb_s1e1_1);
    assert_eq!(res.total_watches, 2);
    assert_eq!(res.other_watches.len(), 1);
    assert_eq!(res.other_watches[0].id, bb_s1e1_2);
    assert_eq!(res.other_watches[0].season_number, Some(1));
    assert_eq!(res.other_watches[0].episode_number, Some(1));

    // 6. Test User A querying Breaking Bad S01E02 (different episode):
    // Has only 1 watch -> total_watches = 1, other_watches = []
    let req = Request::builder()
        .uri(format!("/api/v1/history/{}/watches", bb_s1e2_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", jwt_a))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: bingesync::api::history::ItemWatchesResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(res.current_id, bb_s1e2_1);
    assert_eq!(res.total_watches, 1);
    assert_eq!(res.other_watches.len(), 0);

    // 7. Test User A querying Interstellar:
    let req = Request::builder()
        .uri(format!("/api/v1/history/{}/watches", inter_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", jwt_a))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: bingesync::api::history::ItemWatchesResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(res.total_watches, 1);
    assert_eq!(res.other_watches.len(), 0);

    // 8. Test isolation: User B attempting to query User A's scrobble returns 404 Not Found
    let req = Request::builder()
        .uri(format!("/api/v1/history/{}/watches", incep_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", jwt_b))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_admin_settings_logs_and_tasks_management() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create Admin User and Standard User
    let admin_id = "test_admin_user";
    let admin_jwt = create_jwt(admin_id, "admin@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(admin_id)
        .bind("admin@test.local")
        .bind("Admin")
        .bind("admin_token")
        .execute(&pool)
        .await
        .unwrap();

    let standard_id = "test_standard_user";
    let standard_jwt = create_jwt(standard_id, "user@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(standard_id)
        .bind("user@test.local")
        .bind("Standard")
        .bind("user_token")
        .execute(&pool)
        .await
        .unwrap();

    // 2. Non-admin attempting to access settings returns 403 Forbidden
    let req = Request::builder()
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", standard_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 3. Admin querying settings returns 200 OK
    let req = Request::builder()
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 4. Admin updating settings
    let update_body = json!({
        "tvdb_api_key": "tvdb_dyn_key_789",
        "tvdb_pin": "1234",
        "oidc_enabled": true,
        "oidc_issuer_url": "https://auth.local",
        "oidc_client_id": "bs-client",
        "oidc_client_secret": "secret999",
        "scrobble_threshold_percent": 85
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(update_body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify settings were persisted and secrets are masked
    let req = Request::builder()
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings: bingesync::api::settings::SystemSettingsView = serde_json::from_slice(&body).unwrap();

    assert!(settings.tvdb_api_key_configured);
    assert_eq!(settings.tvdb_pin.as_deref(), Some("1234"));
    assert!(settings.tvdb_api_key_masked.as_deref().unwrap().starts_with("tv"));
    assert!(settings.tvdb_api_key_masked.as_deref().unwrap().contains('•'));
    assert!(settings.oidc_enabled);
    assert_eq!(settings.scrobble_threshold_percent, 85);

    // 5. Test logs endpoint
    let req = Request::builder()
        .uri("/api/v1/admin/logs")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let logs_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(logs_json["entries"].is_array());

    // 6. Test background tasks endpoint
    let req = Request::builder()
        .uri("/api/v1/admin/tasks")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let tasks_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let tasks = tasks_json["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().any(|t| t["id"] == "image_cache"));
    assert!(tasks.iter().any(|t| t["id"] == "tvdb_metadata"));

    // 7. Test triggering background task run
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/image_cache/run")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_stats_top_movies() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "stats_movies_user";
    let token = create_jwt(user_id, "stats@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_id)
        .bind("stats@test.local")
        .bind("Movie Buff")
        .bind("buff_token")
        .execute(&pool)
        .await
        .unwrap();

    // Insert 3 watches of Inception and 1 watch of Interstellar
    for (title, date) in [
        ("Inception", "2023-01-01T20:00:00Z"),
        ("Inception", "2023-02-01T20:00:00Z"),
        ("Inception", "2023-03-01T20:00:00Z"),
        ("Interstellar", "2023-04-01T20:00:00Z"),
    ] {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO scrobbles (id, user_id, title, media_type, year, duration_ms, watched_at, created_at) VALUES (?, ?, ?, 'movie', 2010, 7200000, ?, ?)"
        )
        .bind(id)
        .bind(user_id)
        .bind(title)
        .bind(date)
        .bind(date)
        .execute(&pool)
        .await
        .unwrap();
    }

    let req = Request::builder()
        .uri("/api/v1/stats")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stats: StatsSummary = serde_json::from_slice(&body).unwrap();

    assert_eq!(stats.movies_watched, 4);
    assert_eq!(stats.top_movies.len(), 2);
    assert_eq!(stats.top_movies[0].title, "Inception");
    assert_eq!(stats.top_movies[0].count, 3);
    assert_eq!(stats.top_movies[1].title, "Interstellar");
    assert_eq!(stats.top_movies[1].count, 1);
}

#[tokio::test]
async fn test_scrobble_progress_threshold_and_deduplication() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_progress";
    let webhook_token = "progress_token_123";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(user_id)
        .bind("progress@test.local")
        .bind("Progress Tester")
        .bind(webhook_token)
        .execute(&pool)
        .await
        .unwrap();

    let admin_jwt = create_jwt(user_id, "progress@test.local", &config.jwt_secret).unwrap();

    // 1. Play movie at 30% progress (below 80% default threshold)
    let payload_play_30 = serde_json::json!({
        "event": "media.play",
        "Metadata": {
            "title": "The Matrix",
            "type": "movie",
            "year": 1999,
            "duration": 10000,
            "viewOffset": 3000
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex?token={}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&payload_play_30).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["status"], "session_updated");
    assert_eq!(body["watch_status"], "in_progress");
    assert_eq!(body["progress_percent"], 30);

    // Verify 1 scrobble in DB
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND title = 'The Matrix'")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // 2. Play movie at 85% progress on same day (exceeds 80% threshold -> marked completed and deduplicated)
    let payload_play_85 = serde_json::json!({
        "event": "media.play",
        "Metadata": {
            "title": "The Matrix",
            "type": "movie",
            "year": 1999,
            "duration": 10000,
            "viewOffset": 8500
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex?token={}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&payload_play_85).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["watch_status"], "completed");
    assert_eq!(body["progress_percent"], 85);

    // Verify SAME-DAY DEDUPLICATION: Still exactly 1 record in DB!
    let count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND title = 'The Matrix'")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_after, 1);

    // 3. Dynamically reconfigure completion threshold to 90% via admin settings
    let update_settings_req = Request::builder()
        .method("PUT")
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "scrobble_threshold_percent": 90
        })).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(update_settings_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 4. Play a new movie "Inception" at 85% (now BELOW the new 90% threshold -> in_progress)
    let payload_inception_85 = serde_json::json!({
        "event": "media.play",
        "Metadata": {
            "title": "Inception",
            "type": "movie",
            "year": 2010,
            "duration": 10000,
            "viewOffset": 8500
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex?token={}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&payload_inception_85).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["watch_status"], "in_progress");
    assert_eq!(body["progress_percent"], 85);

    // 5. Play "Inception" at 95% (exceeds 90% -> completed)
    let payload_inception_95 = serde_json::json!({
        "event": "media.play",
        "Metadata": {
            "title": "Inception",
            "type": "movie",
            "year": 2010,
            "duration": 10000,
            "viewOffset": 9500
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex?token={}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&payload_inception_95).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["watch_status"], "completed");
    assert_eq!(body["progress_percent"], 95);

    // Verify deduplication: exactly 1 Inception scrobble
    let count_inception: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND title = 'Inception'")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_inception, 1);
}

#[tokio::test]
async fn test_tautulli_import_and_deduplication() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_tautulli";
    let token = create_jwt(user_id, "tautulli_user@test.local", &config.jwt_secret).unwrap();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, preferred_username, webhook_token, role) VALUES (?, ?, ?, ?, ?, 'user')"
    )
    .bind(user_id)
    .bind("tautulli_user@test.local")
    .bind("Tautulli Fan")
    .bind("daryl_tautulli")
    .bind("tautulli_wh_token")
    .execute(&pool)
    .await
    .unwrap();

    // 1. Verify GET /api/v1/auth/me returns preferred_username
    let me_req = Request::builder()
        .uri("/api/v1/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let me_resp = app.clone().oneshot(me_req).await.unwrap();
    assert_eq!(me_resp.status(), StatusCode::OK);
    let me_body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(me_resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(me_body["preferred_username"], "daryl_tautulli");

    // 2. Import Tautulli JSON content containing 2 items for "daryl_tautulli" and 1 item for "stranger"
    let tautulli_json = serde_json::json!({
        "response": {
            "result": "success",
            "data": {
                "data": [
                    {
                        "user": "daryl_tautulli",
                        "media_type": "movie",
                        "title": "The Prestige",
                        "year": 2006,
                        "date": 1672531200,
                        "duration": 7800,
                        "rating_key": "5001",
                        "percent_complete": 100
                    },
                    {
                        "user": "daryl_tautulli",
                        "media_type": "episode",
                        "grandparent_title": "Succession",
                        "title": "All the Bells Say",
                        "parent_media_index": 3,
                        "media_index": 9,
                        "year": 2021,
                        "date": 1672617600,
                        "duration": 3600,
                        "rating_key": "5002",
                        "percent_complete": 100
                    },
                    {
                        "user": "stranger",
                        "media_type": "movie",
                        "title": "Fight Club",
                        "year": 1999,
                        "date": 1672704000,
                        "duration": 8000,
                        "rating_key": "5003"
                    }
                ]
            }
        }
    });

    let import_req = Request::builder()
        .method("POST")
        .uri("/api/v1/import/tautulli")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "username": "daryl_tautulli",
            "content": tautulli_json.to_string(),
        })).unwrap()))
        .unwrap();

    let import_resp = app.clone().oneshot(import_req).await.unwrap();
    assert_eq!(import_resp.status(), StatusCode::OK);
    let import_body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(import_resp.into_body(), usize::MAX).await.unwrap()).unwrap();

    // 2 imported for daryl_tautulli, 0 skipped, stranger's movie was filtered out!
    assert_eq!(import_body["status"], "success");
    assert_eq!(import_body["imported"], 2);
    assert_eq!(import_body["skipped"], 0);

    // Verify database contents
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    // Verify "Fight Club" is NOT in DB
    let fc_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND title = 'Fight Club'")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(fc_count, 0);

    // 3. Re-import the exact same data -> DEDUPLICATION MUST SKIP THEM ALL!
    let reimport_req = Request::builder()
        .method("POST")
        .uri("/api/v1/import/tautulli")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "username": "daryl_tautulli",
            "content": tautulli_json.to_string(),
        })).unwrap()))
        .unwrap();

    let reimport_resp = app.clone().oneshot(reimport_req).await.unwrap();
    assert_eq!(reimport_resp.status(), StatusCode::OK);
    let reimport_body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(reimport_resp.into_body(), usize::MAX).await.unwrap()).unwrap();

    // 0 imported, 2 skipped
    assert_eq!(reimport_body["imported"], 0);
    assert_eq!(reimport_body["skipped"], 2);

    // DB count remains exactly 2!
    let count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_after, 2);

    // 4. Import from Tautulli CSV export
    let tautulli_csv = "date,friendly_name,user,title,media_type,rating_key,full_title,year,media_index,parent_media_index,duration,percent_complete\n1672800000,Daryl,daryl_tautulli,Good News About Hell,episode,9001,Severance - s01e01 - Good News About Hell,2022,1,1,3200,100\n";

    let csv_req = Request::builder()
        .method("POST")
        .uri("/api/v1/import/tautulli")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "username": "daryl_tautulli",
            "content": tautulli_csv,
        })).unwrap()))
        .unwrap();

    let csv_resp = app.clone().oneshot(csv_req).await.unwrap();
    assert_eq!(csv_resp.status(), StatusCode::OK);
    let csv_body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(csv_resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(csv_body["imported"], 1);
    assert_eq!(csv_body["skipped"], 0);

    // Verify Severance episode in DB
    let sev_scrobble: (String, String, i32, i32) = sqlx::query_as(
        "SELECT grandparent_title, title, season_number, episode_number FROM scrobbles WHERE user_id = ? AND title = 'Good News About Hell'"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(sev_scrobble.0, "Severance");
    assert_eq!(sev_scrobble.1, "Good News About Hell");
    assert_eq!(sev_scrobble.2, 1);
    assert_eq!(sev_scrobble.3, 1);
}

#[tokio::test]
async fn test_rate_watched_item() {
    let (app, pool, _config) = setup_test_app().await;

    // Create User A
    let user_a = bingesync::api::auth::get_or_create_user(
        &pool,
        "rater_a@bingesync.local",
        "Rater A",
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let token_a = create_jwt(&user_a.id, "rater_a@bingesync.local", "test-secret-key-12345").unwrap();

    // Create User B
    let user_b = bingesync::api::auth::get_or_create_user(
        &pool,
        "rater_b@bingesync.local",
        "Rater B",
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let token_b = create_jwt(&user_b.id, "rater_b@bingesync.local", "test-secret-key-12345").unwrap();

    // 1. Create a scrobble for User A
    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "title": "Interstellar",
                "media_type": "movie",
                "year": 2014,
                "duration_ms": 10140000
            })
            .to_string(),
        ))
        .unwrap();

    let res = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let created: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let scrobble_id = created["id"].as_str().unwrap().to_string();

    // 2. User A rates the item 9/10
    let rate_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/rate", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "rating": 9 }).to_string()))
        .unwrap();

    let res = app.clone().oneshot(rate_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["status"], "success");
    assert_eq!(body["rating"], 9);

    // Verify rating in DB
    let db_rating: Option<i32> = sqlx::query_scalar("SELECT user_rating FROM scrobbles WHERE id = ?")
        .bind(&scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(db_rating, Some(9));

    // 3. User A updates rating to 10/10 via PUT
    let put_req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/history/{}/rate", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "rating": 10 }).to_string()))
        .unwrap();

    let res = app.clone().oneshot(put_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let db_rating: Option<i32> = sqlx::query_scalar("SELECT user_rating FROM scrobbles WHERE id = ?")
        .bind(&scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(db_rating, Some(10));

    // 4. Invalid rating (> 10) returns 400 Bad Request
    let invalid_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/rate", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "rating": 11 }).to_string()))
        .unwrap();

    let res = app.clone().oneshot(invalid_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Invalid rating (< 1) returns 400 Bad Request
    let invalid_req_zero = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/rate", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "rating": 0 }).to_string()))
        .unwrap();

    let res = app.clone().oneshot(invalid_req_zero).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // 5. User B attempts to rate User A's scrobble -> 404 (multi-user isolation)
    let b_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/rate", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "rating": 5 }).to_string()))
        .unwrap();

    let res = app.clone().oneshot(b_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 6. User A clears rating (sets to null)
    let clear_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/rate", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "rating": null }).to_string()))
        .unwrap();

    let res = app.clone().oneshot(clear_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let db_rating: Option<i32> = sqlx::query_scalar("SELECT user_rating FROM scrobbles WHERE id = ?")
        .bind(&scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(db_rating, None);
}

#[tokio::test]
async fn test_openapi_spec_and_swagger_ui() {
    let (app, _pool, _config) = setup_test_app().await;

    // 1. Fetch OpenAPI specification JSON
    let spec_req = Request::builder()
        .method("GET")
        .uri("/api/v1/openapi.json")
        .body(Body::empty())
        .unwrap();

    let spec_res = app.clone().oneshot(spec_req).await.unwrap();
    assert_eq!(spec_res.status(), StatusCode::OK);
    assert_eq!(
        spec_res.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json; charset=utf-8"
    );

    let spec_bytes = axum::body::to_bytes(spec_res.into_body(), usize::MAX).await.unwrap();
    let spec_json: serde_json::Value = serde_json::from_slice(&spec_bytes).expect("Valid OpenAPI JSON");
    assert_eq!(spec_json["openapi"], "3.0.3");
    assert_eq!(spec_json["info"]["title"], "BingeSync API");
    assert!(spec_json["paths"]["/api/v1/history/{id}/rate"].is_object());
    assert!(spec_json["paths"]["/api/v1/import/tautulli"].is_object());
    assert!(spec_json["paths"]["/api/v1/admin/tasks"].is_object());

    // 2. Fetch Swagger UI HTML at /docs
    let docs_req = Request::builder()
        .method("GET")
        .uri("/docs")
        .body(Body::empty())
        .unwrap();

    let docs_res = app.clone().oneshot(docs_req).await.unwrap();
    assert_eq!(docs_res.status(), StatusCode::OK);
    let docs_body = String::from_utf8(
        axum::body::to_bytes(docs_res.into_body(), usize::MAX).await.unwrap().to_vec()
    ).unwrap();
    assert!(docs_body.contains("Swagger UI"));
    assert!(docs_body.contains("BingeSync API"));
    assert!(docs_body.contains("swagger-ui"));

    // 3. Fetch Swagger UI HTML at /swagger
    let swagger_req = Request::builder()
        .method("GET")
        .uri("/swagger")
        .body(Body::empty())
        .unwrap();

    let swagger_res = app.oneshot(swagger_req).await.unwrap();
    assert_eq!(swagger_res.status(), StatusCode::OK);
    let swagger_body = String::from_utf8(
        axum::body::to_bytes(swagger_res.into_body(), usize::MAX).await.unwrap().to_vec()
    ).unwrap();
    assert!(swagger_body.contains("Swagger UI"));
}

#[tokio::test]
async fn test_plex_webhook_internal_thumb_and_refresh_metadata_endpoint() {
    let (app, pool, config) = setup_test_app().await;

    let user = bingesync::api::auth::get_or_create_user(
        &pool,
        "southpark_fan@bingesync.local",
        "South Park Fan",
        None,
        None,
        false,
    )
    .await
    .unwrap();

    // 1. Send Plex webhook with internal unreachable Plex relative thumb path (e.g. /library/metadata/123/thumb/456)
    let payload = json!({
        "event": "media.scrobble",
        "user": true,
        "owner": true,
        "Account": { "title": "sp_fan" },
        "Metadata": {
            "type": "episode",
            "title": "Help, My Teenager Hates Me!",
            "grandparentTitle": "South Park",
            "parentTitle": "Season 25",
            "index": 3,
            "parentIndex": 25,
            "year": 2022,
            "duration": 1320000,
            "summary": "The boys get into airsofting.",
            "thumb": "/library/metadata/54321/thumb/1680000000"
        }
    });

    let webhook_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(webhook_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Verify stored scrobble has thumb_url = None (not the broken internal /library/ path)
    let scrobble: Scrobble = sqlx::query_as("SELECT * FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scrobble.thumb_url, None);
    assert_eq!(scrobble.metadata_updated_at, None);

    // 2. Call POST /api/v1/history/{id}/refresh-metadata
    let token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();
    let refresh_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/refresh-metadata", scrobble.id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let refresh_res = app.clone().oneshot(refresh_req).await.unwrap();
    assert_eq!(refresh_res.status(), StatusCode::OK);

    let refresh_body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(refresh_res.into_body(), usize::MAX).await.unwrap()
    ).unwrap();
    assert_eq!(refresh_body["status"], "refreshed");
    assert!(refresh_body["scrobble"]["metadata_updated_at"].is_string());

    // 3. User B cannot refresh User A's scrobble (isolation)
    let user_b = bingesync::api::auth::get_or_create_user(
        &pool,
        "intruder@bingesync.local",
        "Intruder",
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let token_b = create_jwt(&user_b.id, &user_b.email, &config.jwt_secret).unwrap();
    let unauthorized_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/refresh-metadata", scrobble.id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();

    let unauthorized_res = app.oneshot(unauthorized_req).await.unwrap();
    assert_eq!(unauthorized_res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_user_settings_isolation_and_credentials() {
    let (app, pool, config) = setup_test_app().await;

    // Create User A
    let user_a = bingesync::api::auth::get_or_create_user(
        &pool,
        "user_a@bingesync.local",
        "User A",
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let token_a = create_jwt(&user_a.id, &user_a.email, &config.jwt_secret).unwrap();

    // Create User B
    let user_b = bingesync::api::auth::get_or_create_user(
        &pool,
        "user_b@bingesync.local",
        "User B",
        None,
        None,
        false,
    )
    .await
    .unwrap();
    let token_b = create_jwt(&user_b.id, &user_b.email, &config.jwt_secret).unwrap();

    // 1. User A saves their personal Radarr, Sonarr, and Tautulli settings
    let update_body = json!({
        "radarr_url": "http://user-a-radarr:7878",
        "radarr_api_key": "user_a_radarr_secret_key",
        "sonarr_url": "http://user-a-sonarr:8989",
        "sonarr_api_key": "user_a_sonarr_secret_key",
        "tautulli_url": "http://user-a-tautulli:8181/api/v2",
        "tautulli_api_key": "user_a_tautulli_secret_key"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(update_body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. User A retrieves settings and verifies secrets are masked
    let req = Request::builder()
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings_a: bingesync::api::user_settings::UserSettingsView = serde_json::from_slice(&body).unwrap();

    assert_eq!(settings_a.radarr_url.as_deref(), Some("http://user-a-radarr:7878"));
    assert!(settings_a.radarr_api_key_configured);
    assert!(settings_a.radarr_api_key_masked.as_deref().unwrap().starts_with("us"));
    assert!(settings_a.radarr_api_key_masked.as_deref().unwrap().contains('•'));
    assert_eq!(settings_a.radarr_api_key.as_deref(), Some("user_a_radarr_secret_key"));

    assert_eq!(settings_a.sonarr_url.as_deref(), Some("http://user-a-sonarr:8989"));
    assert!(settings_a.sonarr_api_key_configured);

    assert_eq!(settings_a.tautulli_url.as_deref(), Some("http://user-a-tautulli:8181/api/v2"));
    assert!(settings_a.tautulli_api_key_configured);

    // 3. User B retrieves settings - should be completely empty (Multi-user isolation!)
    let req = Request::builder()
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings_b: bingesync::api::user_settings::UserSettingsView = serde_json::from_slice(&body).unwrap();

    assert!(settings_b.radarr_url.is_none());
    assert!(!settings_b.radarr_api_key_configured);
    assert!(settings_b.sonarr_url.is_none());
    assert!(!settings_b.sonarr_api_key_configured);
    assert!(settings_b.tautulli_url.is_none());
    assert!(!settings_b.tautulli_api_key_configured);

    // 4. Integrations status endpoint reflects User A's settings
    let req = Request::builder()
        .uri("/api/v1/integrations/status")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let status_a: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status_a["radarr_configured"], true);
    assert_eq!(status_a["sonarr_configured"], true);

    // User B's integrations status reflects that User B has not configured them
    let req = Request::builder()
        .uri("/api/v1/integrations/status")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let status_b: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status_b["radarr_configured"], false);
    assert_eq!(status_b["sonarr_configured"], false);
}
