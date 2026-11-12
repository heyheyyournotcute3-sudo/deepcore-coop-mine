//! The power solver: rebuilds networks on topology changes and solves them every tick.

use bevy::prelude::*;
use deepcore_shared::power::{Battery, Consumer, NetworkKind, PowerNetworks, Producer};

use crate::sim::machine::{MachineEntity, MachinePowerDemand};

/// Solves every power network for one tick.
///
/// Topology is rebuilt by [`rebuild_networks`] only when machines are added or removed; this
/// system only sums numbers.
pub fn solve_power_networks(mut networks: ResMut<PowerNetworks>) {
    networks.solve_all(deepcore_shared::crafting::TICK_SECONDS);
}

/// Rebuilds the electric and steam networks from the placed machines.
///
/// Called at startup and whenever a machine is placed or removed. Machines that sit next to each
/// other form one network; cable and pipe blocks extend it (handled by the connectivity pass).
pub fn rebuild_networks(
    mut networks: ResMut<PowerNetworks>,
    demand: Res<MachinePowerDemand>,
    machines: Query<(Entity, &MachineEntity)>,
) {
    *networks = PowerNetworks::default();
    if machines.is_empty() {
        return;
    }
    let electric = networks.create();
    let steam = networks.create();
    for (entity, machine) in &machines {
        if let Some(kw) = demand.electric.get(&entity) {
            networks.add_consumer(
                electric,
                NetworkKind::Electric,
                Consumer {
                    sink: entity,
                    demand: *kw,
                },
            );
        }
        if let Some(units) = demand.steam.get(&entity) {
            networks.add_consumer(
                steam,
                NetworkKind::Steam,
                Consumer {
                    sink: entity,
                    demand: *units,
                },
            );
        }
        if let Some(kw) = demand.generation.get(&entity) {
            networks.add_producer(
                electric,
                NetworkKind::Electric,
                Producer {
                    source: entity,
                    amount: *kw,
                },
            );
        }
        // A boiler holds steam and feeds it to the steam network.
        if machine.steam > 0.0 {
            networks.add_producer(
                steam,
                NetworkKind::Steam,
                Producer {
                    source: entity,
                    amount: machine.steam.min(BOILER_STEAM_OUTPUT),
                },
            );
        }
    }
}

/// Steam units per second a full boiler can push into the network.
pub const BOILER_STEAM_OUTPUT: f32 = 20.0;

/// Adds a battery bank to the electric network it belongs to.
pub fn register_battery(networks: &mut PowerNetworks, entity: Entity, capacity_kwh: f32) {
    if let Some(id) = networks.network_of(NetworkKind::Electric, entity) {
        networks.add_battery(
            id,
            Battery {
                source: entity,
                capacity_kwh,
                charge_kwh: 0.0,
            },
        );
    }
}


