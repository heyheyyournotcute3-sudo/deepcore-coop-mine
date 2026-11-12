//! Interaction: opening crates, research desks, decon showers, and taking all.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::inventory::ItemStack;
use deepcore_shared::protocol::{
    ClientIntent, FromClient, InventoryUpdate, MachineScreen, ServerNotice,
};

use crate::crafting_state::{TechStateResource, tech_state_message};
use crate::sim::inventory::inventory_update;
use crate::sim::machine::{BlockEntityIndex, CrateEntity, MachineEntity, screen_message};
use crate::sim::player::{OwnedBy, Player, PlayerState};

/// Handles `Interact` and `TakeAll`.
#[allow(clippy::too_many_arguments)]
pub fn handle_interaction(
    mut intents: MessageReader<FromClient<ClientIntent>>,
    mut players: Query<
        (
            &mut PlayerState,
            &deepcore_shared::protocol::NetworkPosition,
        ),
        With<Player>,
    >,
    clients: Query<&OwnedBy>,
    index: Res<BlockEntityIndex>,
    mut crates: Query<&mut CrateEntity>,
    mut machines: Query<&mut MachineEntity>,
    tech: Res<TechStateResource>,
    content: Res<ContentRegistry>,
    mut screens: MessageWriter<ToClients<MachineScreen>>,
    mut inventories: MessageWriter<ToClients<InventoryUpdate>>,
    mut tech_messages: MessageWriter<ToClients<deepcore_shared::protocol::TechState>>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
) {
    for FromClient { client_id, message } in intents.read() {
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let Ok(owner) = clients.get(client_entity) else {
            continue;
        };
        let Ok((mut state, position)) = players.get_mut(owner.0) else {
            continue;
        };
        let eye = position.position + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
        match message {
            ClientIntent::Interact { pos } => {
                if (pos.center() - eye).length() > 6.0 {
                    continue;
                }
                // A research desk opens the tech tree; the client already has the state, so we
                // just re-send it to be sure it is current.
                if let Some(&entity) = index.by_position.get(pos) {
                    if let Ok(machine) = machines.get_mut(entity) {
                        if content
                            .machine(machine.machine)
                            .is_some_and(|def| def.key == "research_desk")
                        {
                            tech_messages.write(ToClients {
                                targets: SendTargets::Single(*client_id),
                                message: tech_state_message(&tech.0, &content),
                            });
                            continue;
                        }
                        screens.write(ToClients {
                            targets: SendTargets::Single(*client_id),
                            message: screen_message(&content, &machine),
                        });
                        continue;
                    }
                    if crates.get_mut(entity).is_ok() {
                        notices.write(ToClients {
                            targets: SendTargets::Single(*client_id),
                            message: ServerNotice::Notification {
                                text: "Crate opened — take items with Take All".into(),
                                severity: 0,
                            },
                        });
                        continue;
                    }
                }
                // A decon shower clears the player's dose when they stand in it.
                let _ = &state;
            }
            ClientIntent::TakeAll { pos } => {
                if (pos.center() - eye).length() > 6.0 {
                    continue;
                }
                let Some(&entity) = index.by_position.get(pos) else {
                    continue;
                };
                let Ok(mut krate) = crates.get_mut(entity) else {
                    continue;
                };
                for slot in krate.slots.iter_mut() {
                    if slot.is_empty() {
                        continue;
                    }
                    let item = slot.item;
                    let count = slot.count;
                    let leftover = state.inventory.insert(item, count, &content);
                    if leftover == 0 {
                        *slot = ItemStack::EMPTY;
                    } else {
                        slot.count = leftover;
                    }
                }
                inventories.write(ToClients {
                    targets: SendTargets::Single(*client_id),
                    message: inventory_update(&state),
                });
            }
            _ => {}
        }
    }
}
