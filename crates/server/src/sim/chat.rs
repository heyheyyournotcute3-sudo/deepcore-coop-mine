//! Chat and system notifications.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::protocol::{ChatLine, ClientIntent, FromClient, PlayerName};

use crate::sim::player::{OwnedBy, Player};

/// Handles the `Chat` intent: validates the text and broadcasts it.
pub fn handle_chat(
    mut intents: MessageReader<FromClient<ClientIntent>>,
    players: Query<(&PlayerName, &OwnedBy), With<Player>>,
    mut lines: MessageWriter<ToClients<ChatLine>>,
) {
    for FromClient { client_id, message } in intents.read() {
        let ClientIntent::Chat { text } = message else {
            continue;
        };
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let from = players
            .iter()
            .find(|(_, owner)| owner.0 == client_entity)
            .map(|(name, _)| name.0.clone())
            .unwrap_or_else(|| "Unknown".to_owned());
        let cleaned: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
        if cleaned.trim().is_empty() {
            continue;
        }
        lines.write(ToClients {
            targets: SendTargets::All,
            message: ChatLine {
                from,
                text: cleaned,
            },
        });
    }
}
