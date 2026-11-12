//! Ghost preview: shows where a machine kit will go before it is placed.
//!
//! The ghost is drawn from the same footprint data the server validates, so what the player sees
//! is what they get. It is tinted green when the spot is free and red when it is blocked.

use bevy::prelude::*;
use deepcore_shared::content::{ContentId, ContentRegistry, ItemCategory};
use deepcore_shared::voxel::{BlockFace, BlockPos, LoadedChunks};

use crate::input::Targeting;
use crate::net::SessionState;

/// Registers the ghost preview.
pub struct GhostPlugin;

impl Plugin for GhostPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GhostState>()
            .add_systems(Update, (update_ghost, draw_ghost));
    }
}

/// What the ghost is currently showing.
#[derive(Debug, Default, Resource)]
pub struct GhostState {
    /// Machine being previewed.
    pub machine: Option<ContentId>,
    /// Footprint anchor.
    pub anchor: BlockPos,
    /// Footprint size.
    pub size: (u8, u8, u8),
    /// Whether the footprint is free.
    pub valid: bool,
}

/// Works out the ghost from the held item and the crosshair target.
fn update_ghost(
    mut ghost: ResMut<GhostState>,
    targeting: Res<Targeting>,
    session: Res<SessionState>,
    content: Res<ContentRegistry>,
    chunks: Res<LoadedChunks>,
) {
    let Some(inventory) = &session.inventory else {
        *ghost = GhostState::default();
        return;
    };
    let held = inventory
        .hotbar
        .get(inventory.selected as usize)
        .copied()
        .unwrap_or_default();
    if held.is_empty() {
        *ghost = GhostState::default();
        return;
    }
    let Some(item) = content.item(held.item) else {
        *ghost = GhostState::default();
        return;
    };
    let ItemCategory::MachineKit(machine_key) = &item.category else {
        *ghost = GhostState::default();
        return;
    };
    let Some(machine) = content.machine_by_key(machine_key) else {
        *ghost = GhostState::default();
        return;
    };
    let (Some(target), Some(face)) = (targeting.block, targeting.face) else {
        *ghost = GhostState::default();
        return;
    };
    let anchor = face.neighbor(target);
    // A machine is never placed inside the block the player is standing in.
    let mut valid = true;
    let (sx, sy, sz) = machine.size;
    for dy in 0..sy as i32 {
        for dz in 0..sz as i32 {
            for dx in 0..sx as i32 {
                let pos = anchor.offset(dx, dy, dz);
                if !chunks.block(pos).is_some_and(|id| id == content.air.0) {
                    valid = false;
                }
            }
        }
    }
    ghost.machine = Some(machine.id);
    ghost.anchor = anchor;
    ghost.size = machine.size;
    ghost.valid = valid;
}

/// Draws the ghost footprint as a wireframe box.
fn draw_ghost(ghost: Res<GhostState>, mut gizmos: Gizmos) {
    if ghost.machine.is_none() {
        return;
    }
    let (sx, sy, sz) = ghost.size;
    let size = Vec3::new(sx as f32, sy as f32, sz as f32);
    let centre = ghost.anchor.min() + size * 0.5;
    let colour = if ghost.valid {
        Color::srgba(0.35, 1.0, 0.45, 0.55)
    } else {
        Color::srgba(1.0, 0.3, 0.25, 0.55)
    };
    gizmos.cube(Transform::from_translation(centre).with_scale(size), colour);
}

/// Whether the player is holding a machine kit, for the HUD hint.
pub fn holding_kit<'a>(content: &'a ContentRegistry, session: &SessionState) -> Option<&'a str> {
    let inventory = session.inventory.as_ref()?;
    let held = inventory.hotbar.get(inventory.selected as usize)?;
    if held.is_empty() {
        return None;
    }
    match &content.item(held.item)?.category {
        ItemCategory::MachineKit(machine) => Some(machine.as_str()),
        _ => None,
    }
}

/// The face a machine should face when placed while looking at `face`.
pub fn placement_facing(face: BlockFace) -> u8 {
    match face {
        BlockFace::North => 0,
        BlockFace::East => 1,
        BlockFace::South => 2,
        BlockFace::West => 3,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_follows_the_face_clicked() {
        assert_eq!(placement_facing(BlockFace::North), 0);
        assert_eq!(placement_facing(BlockFace::East), 1);
        assert_eq!(placement_facing(BlockFace::South), 2);
        assert_eq!(placement_facing(BlockFace::West), 3);
    }
}
