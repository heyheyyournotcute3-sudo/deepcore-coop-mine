//! Mining and placement, validated on the server.
//!
//! Every rule the client predicts is re-checked here: reach, line of sight, tool tier, the item
//! actually being in the slot, and the target block being loaded. Invalid intents are dropped
//! and logged; they can never crash or desync the server.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::{ContentId, ContentRegistry, ItemCategory};
use deepcore_shared::crafting::mining::{
    DRILL_REACH, MiningVerdict, REACH, area_blocks, evaluate_mining,
};
use deepcore_shared::inventory::ItemStack;
use deepcore_shared::protocol::{
    BlockChanges, ClientIntent, FromClient, InventoryUpdate, MiningFeedback, mining_outcome,
};
use deepcore_shared::voxel::{BlockFace, BlockPos, LoadedChunks};
use tracing::debug;

use crate::sim::player::{OwnedBy, Player, PlayerState};
use crate::world::WorldManager;

/// An in-progress mining action.
#[derive(Debug, Component, Default)]
pub struct MiningAction {
    /// The target block.
    pub target: BlockPos,
    /// The face being mined.
    pub face: BlockFace,
    /// Ticks remaining.
    pub ticks_left: u32,
    /// Total ticks of the action, for the client's crack animation.
    pub ticks_total: u32,
    /// Blocks being broken (an area drill breaks several).
    pub blocks: Vec<BlockPos>,
}

/// Blocks broken this tick, flushed to clients as one message.
#[derive(Debug, Default, Resource)]
pub struct PendingBlockChanges {
    /// `(position, block id)` pairs.
    pub changes: Vec<(BlockPos, u16)>,
}

impl PendingBlockChanges {
    /// Queues a change.
    pub fn push(&mut self, pos: BlockPos, block: u16) {
        if let Some(entry) = self.changes.iter_mut().find(|(p, _)| *p == pos) {
            entry.1 = block;
        } else {
            self.changes.push((pos, block));
        }
    }
}

/// Handles `StartMining` and `StopMining` intents.
pub fn handle_start_mining(
    mut commands: Commands,
    mut intents: MessageReader<FromClient<ClientIntent>>,
    players: Query<
        (
            &PlayerState,
            &deepcore_shared::protocol::NetworkPosition,
            Option<&MiningAction>,
        ),
        With<Player>,
    >,
    clients: Query<&OwnedBy>,
    mut feedback: MessageWriter<ToClients<MiningFeedback>>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
) {
    for FromClient { client_id, message } in intents.read() {
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let Ok(owner) = clients.get(client_entity) else {
            continue;
        };
        let Ok((state, position, action)) = players.get(owner.0) else {
            continue;
        };
        match message {
            ClientIntent::StartMining { pos, face } => {
                if action.is_some() || state.dead {
                    continue;
                }
                let (verdict, blocks, _distance) =
                    validate_mining(&content, &chunks, state, position.position, *pos, *face);
                let (code, required_tier) = outcome_code(verdict);
                report_outcome(&mut feedback, *client_id, *pos, code, required_tier);
                let MiningVerdict::CanMine { ticks } = verdict else {
                    continue;
                };
                if blocks.is_empty() {
                    continue;
                }
                commands.entity(owner.0).insert(MiningAction {
                    target: *pos,
                    face: *face,
                    ticks_left: ticks,
                    ticks_total: ticks,
                    blocks,
                });
            }
            ClientIntent::StopMining => {
                commands.entity(owner.0).remove::<MiningAction>();
            }
            _ => {}
        }
    }
}

/// Checks a mining request against every rule. Returns the verdict and the affected blocks.
pub fn validate_mining(
    content: &ContentRegistry,
    chunks: &LoadedChunks,
    state: &PlayerState,
    player_position: Vec3,
    target: BlockPos,
    face: BlockFace,
) -> (MiningVerdict, Vec<BlockPos>, f32) {
    let tool = state.tool(content);
    let reach = if tool.is_drill() { DRILL_REACH } else { REACH };
    let eye = player_position + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
    let distance = (target.center() - eye).length();
    if distance > reach {
        return (MiningVerdict::Unbreakable, Vec::new(), 0.0);
    }
    let Some(block_id) = chunks.block(target) else {
        return (MiningVerdict::Empty, Vec::new(), 0.0);
    };
    if !has_line_of_sight(chunks, content, eye, target) {
        return (MiningVerdict::Unbreakable, Vec::new(), 0.0);
    }
    let verdict = evaluate_mining(ContentId(block_id), &tool, content, 1.0);
    let blocks = area_blocks(target, face, tool.area)
        .into_iter()
        .filter(|pos| chunks.block(*pos).is_some_and(|id| id != content.air.0))
        .collect();
    (verdict, blocks, distance)
}

/// A voxel line-of-sight test from `eye` to the centre of `target`.
pub fn has_line_of_sight(
    chunks: &LoadedChunks,
    content: &ContentRegistry,
    eye: Vec3,
    target: BlockPos,
) -> bool {
    let end = target.center();
    let delta = end - eye;
    let steps = (delta.length() * 4.0).ceil() as i32;
    if steps == 0 {
        return true;
    }
    for step in 1..steps {
        let t = step as f32 / steps as f32;
        let point = eye + delta * t;
        let pos = BlockPos::new(
            point.x.floor() as i32,
            point.y.floor() as i32,
            point.z.floor() as i32,
        );
        if pos == target {
            continue;
        }
        if let Some(id) = chunks.block(pos)
            && content
                .block(ContentId(id))
                .is_some_and(|block| block.solid)
        {
            return false;
        }
    }
    true
}

/// Breaks a block: removes it from the world, records the edit and grants the drops.
pub fn break_block(
    content: &ContentRegistry,
    chunks: &mut LoadedChunks,
    manager: &mut WorldManager,
    state: &mut PlayerState,
    pos: BlockPos,
    changes: &mut PendingBlockChanges,
) -> Vec<ItemStack> {
    let Some(id) = chunks.block(pos) else {
        return Vec::new();
    };
    let Some(block) = content.block(ContentId(id)) else {
        return Vec::new();
    };
    if block.unbreakable {
        return Vec::new();
    }
    let drops = block.drops.clone();
    chunks.set_block(pos, content.air.0);
    manager.record_edit(pos, content.air.0);
    changes.push(pos, content.air.0);

    let mut overflow = Vec::new();
    for (item, count) in drops {
        let leftover = state.inventory.insert(item, count, content);
        if leftover > 0 {
            overflow.push(ItemStack::new(item, leftover));
        }
    }
    overflow
}

/// Ticks running mining actions and finishes them.
pub fn tick_mining(
    mut commands: Commands,
    mut players: Query<(
        Entity,
        &mut PlayerState,
        &mut MiningAction,
        &deepcore_shared::protocol::NetworkPosition,
    )>,
    mut chunks: ResMut<LoadedChunks>,
    mut manager: ResMut<WorldManager>,
    mut changes: ResMut<PendingBlockChanges>,
    content: Res<ContentRegistry>,
) {
    for (entity, mut state, mut action, position) in &mut players {
        if state.dead || action.ticks_total == 0 {
            continue;
        }
        if action.ticks_left > 0 {
            action.ticks_left -= 1;
            continue;
        }
        // Finish the action: verify the blocks are still there and the tool still fits.
        let mut broken_any = false;
        for target in action.blocks.clone() {
            let (verdict, _, _) = validate_mining(
                &content,
                &chunks,
                &state,
                position.position,
                target,
                action.face,
            );
            if let MiningVerdict::CanMine { .. } = verdict {
                let overflow = break_block(
                    &content,
                    &mut chunks,
                    &mut manager,
                    &mut state,
                    target,
                    &mut changes,
                );
                broken_any = true;
                // Area drills wear their core cell per block.
                if state.tool_charge > 0 {
                    state.tool_charge -= 1;
                }
                if !overflow.is_empty() {
                    debug!("{entity} inventory full, {} stacks dropped", overflow.len());
                }
            }
        }
        // The action is finished either way: a failed break must not leave a stuck timer.
        let _ = broken_any;
        commands.entity(entity).remove::<MiningAction>();
    }
}

/// Sends the batched block changes to every client once per tick.
pub fn flush_block_changes(
    mut changes: ResMut<PendingBlockChanges>,
    mut writer: MessageWriter<ToClients<BlockChanges>>,
) {
    if changes.changes.is_empty() {
        return;
    }
    let batch = BlockChanges {
        changes: std::mem::take(&mut changes.changes),
    };
    writer.write(ToClients {
        targets: SendTargets::All,
        message: batch,
    });
}

/// Handles `PlaceBlock`.
#[allow(clippy::too_many_arguments)]
pub fn handle_place_block(
    mut intents: MessageReader<FromClient<ClientIntent>>,
    mut players: Query<
        (
            &mut PlayerState,
            &deepcore_shared::protocol::NetworkPosition,
        ),
        With<Player>,
    >,
    mut chunks: ResMut<LoadedChunks>,
    mut manager: ResMut<WorldManager>,
    mut changes: ResMut<PendingBlockChanges>,
    content: Res<ContentRegistry>,
    clients: Query<&OwnedBy>,
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
        if state.dead {
            continue;
        }
        let eye = position.position + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
        let target = face.neighbor(*pos);
        if (target.center() - eye).length() > REACH + 1.0 {
            continue;
        }
        if !chunks.block(target).is_some_and(|id| id == content.air.0) {
            continue;
        }
        // The player must not be standing inside the block they place.
        if player_overlaps(&state, target) {
            continue;
        }
        let Some(stack) = state
            .inventory
            .get_mut(deepcore_shared::inventory::SlotRef::Hotbar(*slot))
        else {
            continue;
        };
        if stack.is_empty() {
            continue;
        }
        let Some(item) = content.item(stack.item) else {
            continue;
        };
        let Some(block) = item.places_block else {
            continue;
        };
        // Machine kits are handled by `placement::handle_machine_placement`, which spawns the
        // block entity as well as the block.
        if matches!(item.category, ItemCategory::MachineKit(_)) {
            continue;
        }
        stack.count -= 1;
        if stack.count == 0 {
            *stack = ItemStack::EMPTY;
        }
        chunks.set_block(target, block.0);
        manager.record_edit(target, block.0);
        changes.push(target, block.0);
        inventories.write(ToClients {
            targets: SendTargets::Single(*client_id),
            message: crate::sim::inventory::inventory_update(&state),
        });
    }
}

fn player_overlaps(state: &PlayerState, block: BlockPos) -> bool {
    let min = state.body.position
        + Vec3::new(
            -deepcore_shared::movement::PLAYER_HALF_WIDTH,
            0.0,
            -deepcore_shared::movement::PLAYER_HALF_WIDTH,
        );
    let max = state.body.position
        + Vec3::new(
            deepcore_shared::movement::PLAYER_HALF_WIDTH,
            deepcore_shared::movement::PLAYER_HEIGHT,
            deepcore_shared::movement::PLAYER_HALF_WIDTH,
        );
    let block_min = block.min();
    let block_max = block_min + Vec3::ONE;
    min.x < block_max.x
        && max.x > block_min.x
        && min.y < block_max.y
        && max.y > block_min.y
        && min.z < block_max.z
        && max.z > block_min.z
}

/// Reports a mining outcome to a client.
pub fn report_outcome(
    writer: &mut MessageWriter<ToClients<MiningFeedback>>,
    client: ClientId,
    pos: BlockPos,
    outcome: u8,
    required_tier: u8,
) {
    writer.write(ToClients {
        targets: SendTargets::Single(client),
        message: MiningFeedback {
            pos,
            outcome,
            required_tier,
        },
    });
}

/// Maps a mining verdict onto the wire outcome code.
pub fn outcome_code(verdict: MiningVerdict) -> (u8, u8) {
    match verdict {
        MiningVerdict::CanMine { .. } => (mining_outcome::OK, 0),
        MiningVerdict::TooHard { required_tier } => (mining_outcome::TOO_HARD, required_tier),
        MiningVerdict::Unbreakable => (mining_outcome::UNBREAKABLE, 0),
        MiningVerdict::Empty => (mining_outcome::NOT_LOADED, 0),
    }
}

/// Tool tier of an item, for quick checks elsewhere.
pub fn tool_tier(content: &ContentRegistry, item: ContentId) -> u8 {
    match content.item(item).map(|item| &item.category) {
        Some(ItemCategory::Tool { tier, .. }) => *tier,
        _ => 0,
    }
}

/// Reach used by the given held item.
pub fn reach_for(content: &ContentRegistry, item: ContentId) -> f32 {
    match content.item(item).map(|item| &item.category) {
        Some(ItemCategory::Tool { reach, .. }) => *reach,
        _ => REACH,
    }
}
