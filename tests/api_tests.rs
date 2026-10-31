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
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
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
    let user1 = bingesync::api::auth::get_or_create_user_with_sid(
        &pool,
        "alice@bingesync.local",
        "Alice",
        None,
        None,
        None,
        Some("alice_plex"),
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
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
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
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
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
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
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
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
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
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
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
async fn test_movie_tvdb_metadata_fallback_and_refresh() {
    let pool = init_db("sqlite::memory:").await.unwrap();

    // 1. Mock TVDB v4 API server with strict year mismatch on first attempt
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
                let year = params.get("year").map(|s| s.as_str());

                // If year is 2025, return empty data (simulating exact year mismatch between release dates)
                if query.eq_ignore_ascii_case("Obsession") && year == Some("2025") {
                    axum::Json(serde_json::json!({ "status": "success", "data": [] }))
                } else if query.eq_ignore_ascii_case("Obsession") {
                    // Fallback search without year succeeds with TVDB ID prefix format
                    axum::Json(serde_json::json!({
                        "status": "success",
                        "data": [
                            {
                                "objectID": "movie-369536",
                                "id": "movie-369536",
                                "name": "Obsession",
                                "year": "2026",
                                "primary_type": "movie",
                                "overviews": { "eng": "A suspense thriller about dangerous obsession." },
                                "image_url": "https://artworks.thetvdb.com/banners/movies/369536/posters/1.jpg"
                            }
                        ]
                    }))
                } else {
                    axum::Json(serde_json::json!({ "status": "success", "data": [] }))
                }
            }),
        )
        .route(
            "/movies/{id}/extended",
            axum::routing::get(|axum::extract::Path(id): axum::extract::Path<String>| async move {
                assert_eq!(id, "369536", "Extended endpoint must receive numeric ID without movie- prefix");
                axum::Json(serde_json::json!({
                    "status": "success",
                    "data": {
                        "id": 369536,
                        "name": "Obsession",
                        "overview": "A suspense thriller about dangerous obsession.",
                        "genres": [
                            { "id": 1, "name": "Thriller" },
                            { "id": 2, "name": "Drama" }
                        ],
                        "characters": [
                            { "personName": "Richard Armitage", "peopleType": "Actor" },
                            { "personName": "Charlie Murphy", "peopleType": "Actor" },
                            { "personName": "Glenn Leyburn", "peopleType": "Director" }
                        ],
                        "image": "https://artworks.thetvdb.com/banners/movies/369536/posters/1.jpg"
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
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
    };

    // 2. Create test user and insert movie with title "Obsession" and year 2025
    let user_id = "test_user_movie_tvdb";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_id)
        .bind("movie_tvdb@bingesync.local")
        .bind("Movie TVDB Tester")
        .bind("token_movie_123")
        .execute(&pool)
        .await
        .unwrap();

    let scrobble_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO scrobbles (
            id, user_id, title, media_type, year, overview, genres, actors, directors,
            duration_ms, user_rating, watch_status, watched_at, created_at
        ) VALUES (
            ?, ?, ?, 'movie', 2025, NULL, NULL, NULL, NULL,
            7200000, 8, 'completed', '2025-10-01T20:00:00Z', '2025-10-01T20:00:00Z'
        )
        "#
    )
    .bind(&scrobble_id)
    .bind(user_id)
    .bind("Obsession")
    .execute(&pool)
    .await
    .unwrap();

    // 3. Run background metadata processor once
    let processed = bingesync::api::tvdb::process_missing_metadata_once(&state).await;
    assert_eq!(processed, 1, "Obsession should be processed via year-fallback");

    // 4. Verify movie metadata enriched
    let scrobble = sqlx::query_as::<_, Scrobble>("SELECT * FROM scrobbles WHERE id = ?")
        .bind(&scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert!(scrobble.overview.is_some());
    assert!(scrobble.overview.unwrap().contains("suspense thriller"));
    assert_eq!(scrobble.genres, Some("Thriller, Drama".to_string()));
    assert!(scrobble.actors.unwrap().contains("Richard Armitage"));
    assert_eq!(scrobble.directors, Some("Glenn Leyburn".to_string()));
    assert_eq!(scrobble.thumb_url, Some("https://artworks.thetvdb.com/banners/movies/369536/posters/1.jpg".to_string()));

    // 5. Test manual refresh metadata endpoint
    let app = bingesync::api::create_api_router(state.clone());
    let jwt = create_jwt(user_id, "movie_tvdb@bingesync.local", &config.jwt_secret).unwrap();
    let refresh_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/refresh-metadata", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", jwt))
        .body(Body::empty())
        .unwrap();

    let refresh_res = app.oneshot(refresh_req).await.unwrap();
    assert_eq!(refresh_res.status(), StatusCode::OK);

    let refresh_body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(refresh_res.into_body(), usize::MAX).await.unwrap()
    ).unwrap();
    assert_eq!(refresh_body["status"], "refreshed");
    assert!(refresh_body["scrobble"]["overview"].as_str().unwrap().contains("suspense thriller"));

    // 6. Test query normalization
    let (clean, yr) = bingesync::api::tvdb::normalize_search_query("Obsession (1976)");
    assert_eq!(clean, "Obsession");
    assert_eq!(yr, Some(1976));

    let (clean2, yr2) = bingesync::api::tvdb::normalize_search_query("Obsession - Director's Cut");
    assert_eq!(clean2, "Obsession");
    assert_eq!(yr2, None);
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
    assert_eq!(tasks.len(), 7);
    let image_task = tasks.iter().find(|t| t["id"] == "image_cache").unwrap();
    let tvdb_task = tasks.iter().find(|t| t["id"] == "tvdb_metadata").unwrap();
    let user_sync_task = tasks.iter().find(|t| t["id"] == "user_sync").unwrap();
    let tautulli_sync_task = tasks.iter().find(|t| t["id"] == "tautulli_sync").unwrap();
    let media_sync_task = tasks.iter().find(|t| t["id"] == "media_sync").unwrap();
    let db_backup_task = tasks.iter().find(|t| t["id"] == "db_backup").unwrap();
    let db_maintenance_task = tasks.iter().find(|t| t["id"] == "db_maintenance").unwrap();
    assert_eq!(media_sync_task["name"], "Watch History Sync (Plex & Jellyfin)");
    assert_eq!(media_sync_task["interval_hours"], 4);
    assert!(media_sync_task["next_run_at"].is_string());
    assert_eq!(tautulli_sync_task["interval_hours"], 1);
    assert_eq!(tautulli_sync_task["schedule"], "Every 1 hour");
    assert!(tautulli_sync_task["next_run_at"].is_string());
    assert_eq!(image_task["schedule"], "Every 4 hours");
    assert_eq!(image_task["interval_hours"], 4);
    assert!(image_task["next_run_at"].is_string());
    assert_eq!(tvdb_task["schedule"], "Every 4 hours");
    assert!(tvdb_task["next_run_at"].is_string());
    assert_eq!(user_sync_task["interval_hours"], 4);
    assert_eq!(user_sync_task["schedule"], "Every 4 hours");
    assert!(user_sync_task["next_run_at"].is_string());
    assert_eq!(db_backup_task["interval_hours"], 24);
    assert_eq!(db_backup_task["schedule"], "Every 24 hours");
    assert_eq!(db_maintenance_task["interval_hours"], 168);
    assert_eq!(db_maintenance_task["schedule"], "Every 168 hours (Weekly)");
    assert!(db_maintenance_task["next_run_at"].is_string());

    // 7. Test configuring background task schedule
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/image_cache/schedule")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&serde_json::json!({
            "interval_hours": 6
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let update_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(update_json["interval_hours"], 6);
    assert_eq!(update_json["schedule"], "Every 6 hours");
    assert!(update_json["next_run_at"].is_string());

    // Verify GET /api/v1/admin/tasks reflects updated schedule
    let req = Request::builder()
        .uri("/api/v1/admin/tasks")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let tasks_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let updated_image_task = tasks_json["tasks"].as_array().unwrap().iter().find(|t| t["id"] == "image_cache").unwrap();
    assert_eq!(updated_image_task["interval_hours"], 6);
    assert_eq!(updated_image_task["schedule"], "Every 6 hours");

    // 8. Test triggering background task run
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/image_cache/run")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 9. Test triggering user_sync background task run
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/user_sync/run")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 9. Test logs search filtering (for viewing logs specific to a background job)
    let req = Request::builder()
        .uri("/api/v1/admin/logs?search=tasks")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let filtered_logs: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(filtered_logs["entries"].as_array().unwrap().len() > 0);

    let req = Request::builder()
        .uri("/api/v1/admin/logs?search=completely_non_existent_query_xyz")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let empty_logs: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(empty_logs["entries"].as_array().unwrap().len(), 0);

    // Test triggering db_maintenance
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/db_maintenance/run")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let trigger_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(trigger_json["status"], "triggered");
    assert_eq!(trigger_json["task_id"], "db_maintenance");

    // 10. Verify task Last Completed persistence / fallback across fresh tracker
    let fake_tracker = bingesync::api::jobs::BackgroundTaskTracker::new();
    sqlx::query("INSERT INTO system_settings (key, value, updated_at) VALUES ('task_last_success:tvdb_metadata', '2026-10-30T10:00:00Z', datetime('now'))")
        .execute(&pool)
        .await
        .unwrap();
    let state_with_fake = bingesync::api::AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        metadata_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        user_sync_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        log_buffer: std::sync::Arc::new(bingesync::api::logs::LogBuffer::new(100)),
        task_tracker: std::sync::Arc::new(fake_tracker),
        db_backup_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
    };
    let restored_tasks = state_with_fake.task_tracker.get_tasks_summary(&state_with_fake, true).await;
    let restored_tvdb = restored_tasks.iter().find(|t| t.id == "tvdb_metadata").unwrap();
    assert_eq!(restored_tvdb.last_success_at.as_deref(), Some("2026-10-30T10:00:00Z"));
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
async fn test_stats_filters_and_extended_metrics() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_stats_ext";
    let webhook_token = "stats_ext_token_123";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_id)
        .bind("stats_ext@test.local")
        .bind("Stats Ext Tester")
        .bind(webhook_token)
        .execute(&pool)
        .await
        .unwrap();

    let token = bingesync::auth::jwt::create_jwt(user_id, "stats_ext@test.local", &config.jwt_secret).unwrap();

    let now_utc = chrono::Utc::now();
    let d0 = now_utc.to_rfc3339();
    let d1 = (now_utc - chrono::Duration::days(1)).to_rfc3339();
    let d2 = (now_utc - chrono::Duration::days(2)).to_rfc3339();
    let d10 = (now_utc - chrono::Duration::days(10)).to_rfc3339();

    // Insert scrobble 1: Movie, watched today, rated 10, Christopher Nolan, Leonardo DiCaprio, Sci-Fi
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, media_type, title, duration_ms, view_offset_ms, watched_at, created_at, user_rating, directors, actors, genres) \
         VALUES ('s1', ?, 'movie', 'Inception', 7200000, 7200000, ?, ?, 10, 'Christopher Nolan', 'Leonardo DiCaprio, Joseph Gordon-Levitt', 'Sci-Fi, Action')"
    )
    .bind(user_id).bind(&d0).bind(&d0)
    .execute(&pool).await.unwrap();

    // Insert scrobble 2: Movie, watched yesterday, rated 8, Christopher Nolan, Matthew McConaughey, Sci-Fi
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, media_type, title, duration_ms, view_offset_ms, watched_at, created_at, user_rating, directors, actors, genres) \
         VALUES ('s2', ?, 'movie', 'Interstellar', 10800000, 10800000, ?, ?, 8, 'Christopher Nolan', 'Matthew McConaughey, Anne Hathaway', 'Sci-Fi, Drama')"
    )
    .bind(user_id).bind(&d1).bind(&d1)
    .execute(&pool).await.unwrap();

    // Insert scrobble 3: Episode, watched 2 days ago, unrated, Breaking Bad
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, media_type, title, grandparent_title, duration_ms, view_offset_ms, watched_at, created_at, directors, actors, genres) \
         VALUES ('s3', ?, 'episode', 'Ozymandias', 'Breaking Bad', 3600000, 3600000, ?, ?, 'Rian Johnson', 'Bryan Cranston, Aaron Paul', 'Drama, Crime')"
    )
    .bind(user_id).bind(&d2).bind(&d2)
    .execute(&pool).await.unwrap();

    // Insert scrobble 4: Episode, watched 10 days ago (outside 7d range), Breaking Bad
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, media_type, title, grandparent_title, duration_ms, view_offset_ms, watched_at, created_at, directors, actors, genres) \
         VALUES ('s4', ?, 'episode', 'Pilot', 'Breaking Bad', 3600000, 3600000, ?, ?, 'Vince Gilligan', 'Bryan Cranston', 'Drama, Crime')"
    )
    .bind(user_id).bind(&d10).bind(&d10)
    .execute(&pool).await.unwrap();

    // Test 1: All stats
    let req = Request::builder()
        .uri("/api/v1/stats")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stats: StatsSummary = serde_json::from_slice(&body).unwrap();

    assert_eq!(stats.total_scrobbles, 4);
    assert_eq!(stats.movies_watched, 2);
    assert_eq!(stats.unique_movies, 2);
    assert_eq!(stats.episodes_watched, 2);
    assert_eq!(stats.unique_shows, 1);
    assert_eq!(stats.total_rated, 2);
    assert_eq!(stats.average_rating, Some(9.0)); // (10 + 8) / 2
    assert_eq!(stats.completion_rate_percent, 100.0);
    assert_eq!(stats.day_of_week_activity.len(), 7);
    assert_eq!(stats.time_of_day_activity.len(), 4);
    assert!(!stats.top_directors.is_empty());
    assert_eq!(stats.top_directors[0].name, "Christopher Nolan");
    assert_eq!(stats.top_directors[0].count, 2);

    // Test 2: Filter by media_type=movie
    let req = Request::builder()
        .uri("/api/v1/stats?media_type=movie")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stats_movies: StatsSummary = serde_json::from_slice(&body).unwrap();

    assert_eq!(stats_movies.total_scrobbles, 2);
    assert_eq!(stats_movies.movies_watched, 2);
    assert_eq!(stats_movies.episodes_watched, 0);
    assert_eq!(stats_movies.top_shows.len(), 0);
    assert_eq!(stats_movies.top_movies.len(), 2);

    // Test 3: Filter by time_range=7d (excludes the 10-day-old episode)
    let req = Request::builder()
        .uri("/api/v1/stats?time_range=7d")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stats_7d: StatsSummary = serde_json::from_slice(&body).unwrap();

    assert_eq!(stats_7d.total_scrobbles, 3);
    assert_eq!(stats_7d.movies_watched, 2);
    assert_eq!(stats_7d.episodes_watched, 1);
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

    // Verify source is "plex" and imported_from is "tautulli"
    let (source, imported_from, source_user): (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT source, imported_from, source_user FROM scrobbles WHERE user_id = ? AND title = 'Good News About Hell'"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(source.as_deref(), Some("plex"));
    assert_eq!(imported_from.as_deref(), Some("tautulli"));
    assert_eq!(source_user.as_deref(), Some("daryl_tautulli"));
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

    let user = bingesync::api::auth::get_or_create_user_with_sid(
        &pool,
        "southpark_fan@bingesync.local",
        "South Park Fan",
        None,
        None,
        None,
        Some("sp_fan"),
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
    assert!(settings_a.radarr_api_key.is_none());

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

#[tokio::test]
async fn test_plex_webhook_username_filtering() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create user with preferred_username = "tester_plex"
    let user = bingesync::api::auth::get_or_create_user_with_sid(
        &pool,
        "tester@bingesync.local",
        "Tester",
        None,
        None,
        None,
        Some("tester_plex"),
        false,
    )
    .await
    .unwrap();

    let token = create_jwt(&user.id, &user.email, &config.jwt_secret).unwrap();

    // 2. Incoming webhook from another user on the Plex server ("other_plex_user")
    let other_user_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "Account": { "title": "other_plex_user" },
        "Metadata": {
            "type": "movie",
            "title": "Unwanted Movie",
            "year": 2021,
            "duration": 6000000
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(other_user_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp_json["status"], "ignored");
    assert_eq!(resp_json["reason"], "username_mismatch");
    assert_eq!(resp_json["account"], "other_plex_user");

    // Verify no scrobbles exist in database
    let scrobble_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scrobble_count, 0);

    // Also verify playback session is ignored for other user
    let other_play_payload = json!({
        "event": "media.play",
        "user": true,
        "Account": { "title": "other_plex_user" },
        "Metadata": {
            "type": "movie",
            "title": "Playing Elsewhere",
            "year": 2023,
            "duration": 5000000
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(other_play_payload.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM active_sessions WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(session_count, 0);

    // 3. Webhook from matching user ("tester_plex")
    let valid_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "Account": { "title": "tester_plex" },
        "Metadata": {
            "type": "movie",
            "title": "Wanted Movie 1",
            "year": 2022,
            "duration": 7200000
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(valid_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let scrobble_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scrobble_count, 1);

    // 4. Update user settings with custom comma-separated plex_username filter: "custom_plex, living_room"
    let update_req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "plex_username": "custom_plex, living_room"
        }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(update_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Now "tester_plex" should be ignored because custom filter overrides it
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(valid_payload.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp_json["status"], "ignored");

    // But "Custom_Plex" (case insensitive) and "living_room" should be allowed
    let custom_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "Account": { "title": "Custom_Plex" },
        "Metadata": {
            "type": "movie",
            "title": "Wanted Movie 2",
            "year": 2023,
            "duration": 6000000
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(custom_payload.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let living_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "Account": { "title": "living_room" },
        "Metadata": {
            "type": "movie",
            "title": "Wanted Movie 3",
            "year": 2024,
            "duration": 5400000
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(living_payload.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let final_scrobble_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(final_scrobble_count, 3);

    // 5. Test Plex user ID enumeration and database caching
    // Insert mapping into plex_user_cache for an ID that corresponds to "custom_plex"
    sqlx::query("INSERT INTO plex_user_cache (plex_id, username, friendly_name, source) VALUES (?, ?, ?, 'tautulli')")
        .bind("3078822f-d297-4fa3-b587-befd3a5c0965")
        .bind("custom_plex")
        .bind("Custom User")
        .execute(&pool)
        .await
        .unwrap();

    // Webhook with only user ID in Account.id should resolve to "custom_plex" and be accepted
    let id_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "Account": { "id": "3078822f-d297-4fa3-b587-befd3a5c0965" },
        "Metadata": {
            "type": "movie",
            "title": "Moana",
            "year": 2016,
            "duration": 6400000
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(id_payload.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Webhook with unknown ID not matching allowed should be ignored
    let unknown_id_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "Account": { "id": "random-other-user-999" },
        "Metadata": {
            "type": "movie",
            "title": "Unwanted Movie",
            "year": 2020,
            "duration": 5000000
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(unknown_id_payload.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp_json["status"], "ignored");
    assert_eq!(resp_json["reason"], "username_mismatch");

    let post_cache_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(post_cache_count, 4);
}

#[tokio::test]
async fn test_search_all_history_regardless_of_date_and_return_all_results() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_search_all";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_id)
        .bind("search_all@test.local")
        .bind("Search All Tester")
        .bind("token_search_all_123")
        .execute(&pool)
        .await
        .unwrap();

    let token = bingesync::auth::jwt::create_jwt(user_id, "search_all@test.local", &config.jwt_secret).unwrap();

    // Insert Matrix movies in three different years/months
    // 1. The Matrix in 2021-06-15
    sqlx::query("INSERT INTO scrobbles (id, user_id, media_type, title, watched_at, created_at) VALUES ('m1', ?, 'movie', 'The Matrix', '2021-06-15T20:00:00Z', '2021-06-15T20:00:00Z')")
        .bind(user_id).execute(&pool).await.unwrap();
    // 2. The Matrix Reloaded in 2022-09-10
    sqlx::query("INSERT INTO scrobbles (id, user_id, media_type, title, watched_at, created_at) VALUES ('m2', ?, 'movie', 'The Matrix Reloaded', '2022-09-10T20:00:00Z', '2022-09-10T20:00:00Z')")
        .bind(user_id).execute(&pool).await.unwrap();
    // 3. The Matrix Revolutions in 2024-01-20
    sqlx::query("INSERT INTO scrobbles (id, user_id, media_type, title, watched_at, created_at) VALUES ('m3', ?, 'movie', 'The Matrix Revolutions', '2024-01-20T20:00:00Z', '2024-01-20T20:00:00Z')")
        .bind(user_id).execute(&pool).await.unwrap();
    // 4. Non-matching film in 2024-01-21
    sqlx::query("INSERT INTO scrobbles (id, user_id, media_type, title, watched_at, created_at) VALUES ('m4', ?, 'movie', 'Avatar', '2024-01-21T20:00:00Z', '2024-01-21T20:00:00Z')")
        .bind(user_id).execute(&pool).await.unwrap();

    // 1. Regular search with date filter for 2024-01 only returns 1 result
    let req = Request::builder()
        .uri("/api/v1/timeline?search=Matrix&date=2024-01")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let days: Vec<bingesync::models::scrobble::TimelineDay> = serde_json::from_slice(&body).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].items.len(), 1);
    assert_eq!(days[0].items[0].title, "The Matrix Revolutions");

    // 2. Search across ALL history regardless of date (all=true & all_dates=true, even with date=2024-01 provided)
    let req = Request::builder()
        .uri("/api/v1/timeline?search=Matrix&date=2024-01&all=true&all_dates=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let all_days: Vec<bingesync::models::scrobble::TimelineDay> = serde_json::from_slice(&body).unwrap();
    assert_eq!(all_days.len(), 3);
    let all_titles: Vec<String> = all_days.into_iter().flat_map(|d| d.items).map(|i| i.title).collect();
    assert_eq!(all_titles, vec!["The Matrix Revolutions", "The Matrix Reloaded", "The Matrix"]);

    // 3. History endpoint with all=true returns all results without page clamping
    let req = Request::builder()
        .uri("/api/v1/history?search=Matrix&all=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let history: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(history["total"], 3);
    assert_eq!(history["items"].as_array().unwrap().len(), 3);
    assert_eq!(history["total_pages"], 1);
}

#[tokio::test]
async fn test_jellyfin_webhook_scrobble_movie_episode_session_and_filtering() {
    let (app, _pool, _config) = setup_test_app().await;

    // 1. Dev login to create a user and get webhook_token
    let login_body = serde_json::json!({
        "email": "jellyfin_user@example.com",
        "display_name": "Jellyfin Tester",
        "role": "user"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/dev-login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&login_body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = val["token"].as_str().unwrap().to_string();
    let webhook_token = val["user"]["webhook_token"].as_str().unwrap().to_string();

    // 2. Scrobble a movie via Jellyfin Webhook (POST /api/v1/webhooks/jellyfin/{token})
    let movie_payload = serde_json::json!({
        "NotificationType": "PlaybackStop",
        "ItemType": "Movie",
        "Name": "Interstellar",
        "Year": 2014,
        "Overview": "A team of explorers travel through a wormhole in space in an attempt to ensure humanity's survival.",
        "RunTimeTicks": 101400000000_i64,
        "PlaybackPositionTicks": 101400000000_i64,
        "PlayedToCompletion": true,
        "Genres": ["Adventure", "Drama", "Sci-Fi"],
        "Provider_imdb": "tt0816692",
        "ItemId": "jelly-movie-12345"
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin/{}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&movie_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 3. Scrobble a TV episode via Jellyfin Webhook query param (POST /api/v1/webhooks/jellyfin?token=...)
    let episode_payload = serde_json::json!({
        "NotificationType": "PlaybackStop",
        "ItemType": "Episode",
        "Name": "Ozymandias",
        "SeriesName": "Breaking Bad",
        "SeasonNumber": 5,
        "EpisodeNumber": 14,
        "Year": 2013,
        "RunTimeTicks": 28200000000_i64,
        "PlaybackPositionTicks": 28200000000_i64,
        "PlayedToCompletion": true,
        "Genres": ["Crime", "Drama", "Thriller"],
        "ProviderIds": {
            "Imdb": "tt2301451"
        },
        "ItemId": "jelly-ep-514"
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin?token={}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&episode_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 4. Verify scrobbles exist in history
    let req = Request::builder()
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let history: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(history["total"], 2);

    let items = history["items"].as_array().unwrap();
    let titles: Vec<&str> = items.iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Interstellar"));
    assert!(titles.contains(&"Ozymandias"));

    // 5. Test Jellyfin playback start & active session
    let start_payload = serde_json::json!({
        "NotificationType": "PlaybackStart",
        "ItemType": "Movie",
        "Name": "Dune: Part Two",
        "Year": 2024,
        "RunTimeTicks": 99600000000_i64,
        "PlaybackPositionTicks": 6000000000_i64, // ~10 minutes in
        "PlayedToCompletion": false,
        "DeviceName": "Living Room TV",
        "Client": "Jellyfin for Android TV",
        "ItemId": "jelly-dune-2"
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin/{}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&start_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Check active session via now-playing
    let req = Request::builder()
        .uri("/api/v1/now-playing")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let now_playing: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(now_playing["now_playing"]["title"], "Dune: Part Two");
    assert_eq!(now_playing["now_playing"]["state"], "playing");

    // 6. Test Jellyfin account username filtering
    // Set user setting: jellyfin_username = "alice"
    let settings_payload = serde_json::json!({
        "jellyfin_username": "alice"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&settings_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Webhook from "bob" should be ignored with 200 OK
    let bob_payload = serde_json::json!({
        "NotificationType": "PlaybackStop",
        "NotificationUsername": "bob",
        "ItemType": "Movie",
        "Name": "Gladiator",
        "PlayedToCompletion": true
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin/{}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&bob_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let ignored_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ignored_resp["status"], "ignored");
    assert_eq!(ignored_resp["reason"], "username_mismatch");

    // Webhook from "alice" should be accepted and scrobbled
    let alice_payload = serde_json::json!({
        "NotificationType": "PlaybackStop",
        "NotificationUsername": "alice",
        "ItemType": "Movie",
        "Name": "Gladiator",
        "Year": 2000,
        "RunTimeTicks": 93000000000_i64,
        "PlaybackPositionTicks": 93000000000_i64,
        "PlayedToCompletion": true
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin/{}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&alice_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_user_cache_background_sync_and_jellyfin_id_resolution() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "jellyfin_cache_user";
    let webhook_token = "jellyfin_sync_token_xyz";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_id)
        .bind("jellyfin_sync@test.local")
        .bind("Jellyfin User")
        .bind(webhook_token)
        .execute(&pool)
        .await
        .unwrap();

    // User configured jellyfin_username = "sarah"
    sqlx::query("INSERT INTO user_settings (user_id, key, value) VALUES (?, 'jellyfin_username', 'sarah')")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    // Pre-seed plex_user_cache with Jellyfin mapping: id "999888-uuid" -> "sarah"
    sqlx::query("INSERT INTO plex_user_cache (plex_id, username, friendly_name, source) VALUES ('999888-uuid', 'sarah', 'sarah', 'jellyfin')")
        .execute(&pool)
        .await
        .unwrap();

    // Webhook arrives with UserId = "999888-uuid" without NotificationUsername
    let id_only_payload = serde_json::json!({
        "NotificationType": "PlaybackStop",
        "UserId": "999888-uuid",
        "ItemType": "Movie",
        "Name": "Interstellar",
        "Year": 2014,
        "RunTimeTicks": 101400000000_i64,
        "PlaybackPositionTicks": 101400000000_i64,
        "PlayedToCompletion": true
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin/{}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&id_only_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Verify scrobble created
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND title = 'Interstellar'")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // Test running sync_user_cache_once
    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        metadata_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        user_sync_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        log_buffer: std::sync::Arc::new(bingesync::api::logs::LogBuffer::new(100)),
        task_tracker: std::sync::Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
    };

    let processed = bingesync::api::user_sync::sync_user_cache_once(&state).await;
    // Without active external servers, it safely returns without panicking
    assert_eq!(processed, 0);
}

#[tokio::test]
async fn test_specific_director_and_cinematographer_fields() {
    let (app, _pool, _config) = setup_test_app().await;

    // 1. Dev login to obtain token
    let login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/dev-login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "email": "cinema_enthusiast@test.local",
                "display_name": "Cinema Enthusiast"
            })
            .to_string(),
        ))
        .unwrap();

    let res = app.clone().oneshot(login_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let token = login_data["token"].as_str().unwrap().to_string();

    // 2. Create a scrobble with specific director and cinematographer
    let create_payload = serde_json::json!({
        "title": "Oppenheimer",
        "media_type": "movie",
        "year": 2023,
        "director": "Christopher Nolan",
        "cinematographer": "Hoyte van Hoytema",
        "actors": "Cillian Murphy, Emily Blunt, Matt Damon",
        "overview": "The story of J. Robert Oppenheimer."
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&create_payload).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(created["director"], "Christopher Nolan");
    assert_eq!(created["cinematographer"], "Hoyte van Hoytema");

    // 3. Query history filtered specifically by director
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?director=Nolan")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["title"], "Oppenheimer");

    // 4. Query history filtered specifically by cinematographer
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?cinematographer=Hoyte")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["cinematographer"], "Hoyte van Hoytema");

    // 5. Query history with person filter matching cinematographer
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?person=Hoytema")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // 6. Non-matching cinematographer query returns 0 items
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?cinematographer=Roger+Deakins")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_force_refresh_all_metadata_option() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create Admin User and Standard User
    let admin_id = "admin_user_force_refresh";
    let admin_jwt = create_jwt(admin_id, "admin@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(admin_id)
        .bind("admin@test.local")
        .bind("Admin")
        .bind("admin_token_force")
        .execute(&pool)
        .await
        .unwrap();

    let standard_id = "standard_user_force_refresh";
    let standard_jwt = create_jwt(standard_id, "user@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(standard_id)
        .bind("user@test.local")
        .bind("Standard User")
        .bind("user_token_force")
        .execute(&pool)
        .await
        .unwrap();

    // 2. Insert scrobbles with metadata_updated_at populated
    for i in 1..=3 {
        sqlx::query(
            r#"
            INSERT INTO scrobbles (
                id, user_id, title, media_type, watched_at, metadata_updated_at, overview, genres
            ) VALUES (?, ?, ?, 'movie', datetime('now'), datetime('now'), 'Some overview', 'Action')
            "#,
        )
        .bind(format!("scrobble_force_{}", i))
        .bind(admin_id)
        .bind(format!("Movie {}", i))
        .execute(&pool)
        .await
        .unwrap();
    }

    // Verify 3 records have metadata_updated_at set
    let count_set: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE metadata_updated_at IS NOT NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_set, 3);

    // 3. Test non-admin user forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/metadata/force-refresh")
        .header(header::AUTHORIZATION, format!("Bearer {}", standard_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 4. Test admin user successfully triggers force refresh
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/metadata/force-refresh")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["status"], "triggered");
    assert_eq!(res["forced"], true);
    assert_eq!(res["records_reset"], 3);

    // Verify all records now have metadata_updated_at IS NULL
    let count_null: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE metadata_updated_at IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_null, 3);

    // 5. Test triggering via admin task runner with ?force=true
    // First set metadata_updated_at again
    sqlx::query("UPDATE scrobbles SET metadata_updated_at = datetime('now')")
        .execute(&pool)
        .await
        .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/tvdb_metadata/run?force=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["status"], "triggered");
    assert_eq!(res["forced"], true);

    let count_null_after_task: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE metadata_updated_at IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_null_after_task, 3);
}

#[tokio::test]
async fn test_fix_incorrect_match_with_tvdb_id() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create two users for isolation testing
    let user_a_id = "user_a_fix_match";
    let token_a = create_jwt(user_a_id, "user_a@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("user_a@test.local")
        .bind("User A")
        .bind("token_a_fix")
        .execute(&pool)
        .await
        .unwrap();

    let user_b_id = "user_b_fix_match";
    let token_b = create_jwt(user_b_id, "user_b@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("user_b@test.local")
        .bind("User B")
        .bind("token_b_fix")
        .execute(&pool)
        .await
        .unwrap();

    // 2. Insert TV episodes for User A
    let scrobble_e1 = "scrobble_bb_s1e1";
    let scrobble_e2 = "scrobble_bb_s1e2";

    sqlx::query(
        r#"
        INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at)
        VALUES (?, ?, 'Pilot', 'episode', 'Breaking Bad', 1, 1, datetime('now', '-2 days'))
        "#,
    )
    .bind(scrobble_e1)
    .bind(user_a_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at)
        VALUES (?, ?, 'Cat in the Bag', 'episode', 'Breaking Bad', 1, 2, datetime('now', '-1 day'))
        "#,
    )
    .bind(scrobble_e2)
    .bind(user_a_id)
    .execute(&pool)
    .await
    .unwrap();

    // 3. User B attempts to fix match on User A's item -> returns 404 Not Found
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/fix-match", scrobble_e1))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "tvdb_id": "81189"
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 4. Empty TVDB ID returns 400 Bad Request
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/fix-match", scrobble_e1))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "tvdb_id": "   "
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 5. User A fixes match with "series-81189" (prefix stripped)
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/fix-match", scrobble_e1))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "tvdb_id": "series-81189"
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["status"], "fixed");
    assert_eq!(res["tvdb_id"], "81189");
    assert_eq!(res["scrobble"]["tvdb_id"], "81189");

    // 6. Verify that sibling episode also had its tvdb_id updated for the same show
    let sibling_tvdb_id: Option<String> = sqlx::query_scalar("SELECT tvdb_id FROM scrobbles WHERE id = ?")
        .bind(scrobble_e2)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sibling_tvdb_id.as_deref(), Some("81189"));

    // 7. Verify GET /api/v1/history returns the tvdb_id field
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let items = list["items"].as_array().unwrap();
    assert!(items.iter().any(|it| it["id"] == scrobble_e1 && it["tvdb_id"] == "81189"));
}

#[tokio::test]
async fn test_force_refresh_posters_and_user_cache() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create Admin and Standard User
    let admin_id = "admin_force_worker_test";
    let admin_jwt = create_jwt(admin_id, "admin@worker.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(admin_id)
        .bind("admin@worker.local")
        .bind("Admin Worker")
        .bind("admin_token_worker")
        .execute(&pool)
        .await
        .unwrap();

    let standard_id = "user_force_worker_test";
    let standard_jwt = create_jwt(standard_id, "user@worker.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(standard_id)
        .bind("user@worker.local")
        .bind("User Worker")
        .bind("user_token_worker")
        .execute(&pool)
        .await
        .unwrap();

    // 2. Insert scrobbles with thumb_url set
    for i in 1..=2 {
        sqlx::query(
            "INSERT INTO scrobbles (id, user_id, title, media_type, watched_at, thumb_url) VALUES (?, ?, ?, 'movie', datetime('now'), ?)"
        )
        .bind(format!("scrobble_poster_{}", i))
        .bind(admin_id)
        .bind(format!("Movie {}", i))
        .bind(format!("/images/cached_{}.jpg", i))
        .execute(&pool)
        .await
        .unwrap();
    }

    let count_posters: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE thumb_url IS NOT NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_posters, 2);

    // 3. Insert user cache rows
    for i in 1..=2 {
        sqlx::query(
            "INSERT INTO plex_user_cache (plex_id, username, friendly_name, source) VALUES (?, ?, ?, 'plex')"
        )
        .bind(format!("plex_id_{}", i))
        .bind(format!("plex_user_{}", i))
        .bind(format!("PlexUser{}", i))
        .execute(&pool)
        .await
        .unwrap();
    }

    let count_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plex_user_cache")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_users, 2);

    // 4. Test non-admin permissions for images/force-refresh
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/images/force-refresh")
        .header(header::AUTHORIZATION, format!("Bearer {}", standard_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 5. Test non-admin permissions for users-cache/force-refresh
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/users-cache/force-refresh")
        .header(header::AUTHORIZATION, format!("Bearer {}", standard_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 6. Admin triggers force refresh posters via /admin/images/force-refresh
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/images/force-refresh")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["status"], "triggered");
    assert_eq!(res["forced"], true);
    assert_eq!(res["records_reset"], 2);

    let count_posters_null: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE thumb_url IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_posters_null, 2);

    // 7. Admin triggers force refresh posters via /admin/tasks/image_cache/run?force=true
    sqlx::query("UPDATE scrobbles SET thumb_url = '/images/cached.jpg'")
        .execute(&pool)
        .await
        .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/image_cache/run?force=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["status"], "triggered");
    assert_eq!(res["forced"], true);

    let count_posters_null2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE thumb_url IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_posters_null2, 2);

    // 8. Admin triggers force refresh user cache via /admin/users-cache/force-refresh
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/users-cache/force-refresh")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["status"], "triggered");
    assert_eq!(res["forced"], true);
    assert_eq!(res["records_reset"], 2);

    let count_users_cleared: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plex_user_cache")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_users_cleared, 0);

    // 9. Re-insert user and trigger via /admin/tasks/user_sync/run?force=true
    sqlx::query("INSERT INTO plex_user_cache (plex_id, username, friendly_name, source) VALUES ('p1', 'u1', 'UserOne', 'tautulli')")
        .execute(&pool)
        .await
        .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/user_sync/run?force=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["status"], "triggered");
    assert_eq!(res["forced"], true);

    let count_users_cleared2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plex_user_cache")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_users_cleared2, 0);
}

#[tokio::test]
async fn test_library_cleanup_endpoint() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Setup mock Radarr server
    let mock_radarr_router = axum::Router::new().route(
        "/api/v3/movie",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!([
                {
                    "id": 101,
                    "title": "Inception",
                    "year": 2010,
                    "sizeOnDisk": 10737418240i64,
                    "hasFile": true,
                    "monitored": true,
                    "images": [
                        { "coverType": "poster", "remoteUrl": "https://image.tmdb.org/t/p/w500/inception.jpg" }
                    ]
                },
                {
                    "id": 102,
                    "title": "Unwatched Movie",
                    "year": 2022,
                    "sizeOnDisk": 5368709120i64,
                    "hasFile": true,
                    "monitored": true,
                    "images": []
                }
            ]))
        }),
    );
    let radarr_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let radarr_addr = radarr_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(radarr_listener, mock_radarr_router).await.unwrap();
    });

    // 2. Setup mock Sonarr server
    let mock_sonarr_router = axum::Router::new().route(
        "/api/v3/series",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!([
                {
                    "id": 201,
                    "title": "Breaking Bad",
                    "year": 2008,
                    "monitored": true,
                    "statistics": {
                        "sizeOnDisk": 21474836480i64,
                        "episodeFileCount": 62,
                        "totalEpisodeCount": 62
                    },
                    "images": [
                        { "coverType": "poster", "remoteUrl": "https://image.tmdb.org/t/p/w500/bb.jpg" }
                    ],
                    "seasons": [
                        {
                            "seasonNumber": 1,
                            "monitored": true,
                            "statistics": { "episodeFileCount": 7 }
                        },
                        {
                            "seasonNumber": 2,
                            "monitored": true,
                            "statistics": { "episodeFileCount": 13 }
                        }
                    ]
                },
                {
                    "id": 202,
                    "title": "Unwatched Series",
                    "year": 2023,
                    "monitored": true,
                    "statistics": {
                        "sizeOnDisk": 10737418240i64,
                        "episodeFileCount": 10,
                        "totalEpisodeCount": 10
                    },
                    "images": [],
                    "seasons": []
                }
            ]))
        }),
    );
    let sonarr_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sonarr_addr = sonarr_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(sonarr_listener, mock_sonarr_router).await.unwrap();
    });

    // 3. Create User A and User B
    let user_a_id = "user_a_cleanup";
    let token_a = create_jwt(user_a_id, "user_a_cleanup@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("user_a_cleanup@test.local")
        .bind("User A Cleanup")
        .bind("token_cleanup_a")
        .execute(&pool)
        .await
        .unwrap();

    let user_b_id = "user_b_cleanup";
    let token_b = create_jwt(user_b_id, "user_b_cleanup@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("user_b_cleanup@test.local")
        .bind("User B Cleanup")
        .bind("token_cleanup_b")
        .execute(&pool)
        .await
        .unwrap();

    // 4. Configure Radarr and Sonarr for User A
    bingesync::api::user_settings::set_user_setting(&pool, user_a_id, "radarr_url", &format!("http://{}", radarr_addr)).await.unwrap();
    bingesync::api::user_settings::set_user_setting(&pool, user_a_id, "radarr_api_key", "mock_radarr_key").await.unwrap();
    bingesync::api::user_settings::set_user_setting(&pool, user_a_id, "sonarr_url", &format!("http://{}", sonarr_addr)).await.unwrap();
    bingesync::api::user_settings::set_user_setting(&pool, user_a_id, "sonarr_api_key", "mock_sonarr_key").await.unwrap();

    // 5. Insert scrobbles for User A (watched Inception and Breaking Bad S01)
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, year, watched_at) VALUES ('m_inc', ?, 'Inception', 'movie', 2010, datetime('now', '-3 days'))"
    )
    .bind(user_a_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at) VALUES ('ep_bb1', ?, 'Pilot', 'episode', 'Breaking Bad', 1, 1, datetime('now', '-1 day'))"
    )
    .bind(user_a_id)
    .execute(&pool)
    .await
    .unwrap();

    // 6. User B calls cleanup endpoint -> services not configured for User B
    let req_b = Request::builder()
        .method("GET")
        .uri("/api/v1/integrations/cleanup")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();
    let resp_b = app.clone().oneshot(req_b).await.unwrap();
    assert_eq!(resp_b.status(), StatusCode::OK);
    let body_bytes_b = axum::body::to_bytes(resp_b.into_body(), usize::MAX).await.unwrap();
    let json_b: serde_json::Value = serde_json::from_slice(&body_bytes_b).unwrap();
    assert_eq!(json_b["radarr_configured"], false);
    assert_eq!(json_b["sonarr_configured"], false);
    assert_eq!(json_b["total_items"], 0);
    assert_eq!(json_b["items"].as_array().unwrap().len(), 0);

    // 7. User A calls cleanup endpoint -> finds Inception and Breaking Bad
    let req_a = Request::builder()
        .method("GET")
        .uri("/api/v1/integrations/cleanup")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();
    let resp_a = app.clone().oneshot(req_a).await.unwrap();
    assert_eq!(resp_a.status(), StatusCode::OK);
    let body_bytes_a = axum::body::to_bytes(resp_a.into_body(), usize::MAX).await.unwrap();
    let json_a: serde_json::Value = serde_json::from_slice(&body_bytes_a).unwrap();

    assert_eq!(json_a["radarr_configured"], true);
    assert_eq!(json_a["sonarr_configured"], true);
    assert_eq!(json_a["total_items"], 2);
    assert_eq!(json_a["total_size_bytes"], 32212254720i64);
    assert_eq!(json_a["total_size_formatted"], "30.0 GB");

    let items = json_a["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // Most recently watched first (Breaking Bad was -1 day, Inception was -3 days)
    let first = &items[0];
    assert_eq!(first["service"], "sonarr");
    assert_eq!(first["title"], "Breaking Bad");
    assert_eq!(first["media_type"], "series");
    assert_eq!(first["service_id"], 201);
    assert_eq!(first["size_formatted"], "20.0 GB");
    assert_eq!(first["episode_file_count"], 62);
    let seasons = first["seasons"].as_array().unwrap();
    assert_eq!(seasons.len(), 2);
    let s1 = seasons.iter().find(|s| s["season_number"] == 1).unwrap();
    assert_eq!(s1["watched"], true);
    let s2 = seasons.iter().find(|s| s["season_number"] == 2).unwrap();
    assert_eq!(s2["watched"], false);

    let second = &items[1];
    assert_eq!(second["service"], "radarr");
    assert_eq!(second["title"], "Inception");
    assert_eq!(second["media_type"], "movie");
    assert_eq!(second["service_id"], 101);
    assert_eq!(second["size_formatted"], "10.0 GB");
    assert_eq!(second["has_file"], true);
}

#[tokio::test]
async fn test_delete_by_media_endpoint() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create User A and User B
    let user_a_id = "user_a_del_media";
    let token_a = create_jwt(user_a_id, "user_a_del@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("user_a_del@test.local")
        .bind("User A Del")
        .bind("token_del_a")
        .execute(&pool)
        .await
        .unwrap();

    let user_b_id = "user_b_del_media";
    let _token_b = create_jwt(user_b_id, "user_b_del@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("user_b_del@test.local")
        .bind("User B Del")
        .bind("token_del_b")
        .execute(&pool)
        .await
        .unwrap();

    // 2. Insert movie and episode scrobbles for both users
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, year, watched_at) VALUES ('m_a', ?, 'The Matrix', 'movie', 1999, datetime('now'))")
        .bind(user_a_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, year, watched_at) VALUES ('m_b', ?, 'The Matrix', 'movie', 1999, datetime('now'))")
        .bind(user_b_id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at) VALUES ('ep_a1', ?, 'Pilot', 'episode', 'Breaking Bad', 1, 1, datetime('now'))")
        .bind(user_a_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at) VALUES ('ep_b1', ?, 'Pilot', 'episode', 'Breaking Bad', 1, 1, datetime('now'))")
        .bind(user_b_id)
        .execute(&pool)
        .await
        .unwrap();

    // 3. User A deletes movie "The Matrix"
    let req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/history/by-media?media_type=movie&title=The+Matrix&year=1999")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["deleted_count"], 1);

    // Verify User A has no Matrix, but User B still does
    let count_a_matrix: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND title = 'The Matrix'")
        .bind(user_a_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_a_matrix, 0);

    let count_b_matrix: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND title = 'The Matrix'")
        .bind(user_b_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_b_matrix, 1);

    // 4. User A deletes series "Breaking Bad"
    let req2 = Request::builder()
        .method("DELETE")
        .uri("/api/v1/history/by-media?media_type=series&title=Breaking+Bad")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body_bytes2 = axum::body::to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let res2: serde_json::Value = serde_json::from_slice(&body_bytes2).unwrap();
    assert_eq!(res2["deleted_count"], 1);

    let count_a_bb: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND grandparent_title = 'Breaking Bad'")
        .bind(user_a_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_a_bb, 0);

    let count_b_bb: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND grandparent_title = 'Breaking Bad'")
        .bind(user_b_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_b_bb, 1);
}

#[tokio::test]
async fn test_tmdb_metadata_enrichment_worker() {
    let pool = init_db("sqlite::memory:").await.unwrap();

    // 1. Mock TMDB API server
    let mock_tmdb_router = axum::Router::new()
        .route(
            "/search/tv",
            axum::routing::get(|axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                if params.get("api_key").map(|s| s.as_str()) == Some("test_tmdb_key") {
                    let query = params.get("query").map(|s| s.as_str()).unwrap_or("");
                    if query.contains("Stranger Things") {
                        return (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "page": 1,
                                "results": [
                                    {
                                        "id": 66732,
                                        "name": "Stranger Things",
                                        "first_air_date": "2016-07-15",
                                        "overview": "When a young boy vanishes, a small town uncovers a mystery.",
                                        "poster_path": "/x2LSRK2Cm7MZhjluni1msVJ3wDF.jpg"
                                    }
                                ]
                            })),
                        );
                    }
                }
                (StatusCode::OK, axum::Json(serde_json::json!({ "results": [] })))
            }),
        )
        .route(
            "/tv/{id}",
            axum::routing::get(|axum::extract::Path(id): axum::extract::Path<String>, axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                if params.get("api_key").map(|s| s.as_str()) == Some("test_tmdb_key") && id == "66732" {
                    return (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "id": 66732,
                            "name": "Stranger Things",
                            "overview": "When a young boy vanishes, a small town uncovers a mystery involving secret experiments and terrifying supernatural forces.",
                            "poster_path": "/x2LSRK2Cm7MZhjluni1msVJ3wDF.jpg",
                            "genres": [
                                { "id": 18, "name": "Drama" },
                                { "id": 10765, "name": "Sci-Fi & Fantasy" }
                            ],
                            "created_by": [
                                { "id": 1179782, "name": "The Duffer Brothers" }
                            ],
                            "credits": {
                                "cast": [
                                    { "name": "Millie Bobby Brown", "character": "Eleven" },
                                    { "name": "Winona Ryder", "character": "Joyce Byers" }
                                ],
                                "crew": [
                                    { "name": "The Duffer Brothers", "job": "Director", "department": "Directing" },
                                    { "name": "Tim Ives", "job": "Director of Photography", "department": "Camera" }
                                ]
                            }
                        })),
                    );
                }
                (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({ "error": "Not found" })))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, mock_tmdb_router).await.unwrap();
    });

    let mut config = Config::from_env();
    config.metadata_source = "tmdb".to_string();
    config.tmdb_api_key = Some("test_tmdb_key".to_string());
    config.tmdb_base_url = format!("http://{}", addr);
    let config = Arc::new(config);
    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
    };

    // 2. Insert test user and scrobble for "Stranger Things"
    let user_id = "test_user_tmdb";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token) VALUES (?, ?, ?, ?)")
        .bind(user_id)
        .bind("tmdb_tester@bingesync.local")
        .bind("TMDB Tester")
        .bind("token_tmdb_123")
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
    .bind("Chapter One: The Vanishing of Will Byers")
    .bind("episode")
    .bind("Stranger Things")
    .bind("Season 1")
    .bind(1)
    .bind(1)
    .bind(2016)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(2880000_i64)
    .bind(9)
    .bind("completed")
    .bind("2024-02-01T20:00:00Z")
    .bind("2024-02-01T20:00:00Z")
    .execute(&pool)
    .await
    .unwrap();

    // 3. Run metadata worker with TMDB active
    let processed = bingesync::api::tvdb::process_missing_metadata_once(&state).await;
    assert_eq!(processed, 1);

    // 4. Verify scrobble enriched from TMDB
    let scrobble: Scrobble = sqlx::query_as("SELECT * FROM scrobbles WHERE id = ?")
        .bind(&scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(scrobble.genres.as_deref(), Some("Drama, Sci-Fi & Fantasy"));
    assert_eq!(scrobble.actors.as_deref(), Some("Millie Bobby Brown, Winona Ryder"));
    assert_eq!(scrobble.directors.as_deref(), Some("The Duffer Brothers"));
    assert_eq!(scrobble.cinematographer.as_deref(), Some("Tim Ives"));
    assert!(scrobble.overview.is_some());
    assert!(scrobble.overview.unwrap().contains("young boy vanishes"));
    assert_eq!(
        scrobble.thumb_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w500/x2LSRK2Cm7MZhjluni1msVJ3wDF.jpg")
    );
    assert!(scrobble.metadata_updated_at.is_some());
}

#[tokio::test]
async fn test_admin_settings_metadata_source_and_tmdb() {
    let pool = init_db("sqlite::memory:").await.unwrap();
    let config = Arc::new(Config::from_env());
    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
    };
    let app = bingesync::api::create_api_router(state.clone());

    // Create admin user
    let admin_id = "admin_tmdb_settings_user";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(admin_id)
        .bind("admin_tmdb@bingesync.local")
        .bind("Admin TMDB")
        .bind("token_admin_tmdb")
        .execute(&pool)
        .await
        .unwrap();

    let admin_jwt = create_jwt(admin_id, "admin_tmdb@bingesync.local", &config.jwt_secret).unwrap();

    // 1. Update settings: switch metadata source to TMDB and configure TMDB API key
    let update_body = serde_json::json!({
        "metadata_source": "tmdb",
        "tmdb_api_key": "my_tmdb_secret_key_123",
        "tmdb_base_url": "https://api.themoviedb.org/3"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Fetch admin settings and verify metadata_source and masked tmdb_api_key
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(settings["metadata_source"], "tmdb");
    assert_eq!(settings["tmdb_api_key_configured"], true);
    assert!(settings["tmdb_api_key_masked"].as_str().unwrap().starts_with("my"));
    assert!(settings["tmdb_api_key_masked"].as_str().unwrap().contains('•'));

    // 3. Verify task tracker displays Metadata Enrichment and active provider description
    let tasks_summary = state.task_tracker.get_tasks_summary(&state, false).await;
    let meta_task = tasks_summary.iter().find(|t| t.id == "tvdb_metadata").unwrap();
    assert_eq!(meta_task.name, "Metadata Enrichment");
    assert!(meta_task.description.contains("TMDB") || meta_task.description.contains("The Movie Database"));
}

#[tokio::test]
async fn test_admin_database_backup_and_restore() {
    let test_id = uuid::Uuid::new_v4();
    let db_file = std::env::temp_dir().join(format!("bingesync_test_backup_src_{}.db", test_id));
    let pool = init_db(&format!("sqlite://{}?mode=rwc", db_file.to_string_lossy())).await.unwrap();
    let mut config_val = Config::from_env();
    let temp_backup_dir = std::env::temp_dir().join(format!("bingesync_test_backup_{}", test_id));
    config_val.backup_dir = temp_backup_dir.to_string_lossy().to_string();
    let config = Arc::new(config_val);

    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
    };
    let app = bingesync::api::create_api_router(state.clone());

    // 1. Create admin and regular user
    let admin_id = "backup_admin_user";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(admin_id)
        .bind("backup_admin@bingesync.local")
        .bind("Backup Admin")
        .bind("token_backup_admin")
        .execute(&pool)
        .await
        .unwrap();

    let regular_id = "backup_regular_user";
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(regular_id)
        .bind("regular@bingesync.local")
        .bind("Regular User")
        .bind("token_regular")
        .execute(&pool)
        .await
        .unwrap();

    let admin_jwt = create_jwt(admin_id, "backup_admin@bingesync.local", &config.jwt_secret).unwrap();
    let regular_jwt = create_jwt(regular_id, "regular@bingesync.local", &config.jwt_secret).unwrap();

    // Insert an initial scrobble
    let initial_scrobble_id = "scrobble_snapshot_1";
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, year) VALUES (?, ?, ?, ?, ?)")
        .bind(initial_scrobble_id)
        .bind(admin_id)
        .bind("The Matrix")
        .bind("movie")
        .bind(1999)
        .execute(&pool)
        .await
        .unwrap();

    // 2. Verify non-admin gets 403 Forbidden
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/backups")
        .header(header::AUTHORIZATION, format!("Bearer {}", regular_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 3. Admin lists backups (initially empty)
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/backups")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(list_json["backups"].as_array().unwrap().len(), 0);

    // 4. Admin creates a backup via POST /api/v1/admin/backups/create
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/backups/create")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let create_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let created_filename = create_json["backup"]["filename"].as_str().unwrap().to_string();
    assert!(created_filename.starts_with("bingesync_backup_"));
    assert!(created_filename.ends_with(".db"));
    assert!(create_json["backup"]["size_bytes"].as_u64().unwrap() > 0);

    // 5. Admin lists backups again and verifies the created backup is returned
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/backups")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(list_json["backups"].as_array().unwrap().len(), 1);
    assert_eq!(list_json["backups"][0]["filename"], created_filename);

    // 6. Verify task tracker summary includes Database Backup
    let tasks = state.task_tracker.get_tasks_summary(&state, false).await;
    let backup_task = tasks.iter().find(|t| t.id == "db_backup").unwrap();
    assert_eq!(backup_task.name, "Database Backup");
    assert_eq!(backup_task.interval_hours, 24);
    assert_eq!(backup_task.schedule, "Every 24 hours");
    assert_eq!(backup_task.items_pending, 0);
    assert_eq!(backup_task.items_processed, 1);

    // 7. Mutate current database: add a second movie and delete the first movie
    let second_scrobble_id = "scrobble_mutated_2";
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, year) VALUES (?, ?, ?, ?, ?)")
        .bind(second_scrobble_id)
        .bind(admin_id)
        .bind("Inception")
        .bind("movie")
        .bind(2010)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM scrobbles WHERE id = ?")
        .bind(initial_scrobble_id)
        .execute(&pool)
        .await
        .unwrap();

    let count_before_restore: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE id = ?")
        .bind(initial_scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_before_restore, 0);

    // 8. Test restore path traversal rejection
    let bad_restore = serde_json::json!({ "filename": "../../../etc/passwd" });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/backups/restore")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&bad_restore).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 9. Restore from the valid backup created in step 4
    let restore_payload = serde_json::json!({ "filename": created_filename });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/backups/restore")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&restore_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let restore_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(restore_json["status"], "restored");

    // 10. Verify database contents were restored to the snapshot
    let count_after_restore: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE id = ?")
        .bind(initial_scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_after_restore, 1);

    let count_second_after_restore: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE id = ?")
        .bind(second_scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_second_after_restore, 0);

    // 11. Test background task trigger endpoint /api/v1/admin/tasks/db_backup/run
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/db_backup/run")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 12. Test deleting a backup file
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/admin/backups/{}", created_filename))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Clean up temp dir and test db
    let _ = std::fs::remove_dir_all(&temp_backup_dir);
    let _ = std::fs::remove_file(&db_file);
}

#[tokio::test]
async fn test_emby_webhook_scrobble_movie_episode_and_filtering() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_emby";
    let _token = create_jwt(user_id, "emby_user@test.local", &config.jwt_secret).unwrap();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, preferred_username, webhook_token, role) VALUES (?, ?, ?, ?, ?, 'user')"
    )
    .bind(user_id)
    .bind("emby_user@test.local")
    .bind("Emby Tester")
    .bind("emby_tester")
    .bind("emby_wh_token_123")
    .execute(&pool)
    .await
    .unwrap();

    // Configure Emby username filter
    sqlx::query("INSERT INTO user_settings (user_id, key, value) VALUES (?, 'emby_username', 'emby_tester')")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    // 1. Send Emby webhook via path: /api/v1/webhooks/emby/emby_wh_token_123
    let emby_movie_payload = json!({
        "Event": "playback.stop",
        "User": {
            "Name": "emby_tester",
            "Id": "emby_user_guid_1"
        },
        "Item": {
            "Type": "Movie",
            "Name": "Interstellar",
            "ProductionYear": 2014,
            "RunTimeTicks": 101400000000i64,
            "ProviderIds": {
                "Tmdb": "157336",
                "Imdb": "tt0816692"
            }
        },
        "PlaybackInfo": {
            "PositionTicks": 95000000000i64
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/webhooks/emby/emby_wh_token_123")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&emby_movie_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res_json["status"], "scrobbled");

    // Verify scrobble in database
    let scrobble_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND title = 'Interstellar'")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scrobble_count, 1);

    // 2. Send Emby webhook via query parameter: /api/v1/webhooks/emby?token=emby_wh_token_123 (TV Episode)
    let emby_episode_payload = json!({
        "Event": "playback.stop",
        "User": {
            "Name": "emby_tester"
        },
        "Item": {
            "Type": "Episode",
            "Name": "Ozymandias",
            "SeriesName": "Breaking Bad",
            "SeasonName": "Season 5",
            "IndexNumber": 14,
            "ParentIndexNumber": 5,
            "ProductionYear": 2013,
            "RunTimeTicks": 28200000000i64
        },
        "PlaybackInfo": {
            "PositionTicks": 28200000000i64
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/webhooks/emby?token=emby_wh_token_123")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&emby_episode_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res_json["status"], "scrobbled");

    let ep_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ? AND grandparent_title = 'Breaking Bad'")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ep_count, 1);

    // 3. Send Emby webhook for mismatched username -> should be ignored
    let emby_mismatched_payload = json!({
        "Event": "playback.stop",
        "User": {
            "Name": "other_family_member"
        },
        "Item": {
            "Type": "Movie",
            "Name": "Avatar",
            "ProductionYear": 2009
        },
        "PlaybackInfo": {
            "PositionTicks": 99999999999i64
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/webhooks/emby/emby_wh_token_123")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&emby_mismatched_payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res_json["status"], "ignored");
    assert_eq!(res_json["reason"], "username_mismatch");
}

#[tokio::test]
async fn test_user_data_export_zip() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_export";
    let token = create_jwt(user_id, "export_user@test.local", &config.jwt_secret).unwrap();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')"
    )
    .bind(user_id)
    .bind("export_user@test.local")
    .bind("Export User")
    .bind("export_token_abc")
    .execute(&pool)
    .await
    .unwrap();

    // Insert scrobbles
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, media_type, title, year, watched_at, user_rating) VALUES ('scrobble_exp_1', ?, 'movie', 'Dune', 2021, '2026-03-01T20:00:00Z', 9)"
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, media_type, title, grandparent_title, season_number, episode_number, year, watched_at, user_rating) VALUES ('scrobble_exp_2', ?, 'episode', 'Pilot', 'Severance', 1, 1, 2022, '2026-03-02T21:00:00Z', 10)"
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    // GET /api/v1/user/export
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/user/export")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get(header::CONTENT_TYPE).unwrap(), "application/zip");

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert!(!body_bytes.is_empty());

    // Validate that the ZIP contains the 4 expected files
    let cursor = std::io::Cursor::new(body_bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("Must be valid zip archive");
    let mut file_names: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let file = zip.by_index(i).unwrap();
        file_names.push(file.name().unwrap().to_string());
    }

    assert!(file_names.contains(&"scrobbles.json".to_string()));
    assert!(file_names.contains(&"scrobbles.csv".to_string()));
    assert!(file_names.contains(&"ratings.json".to_string()));
    assert!(file_names.contains(&"ratings.csv".to_string()));
}

#[tokio::test]
async fn test_weekly_heatmap_matrix_and_wrapped_endpoint() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_heatmap_wrapped";
    let token = create_jwt(user_id, "wrapped@test.local", &config.jwt_secret).unwrap();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')"
    )
    .bind(user_id)
    .bind("wrapped@test.local")
    .bind("Wrapped Tester")
    .bind("wrapped_token_abc")
    .execute(&pool)
    .await
    .unwrap();

    // 2026-01-05 is a Monday (day_of_week in SQLite strftime('%w'): Sunday=0, Monday=1)
    // 2026-01-05T14:30:00Z -> Hour 14
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, media_type, title, year, watched_at, duration_ms) VALUES ('h1', ?, 'movie', 'Oppenheimer', 2023, '2026-01-05T14:30:00Z', 10800000)"
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, media_type, title, grandparent_title, year, watched_at, duration_ms) VALUES ('h2', ?, 'episode', 'Ep1', 'Succession', 2023, '2026-01-05T14:45:00Z', 3600000)"
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    // 1. Test GET /api/v1/stats includes weekly_heatmap with 7x24 = 168 cells
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/stats")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let stats_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let heatmap = stats_json["weekly_heatmap"].as_array().expect("weekly_heatmap array");
    assert_eq!(heatmap.len(), 168);

    // Find cell for Monday (1) hour 14
    let target_cell = heatmap.iter().find(|c| (c["day_index"] == 1 || c["day_of_week"] == 1) && c["hour"] == 14);
    assert!(target_cell.is_some());
    let cell = target_cell.unwrap();
    assert_eq!(cell["count"], 2);

    // 2. Test GET /api/v1/stats/wrapped?year=2026
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/stats/wrapped?year=2026")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let wrapped_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(wrapped_json["year"], 2026);
    assert_eq!(wrapped_json["total_scrobbles"], 2);
    assert_eq!(wrapped_json["total_movies"], 1);
    assert_eq!(wrapped_json["total_episodes"], 1);
    assert_eq!(wrapped_json["total_hours"], 4.0);
    assert_eq!(wrapped_json["longest_movie"]["title"], "Oppenheimer");
    assert_eq!(wrapped_json["longest_movie"]["duration_minutes"], 180);
    assert_eq!(wrapped_json["top_shows"][0]["show_title"], "Succession");
    assert_eq!(wrapped_json["top_movies"][0]["title"], "Oppenheimer");
}

#[tokio::test]
async fn test_overseerr_endpoints() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_overseerr";
    let token = create_jwt(user_id, "overseerr@test.local", &config.jwt_secret).unwrap();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')"
    )
    .bind(user_id)
    .bind("overseerr@test.local")
    .bind("Overseerr Tester")
    .bind("overseerr_token_abc")
    .execute(&pool)
    .await
    .unwrap();

    // 1. GET /api/v1/integrations/overseerr/status -> not configured initially
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/integrations/overseerr/status")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let status_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(status_json["configured"], false);

    // 3. Save overseerr settings
    sqlx::query("INSERT INTO user_settings (user_id, key, value) VALUES (?, 'overseerr_url', 'http://192.168.1.100:5055')")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_settings (user_id, key, value) VALUES (?, 'overseerr_api_key', 'test_overseerr_key')")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    // Check status again -> configured = true
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/integrations/overseerr/status")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let status_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(status_json["configured"], true);
}

#[tokio::test]
async fn test_outbound_webhook_test_and_user_settings() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test_user_outbound";
    let token = create_jwt(user_id, "outbound@test.local", &config.jwt_secret).unwrap();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')"
    )
    .bind(user_id)
    .bind("outbound@test.local")
    .bind("Outbound Tester")
    .bind("outbound_token_abc")
    .execute(&pool)
    .await
    .unwrap();

    // 1. Update user settings with outbound webhook and emby username
    let update_req = json!({
        "emby_username": "emby_superfan",
        "outbound_webhook_type": "generic",
        "outbound_webhook_url": "https://example.com/webhook",
        "overseerr_url": "http://192.168.1.50:5055"
    });

    let req = Request::builder()
        .method("PUT")
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&update_req).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. GET user settings and verify values
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(settings_json["emby_username"], "emby_superfan");
    assert_eq!(settings_json["outbound_webhook_type"], "generic");
    assert_eq!(settings_json["outbound_webhook_url"], "https://example.com/webhook");
    assert_eq!(settings_json["overseerr_url"], "http://192.168.1.50:5055");
}

#[tokio::test]
async fn test_history_and_timeline_missing_metadata_filter() {
    let (app, pool, config) = setup_test_app().await;
    let user_id = "test_user_missing_meta";
    let token = create_jwt(user_id, "missing_meta_user@example.com", &config.jwt_secret).unwrap();

    sqlx::query(
        "INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')"
    )
    .bind(user_id)
    .bind("missing_meta_user@example.com")
    .bind("Missing Meta User")
    .bind("missing_meta_token_123")
    .execute(&pool)
    .await
    .unwrap();

    // Scrobble 1: Complete metadata
    let scrobble_complete = serde_json::json!({
        "media_type": "movie",
        "title": "Complete Movie",
        "year": 2024,
        "thumb_url": "https://image.tmdb.org/t/p/w500/complete.jpg",
        "overview": "A full overview of this great film.",
        "genres": "Action, Sci-Fi",
        "watched_at": "2026-10-20T12:00:00Z"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&scrobble_complete).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Scrobble 2: Missing metadata (no thumb, no overview, no genres)
    let scrobble_missing = serde_json::json!({
        "media_type": "movie",
        "title": "Unmatched Mystery Movie",
        "year": 2023,
        "watched_at": "2026-10-20T14:00:00Z"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&scrobble_missing).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 1. History: all items
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?all=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["total"], 2);

    // 2. History: missing_metadata=true
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?all=true&missing_metadata=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["items"][0]["title"], "Unmatched Mystery Movie");

    // 3. History: search query "missing:metadata"
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?all=true&search=missing:metadata")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["items"][0]["title"], "Unmatched Mystery Movie");

    // 4. Timeline: missing_metadata=true
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline?all_dates=true&missing_metadata=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let days: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0]["items"].as_array().unwrap().len(), 1);
    assert_eq!(days[0]["items"][0]["title"], "Unmatched Mystery Movie");
}

#[tokio::test]
async fn test_fix_match_with_tmdb_id_and_search() {
    let (app, pool, config) = setup_test_app().await;

    let user_a_id = "user_tmdb_fix_a";
    let token_a = create_jwt(user_a_id, "user_tmdb_a@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("user_tmdb_a@test.local")
        .bind("User TMDB A")
        .bind("token_tmdb_a")
        .execute(&pool)
        .await
        .unwrap();

    let user_b_id = "user_tmdb_fix_b";
    let token_b = create_jwt(user_b_id, "user_tmdb_b@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("user_tmdb_b@test.local")
        .bind("User TMDB B")
        .bind("token_tmdb_b")
        .execute(&pool)
        .await
        .unwrap();

    let scrobble_1 = "scrobble_tmdb_ep1";
    let scrobble_2 = "scrobble_tmdb_ep2";

    sqlx::query(
        r#"
        INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at)
        VALUES (?, ?, 'Winter Is Coming', 'episode', 'Game of Thrones', 1, 1, datetime('now', '-2 days'))
        "#,
    )
    .bind(scrobble_1)
    .bind(user_a_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at)
        VALUES (?, ?, 'The Kingsroad', 'episode', 'Game of Thrones', 1, 2, datetime('now', '-1 day'))
        "#,
    )
    .bind(scrobble_2)
    .bind(user_a_id)
    .execute(&pool)
    .await
    .unwrap();

    // 1. User B tries to search match for User A's scrobble -> 404 Not Found
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/history/{}/match-search?query=Game+of+Thrones", scrobble_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 2. User A searches match -> 200 OK (returns query and candidates array)
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/history/{}/match-search?query=Game+of+Thrones&provider=tmdb", scrobble_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["query"], "Game of Thrones");
    assert!(res["results"].is_array());

    // 3. User B tries to fix match User A's scrobble -> 404
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/fix-match", scrobble_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "tmdb_id": "1399",
            "provider": "tmdb"
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 4. User A fixes match with TMDB ID "1399"
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/fix-match", scrobble_1))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "tmdb_id": "1399",
            "provider": "tmdb"
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(res["status"], "fixed");
    assert_eq!(res["tmdb_id"], "1399");
    assert_eq!(res["provider"], "tmdb");
    assert_eq!(res["scrobble"]["tmdb_id"], "1399");

    // 5. Verify sibling episode also has tmdb_id set
    let sibling_tmdb_id: Option<String> = sqlx::query_scalar("SELECT tmdb_id FROM scrobbles WHERE id = ?")
        .bind(scrobble_2)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sibling_tmdb_id.as_deref(), Some("1399"));
}

#[tokio::test]
async fn test_metadata_enrichment_worker_supports_tmdb_and_resolves_missing() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "user_worker_tmdb_test";
    let token = create_jwt(user_id, "user_worker@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_id)
        .bind("user_worker@test.local")
        .bind("User Worker")
        .bind("token_worker")
        .execute(&pool)
        .await
        .unwrap();

    let scrobble_id = "scrobble_missing_meta_1";
    sqlx::query(
        r#"
        INSERT INTO scrobbles (id, user_id, title, media_type, year, watched_at)
        VALUES (?, ?, 'Inception', 'movie', 2010, datetime('now'))
        "#,
    )
    .bind(scrobble_id)
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    // Verify it is initially returned by missing_metadata filter
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?all=true&missing_metadata=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["total"], 1);

    // Call fix match with TMDB id
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/fix-match", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&serde_json::json!({
            "id": "27205",
            "provider": "tmdb"
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Populate metadata
    sqlx::query(
        r#"
        UPDATE scrobbles
        SET genres = 'Action, Sci-Fi',
            overview = 'A thief who steals corporate secrets through the use of dream-sharing technology...',
            thumb_url = 'https://image.tmdb.org/t/p/w500/edv5CZvWj09upOsy2Y6IwDhK8bt.jpg',
            metadata_updated_at = datetime('now')
        WHERE id = ?
        "#
    )
    .bind(scrobble_id)
    .execute(&pool)
    .await
    .unwrap();

    // Verify it is NO LONGER returned by missing_metadata filter
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?all=true&missing_metadata=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["total"], 0);
}

#[tokio::test]
async fn test_tmdb_fix_match_cinematographer_filtering() {
    let pool = init_db("sqlite::memory:").await.unwrap();

    // 1. Mock TMDB API server with a large camera & directing crew
    let mock_tmdb_router = axum::Router::new()
        .route(
            "/movie/{id}",
            axum::routing::get(|axum::extract::Path(id): axum::extract::Path<String>, axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                if params.get("api_key").map(|s| s.as_str()) == Some("test_tmdb_key") && id == "99999" {
                    return (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "id": 99999,
                            "title": "Blade Runner 2049",
                            "overview": "Thirty years after the events of the first film, a new blade runner unearths a long-buried secret.",
                            "poster_path": "/gajva2L0rPYkEWjzgFlBXCAVBE5.jpg",
                            "genres": [
                                { "id": 878, "name": "Science Fiction" },
                                { "id": 18, "name": "Drama" }
                            ],
                            "credits": {
                                "cast": [
                                    { "name": "Ryan Gosling", "character": "K" },
                                    { "name": "Harrison Ford", "character": "Rick Deckard" }
                                ],
                                "crew": [
                                    { "name": "Denis Villeneuve", "job": "Director", "department": "Directing" },
                                    { "name": "Donald Murphy", "job": "First Assistant Director", "department": "Directing" },
                                    { "name": "Lucie Brossard", "job": "Second Assistant Director", "department": "Directing" },
                                    { "name": "Roger Deakins", "job": "Director of Photography", "department": "Camera" },
                                    { "name": "Bob Grip", "job": "Key Grip", "department": "Camera" },
                                    { "name": "Alice Operator", "job": "Camera Operator", "department": "Camera" },
                                    { "name": "Charlie Assistant", "job": "First Assistant Camera", "department": "Camera" },
                                    { "name": "Dave Steadicam", "job": "Steadicam Operator", "department": "Camera" },
                                    { "name": "Eve Still", "job": "Still Photographer", "department": "Camera" },
                                    { "name": "Frank Drone", "job": "Drone Operator", "department": "Camera" },
                                    { "name": "Grace Loader", "job": "Clapper Loader", "department": "Camera" },
                                    { "name": "Heidi DIT", "job": "Digital Imaging Technician", "department": "Camera" },
                                    { "name": "Ivan Trainee", "job": "Camera Trainee", "department": "Camera" }
                                ]
                            }
                        })),
                    );
                }
                (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({ "error": "Not found" })))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, mock_tmdb_router).await.unwrap();
    });

    let mut config = Config::from_env();
    config.jwt_secret = "test-secret-key-12345".to_string();
    config.metadata_source = "tmdb".to_string();
    config.tmdb_api_key = Some("test_tmdb_key".to_string());
    config.tmdb_base_url = format!("http://{}", addr);
    let config = Arc::new(config);

    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc_client: None,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer: Arc::new(bingesync::api::logs::LogBuffer::new(500)),
        task_tracker: Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new()),
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
    };

    let app = create_api_router(state.clone());

    let user_id = "user_cinematographer_test";
    let token = create_jwt(user_id, "user_dp@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_id)
        .bind("user_dp@test.local")
        .bind("User DP")
        .bind("token_dp_test")
        .execute(&pool)
        .await
        .unwrap();

    let scrobble_id = "scrobble_br2049";
    sqlx::query(
        r#"
        INSERT INTO scrobbles (id, user_id, title, media_type, watched_at)
        VALUES (?, ?, 'Blade Runner 2049', 'movie', datetime('now'))
        "#
    )
    .bind(scrobble_id)
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    // Fix match using TMDB ID 99999
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/history/{}/fix-match", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&serde_json::json!({
            "tvdb_id": "99999",
            "provider": "tmdb"
        })).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Fetch scrobble and verify cinematographer and director fields
    let updated: Scrobble = sqlx::query_as("SELECT * FROM scrobbles WHERE id = ?")
        .bind(scrobble_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Cinematographer must only be Roger Deakins - NOT any of the camera operators, assistants, grips, loaders, etc.
    assert_eq!(updated.cinematographer.as_deref(), Some("Roger Deakins"));

    // Directors must only be Denis Villeneuve - NOT assistant directors
    assert_eq!(updated.director.as_deref(), Some("Denis Villeneuve"));
    assert_eq!(updated.directors.as_deref(), Some("Denis Villeneuve"));
}

#[tokio::test]
async fn test_plex_and_jellyfin_non_playback_events_ignored() {
    let (app, pool, _config) = setup_test_app().await;

    // Create user
    let user = bingesync::api::auth::get_or_create_user_with_sid(
        &pool,
        "bob_library_new@bingesync.local",
        "Bob",
        None,
        None,
        None,
        Some("bob_plex"),
        false,
    )
    .await
    .unwrap();

    // 1. Plex fires "library.new" when a new item is added to the Plex library
    let new_item_payload = json!({
        "event": "library.new",
        "user": true,
        "owner": true,
        "Account": { "title": "bob_plex" },
        "Server": { "title": "Home Server" },
        "Metadata": {
            "type": "movie",
            "title": "Gladiator II",
            "year": 2024,
            "duration": 8880000,
            "summary": "Years after witnessing the death of Maximus, Lucius must enter the Colosseum.",
            "thumb": "https://example.com/gladiator2.jpg"
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(new_item_payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(resp_json["status"], "ignored");
    assert_eq!(resp_json["reason"], "non_playback_event");
    assert_eq!(resp_json["event"], "library.new");

    // Verify NO scrobble was created in the database
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "library.new must not create a scrobble");

    // 2. Plex fires "library.on.deck"
    let on_deck_payload = json!({
        "event": "library.on.deck",
        "user": true,
        "owner": true,
        "Account": { "title": "bob_plex" },
        "Metadata": {
            "type": "movie",
            "title": "Gladiator II"
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(on_deck_payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(resp_json["status"], "ignored");
    assert_eq!(resp_json["reason"], "non_playback_event");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "library.on.deck must not create a scrobble");

    // 3. Jellyfin fires "ItemAdded" when a new item is added to the library
    let jelly_new_payload = json!({
        "NotificationType": "ItemAdded",
        "ItemType": "Movie",
        "Name": "Gladiator II",
        "Year": 2024
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(jelly_new_payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(resp_json["status"], "ignored");
    assert_eq!(resp_json["reason"], "non_playback_event");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "Jellyfin ItemAdded must not create a scrobble");

    // 4. When actual playback occurs (e.g. media.scrobble), it IS recorded
    let playback_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "owner": true,
        "Account": { "title": "bob_plex" },
        "Metadata": {
            "type": "movie",
            "title": "Gladiator II",
            "year": 2024,
            "duration": 8880000
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", user.webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(playback_payload.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "Playback media.scrobble must create a scrobble");
}

#[tokio::test]
async fn test_scrobble_tags_and_notes_crud_and_filtering() {
    let (app, pool, config) = setup_test_app().await;

    // Create user A
    let user_a_id = "user_tags_a";
    let token_a = create_jwt(user_a_id, "usera@tags.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("usera@tags.local")
        .bind("User A Tags")
        .bind("token_tags_a")
        .execute(&pool)
        .await
        .unwrap();

    // Create user B
    let user_b_id = "user_tags_b";
    let token_b = create_jwt(user_b_id, "userb@tags.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("userb@tags.local")
        .bind("User B Tags")
        .bind("token_tags_b")
        .execute(&pool)
        .await
        .unwrap();

    // 1. User A logs a scrobble with personal tags & review notes
    let new_scrobble = json!({
        "title": "Top Gun: Maverick",
        "media_type": "movie",
        "year": 2022,
        "tags": "Plane Movie, Rewatch",
        "notes": "Spectacular aerial photography in IMAX!"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(new_scrobble.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let created: Scrobble = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(created.tags.as_deref(), Some("Plane Movie, Rewatch"));
    assert_eq!(created.notes.as_deref(), Some("Spectacular aerial photography in IMAX!"));

    let scrobble_id = created.id;

    // 2. User B tries to update user A's scrobble tags/notes -> 404 Not Found
    let update_b = json!({
        "tags": "Hacked Tag",
        "notes": "Hacked Notes"
    });
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/history/{}/user-data", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(update_b.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 3. User A updates tags & notes (e.g. "Plane Movie, Watched with Daryl")
    let update_a = json!({
        "tags": "Plane Movie, Watched with Daryl",
        "notes": "Enjoyed with Daryl on a long flight."
    });
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/history/{}/user-data", scrobble_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(update_a.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let updated_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(updated_res["tags"], "Plane Movie, Watched with Daryl");
    assert_eq!(updated_res["notes"], "Enjoyed with Daryl on a long flight.");

    // 4. Query history filtered by tag="Watched with Daryl"
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?tag=Watched+with+Daryl")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let history_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(history_json["total"], 1);
    assert_eq!(history_json["items"][0]["title"], "Top Gun: Maverick");
    assert_eq!(history_json["items"][0]["tags"], "Plane Movie, Watched with Daryl");

    // 5. Query history with non-existent tag -> 0 items
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?tag=Horror")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let history_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(history_json["total"], 0);

    // 6. Search history by keyword inside notes ("flight")
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?search=flight")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let history_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(history_json["total"], 1);
    assert_eq!(history_json["items"][0]["title"], "Top Gun: Maverick");

    // 7. Verify User B sees 0 items (multi-user isolation)
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?search=flight")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let history_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(history_json["total"], 0);
}

#[tokio::test]
async fn test_duplicate_watch_detector_and_cleanup() {
    let (app, pool, config) = setup_test_app().await;

    // Create User A and User B
    let user_a_id = "user_dup_a";
    let token_a = create_jwt(user_a_id, "user_dup_a@example.com", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("user_dup_a@example.com")
        .bind("User Dup A")
        .bind("token_dup_a")
        .execute(&pool)
        .await
        .unwrap();

    let user_b_id = "user_dup_b";
    let token_b = create_jwt(user_b_id, "user_dup_b@example.com", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("user_dup_b@example.com")
        .bind("User Dup B")
        .bind("token_dup_b")
        .execute(&pool)
        .await
        .unwrap();

    // 1. Insert scrobbles for User A:
    // - Inception (Movie) at 14:00 and 14:20 (duplicate!)
    // - Breaking Bad S01E01 at 16:00 and 16:30 (duplicate!)
    // - The Dark Knight at 20:00 (single watch, not duplicate)
    let incep_1 = uuid::Uuid::new_v4().to_string();
    let incep_2 = uuid::Uuid::new_v4().to_string();
    let bb_1 = uuid::Uuid::new_v4().to_string();
    let bb_2 = uuid::Uuid::new_v4().to_string();
    let dk_1 = uuid::Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, year, watched_at, created_at) VALUES (?, ?, 'Inception', 'movie', 2010, '2024-05-10T14:00:00Z', '2024-05-10T14:00:00Z')")
        .bind(&incep_1).bind(user_a_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, year, watched_at, created_at) VALUES (?, ?, 'Inception', 'movie', 2010, '2024-05-10T14:20:00Z', '2024-05-10T14:20:00Z')")
        .bind(&incep_2).bind(user_a_id).execute(&pool).await.unwrap();

    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at, created_at) VALUES (?, ?, 'Pilot', 'episode', 'Breaking Bad', 1, 1, '2024-05-10T16:00:00Z', '2024-05-10T16:00:00Z')")
        .bind(&bb_1).bind(user_a_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, watched_at, created_at) VALUES (?, ?, 'Pilot', 'episode', 'Breaking Bad', 1, 1, '2024-05-10T16:30:00Z', '2024-05-10T16:30:00Z')")
        .bind(&bb_2).bind(user_a_id).execute(&pool).await.unwrap();

    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, year, watched_at, created_at) VALUES (?, ?, 'The Dark Knight', 'movie', 2008, '2024-05-10T20:00:00Z', '2024-05-10T20:00:00Z')")
        .bind(&dk_1).bind(user_a_id).execute(&pool).await.unwrap();

    // 2. Insert scrobble for User B with Inception at 14:00 (should not leak or match)
    let incep_b = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, year, watched_at, created_at) VALUES (?, ?, 'Inception', 'movie', 2010, '2024-05-10T14:00:00Z', '2024-05-10T14:00:00Z')")
        .bind(&incep_b).bind(user_b_id).execute(&pool).await.unwrap();

    // 3. Call GET /api/v1/history/duplicates for User A
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history/duplicates?window_hours=24")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let dup_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(dup_json["total_duplicate_groups"], 2);
    assert_eq!(dup_json["total_redundant_scrobbles"], 2);
    assert_eq!(dup_json["groups"].as_array().unwrap().len(), 2);

    // 4. Call GET /api/v1/history/duplicates for User B -> 0 duplicates
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history/duplicates?window_hours=24")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_b))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let dup_json_b: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(dup_json_b["total_duplicate_groups"], 0);

    // 5. Test clean duplicates with strategy "keep_oldest" for User A
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/history/duplicates/clean")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&serde_json::json!({
            "strategy": "keep_oldest",
            "window_hours": 24
        })).unwrap()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let clean_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(clean_json["success"], true);
    assert_eq!(clean_json["deleted_count"], 2);

    // 6. Verify User A's database state:
    // incep_1 (oldest) exists, incep_2 (newer) is deleted
    // bb_1 (oldest) exists, bb_2 (newer) is deleted
    // dk_1 exists
    let incep_1_row = sqlx::query("SELECT id FROM scrobbles WHERE id = ?").bind(&incep_1).fetch_optional(&pool).await.unwrap();
    assert!(incep_1_row.is_some());
    let incep_2_row = sqlx::query("SELECT id FROM scrobbles WHERE id = ?").bind(&incep_2).fetch_optional(&pool).await.unwrap();
    assert!(incep_2_row.is_none());

    let bb_1_row = sqlx::query("SELECT id FROM scrobbles WHERE id = ?").bind(&bb_1).fetch_optional(&pool).await.unwrap();
    assert!(bb_1_row.is_some());
    let bb_2_row = sqlx::query("SELECT id FROM scrobbles WHERE id = ?").bind(&bb_2).fetch_optional(&pool).await.unwrap();
    assert!(bb_2_row.is_none());

    let dk_1_row = sqlx::query("SELECT id FROM scrobbles WHERE id = ?").bind(&dk_1).fetch_optional(&pool).await.unwrap();
    assert!(dk_1_row.is_some());

    // User B's scrobble is completely untouched
    let incep_b_row = sqlx::query("SELECT id FROM scrobbles WHERE id = ?").bind(&incep_b).fetch_optional(&pool).await.unwrap();
    assert!(incep_b_row.is_some());

    // 7. Verify scanning duplicates now returns 0 groups for User A
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history/duplicates?window_hours=24")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let dup_json_after: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(dup_json_after["total_duplicate_groups"], 0);
    assert_eq!(dup_json_after["total_redundant_scrobbles"], 0);
}

#[tokio::test]
async fn test_history_rating_threshold_filter() {
    let (app, pool, config) = setup_test_app().await;

    // Create user A and user B
    let user_a_id = "user_rate_a";
    let token_a = create_jwt(user_a_id, "rate_user_a@test.com", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("rate_user_a@test.com")
        .bind("Rating User A")
        .bind("token_rate_a")
        .execute(&pool)
        .await
        .unwrap();

    let user_b_id = "user_rate_b";
    let _token_b = create_jwt(user_b_id, "rate_user_b@test.com", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("rate_user_b@test.com")
        .bind("Rating User B")
        .bind("token_rate_b")
        .execute(&pool)
        .await
        .unwrap();

    // Insert scrobbles for user A
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, year, user_rating, watch_status, watched_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 'completed', '2026-03-01T20:00:00Z', '2026-03-01T20:00:00Z')"
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_a_id)
    .bind("The Dark Knight")
    .bind("movie")
    .bind(2008)
    .bind(10)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, year, user_rating, watch_status, watched_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 'completed', '2026-03-02T20:00:00Z', '2026-03-02T20:00:00Z')"
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_a_id)
    .bind("Inception")
    .bind("movie")
    .bind(2010)
    .bind(8)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, year, user_rating, watch_status, watched_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 'completed', '2026-03-03T20:00:00Z', '2026-03-03T20:00:00Z')"
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_a_id)
    .bind("The Matrix")
    .bind("movie")
    .bind(1999)
    .bind(6)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, user_rating, watch_status, watched_at, created_at)
         VALUES (?, ?, ?, ?, ?, 5, 14, ?, 'completed', '2026-03-04T20:00:00Z', '2026-03-04T20:00:00Z')"
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_a_id)
    .bind("Ozymandias")
    .bind("episode")
    .bind("Breaking Bad")
    .bind(9)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, user_rating, watch_status, watched_at, created_at)
         VALUES (?, ?, ?, ?, ?, 4, 3, ?, 'completed', '2026-03-05T20:00:00Z', '2026-03-05T20:00:00Z')"
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_a_id)
    .bind("Connor's Wedding")
    .bind("episode")
    .bind("Succession")
    .bind(7)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, grandparent_title, season_number, episode_number, user_rating, watch_status, watched_at, created_at)
         VALUES (?, ?, ?, ?, ?, 1, 1, ?, 'completed', '2026-03-06T20:00:00Z', '2026-03-06T20:00:00Z')"
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_a_id)
    .bind("Pilot")
    .bind("episode")
    .bind("Random Show")
    .bind(4)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, year, user_rating, watch_status, watched_at, created_at)
         VALUES (?, ?, ?, ?, ?, NULL, 'completed', '2026-03-07T20:00:00Z', '2026-03-07T20:00:00Z')"
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_a_id)
    .bind("Unrated Movie")
    .bind("movie")
    .bind(2021)
    .execute(&pool)
    .await
    .unwrap();

    // User B scrobble (must be isolated)
    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, year, user_rating, watch_status, watched_at, created_at)
         VALUES (?, ?, ?, ?, ?, 10, 'completed', '2026-03-08T20:00:00Z', '2026-03-08T20:00:00Z')"
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_b_id)
    .bind("The Godfather")
    .bind("movie")
    .bind(1972)
    .execute(&pool)
    .await
    .unwrap();

    // 1. Filter all media above 8+ stars
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?rating=8+")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hist.total, 3);
    let titles: Vec<&str> = hist.items.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.contains(&"The Dark Knight"));
    assert!(titles.contains(&"Ozymandias"));
    assert!(titles.contains(&"Inception"));
    assert!(!titles.contains(&"The Godfather")); // User B isolated

    // 2. Filter movies only above 8+ stars
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?media_type=movie&rating=8+")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hist.total, 2);
    let movie_titles: Vec<&str> = hist.items.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(movie_titles, vec!["Inception", "The Dark Knight"]);

    // 3. Filter TV episodes only above 8+ stars
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?media_type=episode&rating=8+")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hist.total, 1);
    assert_eq!(hist.items[0].title, "Ozymandias");

    // 4. Filter threshold 4+ stars
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?rating=4+")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hist.total, 6);

    // 5. Filter exact 10
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?rating=10")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hist.total, 1);
    assert_eq!(hist.items[0].title, "The Dark Knight");

    // 6. Filter unrated
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?rating=unrated")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hist.total, 1);
    assert_eq!(hist.items[0].title, "Unrated Movie");

    // 7. Filter rated only
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?rating=rated")
        .header(header::AUTHORIZATION, format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let hist: HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hist.total, 6);
}

#[tokio::test]
async fn test_oidc_admin_group_configuration_and_evaluation() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create an admin user to test admin settings endpoints
    let admin_id = "test_admin_oidc";
    let admin_jwt = create_jwt(admin_id, "admin_oidc@test.com", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(admin_id)
        .bind("admin_oidc@test.com")
        .bind("OIDC Admin Settings Tester")
        .bind("token_oidc_admin")
        .execute(&pool)
        .await
        .unwrap();

    // 2. Test updating oidc_admin_group via admin settings endpoint
    let update_body = json!({
        "oidc_admin_group": "pocketid-admins, bingesync-moderators"
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

    // 3. Verify oidc_admin_group is returned in GET /api/v1/admin/settings
    let req = Request::builder()
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings: bingesync::api::settings::SystemSettingsView = serde_json::from_slice(&body).unwrap();
    assert_eq!(settings.oidc_admin_group.as_deref(), Some("pocketid-admins, bingesync-moderators"));

    // 4. Test OidcUserInfo.is_admin with various group claim formats and custom_admin_group
    let custom_group_setting = Some("pocketid-admins, bingesync-moderators");

    // Case A: User in custom group "pocketid-admins" -> should be admin
    let user_a: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-123",
        "email": "user_a@example.com",
        "groups": ["pocketid-admins", "regular-users"]
    })).unwrap();
    assert!(user_a.is_admin(custom_group_setting));

    // Case B: Case insensitivity and leading slash "/PocketId-Admins" -> should be admin
    let user_b: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-456",
        "email": "user_b@example.com",
        "groups": ["/PocketID-Admins"]
    })).unwrap();
    assert!(user_b.is_admin(custom_group_setting));

    // Case C: Standard default group "administrator" still recognized -> should be admin
    let user_c: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-789",
        "email": "user_c@example.com",
        "groups": ["administrator"]
    })).unwrap();
    assert!(user_c.is_admin(custom_group_setting));

    // Case D: User in object-based group list [{"name": "bingesync-moderators"}] -> should be admin
    let user_d: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-101",
        "email": "user_d@example.com",
        "groups": [{"slug": "bingesync-moderators"}]
    })).unwrap();
    assert!(user_d.is_admin(custom_group_setting));

    // Case E: User with memberOf claim alias -> should be admin
    let user_e: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-102",
        "email": "user_e@example.com",
        "memberOf": ["bingesync-moderators"]
    })).unwrap();
    assert!(user_e.is_admin(custom_group_setting));

    // Case F: Regular user in "viewers" only -> should NOT be admin
    let user_f: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-103",
        "email": "user_f@example.com",
        "groups": ["viewers", "family"]
    })).unwrap();
    assert!(!user_f.is_admin(custom_group_setting));
    assert!(!user_f.is_admin(None));
}

#[tokio::test]
async fn test_first_run_setup_flow_and_initial_admin_promotion() {
    let (app, pool, _config) = setup_test_app().await;

    // 1. Check initial setup status on a fresh database with 0 users
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/setup/status")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status["needs_setup"], true);
    assert_eq!(status["completed"], false);
    assert_eq!(status["user_count"], 0);
    assert_eq!(status["redirect_url"], "http://localhost:8080/api/v1/auth/callback");
    assert_eq!(status["backchannel_logout_url"], "http://localhost:8080/api/v1/auth/backchannel-logout");

    // 2. Validate save requires issuer_url and client_id
    let req_invalid = Request::builder()
        .method("POST")
        .uri("/api/v1/setup/save")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "oidc_issuer_url": "",
            "oidc_client_id": ""
        }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req_invalid).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 3. Save setup configuration
    let req_save = Request::builder()
        .method("POST")
        .uri("/api/v1/setup/save")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "oidc_issuer_url": "https://auth.example.com",
            "oidc_client_id": "bingesync-client",
            "oidc_client_secret": "super-secret-key-xyz",
            "oidc_admin_group": "my-idp-admins"
        }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req_save).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify settings stored in DB
    let oidc_enabled = bingesync::api::settings::get_db_setting(&pool, "oidc_enabled").await;
    assert_eq!(oidc_enabled.as_deref(), Some("true"));
    let issuer = bingesync::api::settings::get_db_setting(&pool, "oidc_issuer_url").await;
    assert_eq!(issuer.as_deref(), Some("https://auth.example.com"));
    let client_id = bingesync::api::settings::get_db_setting(&pool, "oidc_client_id").await;
    assert_eq!(client_id.as_deref(), Some("bingesync-client"));
    let admin_group = bingesync::api::settings::get_db_setting(&pool, "oidc_admin_group").await;
    assert_eq!(admin_group.as_deref(), Some("my-idp-admins"));
    let setup_completed = bingesync::api::settings::get_db_setting(&pool, "setup_completed").await;
    assert_eq!(setup_completed.as_deref(), Some("true"));

    // 4. Verify initial user promotion to admin
    // When the first user logs in via get_or_create_user_with_sid (even with is_admin=false),
    // they must be automatically assigned the admin role.
    let user1 = bingesync::api::auth::get_or_create_user_with_sid(
        &pool,
        "first_user@example.com",
        "First User",
        Some("sub-001"),
        None,
        None,
        Some("firstuser"),
        false, // not specified as admin
    ).await.unwrap();

    assert_eq!(user1.role, "admin");

    // A second user with is_admin=false should receive the normal "user" role
    let user2 = bingesync::api::auth::get_or_create_user_with_sid(
        &pool,
        "second_user@example.com",
        "Second User",
        Some("sub-002"),
        None,
        None,
        Some("seconduser"),
        false,
    ).await.unwrap();

    assert_eq!(user2.role, "user");

    // 5. Verify setup/status now returns needs_setup: false and completed: true
    let req_status2 = Request::builder()
        .method("GET")
        .uri("/api/v1/setup/status")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req_status2).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let status2: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status2["needs_setup"], false);
    assert_eq!(status2["completed"], true);
    assert_eq!(status2["user_count"], 2);

    // 6. Verify subsequent attempts to call /api/v1/setup/save are rejected with 403 Forbidden
    let req_save_forbidden = Request::builder()
        .method("POST")
        .uri("/api/v1/setup/save")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "oidc_issuer_url": "https://attacker.example.com",
            "oidc_client_id": "malicious"
        }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req_save_forbidden).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_user_api_keys_crud_scoping_and_authentication() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create two users: User A (standard) and User B (standard), and an Admin
    let user_a_id = "user_a_apikey_test";
    let user_a_jwt = create_jwt(user_a_id, "user_a@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_a_id)
        .bind("user_a@test.local")
        .bind("User A")
        .bind("token_a")
        .execute(&pool)
        .await
        .unwrap();

    let user_b_id = "user_b_apikey_test";
    let user_b_jwt = create_jwt(user_b_id, "user_b@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'user')")
        .bind(user_b_id)
        .bind("user_b@test.local")
        .bind("User B")
        .bind("token_b")
        .execute(&pool)
        .await
        .unwrap();

    let admin_id = "admin_apikey_test";
    let admin_jwt = create_jwt(admin_id, "admin@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(admin_id)
        .bind("admin@test.local")
        .bind("Admin")
        .bind("token_admin")
        .execute(&pool)
        .await
        .unwrap();

    // 2. User A initially has 0 API keys
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/user/api-keys")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_a_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let keys: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(keys.is_empty());

    // 3. User A creates an API key "Home Assistant"
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/api-keys")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_a_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "name": "Home Assistant" }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(created["name"], "Home Assistant");
    let full_api_key_a = created["api_key"].as_str().unwrap().to_string();
    let key_id_a = created["id"].as_str().unwrap().to_string();
    assert!(full_api_key_a.starts_with("bs_live_"));
    assert!(created["key_prefix"].as_str().unwrap().starts_with("bs_live_"));

    // 4. User A lists API keys -> 1 key present
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/user/api-keys")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_a_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let keys: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["id"], key_id_a);
    assert_eq!(keys[0]["name"], "Home Assistant");

    // 5. Multi-User Isolation: User B cannot see User A's API key
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/user/api-keys")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_b_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let keys_b: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(keys_b.is_empty());

    // User B cannot delete User A's API key
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/user/api-keys/{}", key_id_a))
        .header(header::AUTHORIZATION, format!("Bearer {}", user_b_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 6. Third-party authentication via X-Api-Key
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header("x-api-key", &full_api_key_a)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let me: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(me["id"], user_a_id);
    assert_eq!(me["email"], "user_a@test.local");
    assert_eq!(me["role"], "user");

    // 7. Third-party authentication via Authorization: Bearer <api_key>
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {}", full_api_key_a))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 8. Third-party authentication via Query Param ?api_key=<api_key>
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/auth/me?api_key={}", full_api_key_a))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 9. Scoping verification:
    // Create scrobbles for User A and User B
    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, watched_at) VALUES ('scrob_a', ?, 'Movie A', 'movie', '2026-10-30T10:00:00Z')")
        .bind(user_a_id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO scrobbles (id, user_id, title, media_type, watched_at) VALUES ('scrob_b', ?, 'Movie B', 'movie', '2026-10-30T11:00:00Z')")
        .bind(user_b_id)
        .execute(&pool)
        .await
        .unwrap();

    // Query history using User A's API key: returns ONLY Movie A
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header("x-api-key", &full_api_key_a)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let hist: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = hist["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "Movie A");

    // Create a new scrobble using User A's API key
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header("x-api-key", &full_api_key_a)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "title": "Interstellar",
            "media_type": "movie",
            "year": 2014
        }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Verify scrobble created under User A
    let scrobble_owner: String = sqlx::query_scalar("SELECT user_id FROM scrobbles WHERE title = 'Interstellar'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scrobble_owner, user_a_id);

    // 10. Role Permission Scoping:
    // User A API key (standard role) attempting admin endpoint -> 403 Forbidden
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/users")
        .header("x-api-key", &full_api_key_a)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Admin creates API key and accesses admin endpoint -> 200 OK
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/api-keys")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "name": "Admin Script Key" }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let admin_key_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let admin_api_key = admin_key_resp["api_key"].as_str().unwrap();

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/users")
        .header("x-api-key", admin_api_key)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 11. Invalid API key returns 401 Unauthorized
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history")
        .header("x-api-key", "bs_live_invalid_nonexistent_key_123")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 12. Revoke API Key: User A revokes their key
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/user/api-keys/{}", key_id_a))
        .header(header::AUTHORIZATION, format!("Bearer {}", user_a_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Subsequent call with revoked key returns 401 Unauthorized
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header("x-api-key", &full_api_key_a)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 13. Cascading Deletion on User Delete
    // Create an API key for User B, then admin deletes User B
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/api-keys")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_b_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "name": "User B Key" }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let key_count_b: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_api_keys WHERE user_id = ?")
        .bind(user_b_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(key_count_b, 1);

    // Admin deletes User B
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/admin/users/{}", user_b_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify User B's API keys were cascade-deleted
    let key_count_b_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_api_keys WHERE user_id = ?")
        .bind(user_b_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(key_count_b_after, 0);
}

#[tokio::test]
async fn test_admin_version_and_oidc_user_group_configuration() {
    let (app, pool, config) = setup_test_app().await;

    let admin_id = "admin_version_test";
    let admin_jwt = create_jwt(admin_id, "admin@test.local", &config.jwt_secret).unwrap();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, 'admin')")
        .bind(admin_id)
        .bind("admin@test.local")
        .bind("Admin Version Tester")
        .bind("tok_admin_version")
        .execute(&pool)
        .await
        .unwrap();

    // 1. Test GET /api/v1/admin/version and /api/v1/admin/system-info
    let req = Request::builder()
        .uri("/api/v1/admin/version")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let ver_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(ver_json["current_version"].is_string());
    assert!(ver_json["docker_base_image"].is_string());
    assert!(ver_json["sqlite_version"].is_string());
    assert!(ver_json["rust_version"].is_string());
    assert!(ver_json["components"].is_array());
    let comps = ver_json["components"].as_array().unwrap();
    assert!(comps.iter().any(|c| c["name"] == "Docker Base Image"));
    assert!(comps.iter().any(|c| c["name"] == "SQLite Engine"));
    assert!(comps.iter().any(|c| c["name"] == "Rust Toolchain"));

    let axum_comp = comps.iter().find(|c| c["name"] == "Axum Web Framework").expect("Axum component");
    assert!(axum_comp["version"].as_str().unwrap().starts_with("v0.8"));
    let tokio_comp = comps.iter().find(|c| c["name"] == "Tokio Async Runtime").expect("Tokio component");
    assert!(tokio_comp["version"].as_str().unwrap().starts_with("v1."));
    let sqlx_comp = comps.iter().find(|c| c["name"] == "SQLx Database Toolkit").expect("SQLx component");
    assert!(sqlx_comp["version"].as_str().unwrap().starts_with("v0.9"));
    let web_comp = comps.iter().find(|c| c["name"] == "Web Interface").expect("Web Interface component");
    assert!(web_comp["version"].as_str().unwrap().contains("Vite 8"));

    let req = Request::builder()
        .uri("/api/v1/admin/system-info")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Test is_version_newer helper
    assert!(bingesync::api::settings::is_version_newer("0.2.0", "0.1.0"));
    assert!(bingesync::api::settings::is_version_newer("v0.1.1", "0.1.0"));
    assert!(!bingesync::api::settings::is_version_newer("0.1.0", "0.1.0"));
    assert!(!bingesync::api::settings::is_version_newer("0.0.9", "0.1.0"));

    // 3. Test saving oidc_user_group in admin settings
    let update_body = json!({
        "oidc_user_group": "bingesync-viewers, family-members"
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

    // Verify settings response contains oidc_user_group and app_version
    let req = Request::builder()
        .uri("/api/v1/admin/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings: bingesync::api::settings::SystemSettingsView = serde_json::from_slice(&body).unwrap();
    assert_eq!(settings.oidc_user_group.as_deref(), Some("bingesync-viewers, family-members"));
    assert_eq!(settings.app_version, bingesync::api::settings::APP_VERSION);

    // 4. Test OidcUserInfo.has_group with various formats
    let required_user_group = Some("bingesync-viewers, family-members");

    let allowed_user: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-user-allowed",
        "email": "allowed@example.com",
        "groups": ["bingesync-viewers"]
    })).unwrap();
    assert!(allowed_user.has_group(required_user_group));

    let case_insensitive_user: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-user-case",
        "email": "case@example.com",
        "groups": ["/Family-Members"]
    })).unwrap();
    assert!(case_insensitive_user.has_group(required_user_group));

    let forbidden_user: bingesync::auth::OidcUserInfo = serde_json::from_value(json!({
        "sub": "sub-user-forbidden",
        "email": "forbidden@example.com",
        "groups": ["strangers", "guests"]
    })).unwrap();
    assert!(!forbidden_user.has_group(required_user_group));
}

#[tokio::test]
async fn test_security_headers_and_secure_cookie() {
    let (app, _pool, _config) = setup_test_app().await;

    // 1. Test standard HTTP response has CSP, COOP, Permissions-Policy, Nosniff, Sameorigin
    let req = Request::builder()
        .uri("/api/v1/auth/status")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let headers = resp.headers();
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(headers.get("x-frame-options").unwrap(), "SAMEORIGIN");
    assert_eq!(headers.get("referrer-policy").unwrap(), "strict-origin-when-cross-origin");
    assert_eq!(headers.get("cross-origin-opener-policy").unwrap(), "same-origin");
    assert!(headers.get("permissions-policy").unwrap().to_str().unwrap().contains("camera=()"));
    assert!(headers.get("content-security-policy").unwrap().to_str().unwrap().contains("default-src 'self'"));
    // HSTS should not be sent on plain HTTP without x-forwarded-proto: https
    assert!(headers.get("strict-transport-security").is_none());

    // 2. Test HTTPS response (via x-forwarded-proto: https) sends HSTS
    let req_https = Request::builder()
        .uri("/api/v1/auth/status")
        .header("x-forwarded-proto", "https")
        .body(Body::empty())
        .unwrap();
    let resp_https = app.clone().oneshot(req_https).await.unwrap();
    assert_eq!(resp_https.status(), StatusCode::OK);
    assert!(resp_https.headers().get("strict-transport-security").is_some());
    assert!(resp_https.headers().get("strict-transport-security").unwrap().to_str().unwrap().contains("max-age="));

    // 3. Test dev login under HTTPS marks cookie with Secure;
    let login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/dev-login")
        .header("x-forwarded-proto", "https")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&json!({
            "email": "secure_user@example.com",
            "display_name": "Secure User"
        })).unwrap()))
        .unwrap();
    let login_resp = app.clone().oneshot(login_req).await.unwrap();
    assert_eq!(login_resp.status(), StatusCode::OK);
    let cookie_header = login_resp.headers().get("set-cookie").unwrap().to_str().unwrap();
    assert!(cookie_header.contains("bingesync_token="));
    assert!(cookie_header.contains("HttpOnly"));
    assert!(cookie_header.contains("SameSite=Lax"));
    assert!(cookie_header.contains("Secure"));
}

#[tokio::test]
async fn test_tautulli_continuous_sync_and_settings() {
    let (app, pool, _) = setup_test_app().await;

    // 1. Log in admin
    let admin_login = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/dev-login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&json!({
            "email": "admin_tautulli@example.com",
            "display_name": "Tautulli Admin"
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(admin_login).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let admin_jwt = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_string();

    // 2. Log in standard user
    let user_login = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/dev-login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&json!({
            "email": "sync_fan@example.com",
            "display_name": "Sync Fan"
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(user_login).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let user_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let user_jwt = user_json["token"].as_str().unwrap().to_string();
    let user_id = user_json["user"]["id"].as_str().unwrap().to_string();

    // 3. Update user settings with tautulli_user and tautulli_sync_enabled
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&json!({
            "tautulli_url": "http://127.0.0.1:8181",
            "tautulli_api_key": "sec_key_123456",
            "tautulli_user": "plex_fan_1",
            "tautulli_sync_enabled": true
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 4. Retrieve user settings and verify values
    let req = Request::builder()
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", user_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(settings_json["tautulli_user"], "plex_fan_1");
    assert_eq!(settings_json["tautulli_sync_enabled"], true);
    assert_eq!(settings_json["tautulli_url"], "http://127.0.0.1:8181");
    assert_eq!(settings_json["tautulli_api_key_configured"], true);

    // 5. Check background task summary for tautulli_sync
    let req = Request::builder()
        .uri("/api/v1/admin/tasks")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let tasks_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let t_task = tasks_json["tasks"].as_array().unwrap().iter().find(|t| t["id"] == "tautulli_sync").unwrap();
    assert_eq!(t_task["interval_hours"], 1);
    assert_eq!(t_task["schedule"], "Every 1 hour");
    assert_eq!(t_task["name"], "Tautulli Continuous 1-Way Sync");

    // 6. Update schedule for tautulli_sync to 2 hours
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/tautulli_sync/schedule")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&json!({
            "interval_hours": 2
        })).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let sched_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sched_json["interval_hours"], 2);
    assert_eq!(sched_json["schedule"], "Every 2 hours");

    // 7. Trigger tautulli_sync task run
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/tasks/tautulli_sync/run")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_jwt))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let trig_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(trig_json["status"], "triggered");
    assert_eq!(trig_json["task_id"], "tautulli_sync");

    // 8. Test insert_tautulli_watch_items deduplication directly
    use bingesync::api::tautulli_import::{insert_tautulli_watch_items, TautulliWatchItem};

    let sample_items = vec![
        TautulliWatchItem {
            title: "Inception".to_string(),
            media_type: "movie".to_string(),
            grandparent_title: None,
            parent_title: None,
            season_number: None,
            episode_number: None,
            year: Some(2010),
            watched_at: "2026-05-10T20:00:00Z".to_string(),
            duration_ms: Some(8880000),
            rating_key: Some("10101".to_string()),
            thumb_url: None,
            progress_percent: Some(100),
            user: Some("plex_fan_1".to_string()),
            friendly_name: Some("Sync Fan".to_string()),
        },
        TautulliWatchItem {
            title: "Pilot".to_string(),
            media_type: "episode".to_string(),
            grandparent_title: Some("Severance".to_string()),
            parent_title: Some("Season 1".to_string()),
            season_number: Some(1),
            episode_number: Some(1),
            year: Some(2022),
            watched_at: "2026-05-11T21:00:00Z".to_string(),
            duration_ms: Some(3400000),
            rating_key: Some("20202".to_string()),
            thumb_url: None,
            progress_percent: Some(100),
            user: Some("plex_fan_1".to_string()),
            friendly_name: Some("Sync Fan".to_string()),
        },
    ];

    let (imported, skipped) = insert_tautulli_watch_items(&pool, &user_id, &sample_items, None).await.unwrap();
    assert_eq!(imported, 2);
    assert_eq!(skipped, 0);

    // Re-inserting the exact same items should be skipped due to deduplication
    let (imported2, skipped2) = insert_tautulli_watch_items(&pool, &user_id, &sample_items, None).await.unwrap();
    assert_eq!(imported2, 0);
    assert_eq!(skipped2, 2);

    // Verify scrobbles exist in database for this user
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scrobbles WHERE user_id = ?")
        .bind(&user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_user_settings_bidirectional_sync_and_plex_config() {
    let (app, pool, config) = setup_test_app().await;

    // Create user
    let user_id = "test-user-sync-bidirectional".to_string();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, ?)")
        .bind(&user_id)
        .bind("sync_test@bingesync.local")
        .bind("Sync Tester")
        .bind("token-bidirectional-test")
        .bind("user")
        .execute(&pool)
        .await
        .unwrap();

    let token = create_jwt(&user_id, "sync_test@bingesync.local", &config.jwt_secret).unwrap();

    // 1. Initial settings should have bidirectional sync false and plex not configured
    let req = Request::builder()
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(settings["bidirectional_sync_enabled"], false);
    assert_eq!(settings["plex_api_key_configured"], false);
    assert!(settings["plex_url"].is_null());

    // 2. Update settings with Plex credentials and bidirectional sync enabled
    let update_payload = json!({
        "plex_url": "http://192.168.1.100:32400",
        "plex_api_key": "plex-secret-token-xyz123",
        "bidirectional_sync_enabled": true
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. Fetch settings and verify masked secret and enabled status
    let req = Request::builder()
        .uri("/api/v1/user/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(settings["plex_url"], "http://192.168.1.100:32400");
    assert_eq!(settings["plex_api_key_configured"], true);
    assert_eq!(settings["bidirectional_sync_enabled"], true);
    let masked = settings["plex_api_key_masked"].as_str().unwrap();
    assert!(masked.contains('•'));
    assert!(masked.starts_with("pl"));
}

#[tokio::test]
async fn test_media_writeback_endpoints_and_trigger() {
    let (app, pool, config) = setup_test_app().await;

    let user_id = "test-user-writeback".to_string();
    sqlx::query("INSERT INTO users (id, email, display_name, webhook_token, role) VALUES (?, ?, ?, ?, ?)")
        .bind(&user_id)
        .bind("writeback_user@bingesync.local")
        .bind("Writeback Tester")
        .bind("token-writeback-test")
        .bind("user")
        .execute(&pool)
        .await
        .unwrap();

    let token = create_jwt(&user_id, "writeback_user@bingesync.local", &config.jwt_secret).unwrap();

    // 1. Calling writeback without media server configured should return 400
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/sync/writeback")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({}).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 2. Test Plex connection endpoint validation (missing token)
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/settings/test-plex")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "plex_url": "http://192.168.1.100:32400" }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 3. Test Jellyfin connection endpoint validation (missing api key)
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/settings/test-jellyfin")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "jellyfin_url": "http://192.168.1.100:8096" }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 4. Configure dummy Plex settings and insert a completed scrobble
    bingesync::api::user_settings::set_user_setting(&pool, &user_id, "plex_url", "http://127.0.0.1:9").await.unwrap();
    bingesync::api::user_settings::set_user_setting(&pool, &user_id, "plex_api_key", "dummy-plex-token").await.unwrap();

    sqlx::query(
        "INSERT INTO scrobbles (id, user_id, title, media_type, year, watch_status, progress_percent, watched_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))"
    )
    .bind("scrobble-wb-1")
    .bind(&user_id)
    .bind("Inception")
    .bind("movie")
    .bind(2010)
    .bind("completed")
    .bind(100)
    .execute(&pool)
    .await
    .unwrap();

    // 5. Trigger writeback now that a server is configured
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/user/sync/writeback")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "scrobble_id": "scrobble-wb-1" }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result["status"], "success");
    assert_eq!(result["scrobbles_processed"], 1);
}

#[tokio::test]
async fn test_scrobble_source_and_user_logging_and_search() {
    let (app, pool, config) = setup_test_app().await;

    // 1. Create a user
    let user_id = "user-source-test-1".to_string();
    let webhook_token = "token-source-test-1".to_string();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, webhook_token) VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind("multisync@example.com")
    .bind("Family Sync")
    .bind(&webhook_token)
    .execute(&pool)
    .await
    .unwrap();

    // Configure multiple allowed usernames for sync: "daryl, sarah, kids"
    bingesync::api::user_settings::set_user_setting(&pool, &user_id, "plex_username", "daryl, sarah, kids").await.unwrap();
    bingesync::api::user_settings::set_user_setting(&pool, &user_id, "jellyfin_username", "daryl, sarah, kids").await.unwrap();

    let token = create_jwt(&user_id, "multisync@example.com", &config.jwt_secret).unwrap();

    // 2. Plex webhook for "sarah" watching "Moana"
    let plex_payload = json!({
        "event": "media.scrobble",
        "user": true,
        "owner": true,
        "Account": {
            "id": 101,
            "title": "Sarah"
        },
        "Metadata": {
            "type": "movie",
            "title": "Moana",
            "year": 2016,
            "duration": 6420000,
            "viewOffset": 6420000,
            "ratingKey": "1001"
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/plex/{}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(plex_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 3. Jellyfin webhook for "daryl" watching "Severance"
    let jellyfin_payload = json!({
        "NotificationType": "PlaybackStop",
        "NotificationUsername": "daryl",
        "UserId": "jf-user-daryl-1",
        "ItemType": "Episode",
        "Name": "Good News About Hell",
        "SeriesName": "Severance",
        "SeasonNumber": 1,
        "EpisodeNumber": 1,
        "Year": 2022,
        "PlayedToCompletion": true,
        "PlaybackPositionTicks": 36000000000_i64,
        "RunTimeTicks": 36000000000_i64,
        "ItemId": "jf-item-sev-1"
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin/{}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(jellyfin_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 4. Manual scrobble for "kids" watching "Toy Story"
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/history")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "title": "Toy Story",
            "media_type": "movie",
            "year": 1995,
            "source": "manual",
            "source_user": "kids"
        }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 4b. Tautulli import for "daryl" watching "Inception"
    let tautulli_json = json!({
        "response": {
            "result": "success",
            "data": {
                "data": [
                    {
                        "user": "daryl",
                        "media_type": "movie",
                        "title": "Inception",
                        "year": 2010,
                        "date": 1672531199,
                        "duration": 8880,
                        "rating_key": "rk_inc_999",
                        "percent_complete": 100
                    }
                ]
            }
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/import/tautulli")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({
            "username": "daryl",
            "content": tautulli_json.to_string()
        }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Verify records stored source, source_user, and imported_from
    let scrobbles: Vec<bingesync::models::Scrobble> = sqlx::query_as(
        "SELECT * FROM scrobbles WHERE user_id = ? ORDER BY title ASC"
    )
    .bind(&user_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(scrobbles.len(), 4);
    
    // Moana (Plex, Sarah, direct webhook)
    let moana = scrobbles.iter().find(|s| s.title == "Moana").unwrap();
    assert_eq!(moana.source.as_deref(), Some("plex"));
    assert_eq!(moana.source_user.as_deref(), Some("Sarah"));
    assert_eq!(moana.imported_from.as_deref(), None);

    // Inception (Tautulli import -> source is "plex" because Tautulli monitors Plex, imported_from is "tautulli")
    let inc = scrobbles.iter().find(|s| s.title == "Inception").unwrap();
    assert_eq!(inc.source.as_deref(), Some("plex"));
    assert_eq!(inc.source_user.as_deref(), Some("daryl"));
    assert_eq!(inc.imported_from.as_deref(), Some("tautulli"));

    // Severance (Jellyfin, daryl)
    let sev = scrobbles.iter().find(|s| s.title == "Good News About Hell").unwrap();
    assert_eq!(sev.source.as_deref(), Some("jellyfin"));
    assert_eq!(sev.source_user.as_deref(), Some("daryl"));

    // Toy Story (Manual, kids)
    let ts = scrobbles.iter().find(|s| s.title == "Toy Story").unwrap();
    assert_eq!(ts.source.as_deref(), Some("manual"));
    assert_eq!(ts.source_user.as_deref(), Some("kids"));

    // 6. Test search by user who watched it: search=Sarah
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?search=Sarah")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let history: bingesync::models::HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(history.total, 1);
    assert_eq!(history.items[0].title, "Moana");

    // 7. Test filter by source: source=jellyfin
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?source=jellyfin")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let history: bingesync::models::HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(history.total, 1);
    assert_eq!(history.items[0].title, "Good News About Hell");

    // 8. Test filter by source_user / watched_by: source_user=kids
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?source_user=kids")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let history: bingesync::models::HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(history.total, 1);
    assert_eq!(history.items[0].title, "Toy Story");

    // 9. Test history filter by imported_from: imported_from=tautulli
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/history?imported_from=tautulli")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let history: bingesync::models::HistoryResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(history.total, 1);
    assert_eq!(history.items[0].title, "Inception");
    assert_eq!(history.items[0].source.as_deref(), Some("plex"));
    assert_eq!(history.items[0].imported_from.as_deref(), Some("tautulli"));

    // 10. Test timeline search by source: search=plex&all=true (matches both live Plex Moana and Tautulli Plex Inception)
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline?search=plex&all=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let timeline: Vec<bingesync::models::TimelineDay> = serde_json::from_slice(&bytes).unwrap();
    let titles: Vec<&str> = timeline.iter().flat_map(|d| d.items.iter().map(|it| it.title.as_str())).collect();
    assert!(titles.contains(&"Moana"));
    assert!(titles.contains(&"Inception"));

    // 11. Test timeline filter by source: source=jellyfin&all=true
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/timeline?source=jellyfin&all=true")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let timeline: Vec<bingesync::models::TimelineDay> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].items[0].title, "Good News About Hell");

    // 12. Test active session logs source and source_user
    let jf_play = json!({
        "NotificationType": "PlaybackStart",
        "NotificationUsername": "Sarah",
        "UserId": "jf-user-sarah",
        "ItemType": "Movie",
        "Name": "Frozen",
        "Year": 2013,
        "PlaybackPositionTicks": 10000000000_i64,
        "RunTimeTicks": 60000000000_i64,
        "DeviceName": "Living Room TV"
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/webhooks/jellyfin/{}", webhook_token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(jf_play.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/now-playing")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let np: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(np["now_playing"]["source"], "jellyfin");
    assert_eq!(np["now_playing"]["source_user"], "Sarah");
}








