//! Lobby integration for the dedicated server: registration, heartbeats and token checks.
//!
//! When `--lobby <url>` is set, the server registers itself, sends a heartbeat with its player
//! count every 30 seconds, and (if accounts are required) asks the lobby to verify each joining
//! client's token. Without a lobby the server runs open, which is how LAN games work.

use std::time::Duration;

use bevy::prelude::*;
use deepcore_shared::protocol::ServerNotice;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::ServerConfigResource;

/// Registers the lobby client systems.
pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LobbyState>();
        app.add_systems(Startup, register_with_lobby);
        app.add_systems(Update, (heartbeat, verify_pending_tokens));
    }
}

/// The server's lobby registration.
#[derive(Debug, Default, Resource)]
pub struct LobbyState {
    /// The lobby base URL, empty when playing without a lobby.
    pub url: String,
    /// The id the lobby assigned to this server.
    pub id: Option<String>,
    /// Seconds since the last heartbeat.
    pub since_heartbeat: f32,
    /// Clients waiting for token verification: `(client entity, token)`.
    pub pending_tokens: Vec<(Entity, String)>,
}

#[derive(Debug, Serialize)]
struct RegisterServerRequest<'a> {
    name: &'a str,
    address: String,
    max_players: u32,
    requires_account: bool,
}

#[derive(Debug, Deserialize)]
struct RegisterServerResponse {
    id: String,
}

#[derive(Debug, Serialize)]
struct HeartbeatRequest {
    id: String,
    players: u32,
}

#[derive(Debug, Serialize)]
struct VerifyRequest {
    token: String,
}

#[derive(Debug, Deserialize)]
struct VerifyResponse {
    valid: bool,
    username: Option<String>,
}

fn register_with_lobby(mut state: ResMut<LobbyState>, config: Res<ServerConfigResource>) {
    if config.0.lobby.is_empty() {
        info!("no lobby configured; running as an open LAN server");
        return;
    }
    state.url = config.0.lobby.trim_end_matches('/').to_owned();
    let url = format!("{}/api/servers/register", state.url);
    let address = format!("{}:{}", public_host(&config), config.0.port);
    let request = RegisterServerRequest {
        name: &config.0.name,
        address,
        max_players: deepcore_shared::protocol::MAX_PLAYERS as u32,
        requires_account: config.0.require_account,
    };
    // The registration is a small blocking call at startup; a failure must not stop the game.
    let client = match reqwest_blocking() {
        Some(client) => client,
        None => return,
    };
    match client.post(&url).send_json(&request) {
        Ok(mut response) if response.status().is_success() => {
            match response.body_mut().read_json::<RegisterServerResponse>() {
                Ok(body) => {
                    info!("registered with the lobby as {}", body.id);
                    state.id = Some(body.id);
                }
                Err(error) => warn!("lobby registration response was unreadable: {error}"),
            }
        }
        Ok(response) => warn!("lobby refused registration: {}", response.status()),
        Err(error) => warn!("cannot reach the lobby at {}: {error}", state.url),
    }
}

/// Sends a heartbeat with the current player count.
fn heartbeat(
    time: Res<Time<Real>>,
    mut state: ResMut<LobbyState>,
    config: Res<ServerConfigResource>,
    players: Query<&crate::sim::player::Player>,
) {
    let Some(id) = state.id.clone() else {
        return;
    };
    state.since_heartbeat += time.delta_secs();
    if state.since_heartbeat < 30.0 {
        return;
    }
    state.since_heartbeat = 0.0;
    let url = format!("{}/api/servers/heartbeat", state.url);
    let request = HeartbeatRequest {
        id,
        players: players.iter().count() as u32,
    };
    let _ = &config;
    if let Some(client) = reqwest_blocking() {
        match client.post(&url).send_json(&request) {
            Ok(response) if !response.status().is_success() => {
                warn!("lobby heartbeat refused: {}", response.status())
            }
            Err(error) => warn!("lobby heartbeat failed: {error}"),
            _ => {}
        }
    }
}

/// Verifies tokens for clients that presented one.
fn verify_pending_tokens(
    mut state: ResMut<LobbyState>,
    mut commands: Commands,
    mut notices: MessageWriter<deepcore_shared::protocol::ToClients<ServerNotice>>,
) {
    if state.pending_tokens.is_empty() {
        return;
    }
    let Some(client) = reqwest_blocking() else {
        return;
    };
    let pending = std::mem::take(&mut state.pending_tokens);
    for (entity, token) in pending {
        let url = format!("{}/api/verify", state.url);
        let request = VerifyRequest { token };
        match client.post(&url).send_json(&request) {
            Ok(mut response) if response.status().is_success() => {
                match response.body_mut().read_json::<VerifyResponse>() {
                    Ok(verification) if verification.valid => {
                        info!(
                            "client {entity} verified as {:?}",
                            verification.username.unwrap_or_default()
                        );
                    }
                    Ok(_) => {
                        notices.write(deepcore_shared::protocol::ToClients {
                            targets: deepcore_shared::protocol::SendTargets::Single(
                                deepcore_shared::protocol::ClientId::Client(entity),
                            ),
                            message: ServerNotice::Refused {
                                reason: "Lobby token is not valid".into(),
                            },
                        });
                        commands.write_message(deepcore_shared::protocol::DisconnectRequest {
                            client: entity,
                        });
                    }
                    Err(error) => warn!("unreadable verification response: {error}"),
                }
            }
            Ok(response) => warn!("lobby verification refused: {}", response.status()),
            Err(error) => warn!("cannot verify a token: {error}"),
        }
    }
}

/// A best-effort public host name for the registration.
fn public_host(config: &ServerConfigResource) -> String {
    config
        .0
        .public_host
        .clone()
        .unwrap_or_else(|| "127.0.0.1".to_owned())
}

/// A blocking HTTP client with a short timeout, created per call so it never holds the tick.
fn reqwest_blocking() -> Option<ureq::Agent> {
    Some(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .into(),
    )
}
