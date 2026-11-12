//! First-person input: mouse look, WASD, hotbar, mining and placing.
//!
//! The client sends intents; it never edits the world itself. Mining starts immediately on the
//! client with a predicted crack animation (shared break-time formula) and the server's answer
//! arrives a round trip later.

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::crafting::mining::{REACH, ToolStats, evaluate_mining};
use deepcore_shared::crafting::MiningVerdict;
use deepcore_shared::inventory::SlotRef;
use deepcore_shared::movement::{PlayerInput, PLAYER_HEIGHT};
use deepcore_shared::protocol::{ClientIntent, NetworkPosition};
use deepcore_shared::voxel::{BlockFace, BlockPos, LoadedChunks};
use tracing::debug;

use crate::net::SessionState;
use crate::prediction::PredictionState;
use crate::LocalPlayer;

/// Registers the input systems.
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalInput>()
            .init_resource::<Targeting>()
            .add_systems(Startup, grab_cursor)
            .add_systems(
                Update,
                (
                    toggle_cursor,
                    mouse_look,
                    keyboard_input,
                    raycast_target,
                    mining_input,
                    placing_input,
                    interaction_input,
                    hotbar_input,
                )
                    .chain(),
            )
            .add_systems(Update, draw_target_outline);
    }
}

/// The local player's input state for this frame.
#[derive(Debug, Resource, Default)]
pub struct LocalInput {
    /// Yaw in radians.
    pub yaw: f32,
    /// Pitch in radians.
    pub pitch: f32,
    /// Next sequence number.
    pub seq: u32,
    /// The cursor is captured.
    pub captured: bool,
    /// The player is mining right now.
    pub mining: bool,
    /// Whether a menu has the pointer.
    pub ui_open: bool,
}

/// What the crosshair is pointing at.
#[derive(Debug, Resource, Default)]
pub struct Targeting {
    /// The targeted block.
    pub block: Option<BlockPos>,
    /// The face hit.
    pub face: Option<BlockFace>,
    /// Distance to the hit.
    pub distance: f32,
    /// The tier the block needs, for the red crosshair.
    pub required_tier: u8,
    /// Whether the held tool is too weak.
    pub too_hard: bool,
}

fn grab_cursor(
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut input: ResMut<LocalInput>,
) {
    if let Ok(mut cursor) = cursors.single_mut() {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        input.captured = true;
    }
}

fn toggle_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut input: ResMut<LocalInput>,
) {
    if !keys.just_pressed(KeyCode::Escape) && !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    input.captured = !input.captured;
    cursor.grab_mode = if input.captured {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    cursor.visible = !input.captured;
}

fn mouse_look(
    mut motion: MessageReader<MouseMotion>,
    mut input: ResMut<LocalInput>,
    settings: Res<crate::ClientConfig>,
) {
    if !input.captured {
        motion.clear();
        return;
    }
    let sensitivity = 0.0022;
    let _ = &settings;
    for event in motion.read() {
        input.yaw -= event.delta.x * sensitivity;
        input.pitch = (input.pitch - event.delta.y * sensitivity)
            .clamp(-std::f32::consts::FRAC_PI_2 + 0.01, std::f32::consts::FRAC_PI_2 - 0.01);
    }
}

/// Builds the `PlayerInput` for this frame and hands it to the predictor.
fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    input: Res<LocalInput>,
    mut prediction: ResMut<PredictionState>,
    mut inputs: MessageWriter<PlayerInput>,
) {
    let mut forward = 0i8;
    let mut right = 0i8;
    if keys.pressed(KeyCode::KeyW) {
        forward = 127;
    }
    if keys.pressed(KeyCode::KeyS) {
        forward = -127;
    }
    if keys.pressed(KeyCode::KeyD) {
        right = 127;
    }
    if keys.pressed(KeyCode::KeyA) {
        right = -127;
    }
    let player_input = PlayerInput {
        seq: prediction.next_seq(),
        forward,
        right,
        jump: keys.pressed(KeyCode::Space),
        sprint: keys.pressed(KeyCode::ShiftLeft),
        crouch: keys.pressed(KeyCode::ControlLeft),
        yaw: (input.yaw * 1000.0) as i32,
        pitch: (input.pitch * 1000.0) as i32,
    };
    prediction.push_input(player_input);
    inputs.write(player_input);
}

/// Casts the crosshair ray against the voxel world.
fn raycast_target(
    input: Res<LocalInput>,
    session: Res<SessionState>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
    players: Query<&NetworkPosition, With<LocalPlayer>>,
    mut targeting: ResMut<Targeting>,
) {
    let _ = &session;
    let Ok(position) = players.single() else {
        return;
    };
    let eye = position.position + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
    let direction = forward_vector(input.yaw, input.pitch);
    let hit = raycast(&chunks, eye, direction, REACH + 1.0);
    match hit {
        Some((block, face, distance)) => {
            let block_id = chunks.block(block).unwrap_or(0);
            let tool = held_tool(&content, &session);
            let verdict = evaluate_mining(
                deepcore_shared::content::ContentId(block_id),
                &tool,
                &content,
                1.0,
            );
            targeting.block = Some(block);
            targeting.face = Some(face);
            targeting.distance = distance;
            targeting.too_hard = matches!(verdict, MiningVerdict::TooHard { .. });
            targeting.required_tier = match verdict {
                MiningVerdict::TooHard { required_tier } => required_tier,
                _ => 0,
            };
        }
        None => {
            targeting.block = None;
            targeting.face = None;
            targeting.too_hard = false;
            targeting.required_tier = 0;
        }
    }
}

/// A unit vector from yaw and pitch.
pub fn forward_vector(yaw: f32, pitch: f32) -> Vec3 {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    Vec3::new(-sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
}

/// The stats of the item in hand.
pub fn held_tool(content: &ContentRegistry, session: &SessionState) -> ToolStats {
    let Some(inventory) = &session.inventory else {
        return ToolStats::STARTING_PICK;
    };
    let held = inventory
        .hotbar
        .get(inventory.selected as usize)
        .copied()
        .unwrap_or_default();
    if held.is_empty() {
        return ToolStats::STARTING_PICK;
    }
    ToolStats::from_item(held.item, content)
}

/// A voxel DDA raycast returning the first non-air block and the face entered.
pub fn raycast(
    chunks: &LoadedChunks,
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
) -> Option<(BlockPos, BlockFace, f32)> {
    let direction = direction.normalize_or_zero();
    if direction == Vec3::ZERO {
        return None;
    }
    let mut pos = BlockPos::from_vec3(origin);
    let mut step = [0i32; 3];
    let mut t_max = [f32::INFINITY; 3];
    let mut t_delta = [f32::INFINITY; 3];
    let origin_array = [origin.x, origin.y, origin.z];
    let dir_array = [direction.x, direction.y, direction.z];
    for axis in 0..3 {
        if dir_array[axis] > 0.0 {
            step[axis] = 1;
            t_max[axis] = ((pos_array(pos, axis) + 1.0) - origin_array[axis]) / dir_array[axis];
            t_delta[axis] = 1.0 / dir_array[axis];
        } else if dir_array[axis] < 0.0 {
            step[axis] = -1;
            t_max[axis] = (pos_array(pos, axis) - origin_array[axis]) / dir_array[axis];
            t_delta[axis] = -1.0 / dir_array[axis];
        }
    }
    let mut face = BlockFace::Up;
    let mut travelled = 0.0f32;
    while travelled <= max_distance {
        if let Some(block) = chunks.block(pos)
            && block != 0
        {
            return Some((pos, face, travelled));
        }
        let axis = if t_max[0] < t_max[1] && t_max[0] < t_max[2] {
            0
        } else if t_max[1] < t_max[2] {
            1
        } else {
            2
        };
        travelled = t_max[axis];
        t_max[axis] += t_delta[axis];
        match axis {
            0 => {
                pos.x += step[0];
                face = if step[0] > 0 {
                    BlockFace::West
                } else {
                    BlockFace::East
                };
            }
            1 => {
                pos.y += step[1];
                face = if step[1] > 0 {
                    BlockFace::Down
                } else {
                    BlockFace::Up
                };
            }
            _ => {
                pos.z += step[2];
                face = if step[2] > 0 {
                    BlockFace::North
                } else {
                    BlockFace::South
                };
            }
        }
    }
    None
}

fn pos_array(pos: BlockPos, axis: usize) -> f32 {
    match axis {
        0 => pos.x as f32,
        1 => pos.y as f32,
        _ => pos.z as f32,
    }
}

#[allow(clippy::too_many_arguments)]
fn mining_input(
    mouse: Res<ButtonInput<MouseButton>>,
    targeting: Res<Targeting>,
    input: Res<LocalInput>,
    mut intents: MessageWriter<ClientIntent>,
    mut prediction: ResMut<PredictionState>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
    session: Res<SessionState>,
) {
    if !input.captured {
        return;
    }
    if mouse.just_pressed(MouseButton::Left)
        && let (Some(block), Some(face)) = (targeting.block, targeting.face)
    {
        let tool = held_tool(&content, &session);
        let block_id = chunks.block(block).unwrap_or(0);
        if let MiningVerdict::CanMine { ticks } = evaluate_mining(
            deepcore_shared::content::ContentId(block_id),
            &tool,
            &content,
            1.0,
        ) {
            prediction.start_mining(block, ticks);
        }
        intents.write(ClientIntent::StartMining { pos: block, face });
        debug!("start mining {block:?} {face:?}");
    }
    if mouse.just_released(MouseButton::Left) {
        prediction.stop_mining();
        intents.write(ClientIntent::StopMining);
    }
}

fn placing_input(
    mouse: Res<ButtonInput<MouseButton>>,
    targeting: Res<Targeting>,
    input: Res<LocalInput>,
    session: Res<SessionState>,
    mut intents: MessageWriter<ClientIntent>,
) {
    if !input.captured || !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let (Some(block), Some(face)) = (targeting.block, targeting.face) else {
        return;
    };
    let Some(inventory) = &session.inventory else {
        return;
    };
    let slot = inventory.selected;
    intents.write(ClientIntent::PlaceBlock {
        pos: block,
        face,
        slot,
        facing: 0,
    });
}

fn interaction_input(
    mouse: Res<ButtonInput<MouseButton>>,
    targeting: Res<Targeting>,
    input: Res<LocalInput>,
    mut intents: MessageWriter<ClientIntent>,
) {
    if !input.captured {
        return;
    }
    // Middle click interacts (a machine, crate or desk), right-click also interacts when the
    // held slot is empty.
    if mouse.just_pressed(MouseButton::Middle)
        && let Some(block) = targeting.block
    {
        intents.write(ClientIntent::Interact { pos: block });
    }
}

fn hotbar_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut intents: MessageWriter<ClientIntent>,
    session: Res<SessionState>,
) {
    let _ = &session;
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (slot, key) in digits.iter().enumerate() {
        if keys.just_pressed(*key) {
            intents.write(ClientIntent::SelectSlot { slot: slot as u8 });
            let _ = &session;
            let _ = SlotRef::Hotbar(slot as u8);
        }
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        intents.write(ClientIntent::DropHeld);
    }
    if keys.just_pressed(KeyCode::KeyE) {
        intents.write(ClientIntent::Interact {
            pos: BlockPos::new(0, 0, 0),
        });
    }
}

/// Draws the block outline on the targeted block.
fn draw_target_outline(
    mut gizmos: Gizmos,
    targeting: Res<Targeting>,
) {
    let Some(block) = targeting.block else {
        return;
    };
    let centre = block.center();
    let colour = if targeting.too_hard {
        Color::srgb(1.0, 0.25, 0.2)
    } else {
        Color::srgb(1.0, 1.0, 1.0)
    };
    let size = Vec3::splat(1.002);
    gizmos.cube(
        Transform::from_translation(centre).with_scale(size),
        colour,
    );
    let _ = PLAYER_HEIGHT;
}
