//! The server browser and login screen.
//!
//! Talks to the lobby service over HTTP: register, log in, list servers, then hand the chosen
//! address to the connection settings. Everything is best effort — with no lobby configured the
//! client falls back to direct IP join.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::ui::Screen;
use crate::ui::UiState;

/// Registers the browser.
pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ServerBrowser>()
            .add_systems(Update, (refresh_servers, apply_join_request));
    }
}

/// A server as the lobby reports it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserEntry {
    /// Registration id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// `host:port`.
    pub address: String,
    /// Current players.
    pub players: u32,
    /// Capacity.
    pub max_players: u32,
    /// Whether an account is required.
    pub requires_account: bool,
}

/// The browser's state.
#[derive(Debug, Resource, Default)]
pub struct ServerBrowser {
    /// Lobby base URL.
    pub lobby_url: String,
    /// Servers as last fetched.
    pub servers: Vec<BrowserEntry>,
    /// The account token, once logged in.
    pub token: Option<String>,
    /// The logged-in username.
    pub username: Option<String>,
    /// The address the player asked to join.
    pub join_request: Option<String>,
    /// A message to show in the browser.
    pub status: String,
    /// Seconds since the last refresh.
    pub since_refresh: f32,
}

impl ServerBrowser {
    /// Whether the browser has a lobby to talk to.
    pub fn enabled(&self) -> bool {
        !self.lobby_url.is_empty()
    }

    /// Fetches the server list from the lobby.
    pub fn fetch_servers(&mut self) {
        if !self.enabled() {
            self.status = "No lobby configured — join by IP".to_owned();
            return;
        }
        let url = format!("{}/api/servers", self.lobby_url.trim_end_matches('/'));
        match http_get_json::<Vec<BrowserEntry>>(&url) {
            Ok(servers) => {
                self.status = format!("{} servers online", servers.len());
                self.servers = servers;
            }
            Err(error) => {
                warn!("cannot fetch the server list: {error}");
                self.status = "Cannot reach the lobby".to_owned();
            }
        }
    }

    /// Logs in and stores the token.
    pub fn login(&mut self, username: &str, password: &str) -> bool {
        if !self.enabled() {
            self.status = "No lobby configured".to_owned();
            return false;
        }
        let url = format!("{}/api/login", self.lobby_url.trim_end_matches('/'));
        match http_post_json::<TokenReply>(
            &url,
            &serde_json::json!({
                "username": username,
                "password": password,
            }),
        ) {
            Ok(reply) => {
                self.token = Some(reply.token);
                self.username = Some(reply.username.clone());
                self.status = format!("Logged in as {}", reply.username);
                info!("logged in as {}", reply.username);
                true
            }
            Err(error) => {
                self.status = format!("Login failed: {error}");
                false
            }
        }
    }

    /// Registers a new account and logs in.
    pub fn register(&mut self, username: &str, password: &str) -> bool {
        if !self.enabled() {
            self.status = "No lobby configured".to_owned();
            return false;
        }
        let url = format!("{}/api/register", self.lobby_url.trim_end_matches('/'));
        match http_post_json::<TokenReply>(
            &url,
            &serde_json::json!({
                "username": username,
                "password": password,
            }),
        ) {
            Ok(reply) => {
                self.token = Some(reply.token);
                self.username = Some(reply.username.clone());
                self.status = format!("Account created: {}", reply.username);
                true
            }
            Err(error) => {
                self.status = format!("Registration failed: {error}");
                false
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct TokenReply {
    token: String,
    username: String,
}

/// Refreshes the list every 15 seconds while the menu is open.
fn refresh_servers(time: Res<Time>, mut browser: ResMut<ServerBrowser>, ui: Res<UiState>) {
    if !browser.enabled() || ui.screen != Screen::MainMenu {
        return;
    }
    browser.since_refresh += time.delta_secs();
    if browser.since_refresh < 15.0 && !browser.servers.is_empty() {
        return;
    }
    browser.since_refresh = 0.0;
    browser.fetch_servers();
}

/// Turns a join request into a connection.
fn apply_join_request(
    mut browser: ResMut<ServerBrowser>,
    mut settings: ResMut<crate::ConnectionSettings>,
    mut ui: ResMut<UiState>,
) {
    let Some(address) = browser.join_request.take() else {
        return;
    };
    settings.connect_address = Some(address.clone());
    browser.status = format!("Joining {address}");
    ui.screen = Screen::Playing;
    info!("joining {address}");
}

fn http_get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(5)))
        .build();
    let agent: ureq::Agent = agent.into();
    let mut response = agent.get(url).call().map_err(|error| error.to_string())?;
    response
        .body_mut()
        .read_json::<T>()
        .map_err(|error| error.to_string())
}

fn http_post_json<T: serde::de::DeserializeOwned>(
    url: &str,
    body: &serde_json::Value,
) -> Result<T, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(5)))
        .build();
    let agent: ureq::Agent = agent.into();
    let mut response = agent
        .post(url)
        .send_json(body)
        .map_err(|error| error.to_string())?;
    response
        .body_mut()
        .read_json::<T>()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_without_a_lobby_is_disabled() {
        let browser = ServerBrowser::default();
        assert!(!browser.enabled());
    }

    #[test]
    fn browser_with_a_lobby_is_enabled() {
        let browser = ServerBrowser {
            lobby_url: "http://localhost:24289".to_owned(),
            ..Default::default()
        };
        assert!(browser.enabled());
    }
}
