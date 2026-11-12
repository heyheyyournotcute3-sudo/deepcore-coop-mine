//! Lobby integration tests: register, log in, verify tokens, and the server browser.
//!
//! These run the real router against an in-memory SQLite database, so the hashing, the token
//! storage and the heartbeat expiry logic are all exercised.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use deepcore_lobby::{Lobby, open_database, router};
use tower::ServiceExt;

/// Builds the test lobby inside a Tokio runtime.
///
/// `sqlx` pools need a runtime, and the in-memory database lives as long as the pool, so the
/// runtime is kept alive for the whole test.
fn test_lobby() -> (tokio::runtime::Runtime, Arc<Lobby>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let db = runtime
        .block_on(open_database("sqlite::memory:"))
        .expect("in-memory database");
    let lobby = Arc::new(Lobby {
        db,
        token_hours: 24,
        attempts: Mutex::new(HashMap::new()),
    });
    (runtime, lobby)
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body bytes");
    serde_json::from_slice(&bytes).expect("json body")
}

#[test]
fn register_login_and_verify() {
    let (runtime, lobby) = test_lobby();
    let app = router(lobby);

    runtime.block_on(async move {
        // Register.
        let request = Request::builder()
            .method("POST")
            .uri("/api/register")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"username":"miner_one","password":"deepcore123"}"#,
            ))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("register");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        let token = body["token"].as_str().expect("token").to_owned();
        assert_eq!(body["username"], "miner_one");

        // The same username cannot be taken twice.
        let request = Request::builder()
            .method("POST")
            .uri("/api/register")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"username":"miner_one","password":"deepcore123"}"#,
            ))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("duplicate");
        assert_eq!(response.status(), StatusCode::CONFLICT);

        // Log in with the right password.
        let request = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"username":"miner_one","password":"deepcore123"}"#,
            ))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("login");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        let login_token = body["token"].as_str().expect("token").to_owned();
        assert_ne!(login_token, token, "each login mints a fresh token");

        // A wrong password is refused.
        let request = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"username":"miner_one","password":"wrongpassword"}"#,
            ))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("bad login");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // Tokens verify.
        let request = Request::builder()
            .method("POST")
            .uri("/api/verify")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"token":"{login_token}"}}"#)))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("verify");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["valid"], true);
        assert_eq!(body["username"], "miner_one");

        // A made-up token does not.
        let request = Request::builder()
            .method("POST")
            .uri("/api/verify")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"token":"deadbeef"}"#))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("verify bad");
        let body = body_json(response).await;
        assert_eq!(body["valid"], false);
    });
}

#[test]
fn weak_credentials_are_rejected() {
    let (runtime, lobby) = test_lobby();
    let app = router(lobby);
    runtime.block_on(async move {
        for body in [
            r#"{"username":"ab","password":"deepcore123"}"#,
            r#"{"username":"has spaces","password":"deepcore123"}"#,
            r#"{"username":"miner_two","password":"short"}"#,
        ] {
            let request = Request::builder()
                .method("POST")
                .uri("/api/register")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("register");
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "body {body}");
        }
    });
}

#[test]
fn server_browser_registers_and_heartbeats() {
    let (runtime, lobby) = test_lobby();
    let app = router(lobby);
    runtime.block_on(async move {
        // Register a dedicated server.
        let request = Request::builder()
            .method("POST")
            .uri("/api/servers/register")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"name":"Co-op Mine","address":"1.2.3.4:24288","max_players":16,"requires_account":true}"#,
            ))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("register server");
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        let id = body["id"].as_str().expect("server id").to_owned();

        // It appears in the list.
        let request = Request::builder()
            .uri("/api/servers")
            .body(Body::empty())
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("list");
        let body = body_json(response).await;
        let servers = body.as_array().expect("array");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0]["name"], "Co-op Mine");
        assert_eq!(servers[0]["requires_account"], true);

        // A heartbeat updates the player count.
        let request = Request::builder()
            .method("POST")
            .uri("/api/servers/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"id":"{id}","players":5}}"#)))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("heartbeat");
        assert_eq!(response.status(), StatusCode::OK);

        let request = Request::builder()
            .uri("/api/servers")
            .body(Body::empty())
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("list");
        let body = body_json(response).await;
        assert_eq!(body[0]["players"], 5);

        // An unknown server id is refused.
        let request = Request::builder()
            .method("POST")
            .uri("/api/servers/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"id":"nope","players":1}"#))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("bad heartbeat");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    });
}

#[test]
fn tokens_are_not_stored_in_plain_text() {
    let (runtime, lobby) = test_lobby();
    let db = lobby.db.clone();
    let app = router(lobby);
    runtime.block_on(async move {
        let request = Request::builder()
            .method("POST")
            .uri("/api/register")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"username":"miner_three","password":"deepcore123"}"#,
            ))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("register");
        let body = body_json(response).await;
        let token = body["token"].as_str().expect("token").to_owned();

        // The raw token must not appear anywhere in the sessions table.
        let digests: Vec<String> = sqlx::query_scalar("SELECT token_digest FROM sessions")
            .fetch_all(&db)
            .await
            .expect("query");
        assert!(!digests.is_empty());
        assert!(
            digests.iter().all(|digest| digest != &token),
            "the raw token was stored"
        );

        // The password hash is argon2, not the password.
        let hash: String = sqlx::query_scalar("SELECT password_hash FROM accounts")
            .fetch_one(&db)
            .await
            .expect("query");
        assert!(
            hash.starts_with("$argon2"),
            "unexpected hash format: {hash}"
        );
        assert!(!hash.contains("deepcore123"));
    });
}
