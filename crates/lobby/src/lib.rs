//! Accounts, session tokens and the server browser for Deepcore.
//!
//! This is a small web service, separate from the game servers:
//!
//! * players register and log in here and receive a session token;
//! * dedicated servers register here and send a heartbeat with their player count;
//! * clients ask for the server list, and game servers ask the lobby to verify a token.
//!
//! Passwords are hashed with argon2id. Tokens are opaque random strings stored hashed, so a
//! database leak does not leak live sessions. Login is rate limited per username and per IP.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use deepcore_shared::protocol::DEFAULT_LOBBY_PORT;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

/// Command line options.
#[derive(Debug, Parser)]
#[command(
    name = "deepcore-lobby",
    about = "Deepcore accounts and server browser"
)]
pub struct LobbyArgs {
    /// Port to listen on.
    #[arg(long, default_value_t = DEFAULT_LOBBY_PORT)]
    pub port: u16,
    /// SQLite database path (`sqlite://...`), Postgres-ready.
    #[arg(long, default_value = "sqlite://lobby.sqlite?mode=rwc")]
    pub database: String,
    /// Token lifetime in hours.
    #[arg(long, default_value_t = 24 * 14)]
    pub token_hours: u64,
}

/// Shared service state.
pub struct Lobby {
    /// Database pool.
    pub db: SqlitePool,
    /// Token lifetime.
    pub token_hours: u64,
    /// Failed login attempts per key, for rate limiting.
    pub attempts: Mutex<HashMap<String, (u32, Instant)>>,
}

/// A server registered in the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEntry {
    /// Registration id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Public address, `host:port`.
    pub address: String,
    /// Current player count.
    pub players: u32,
    /// Maximum players.
    pub max_players: u32,
    /// Seconds since the last heartbeat.
    pub age_seconds: u64,
    /// Whether the server requires a lobby account.
    pub requires_account: bool,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct TokenResponse {
    token: String,
    username: String,
    expires_in_hours: u64,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct VerifyRequest {
    token: String,
}

#[derive(Debug, Serialize)]
struct VerifyResponse {
    valid: bool,
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegisterServerRequest {
    name: String,
    address: String,
    max_players: u32,
    requires_account: bool,
}

#[derive(Debug, Deserialize)]
struct HeartbeatRequest {
    id: String,
    players: u32,
}

/// Error response body.
#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

/// A service error that maps onto an HTTP status.
#[derive(Debug)]
pub enum LobbyError {
    /// 400
    BadRequest(String),
    /// 401
    Unauthorized(String),
    /// 409
    Conflict(String),
    /// 429
    TooManyRequests,
    /// 500
    Internal(String),
}

impl IntoResponse for LobbyError {
    fn into_response(self) -> axum::response::Response {
        let (status, error) = match self {
            LobbyError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            LobbyError::Unauthorized(message) => (StatusCode::UNAUTHORIZED, message),
            LobbyError::Conflict(message) => (StatusCode::CONFLICT, message),
            LobbyError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "too many attempts, try again later".to_owned(),
            ),
            LobbyError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(ErrorBody { error })).into_response()
    }
}

impl From<sqlx::Error> for LobbyError {
    fn from(error: sqlx::Error) -> Self {
        warn!("database error: {error}");
        LobbyError::Internal("database error".into())
    }
}

/// Builds the router.
pub fn router(lobby: Arc<Lobby>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/verify", post(verify))
        .route("/api/servers", get(list_servers))
        .route("/api/servers/register", post(register_server))
        .route("/api/servers/heartbeat", post(heartbeat))
        .route("/api/servers/{id}", axum::routing::delete(remove_server))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(lobby)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "service": "deepcore-lobby" }))
}

fn validate_credentials(username: &str, password: &str) -> Result<(), LobbyError> {
    if username.len() < 3 || username.len() > 24 {
        return Err(LobbyError::BadRequest(
            "username must be 3-24 characters".into(),
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(LobbyError::BadRequest(
            "username may only contain letters, digits and underscore".into(),
        ));
    }
    if password.len() < 8 {
        return Err(LobbyError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, LobbyError> {
    // Use argon2's own `rand_core` so the RNG version matches what `SaltString` expects.
    let mut rng = rand_core::OsRng;
    let salt = SaltString::generate(&mut rng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| LobbyError::Internal(format!("cannot hash password: {error}")))
}

fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

fn random_token() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 32] = rng.random();
    let mut token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    // A per-process counter guarantees uniqueness even if two calls land in the same
    // millisecond and the RNG is seeded identically (e.g. two processes started together).
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    token.push_str(&format!("{count:08x}"));
    token
}

/// Hashes a token for storage, so the database never holds a usable token.
fn token_digest(token: &str) -> String {
    format!("{:016x}", deepcore_shared::stable_hash(token.as_bytes()))
}

fn rate_limit(lobby: &Lobby, key: &str) -> Result<(), LobbyError> {
    let mut attempts = lobby
        .attempts
        .lock()
        .map_err(|_| LobbyError::Internal("rate limiter unavailable".into()))?;
    let now = Instant::now();
    let entry = attempts.entry(key.to_owned()).or_insert((0, now));
    if now.duration_since(entry.1) > Duration::from_secs(60) {
        *entry = (0, now);
    }
    entry.0 += 1;
    if entry.0 > 10 {
        return Err(LobbyError::TooManyRequests);
    }
    Ok(())
}

async fn register(
    State(lobby): State<Arc<Lobby>>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<TokenResponse>, LobbyError> {
    validate_credentials(&request.username, &request.password)?;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT username FROM accounts WHERE username = ?")
            .bind(&request.username)
            .fetch_optional(&lobby.db)
            .await?;
    if existing.is_some() {
        return Err(LobbyError::Conflict("that username is taken".into()));
    }
    let hash = hash_password(&request.password)?;
    sqlx::query("INSERT INTO accounts (username, password_hash) VALUES (?, ?)")
        .bind(&request.username)
        .bind(&hash)
        .execute(&lobby.db)
        .await?;
    let token = random_token();
    let digest = token_digest(&token);
    sqlx::query("INSERT INTO sessions (token_digest, username) VALUES (?, ?)")
        .bind(&digest)
        .bind(&request.username)
        .execute(&lobby.db)
        .await?;
    info!("registered {}", request.username);
    Ok(Json(TokenResponse {
        token,
        username: request.username,
        expires_in_hours: lobby.token_hours,
    }))
}

async fn login(
    State(lobby): State<Arc<Lobby>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, LobbyError> {
    // Rate limit per client address when a proxy forwards one, otherwise per username only.
    let ip_key = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .map(|value| format!("ip:{value}"))
        .unwrap_or_else(|| "ip:unknown".to_owned());
    rate_limit(&lobby, &ip_key)?;
    rate_limit(&lobby, &format!("user:{}", request.username))?;
    let row = sqlx::query("SELECT username, password_hash FROM accounts WHERE username = ?")
        .bind(&request.username)
        .fetch_optional(&lobby.db)
        .await?;
    let Some(row) = row else {
        return Err(LobbyError::Unauthorized(
            "invalid username or password".into(),
        ));
    };
    let hash: String = row.get("password_hash");
    if !verify_password(&request.password, &hash) {
        return Err(LobbyError::Unauthorized(
            "invalid username or password".into(),
        ));
    }
    let token = random_token();
    let digest = token_digest(&token);
    sqlx::query("INSERT INTO sessions (token_digest, username) VALUES (?, ?)")
        .bind(&digest)
        .bind(&request.username)
        .execute(&lobby.db)
        .await?;
    info!("login from {}", request.username);
    Ok(Json(TokenResponse {
        token,
        username: request.username,
        expires_in_hours: lobby.token_hours,
    }))
}

async fn verify(
    State(lobby): State<Arc<Lobby>>,
    Json(request): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, LobbyError> {
    let digest = token_digest(&request.token);
    let username: Option<String> =
        sqlx::query_scalar("SELECT username FROM sessions WHERE token_digest = ?")
            .bind(&digest)
            .fetch_optional(&lobby.db)
            .await?;
    Ok(Json(VerifyResponse {
        valid: username.is_some(),
        username,
    }))
}

async fn register_server(
    State(lobby): State<Arc<Lobby>>,
    Json(request): Json<RegisterServerRequest>,
) -> Result<Json<ServerEntry>, LobbyError> {
    if request.name.trim().is_empty() || request.address.trim().is_empty() {
        return Err(LobbyError::BadRequest(
            "name and address are required".into(),
        ));
    }
    let id = random_token()[..16].to_owned();
    sqlx::query(
        "INSERT INTO servers (id, name, address, max_players, requires_account) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&request.name)
    .bind(&request.address)
    .bind(request.max_players as i64)
    .bind(request.requires_account)
    .execute(&lobby.db)
    .await?;
    info!("registered server {} at {}", request.name, request.address);
    Ok(Json(ServerEntry {
        id,
        name: request.name,
        address: request.address,
        players: 0,
        max_players: request.max_players,
        age_seconds: 0,
        requires_account: request.requires_account,
    }))
}

async fn heartbeat(
    State(lobby): State<Arc<Lobby>>,
    Json(request): Json<HeartbeatRequest>,
) -> Result<Json<serde_json::Value>, LobbyError> {
    let updated =
        sqlx::query("UPDATE servers SET players = ?, last_seen = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(request.players as i64)
            .bind(&request.id)
            .execute(&lobby.db)
            .await?;
    if updated.rows_affected() == 0 {
        return Err(LobbyError::Unauthorized("unknown server id".into()));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn remove_server(
    State(lobby): State<Arc<Lobby>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, LobbyError> {
    sqlx::query("DELETE FROM servers WHERE id = ?")
        .bind(&id)
        .execute(&lobby.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn list_servers(
    State(lobby): State<Arc<Lobby>>,
) -> Result<Json<Vec<ServerEntry>>, LobbyError> {
    // A server that has not sent a heartbeat for two minutes is considered gone.
    let rows = sqlx::query(
        "SELECT id, name, address, players, max_players, requires_account,
                CAST((julianday('now') - julianday(last_seen)) * 86400 AS INTEGER) AS age
         FROM servers
         WHERE (julianday('now') - julianday(last_seen)) * 86400 < 120
         ORDER BY players DESC, name ASC",
    )
    .fetch_all(&lobby.db)
    .await?;
    let mut servers = Vec::with_capacity(rows.len());
    for row in rows {
        servers.push(ServerEntry {
            id: row.get("id"),
            name: row.get("name"),
            address: row.get("address"),
            players: row.get::<i64, _>("players").max(0) as u32,
            max_players: row.get::<i64, _>("max_players").max(0) as u32,
            age_seconds: row.get::<i64, _>("age").max(0) as u64,
            requires_account: row.get("requires_account"),
        });
    }
    Ok(Json(servers))
}

/// Creates the schema. Written for SQLite; the statements avoid dialect-specific syntax so a
/// Postgres pool can run the same shape with a different connection string.
pub async fn init_schema(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS accounts (
            username TEXT PRIMARY KEY,
            password_hash TEXT NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            token_digest TEXT PRIMARY KEY,
            username TEXT NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS servers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            address TEXT NOT NULL,
            players INTEGER NOT NULL DEFAULT 0,
            max_players INTEGER NOT NULL DEFAULT 16,
            requires_account INTEGER NOT NULL DEFAULT 0,
            last_seen TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Opens the database and initialises the schema.
pub async fn open_database(url: &str) -> anyhow::Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect(url)
        .await?;
    init_schema(&pool).await?;
    Ok(pool)
}

/// Runs the lobby service.
pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,deepcore_lobby=debug".into()),
        )
        .init();
    let args = LobbyArgs::parse();
    let db = open_database(&args.database).await?;
    let lobby = Arc::new(Lobby {
        db,
        token_hours: args.token_hours,
        attempts: Mutex::new(HashMap::new()),
    });
    let app = router(lobby);
    let address = SocketAddr::from(([0, 0, 0, 0], args.port));
    info!("lobby listening on {address}");
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
