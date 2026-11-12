//! Server-side player state: spawning, input application, vitals and death.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::{ContentRegistry, ItemCategory};
use deepcore_shared::crafting::ToolStats;
use deepcore_shared::hazards::{Environment, Vitals, WornGear, tick_vitals};
use deepcore_shared::inventory::{Inventory, ItemStack};
use deepcore_shared::movement::{BlockQuery, BodyState, MovementConfig, PlayerInput, step_body};
pub use deepcore_shared::protocol::PlayerName as PlayerNameComponent;
use deepcore_shared::protocol::{
    NetworkHeld, NetworkPosition, NetworkRotation, PlayerName, PlayerVitals, ServerNotice,
};
use deepcore_shared::voxel::{BlockPos, LoadedChunks};
use tracing::info;

use crate::ServerConfigResource;

/// Marker for a server-side player entity.
#[derive(Debug, Component)]
pub struct Player;

/// A saved player record that is not connected. Used to keep a name reserved until they rejoin.
#[derive(Debug, Component)]
pub struct Dormant;

/// The client entity that owns this player.
#[derive(Debug, Component)]
pub struct OwnedBy(pub Entity);

/// Per-player server state that never leaves the server.
#[derive(Debug, Clone, Component)]
pub struct PlayerState {
    /// The authoritative body.
    pub body: BodyState,
    /// Inventory.
    pub inventory: Inventory,
    /// Worn gear.
    pub gear: WornGear,
    /// Vitals.
    pub vitals: Vitals,
    /// The last input sequence applied.
    pub last_seq: u32,
    /// Queued inputs, applied in order.
    pub input_queue: Vec<PlayerInput>,
    /// Remaining breaks on the current tool (core cells, batteries, steam).
    pub tool_charge: u32,
    /// The player is dead and waiting to respawn.
    pub dead: bool,
    /// Where the death crate was dropped.
    pub death_crate: Option<BlockPos>,
    /// Ticks since the player last damaged itself, for fall damage.
    pub fall_start_y: Option<f32>,
}

impl PlayerState {
    /// The stats of the tool in hand.
    pub fn tool(&self, content: &ContentRegistry) -> ToolStats {
        let held = self.inventory.held();
        if held.is_empty() {
            // Every player owns a steel pickaxe; if the hotbar is somehow empty they still
            // mine at starting-pick speed rather than being stuck.
            return ToolStats::STARTING_PICK;
        }
        ToolStats::from_item(held.item, content)
    }

    /// The held item id, or `ContentId::NONE`.
    pub fn held_item(&self) -> deepcore_shared::content::ContentId {
        self.inventory.held().item
    }
}

/// Spawns the player entity for a freshly handshaked client.
pub fn spawn_player_for_client(
    commands: &mut Commands,
    content: &ContentRegistry,
    _config: &ServerConfigResource,
    saved: Option<&crate::persistence::load::SavedPlayers>,
    client: Entity,
    name: String,
) {
    let mut inventory = Inventory::default();
    let pickaxe = content
        .item_by_key("steel_pickaxe")
        .map(|item| item.id)
        .unwrap_or_default();
    inventory.hotbar[0] = ItemStack::new(pickaxe, 1);

    // A returning player keeps their position and inventory; a new player starts at the camp
    // with a steel pickaxe.
    let (spawn, restored) = match saved.and_then(|saved| saved.by_name.get(&name).cloned()) {
        Some((position, saved_inventory)) => {
            info!("restoring saved state for {name} at {position:?}");
            (position, Some(saved_inventory))
        }
        None => (
            deepcore_shared::worldgen::spawn_position_for_seed(_config.0.seed, content),
            None,
        ),
    };
    if let Some(saved_inventory) = restored {
        // Keep the saved hotbar/backpack, but make sure a player can never be left without a
        // tool if their save predates the starting pickaxe.
        inventory = saved_inventory;
        if inventory.hotbar.iter().all(|stack| stack.is_empty()) {
            inventory.hotbar[0] = ItemStack::new(pickaxe, 1);
        }
    }

    let body = BodyState {
        position: Vec3::new(
            spawn.x as f32 + 0.5,
            spawn.y as f32 + 0.5,
            spawn.z as f32 + 0.5,
        ),
        ..Default::default()
    };

    let player = commands
        .spawn((
            Player,
            OwnedBy(client),
            PlayerName(name.clone()),
            NetworkPosition {
                position: body.position,
                last_seq: 0,
            },
            NetworkRotation::default(),
            NetworkHeld {
                item: inventory.hotbar[0].item,
            },
            PlayerVitals::default(),
            crate::net::OwnedByPlayer(client),
            deepcore_shared::protocol::LocalOwner,
            PlayerState {
                body,
                inventory,
                gear: WornGear::default(),
                vitals: Vitals::default(),
                last_seq: 0,
                input_queue: Vec::new(),
                tool_charge: 0,
                dead: false,
                death_crate: None,
                fall_start_y: None,
            },
            Replicated,
        ))
        .id();

    commands.entity(client).insert((
        HandshakedPlayer(player),
        deepcore_shared::protocol::OwnedPlayer,
    ));
    info!("spawned player {player} for client {client} ({name})");
}

/// Marker on a client entity that has completed the handshake.
#[derive(Debug, Component)]
pub struct HandshakedPlayer(pub Entity);

/// Applies queued player inputs in `FixedUpdate`.
///
/// Movement is the only thing clients predict, so this is where authority is re-established:
/// every input is applied with the same pure function the client used.
pub fn apply_player_inputs(
    mut players: Query<(
        &mut PlayerState,
        &mut NetworkPosition,
        &mut NetworkRotation,
        &mut PlayerVitals,
    )>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
) {
    let config = MovementConfig::default();
    for (mut state, mut net_position, mut net_rotation, mut vitals) in &mut players {
        if state.dead {
            state.input_queue.clear();
            continue;
        }
        let world = ChunkQuery {
            chunks: &chunks,
            content: &content,
        };
        let mut inputs = std::mem::take(&mut state.input_queue);
        inputs.sort_by_key(|input| input.seq);
        for input in inputs {
            if input.seq <= state.last_seq {
                continue;
            }
            let before_y = state.body.position.y;
            let was_on_ground = state.body.on_ground;
            step_body(&mut state.body, input, &world, &config);

            // Fall damage: track the highest point since leaving the ground.
            if state.body.on_ground {
                if let Some(start) = state.fall_start_y.take()
                    && !was_on_ground
                {
                    let fell = (start - state.body.position.y).max(0.0);
                    let damage = deepcore_shared::hazards::fall_damage(&content, fell);
                    if damage > 0.0 {
                        state.vitals.health = (state.vitals.health - damage).max(0.0);
                        notices.write(ToClients {
                            targets: SendTargets::All,
                            message: ServerNotice::Notification {
                                text: format!("You fell {fell:.0} blocks"),
                                severity: 1,
                            },
                        });
                    }
                }
            } else if state.fall_start_y.is_none() {
                state.fall_start_y = Some(before_y);
            }

            state.last_seq = input.seq;
            net_position.position = state.body.position;
            net_position.last_seq = input.seq;
            net_rotation.yaw = input.yaw_radians();
            net_rotation.pitch = input.pitch_radians();
        }
        // Keep the replicated vitals in sync with the authoritative state.
        vitals.vitals = state.vitals;
        vitals.gear = state.gear;
        if state.vitals.health <= 0.0 && !state.dead {
            state.dead = true;
        }
    }
}

/// A `BlockQuery` over the server's loaded chunks.
pub struct ChunkQuery<'a> {
    /// The chunk store.
    pub chunks: &'a LoadedChunks,
    /// Content, for the solid/fluid flags.
    pub content: &'a ContentRegistry,
}

impl BlockQuery for ChunkQuery<'_> {
    fn is_solid(&self, pos: BlockPos) -> bool {
        self.chunks.is_solid(pos, self.content)
    }

    fn is_ladder(&self, pos: BlockPos) -> bool {
        self.chunks
            .block(pos)
            .and_then(|id| self.content.block(deepcore_shared::content::ContentId(id)))
            .is_some_and(|block| block.key == "ladder")
    }

    fn is_fluid(&self, pos: BlockPos) -> bool {
        self.chunks
            .block(pos)
            .and_then(|id| self.content.block(deepcore_shared::content::ContentId(id)))
            .is_some_and(|block| block.fluid)
    }
}

/// Ticks vitals: radiation, heat, gas, oxygen and stamina.
pub fn tick_player_vitals(
    mut players: Query<(&mut PlayerState, &NetworkPosition)>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
) {
    let dt = deepcore_shared::crafting::TICK_SECONDS;
    for (mut state, position) in &mut players {
        if state.dead {
            continue;
        }
        let environment = build_environment(&content, &chunks, &state, position);
        let gear = state.gear;
        let effects = tick_vitals(&content, &mut state.vitals, &gear, &environment, dt);
        if !effects.alive {
            state.dead = true;
        }
    }
}

/// Samples the environment around a player.
pub fn build_environment(
    content: &ContentRegistry,
    chunks: &LoadedChunks,
    state: &PlayerState,
    position: &NetworkPosition,
) -> Environment {
    let feet = position.position;
    let eye = feet + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
    let eye_block = BlockPos::new(
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    );

    let mut environment = Environment {
        underwater: chunks
            .block(eye_block)
            .and_then(|id| content.block(deepcore_shared::content::ContentId(id)))
            .is_some_and(|block| block.fluid),
        ambient_heat: 0.0,
        ..Default::default()
    };

    let depth = eye_block.depth().max(0) as u32;
    if let Some(layer) = content.layer_at_depth(depth) {
        environment.ambient_heat = layer.ambient_temp - 15.0;
        for hazard in &layer.hazards {
            match hazard {
                deepcore_shared::content::LayerHazard::Heat { base_rate, .. } => {
                    environment.ambient_heat += base_rate;
                }
                deepcore_shared::content::LayerHazard::Radiation { dose_per_sec } => {
                    environment.radiation.push((
                        deepcore_shared::content::RadiationDef {
                            radius: 32.0,
                            dose_per_sec: *dose_per_sec,
                        },
                        0.0,
                    ));
                }
                _ => {}
            }
        }
    }

    // Nearby radioactive blocks: pitchblende, core crystals, loose waste.
    let radius = 5i32;
    for dy in -radius..=radius {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = eye_block.offset(dx, dy, dz);
                let Some(id) = chunks.block(pos) else {
                    continue;
                };
                let Some(block) = content.block(deepcore_shared::content::ContentId(id)) else {
                    continue;
                };
                let Some(radiation) = block.radiation else {
                    continue;
                };
                let distance = Vec3::new(dx as f32, dy as f32, dz as f32).length();
                if distance <= radiation.radius {
                    environment.radiation.push((radiation, distance));
                }
            }
        }
    }

    // Worn gear effects are applied inside `tick_vitals`.
    let _ = state;
    environment.sprinting = false;
    environment
}

/// Whether the player holds a tool of a given tier or better.
pub fn holds_tier(content: &ContentRegistry, state: &PlayerState, tier: u8) -> bool {
    match content.item(state.held_item()).map(|item| &item.category) {
        Some(ItemCategory::Tool {
            tier: tool_tier, ..
        }) => *tool_tier >= tier,
        _ => tier == 0,
    }
}
