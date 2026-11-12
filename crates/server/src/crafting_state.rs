//! The team-shared tech tree on the server, and the research desk intent.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::{ContentId, ContentRegistry};
use deepcore_shared::crafting::TechTree;
use deepcore_shared::protocol::{ClientIntent, FromClient, ServerNotice, TechState};

use crate::sim::player::{OwnedBy, Player, PlayerState};

/// The team's research progress.
#[derive(Debug, Resource, Default)]
pub struct TechStateResource(pub TechTree);

/// Tracks whether the tech tree changed, so the state is broadcast on change and not per frame.
#[derive(Debug, Resource, Default)]
pub struct TechStateDirty(pub bool);

/// Broadcasts the tech tree whenever a deposit or completion changes it.
pub fn broadcast_on_change(
    mut dirty: ResMut<TechStateDirty>,
    tech: Res<TechStateResource>,
    content: Res<ContentRegistry>,
    mut writer: MessageWriter<ToClients<TechState>>,
) {
    if !dirty.0 {
        return;
    }
    dirty.0 = false;
    writer.write(ToClients {
        targets: SendTargets::All,
        message: tech_state_message(&tech.0, &content),
    });
}

/// The message describing the tech tree.
pub fn tech_state_message(tech: &TechTree, content: &ContentRegistry) -> TechState {
    let mut deposits = Vec::new();
    for node in tech.researched().iter().copied().chain(
        content
            .tech
            .iter()
            .map(|node| node.id)
            .filter(|node| !tech.is_researched(*node)),
    ) {
        if let Some(def) = content.tech(node) {
            for (item, _count) in &def.cost {
                let delivered = tech.deposited(node, *item);
                if delivered > 0 {
                    deposits.push((node, *item, delivered));
                }
            }
        }
    }
    TechState {
        researched: tech.researched().to_vec(),
        deposits,
    }
}

/// Sends the tech tree to a client (on join, and on every change).
pub fn broadcast_tech_state(
    tech: Res<TechStateResource>,
    content: Res<ContentRegistry>,
    mut writer: MessageWriter<ToClients<TechState>>,
) {
    writer.write(ToClients {
        targets: SendTargets::All,
        message: tech_state_message(&tech.0, &content),
    });
}

/// Handles `ResearchDeposit`.
pub fn handle_research(
    mut intents: MessageReader<FromClient<ClientIntent>>,
    mut tech: ResMut<TechStateResource>,
    mut players: Query<&mut PlayerState, With<Player>>,
    clients: Query<&OwnedBy>,
    content: Res<ContentRegistry>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
    mut dirty: ResMut<TechStateDirty>,
) {
    for FromClient { client_id, message } in intents.read() {
        let ClientIntent::ResearchDeposit { node, items } = message else {
            continue;
        };
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let Ok(owner) = clients.get(client_entity) else {
            continue;
        };
        let Ok(mut state) = players.get_mut(owner.0) else {
            continue;
        };
        match tech.0.deposit(&content, *node, items, &mut state.inventory) {
            Ok(completed) => {
                let name = content
                    .tech(*node)
                    .map_or("research", |tech| tech.name.as_str())
                    .to_owned();
                let text = if completed {
                    format!("Research complete: {name}")
                } else {
                    format!("Delivered items to {name}")
                };
                notices.write(ToClients {
                    targets: SendTargets::All,
                    message: ServerNotice::Notification { text, severity: 0 },
                });
                dirty.0 = true;
            }
            Err(reason) => {
                notices.write(ToClients {
                    targets: SendTargets::Single(*client_id),
                    message: ServerNotice::Notification {
                        text: reason,
                        severity: 1,
                    },
                });
            }
        }
    }
}

/// Whether a tech node is researched, for the recipe gates.
pub fn is_researched(tech: &TechTree, node: ContentId) -> bool {
    tech.is_researched(node)
}
