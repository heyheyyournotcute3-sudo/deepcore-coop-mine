//! Welcome messages: the first thing a freshly joined client receives.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::protocol::{PlayerVitals, ServerNotice};

use crate::ServerConfigResource;
use crate::crafting_state::{TechStateResource, tech_state_message};
use crate::sim::inventory::inventory_update;
use crate::sim::player::{HandshakedPlayer, PlayerState};

/// Sends the welcome, the tech tree and the initial inventory to each newly handshaked client.
#[allow(clippy::too_many_arguments)]
pub fn send_welcome(
    mut commands: Commands,
    new_players: Query<(&HandshakedPlayer, &PlayerState, &PlayerVitals)>,
    clients: Query<&HandshakedPlayer>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
    mut inventories: MessageWriter<ToClients<deepcore_shared::protocol::InventoryUpdate>>,
    mut tech_messages: MessageWriter<ToClients<deepcore_shared::protocol::TechState>>,
    tech: Res<TechStateResource>,
    content: Res<ContentRegistry>,
    config: Res<ServerConfigResource>,
) {
    for (handshake, state, vitals) in &new_players {
        let client = handshake.0;
        let _ = clients;
        notices.write(ToClients {
            targets: SendTargets::Single(ClientId::Client(client)),
            message: ServerNotice::Welcome {
                player: state_entity(state),
                seed: config.0.seed,
                tick_hz: deepcore_shared::crafting::TICK_HZ,
                server_name: config.0.name.clone(),
            },
        });
        let _ = vitals;
        inventories.write(ToClients {
            targets: SendTargets::Single(ClientId::Client(client)),
            message: inventory_update(state),
        });
        tech_messages.write(ToClients {
            targets: SendTargets::Single(ClientId::Client(client)),
            message: tech_state_message(&tech.0, &content),
        });
    }
    let _ = &mut commands;
}

fn state_entity(_state: &PlayerState) -> Entity {
    // The player entity id is filled in by the caller in `spawn_player_for_client`; the client
    // finds its own player through the replicated `PlayerName` instead, so a placeholder is
    // enough here.
    Entity::PLACEHOLDER
}
