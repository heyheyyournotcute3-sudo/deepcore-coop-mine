//! Block entities: machines, chests, lamps and the multi-block structures.
//!
//! A machine is an ECS entity with a [`MachineEntity`] component holding its position, its
//! machine id and its inventory. The voxel at its controller position holds the machine's
//! block, so the world and the entity never disagree about whether a machine is there.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::{ContentId, ContentRegistry, ItemCategory, MachineEnergy};
use deepcore_shared::crafting::machine::machine_draw_kw;
use deepcore_shared::crafting::{MachineInventory, MachineState, tick_machine};
use deepcore_shared::inventory::{ItemStack, SlotRef};
use deepcore_shared::protocol::{
    BlockEntityKind, ClientIntent, FromClient, MachineReplica, MachineScreen, ServerNotice,
    idle_code,
};
use deepcore_shared::voxel::{BlockFace, BlockPos, LoadedChunks};
use tracing::{debug, warn};

use crate::crafting_state::TechStateResource;
use crate::sim::player::{OwnedBy, Player, PlayerState};
use crate::world::WorldManager;

/// A machine placed in the world.
#[derive(Debug, Component)]
pub struct MachineEntity {
    /// The machine definition.
    pub machine: ContentId,
    /// Controller block position.
    pub position: BlockPos,
    /// Facing, 0..4 quarter turns.
    pub facing: u8,
    /// Slots.
    pub inventory: MachineInventory,
    /// Running state.
    pub state: MachineState,
    /// Steam stored, for boilers and buffers.
    pub steam: f32,
    /// Battery charge in kWh, for battery banks.
    pub charge_kwh: f32,
    /// A player currently looking at this machine's screen, if any.
    pub viewer: Option<Entity>,
    /// The reactor's control-rod insertion, 0..1 (reactor only).
    pub rod_insertion: f32,
    /// The reactor's core temperature in °C.
    pub core_temp: f32,
}

/// A storage crate.
#[derive(Debug, Component)]
pub struct CrateEntity {
    /// Position.
    pub position: BlockPos,
    /// Contents.
    pub slots: Vec<ItemStack>,
}

/// Index from block position to block entity, so interaction is O(1).
#[derive(Debug, Default, Resource)]
pub struct BlockEntityIndex {
    /// Position → entity.
    pub by_position: HashMap<BlockPos, Entity>,
}

/// Every machine's power draw, recomputed each tick before the power solve.
#[derive(Debug, Default, Resource)]
pub struct MachinePowerDemand {
    /// `entity → kW`.
    pub electric: HashMap<Entity, f32>,
    /// `entity → steam units/s`.
    pub steam: HashMap<Entity, f32>,
    /// `entity → kW generated`.
    pub generation: HashMap<Entity, f32>,
}

/// Places a machine when a kit is used.
#[allow(clippy::too_many_arguments)]
pub fn place_machine(
    commands: &mut Commands,
    content: &ContentRegistry,
    chunks: &mut LoadedChunks,
    manager: &mut WorldManager,
    index: &mut BlockEntityIndex,
    machine: ContentId,
    position: BlockPos,
    facing: u8,
) -> Option<Entity> {
    let def = content.machine(machine)?;
    let block = content.block_for_machine(machine)?;
    // Check the whole footprint is free.
    let (sx, sy, sz) = def.size;
    for dy in 0..sy as i32 {
        for dz in 0..sz as i32 {
            for dx in 0..sx as i32 {
                let pos = position.offset(dx, dy, dz);
                if !chunks.block(pos).is_some_and(|id| id == content.air.0) {
                    return None;
                }
            }
        }
    }
    for dy in 0..sy as i32 {
        for dz in 0..sz as i32 {
            for dx in 0..sx as i32 {
                let pos = position.offset(dx, dy, dz);
                chunks.set_block(pos, block.0);
                manager.record_edit(pos, block.0);
            }
        }
    }
    let inventory = MachineInventory::for_machine(content, machine);
    let entity = commands
        .spawn((
            MachineEntity {
                machine,
                position,
                facing,
                inventory,
                state: MachineState::default(),
                steam: 0.0,
                charge_kwh: 0.0,
                viewer: None,
                rod_insertion: 1.0,
                core_temp: 20.0,
            },
            MachineReplica {
                machine,
                position,
                facing,
                running: false,
                progress: 0,
                idle: 0,
            },
            BlockEntityKind::Machine,
            Replicated,
        ))
        .id();
    index.by_position.insert(position, entity);
    Some(entity)
}

/// Ticks every machine: power demand, recipe progress, reactor physics.
pub fn tick_machines(
    mut machines: Query<(Entity, &mut MachineEntity, &mut MachineReplica)>,
    content: Res<ContentRegistry>,
    tech: Res<TechStateResource>,
    networks: Res<deepcore_shared::power::PowerNetworks>,
) {
    let dt = deepcore_shared::crafting::TICK_SECONDS;
    let unlocked = |node: ContentId| tech.0.is_researched(node);
    for (entity, mut machine, mut replica) in &mut machines {
        let def = match content.machine(machine.machine) {
            Some(def) => def,
            None => continue,
        };
        let satisfaction =
            networks.satisfaction_for(deepcore_shared::power::NetworkKind::Electric, entity);
        // Split the machine fields so the recipe tick can borrow both at once.
        let MachineEntity {
            machine: machine_id,
            inventory,
            state,
            ..
        } = &mut *machine;
        let produced = tick_machine(
            &content,
            *machine_id,
            inventory,
            state,
            dt,
            satisfaction,
            &unlocked,
        );

        // Steam and fuel buffers drain while the machine runs.
        if machine.state.running
            && let MachineEnergy::Steam { units_per_sec } = def.energy
        {
            machine.steam = (machine.steam - units_per_sec * dt).max(0.0);
            if machine.steam <= 0.0 && satisfaction > 0.0 {
                machine.state.idle = deepcore_shared::crafting::IdleReason::NoPower;
            }
        }

        if def.generation_kw > 0.0 && machine.state.running {
            machine.charge_kwh = (machine.charge_kwh + def.generation_kw * dt / 3600.0).min(1.0);
        }

        if produced {
            debug!(
                "machine {:?} at {:?} produced output",
                def.key, machine.position
            );
        }

        // Reactor physics: rods out → heat up, coolant flow → cool down.
        if def.key == "reactor" {
            let heat_gain = (1.0 - machine.rod_insertion) * 6.0;
            let cooling = machine.rod_insertion * 3.0 + 1.0;
            machine.core_temp = (machine.core_temp + (heat_gain - cooling) * dt).max(20.0);
            if machine.core_temp > 950.0 {
                warn!("reactor at {:?} is overheating", machine.position);
            }
        }

        replica.running = machine.state.running;
        replica.progress = (machine.state.fraction() * 255.0) as u8;
        replica.idle = idle_code(machine.state.idle);
    }
}

/// Computes each machine's power draw for the network solve.
pub fn compute_power_demand(
    mut demand: ResMut<MachinePowerDemand>,
    machines: Query<(Entity, &MachineEntity)>,
    content: Res<ContentRegistry>,
) {
    demand.electric.clear();
    demand.steam.clear();
    demand.generation.clear();
    for (entity, machine) in &machines {
        let Some(def) = content.machine(machine.machine) else {
            continue;
        };
        let (speed, power) = machine.inventory.module_multipliers(&content);
        let _ = speed;
        match def.energy {
            MachineEnergy::Electric { .. } => {
                let draw = machine_draw_kw(&content, machine.machine, machine.state.recipe) * power;
                demand.electric.insert(entity, draw);
            }
            MachineEnergy::Steam { units_per_sec } => {
                demand.steam.insert(entity, units_per_sec);
            }
            _ => {}
        }
        if def.generation_kw > 0.0 {
            demand.generation.insert(entity, def.generation_kw);
        }
    }
}

/// Handles opening and closing machine screens, and machine recipe selection.
#[allow(clippy::too_many_arguments)]
pub fn handle_machine_intents(
    mut intents: MessageReader<FromClient<ClientIntent>>,
    mut machines: Query<&mut MachineEntity>,
    mut players: Query<(&PlayerState, &deepcore_shared::protocol::NetworkPosition), With<Player>>,
    clients: Query<&OwnedBy>,
    index: Res<BlockEntityIndex>,
    content: Res<ContentRegistry>,
    mut screens: MessageWriter<ToClients<MachineScreen>>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
) {
    for FromClient { client_id, message } in intents.read() {
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        let Ok(owner) = clients.get(client_entity) else {
            continue;
        };
        let Ok((state, position)) = players.get_mut(owner.0) else {
            continue;
        };
        match message {
            ClientIntent::OpenMachine { pos } => {
                let Some(&entity) = index.by_position.get(pos) else {
                    continue;
                };
                let Ok(mut machine) = machines.get_mut(entity) else {
                    continue;
                };
                let eye = position.position + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
                if (pos.center() - eye).length() > 6.0 {
                    notices.write(ToClients {
                        targets: SendTargets::Single(*client_id),
                        message: ServerNotice::Notification {
                            text: "Too far from the machine".into(),
                            severity: 1,
                        },
                    });
                    continue;
                }
                machine.viewer = Some(owner.0);
                screens.write(ToClients {
                    targets: SendTargets::Single(*client_id),
                    message: screen_message(&content, &machine),
                });
            }
            ClientIntent::SetMachineRecipe {
                machine: pos,
                recipe,
            } => {
                let Some(&entity) = index.by_position.get(pos) else {
                    continue;
                };
                let Ok(mut machine) = machines.get_mut(entity) else {
                    continue;
                };
                if machine.viewer != Some(owner.0) {
                    continue;
                }
                if *recipe != ContentId::NONE {
                    let Some(def) = content.recipe(*recipe) else {
                        continue;
                    };
                    if def.machine != machine.machine {
                        continue;
                    }
                }
                machine.state.recipe = if *recipe == ContentId::NONE {
                    None
                } else {
                    Some(*recipe)
                };
                machine.state.progress = 0.0;
                screens.write(ToClients {
                    targets: SendTargets::Single(*client_id),
                    message: screen_message(&content, &machine),
                });
            }
            ClientIntent::CloseMachine => {
                for mut machine in &mut machines {
                    if machine.viewer == Some(owner.0) {
                        machine.viewer = None;
                    }
                }
            }
            _ => {}
        }
        let _ = state;
    }
}

/// Builds the machine screen message.
pub fn screen_message(content: &ContentRegistry, machine: &MachineEntity) -> MachineScreen {
    MachineScreen {
        pos: machine.position,
        machine: machine.machine,
        inputs: machine.inventory.inputs.clone(),
        outputs: machine.inventory.outputs.clone(),
        fuel: machine.inventory.fuel.clone(),
        modules: machine.inventory.modules.clone(),
        progress: machine.state.fraction(),
        idle: machine.state.idle.message(content),
        recipe: machine.state.recipe,
        accessible: true,
    }
}

/// Pushes a fresh screen to every player who has a machine open.
pub fn refresh_open_screens(
    machines: Query<&MachineEntity, Changed<MachineEntity>>,
    players: Query<&OwnedBy>,
    content: Res<ContentRegistry>,
    mut screens: MessageWriter<ToClients<MachineScreen>>,
) {
    for machine in &machines {
        let Some(viewer) = machine.viewer else {
            continue;
        };
        let Ok(owner) = players.get(viewer) else {
            continue;
        };
        screens.write(ToClients {
            targets: SendTargets::Single(ClientId::Client(owner.0)),
            message: screen_message(&content, machine),
        });
    }
}

/// Removes a machine and its block when it is broken.
pub fn remove_machine(
    commands: &mut Commands,
    chunks: &mut LoadedChunks,
    manager: &mut WorldManager,
    index: &mut BlockEntityIndex,
    content: &ContentRegistry,
    entity: Entity,
    machine: &MachineEntity,
) {
    let def = content.machine(machine.machine);
    let size = def.map_or((1, 1, 1), |def| def.size);
    for dy in 0..size.1 as i32 {
        for dz in 0..size.2 as i32 {
            for dx in 0..size.0 as i32 {
                let pos = machine.position.offset(dx, dy, dz);
                chunks.set_block(pos, content.air.0);
                manager.record_edit(pos, content.air.0);
            }
        }
    }
    index.by_position.remove(&machine.position);
    commands.entity(entity).despawn();
}

/// A chest's slot count.
pub const CRATE_SLOTS: usize = 27;

/// Places a chest.
pub fn place_crate(
    commands: &mut Commands,
    index: &mut BlockEntityIndex,
    position: BlockPos,
) -> Entity {
    let entity = commands
        .spawn((
            CrateEntity {
                position,
                slots: vec![ItemStack::EMPTY; CRATE_SLOTS],
            },
            BlockEntityKind::Crate,
            Replicated,
        ))
        .id();
    index.by_position.insert(position, entity);
    entity
}

/// The machine whose kit an item places, if any.
pub fn kit_machine(content: &ContentRegistry, item: ContentId) -> Option<ContentId> {
    match content.item(item).map(|item| &item.category) {
        Some(ItemCategory::MachineKit(machine)) => content.machine_ids.get(machine),
        _ => None,
    }
}

/// The block placed by an item, if any.
pub fn item_block(content: &ContentRegistry, item: ContentId) -> Option<ContentId> {
    content.item(item).and_then(|item| item.places_block)
}

/// Faces a machine is rotated to, as a block face.
pub fn facing_face(facing: u8) -> BlockFace {
    match facing % 4 {
        0 => BlockFace::North,
        1 => BlockFace::East,
        2 => BlockFace::South,
        _ => BlockFace::West,
    }
}

/// Moves items between a player's inventory and an open machine.
pub fn machine_transfer(
    content: &ContentRegistry,
    machine: &mut MachineEntity,
    player: &mut PlayerState,
    from: deepcore_shared::protocol::SlotAddress,
    to: deepcore_shared::protocol::SlotAddress,
    count: Option<u32>,
) {
    use deepcore_shared::protocol::ContainerId;
    let (source, destination) = (from, to);
    // Player → machine.
    if matches!(
        source.container,
        ContainerId::Hotbar | ContainerId::Backpack
    ) && !matches!(
        destination.container,
        ContainerId::Hotbar | ContainerId::Backpack
    ) {
        let Some(slot) = player_slot(source) else {
            return;
        };
        let Some(stack) = player.inventory.get(slot).copied() else {
            return;
        };
        if stack.is_empty() {
            return;
        }
        let machine_slots: &mut Vec<ItemStack> = match destination.container {
            ContainerId::MachineInput => &mut machine.inventory.inputs,
            ContainerId::MachineFuel => &mut machine.inventory.fuel,
            ContainerId::MachineModule => &mut machine.inventory.modules,
            _ => return,
        };
        let Some(target) = machine_slots.get_mut(destination.index as usize) else {
            return;
        };
        let max_stack = content.item(stack.item).map_or(64, |item| item.stack);
        let mut from_stack = stack;
        let moved = deepcore_shared::inventory::transfer(
            &mut from_stack,
            target,
            deepcore_shared::inventory::SlotKind::Any,
            max_stack,
            count,
        );
        if moved > 0
            && let Some(player_slot) = player.inventory.get_mut(slot)
        {
            *player_slot = from_stack;
        }
    } else if !matches!(
        source.container,
        ContainerId::Hotbar | ContainerId::Backpack
    ) && matches!(
        destination.container,
        ContainerId::Hotbar | ContainerId::Backpack
    ) {
        // Machine → player.
        let Some(target) = player_slot(destination) else {
            return;
        };
        let machine_slots: &mut Vec<ItemStack> = match source.container {
            ContainerId::MachineOutput => &mut machine.inventory.outputs,
            ContainerId::MachineInput => &mut machine.inventory.inputs,
            ContainerId::MachineFuel => &mut machine.inventory.fuel,
            ContainerId::MachineModule => &mut machine.inventory.modules,
            _ => return,
        };
        let Some(source_slot) = machine_slots.get_mut(source.index as usize) else {
            return;
        };
        if source_slot.is_empty() {
            return;
        }
        let item = source_slot.item;
        let amount = count.unwrap_or(source_slot.count);
        let taken = source_slot.take(amount);
        let leftover = player.inventory.insert(item, taken.count, content);
        if leftover > 0 {
            source_slot.count += leftover;
            source_slot.item = item;
        }
        let _ = target;
    }
}

fn player_slot(address: deepcore_shared::protocol::SlotAddress) -> Option<SlotRef> {
    use deepcore_shared::protocol::ContainerId;
    match address.container {
        ContainerId::Hotbar => Some(SlotRef::Hotbar(address.index)),
        ContainerId::Backpack => Some(SlotRef::Backpack(address.index)),
        _ => None,
    }
}
