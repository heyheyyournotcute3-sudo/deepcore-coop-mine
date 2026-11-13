//! The simulation plugin: every authoritative gameplay system, wired in tick order.
//!
//! Order inside `FixedUpdate` (30 Hz):
//! 1. player inputs → movement
//! 2. mining actions → block breaks
//! 3. machine ticks → power demand → network solve
//! 4. hazards and vitals
//! 5. chunk streaming and message flushing
//!
//! Keeping the order fixed and explicit is what makes the simulation reproducible and the tests
//! meaningful.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::power::PowerNetworks;

use crate::crafting_state::{
    TechStateDirty, TechStateResource, broadcast_on_change, handle_research,
};
use crate::sim::chat::handle_chat;
use crate::sim::handshake::send_welcome;
use crate::sim::interaction::handle_interaction;
use crate::sim::machine::{
    BlockEntityIndex, MachinePowerDemand, compute_power_demand, refresh_open_screens, tick_machines,
};
use crate::sim::mining::{
    PendingBlockChanges, flush_block_changes, handle_place_block, handle_start_mining, tick_mining,
};
use crate::sim::player::{apply_player_inputs, tick_player_vitals};
use crate::sim::power::solve_power_networks;

pub mod chat;
pub mod handshake;
pub mod interaction;
pub mod inventory;
pub mod logistics;
pub mod machine;
pub mod mining;
pub mod objectives;
pub mod placement;
pub mod player;
pub mod power;
pub mod respawn;

/// Registers every server simulation system.
pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingBlockChanges>()
            .init_resource::<BlockEntityIndex>()
            .init_resource::<MachinePowerDemand>()
            .init_resource::<PowerNetworks>()
            .init_resource::<TechStateResource>()
            .init_resource::<TechStateDirty>()
            .init_resource::<objectives::ObjectiveState>()
            .init_resource::<logistics::Conveyors>()
            .add_systems(Startup, startup_resources);

        // Inputs and world changes run every tick.
        app.add_systems(
            FixedUpdate,
            (
                apply_player_inputs,
                tick_mining,
                tick_player_vitals,
                tick_machines,
                compute_power_demand,
                solve_power_networks,
                logistics::scan_conveyors,
                logistics::move_items_along_conveyors,
                flush_block_changes,
            )
                .chain()
                .run_if(in_state(ServerState::Running)),
        );

        // Intent handlers run on the message stream.
        app.add_systems(
            PreUpdate,
            (
                handle_start_mining,
                placement::handle_machine_placement,
                handle_place_block,
                handle_interaction,
                inventory::handle_inventory_intents,
                handle_research,
                handle_chat,
                machine::handle_machine_intents,
                respawn::handle_respawn,
            )
                .after(ServerSystems::Receive)
                .run_if(in_state(ServerState::Running)),
        );

        // Owner-only inventory sync and machine screens.
        app.add_systems(
            Update,
            (
                inventory::sync_inventories.run_if(in_state(ServerState::Running)),
                refresh_open_screens.run_if(in_state(ServerState::Running)),
                send_welcome.run_if(in_state(ServerState::Running)),
                broadcast_on_change.run_if(in_state(ServerState::Running)),
                objectives::evaluate_objectives.run_if(in_state(ServerState::Running)),
            ),
        );
    }
}

fn startup_resources(content: Res<ContentRegistry>, mut tech: ResMut<TechStateResource>) {
    // A new world starts with nothing researched; the first Workbench unlocks the root node
    // by being built, which the objective list guides the player towards.
    let _ = &mut tech;
    info!(
        "simulation ready: {} recipes, {} machines, {} tech nodes",
        content.recipes.len(),
        content.machines.len(),
        content.tech.len()
    );
}
