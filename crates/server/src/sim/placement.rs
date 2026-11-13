//! Machine placement: turning a kit item into a machine in the world.
//!
//! The client sends `PlaceBlock` with the slot holding a kit; the server checks the footprint is
//! free, the tech is researched, the player is close enough and the item is really in the slot,
//! then spawns the machine entity and fills the footprint with its block.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::{ContentId, ContentRegistry, ItemCategory};
use deepcore_shared::protocol::{ClientIntent, FromClient, InventoryUpdate, ServerNotice};
use deepcore_shared::voxel::{BlockFace, BlockPos, LoadedChunks};
use tracing::info;

use crate::crafting_state::TechStateResource;
use crate::sim::inventory::inventory_update;
use crate::sim::machine::{BlockEntityIndex, place_crate, place_machine};
use crate::sim::mining::PendingBlockChanges;
use crate::sim::player::{OwnsPlayer, Player, PlayerState};
use crate::world::WorldManager;

/// Handles placing machine kits and storage crates.
#[allow(clippy::too_many_arguments)]
pub fn handle_machine_placement(
    mut commands: Commands,
    mut intents: MessageReader<FromClient<ClientIntent>>,
    mut players: Query<
        (
            &mut PlayerState,
            &deepcore_shared::protocol::NetworkPosition,
        ),
        With<Player>,
    >,
    clients: Query<&OwnsPlayer>,
    mut chunks: ResMut<LoadedChunks>,
    mut manager: ResMut<WorldManager>,
    mut index: ResMut<BlockEntityIndex>,
    mut changes: ResMut<PendingBlockChanges>,
    tech: Res<TechStateResource>,
    content: Res<ContentRegistry>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
    mut inventories: MessageWriter<ToClients<InventoryUpdate>>,
) {
    for FromClient { client_id, message } in intents.read() {
        let ClientIntent::PlaceBlock {
            pos,
            face,
            slot,
            facing,
        } = message
        else {
            continue;
        };
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let Ok(owner) = clients.get(client_entity) else {
            continue;
        };
        let Ok((mut state, position)) = players.get_mut(owner.0) else {
            continue;
        };
        if state.dead {
            continue;
        }

        // Which item is in the slot? Only kits and crate items are handled here; ordinary
        // blocks go through `handle_place_block`.
        let Some(stack) = state
            .inventory
            .get(deepcore_shared::inventory::SlotRef::Hotbar(*slot))
            .copied()
        else {
            continue;
        };
        if stack.is_empty() {
            continue;
        }
        let Some(item) = content.item(stack.item) else {
            continue;
        };
        let machine = match &item.category {
            ItemCategory::MachineKit(machine_key) => content.machine_ids.get(machine_key),
            _ => None,
        };
        let Some(machine) = machine else {
            continue;
        };

        // Reach check against the target block.
        let eye = position.position + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
        if (pos.center() - eye).length() > 7.0 {
            continue;
        }

        // Tech gate.
        if let Some(def) = content.machine(machine)
            && let Some(tech_node) = def.tech
            && !tech.0.is_researched(tech_node)
        {
            notices.write(ToClients {
                targets: SendTargets::Single(*client_id),
                message: ServerNotice::Notification {
                    text: format!(
                        "{} is not researched yet",
                        content
                            .tech(tech_node)
                            .map_or("That machine", |node| node.name.as_str())
                    ),
                    severity: 1,
                },
            });
            continue;
        }

        // The machine's controller goes where the player clicked, at the face they clicked.
        let anchor = *pos;
        let placed = place_machine(
            &mut commands,
            &content,
            &mut chunks,
            &mut manager,
            &mut index,
            machine,
            anchor,
            *facing % 4,
        );
        let Some(_entity) = placed else {
            notices.write(ToClients {
                targets: SendTargets::Single(*client_id),
                message: ServerNotice::Notification {
                    text: "Not enough room for that machine".into(),
                    severity: 1,
                },
            });
            continue;
        };

        // Report every block the machine filled so clients re-mesh.
        if let Some(def) = content.machine(machine) {
            let block = content.block_for_machine(machine);
            if let Some(block) = block {
                let (sx, sy, sz) = def.size;
                for dy in 0..sy as i32 {
                    for dz in 0..sz as i32 {
                        for dx in 0..sx as i32 {
                            changes.push(anchor.offset(dx, dy, dz), block.0);
                        }
                    }
                }
            }
            info!("{} placed at {anchor:?}", def.key);
        }

        // Consume the kit.
        if let Some(slot) = state
            .inventory
            .get_mut(deepcore_shared::inventory::SlotRef::Hotbar(*slot))
        {
            slot.count = slot.count.saturating_sub(1);
            if slot.count == 0 {
                *slot = deepcore_shared::inventory::ItemStack::EMPTY;
            }
        }
        inventories.write(ToClients {
            targets: SendTargets::Single(*client_id),
            message: inventory_update(&state),
        });
        let _ = face;
    }
}

/// Places a storage crate when the crate kit is used.
#[allow(clippy::too_many_arguments)]
pub fn handle_crate_placement(
    mut commands: Commands,
    mut intents: MessageReader<FromClient<ClientIntent>>,
    mut players: Query<
        (
            &mut PlayerState,
            &deepcore_shared::protocol::NetworkPosition,
        ),
        With<Player>,
    >,
    clients: Query<&OwnsPlayer>,
    mut chunks: ResMut<LoadedChunks>,
    mut manager: ResMut<WorldManager>,
    mut index: ResMut<BlockEntityIndex>,
    mut changes: ResMut<PendingBlockChanges>,
    content: Res<ContentRegistry>,
    mut inventories: MessageWriter<ToClients<InventoryUpdate>>,
) {
    for FromClient { client_id, message } in intents.read() {
        let ClientIntent::PlaceBlock {
            pos, face, slot, ..
        } = message
        else {
            continue;
        };
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let Ok(owner) = clients.get(client_entity) else {
            continue;
        };
        let Ok((mut state, position)) = players.get_mut(owner.0) else {
            continue;
        };
        let Some(stack) = state
            .inventory
            .get(deepcore_shared::inventory::SlotRef::Hotbar(*slot))
            .copied()
        else {
            continue;
        };
        if stack.is_empty() {
            continue;
        }
        let Some(item) = content.item(stack.item) else {
            continue;
        };
        let is_crate =
            matches!(&item.category, ItemCategory::MachineKit(key) if key == "storage_crate");
        if !is_crate {
            continue;
        }
        let target = face.neighbor(*pos);
        let eye = position.position + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
        if (target.center() - eye).length() > 6.0 {
            continue;
        }
        if !chunks.block(target).is_some_and(|id| id == content.air.0) {
            continue;
        }
        if let Some(block) = content.block_for_machine(
            content
                .machine_ids
                .get("storage_crate")
                .unwrap_or(ContentId::NONE),
        ) {
            chunks.set_block(target, block.0);
            manager.record_edit(target, block.0);
            changes.push(target, block.0);
        }
        place_crate(&mut commands, &mut index, target);
        if let Some(slot) = state
            .inventory
            .get_mut(deepcore_shared::inventory::SlotRef::Hotbar(*slot))
        {
            slot.count = slot.count.saturating_sub(1);
            if slot.count == 0 {
                *slot = deepcore_shared::inventory::ItemStack::EMPTY;
            }
        }
        inventories.write(ToClients {
            targets: SendTargets::Single(*client_id),
            message: inventory_update(&state),
        });
    }
}

/// The block face a machine is rotated to face.
pub fn facing_for_block(face: BlockFace) -> u8 {
    match face {
        BlockFace::North => 0,
        BlockFace::East => 1,
        BlockFace::South => 2,
        BlockFace::West => 3,
        _ => 0,
    }
}

/// The block position a kit should be placed at, given the targeted block and face.
pub fn anchor_for_kit(target: BlockPos, face: BlockFace) -> BlockPos {
    face.neighbor(target)
}

/// Whether the item in a slot is a machine kit.
pub fn is_machine_kit(content: &ContentRegistry, item: ContentId) -> bool {
    matches!(
        content.item(item).map(|item| &item.category),
        Some(ItemCategory::MachineKit(_))
    )
}
