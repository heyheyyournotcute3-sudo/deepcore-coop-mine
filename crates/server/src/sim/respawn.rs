//! Death and respawn: the backpack drops as a recoverable crate, the hotbar stays.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::inventory::{Inventory, ItemStack};
use deepcore_shared::protocol::{
    ClientIntent, FromClient, InventoryUpdate, NetworkPosition, PlayerVitals, ServerNotice,
};
use deepcore_shared::voxel::{BlockPos, LoadedChunks};

use crate::sim::inventory::inventory_update;
use crate::sim::machine::{BlockEntityIndex, CrateEntity, place_crate};
use crate::sim::player::{OwnedBy, Player, PlayerState};
use crate::world::WorldManager;

/// Handles the `Respawn` intent.
#[allow(clippy::too_many_arguments)]
pub fn handle_respawn(
    mut commands: Commands,
    mut intents: MessageReader<FromClient<ClientIntent>>,
    mut players: Query<
        (
            &mut PlayerState,
            &mut NetworkPosition,
            &mut PlayerVitals,
            Entity,
        ),
        With<Player>,
    >,
    clients: Query<&OwnedBy>,
    content: Res<ContentRegistry>,
    mut chunks: ResMut<LoadedChunks>,
    mut manager: ResMut<WorldManager>,
    mut index: ResMut<BlockEntityIndex>,
    mut crates: Query<&mut CrateEntity>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
    mut inventories: MessageWriter<ToClients<InventoryUpdate>>,
) {
    for FromClient { client_id, message } in intents.read() {
        if !matches!(message, ClientIntent::Respawn) {
            continue;
        }
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let Ok(owner) = clients.get(client_entity) else {
            continue;
        };
        let Ok((mut state, mut position, mut vitals, entity)) = players.get_mut(owner.0) else {
            continue;
        };
        if !state.dead {
            continue;
        }

        // Drop the backpack as a crate at the death spot, keeping the hotbar.
        let death_pos = state
            .death_crate
            .take()
            .unwrap_or_else(|| BlockPos::from_vec3(state.body.position));
        let backpack: Vec<ItemStack> = state
            .inventory
            .backpack
            .iter()
            .copied()
            .filter(|stack| !stack.is_empty())
            .collect();
        if !backpack.is_empty() {
            let crate_entity = place_crate(&mut commands, &mut index, death_pos);
            if let Ok(mut krate) = crates.get_mut(crate_entity) {
                for (slot, stack) in krate.slots.iter_mut().zip(backpack.iter()) {
                    *slot = *stack;
                }
            }
            chunks.set_block(death_pos, content.air.0);
            manager.record_edit(death_pos, content.air.0);
            notices.write(ToClients {
                targets: SendTargets::All,
                message: ServerNotice::Notification {
                    text: "A player died; their backpack dropped as a crate".into(),
                    severity: 1,
                },
            });
        }

        // Reset the player at the base.
        let spawn = deepcore_shared::worldgen::spawn_position(&content);
        state.body.position = Vec3::new(
            spawn.x as f32 + 0.5,
            spawn.y as f32 + 0.5,
            spawn.z as f32 + 0.5,
        );
        state.body.velocity = Vec3::ZERO;
        state.body.on_ground = false;
        state.vitals = deepcore_shared::hazards::Vitals::default();
        state.dead = false;
        state.fall_start_y = None;
        state.inventory = Inventory {
            hotbar: state.inventory.hotbar,
            backpack: vec![ItemStack::EMPTY; 36],
            selected: state.inventory.selected,
        };
        position.position = state.body.position;
        vitals.vitals = state.vitals;
        notices.write(ToClients {
            targets: SendTargets::Single(*client_id),
            message: ServerNotice::Notification {
                text: "Respawned at the Base".into(),
                severity: 0,
            },
        });
        inventories.write(ToClients {
            targets: SendTargets::Single(*client_id),
            message: inventory_update(&state),
        });
        let _ = entity;
    }
}
