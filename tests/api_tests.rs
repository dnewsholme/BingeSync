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
    assert_eq!(found_standard.scrobble_count, 1);

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



