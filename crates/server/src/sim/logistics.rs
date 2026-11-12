//! Item logistics: conveyors and item pipes moving stacks between machine outputs and inputs.
//!
//! A conveyor is a block with a direction. Each tick it looks at the machine behind it, pulls a
//! stack from that machine's output, and pushes it into the machine in front. That is enough to
//! build a real production line (crusher → washer → furnace) without any item entities in the
//! world, and it keeps the tick cheap: one lookup per conveyor.

use std::collections::HashMap;

use bevy::prelude::*;
use deepcore_shared::content::{ContentId, ContentRegistry};
use deepcore_shared::inventory::ItemStack;
use deepcore_shared::voxel::{BlockFace, BlockPos, LoadedChunks};

use crate::sim::machine::{BlockEntityIndex, MachineEntity};

/// Registers the logistics systems.
pub struct LogisticsPlugin;

impl Plugin for LogisticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Conveyors>().add_systems(
            FixedUpdate,
            (scan_conveyors, move_items_along_conveyors).chain(),
        );
    }
}

/// One conveyor block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conveyor {
    /// Where the conveyor sits.
    pub position: BlockPos,
    /// The direction items travel.
    pub direction: BlockFace,
    /// Seconds until the next transfer attempt.
    pub cooldown_ticks: u32,
}

/// Every conveyor in the world, and the ticks they have left before moving an item.
#[derive(Debug, Default, Resource)]
pub struct Conveyors {
    /// Conveyors keyed by position.
    pub by_position: HashMap<BlockPos, Conveyor>,
    /// Whether the world has been scanned for conveyors.
    pub scanned: bool,
}

/// How many ticks between item moves on one conveyor (30 Hz → twice a second).
pub const CONVEYOR_TICKS: u32 = 15;

/// Scans the loaded chunks for conveyor blocks.
///
/// This runs on `FixedUpdate` but exits immediately once it has scanned, and rescans when the
/// chunk set changes size (a player moved and new chunks streamed in).
pub fn scan_conveyors(
    mut conveyors: ResMut<Conveyors>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
    mut last_chunk_count: Local<usize>,
) {
    if conveyors.scanned && *last_chunk_count == chunks.len() {
        return;
    }
    *last_chunk_count = chunks.len();
    conveyors.by_position.clear();
    let Some(conveyor_block) = content.block_by_key("conveyor") else {
        return;
    };
    for chunk_pos in chunks.positions() {
        let Some(chunk) = chunks.get(chunk_pos) else {
            continue;
        };
        for (local, block) in chunk.iter_blocks() {
            if block != conveyor_block.id.0 {
                continue;
            }
            let origin = chunk_pos.origin();
            let position = BlockPos::new(
                origin.x + local.x as i32,
                origin.y + local.y as i32,
                origin.z + local.z as i32,
            );
            // Conveyors default to north; the placement facing is stored in the block entity
            // when one exists, otherwise north is the convention.
            conveyors.by_position.insert(
                position,
                Conveyor {
                    position,
                    direction: BlockFace::North,
                    cooldown_ticks: 0,
                },
            );
        }
    }
    conveyors.scanned = true;
}

/// Moves items along every conveyor.
pub fn move_items_along_conveyors(
    mut conveyors: ResMut<Conveyors>,
    mut machines: Query<&mut MachineEntity>,
    index: Res<BlockEntityIndex>,
    content: Res<ContentRegistry>,
) {
    if conveyors.by_position.is_empty() {
        return;
    }
    let positions: Vec<BlockPos> = conveyors.by_position.keys().copied().collect();
    for position in positions {
        let Some(mut conveyor) = conveyors.by_position.get(&position).copied() else {
            continue;
        };
        if conveyor.cooldown_ticks > 0 {
            conveyor.cooldown_ticks -= 1;
            conveyors.by_position.insert(position, conveyor);
            continue;
        }
        let source_pos = conveyor.direction.opposite().neighbor(position);
        let target_pos = conveyor.direction.neighbor(position);
        let (Some(&source), Some(&target)) = (
            index.by_position.get(&source_pos),
            index.by_position.get(&target_pos),
        ) else {
            conveyor.cooldown_ticks = CONVEYOR_TICKS;
            conveyors.by_position.insert(position, conveyor);
            continue;
        };
        if source == target {
            continue;
        }
        // Pull one stack out of the source's output and push it into the target's inputs.
        let mut moved = false;
        let stack = {
            let Ok(mut source_machine) = machines.get_mut(source) else {
                continue;
            };
            source_machine
                .inventory
                .outputs
                .iter()
                .position(|slot| !slot.is_empty())
                .map(|slot_index| {
                    let stack = source_machine.inventory.outputs[slot_index];
                    source_machine.inventory.outputs[slot_index] = ItemStack::EMPTY;
                    stack
                })
        };
        if let Some(stack) = stack {
            let Ok(mut target_machine) = machines.get_mut(target) else {
                // Put it back: nothing to receive it.
                if let Ok(mut source_machine) = machines.get_mut(source)
                    && let Some(slot) = source_machine
                        .inventory
                        .outputs
                        .iter_mut()
                        .find(|slot| slot.is_empty())
                {
                    *slot = stack;
                }
                continue;
            };
            let max_stack = content.item(stack.item).map_or(64, |item| item.stack);
            let mut remaining = stack.count;
            for slot in target_machine.inventory.inputs.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if slot.item == stack.item && slot.count < max_stack {
                    let room = max_stack - slot.count;
                    let amount = room.min(remaining);
                    slot.count += amount;
                    remaining -= amount;
                }
            }
            for slot in target_machine.inventory.inputs.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if slot.is_empty() {
                    let amount = max_stack.min(remaining);
                    *slot = ItemStack::new(stack.item, amount);
                    remaining -= amount;
                }
            }
            moved = remaining < stack.count;
            if remaining > 0 {
                // Anything that did not fit goes back to the source.
                if let Ok(mut source_machine) = machines.get_mut(source)
                    && let Some(slot) = source_machine
                        .inventory
                        .outputs
                        .iter_mut()
                        .find(|slot| slot.is_empty() || slot.item == stack.item)
                {
                    slot.item = stack.item;
                    slot.count += remaining;
                }
            }
        }
        conveyor.cooldown_ticks = if moved { CONVEYOR_TICKS } else { 4 };
        conveyors.by_position.insert(position, conveyor);
    }
}

/// The block a conveyor pulls from and pushes to, for tests and for the UI.
pub fn conveyor_ends(position: BlockPos, direction: BlockFace) -> (BlockPos, BlockPos) {
    (
        direction.opposite().neighbor(position),
        direction.neighbor(position),
    )
}

/// The machine a kit item places, if any (used by the conveyor placement code).
pub fn machine_of_kit(content: &ContentRegistry, item: ContentId) -> Option<ContentId> {
    match content.item(item).map(|item| &item.category) {
        Some(deepcore_shared::content::ItemCategory::MachineKit(key)) => {
            content.machine_ids.get(key)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conveyor_ends_are_on_both_sides() {
        let position = BlockPos::new(10, 5, 10);
        let (source, target) = conveyor_ends(position, BlockFace::North);
        assert_eq!(source, BlockPos::new(10, 5, 11));
        assert_eq!(target, BlockPos::new(10, 5, 9));
    }

    #[test]
    fn every_direction_has_distinct_ends() {
        for direction in BlockFace::ALL {
            let (source, target) = conveyor_ends(BlockPos::new(0, 0, 0), direction);
            assert_ne!(source, target, "{direction:?} has the same ends");
        }
    }
}
