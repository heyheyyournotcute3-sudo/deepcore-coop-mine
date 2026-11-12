//! Power networks: steam pipes and electric cables.
//!
//! Topology is recomputed only when a pipe, cable or machine is placed or removed. Every tick
//! the solver only sums numbers per network:
//!
//! * **Steam**: boilers produce units per second, consumers draw. Supply is spread to consumers
//!   in proportion to demand; if supply is short every consumer runs at `supply / demand`.
//! * **Electric**: generators produce kW, consumers draw kW. Surplus charges batteries, a deficit
//!   is covered by batteries, and if that is still not enough every consumer runs at
//!   `supply / demand` — the brownout.
//!
//! Batteries are modelled as a per-network energy store in kWh.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::voxel::BlockPos;

/// Registers power resources.
pub struct PowerPlugin;

impl Plugin for PowerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PowerNetworks>();
    }
}

/// Which kind of network an entity belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkKind {
    /// Steam pipes.
    Steam,
    /// Electric cables.
    Electric,
}

/// A producer on a network.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Producer {
    /// The entity or machine that produces.
    pub source: Entity,
    /// Steam units per second, or kW.
    pub amount: f32,
}

/// A consumer on a network.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Consumer {
    /// The entity or machine that consumes.
    pub sink: Entity,
    /// Steam units per second, or kW, at full speed.
    pub demand: f32,
}

/// A battery store on an electric network.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    /// The battery entity.
    pub source: Entity,
    /// Capacity in kWh.
    pub capacity_kwh: f32,
    /// Current charge in kWh.
    pub charge_kwh: f32,
}

/// One connected network.
#[derive(Debug, Clone, Default)]
pub struct Network {
    /// Producers.
    pub producers: Vec<Producer>,
    /// Consumers.
    pub consumers: Vec<Consumer>,
    /// Batteries.
    pub batteries: Vec<Battery>,
    /// Total production (units/s or kW).
    pub supply: f32,
    /// Total demand (units/s or kW).
    pub demand: f32,
    /// Fraction of demand that can be met, 0..1.
    pub satisfaction: f32,
    /// Energy stored across batteries, kWh.
    pub stored_kwh: f32,
    /// Energy charged this tick, kWh (negative while discharging).
    pub flow_kwh: f32,
}

impl Network {
    /// Recomputes supply, demand and satisfaction from the members.
    pub fn solve(&mut self, dt_hours: f32) {
        self.supply = self.producers.iter().map(|producer| producer.amount).sum();
        self.demand = self.consumers.iter().map(|consumer| consumer.demand).sum();
        let capacity: f32 = self.batteries.iter().map(|battery| battery.capacity_kwh).sum();
        // Read the store from the batteries before deciding, so a network that was just built
        // (or loaded from a save) does not start by thinking its batteries are empty.
        self.stored_kwh = self.batteries.iter().map(|battery| battery.charge_kwh).sum();

        if self.demand <= 0.0001 {
            self.satisfaction = 1.0;
            // Charge from whatever the generators produce.
            self.charge(dt_hours, self.supply, capacity);
        } else if self.supply >= self.demand {
            self.satisfaction = 1.0;
            self.charge(dt_hours, self.supply - self.demand, capacity);
        } else {
            let deficit = self.demand - self.supply;
            let available = self.discharge(dt_hours, deficit);
            let covered = self.supply + available;
            self.satisfaction = (covered / self.demand).clamp(0.0, 1.0);
        }
        self.stored_kwh = self.batteries.iter().map(|battery| battery.charge_kwh).sum();
    }

    fn charge(&mut self, dt_hours: f32, surplus: f32, capacity: f32) {
        if surplus <= 0.0 || capacity <= 0.0 {
            self.flow_kwh = 0.0;
            return;
        }
        let energy = surplus * dt_hours;
        let room = capacity - self.stored_kwh;
        let stored = energy.min(room);
        self.flow_kwh = stored;
        distribute_charge(&mut self.batteries, stored);
    }

    fn discharge(&mut self, dt_hours: f32, deficit: f32) -> f32 {
        if self.stored_kwh <= 0.0 {
            self.flow_kwh = 0.0;
            return 0.0;
        }
        let energy = deficit * dt_hours;
        let available = self.stored_kwh.min(energy);
        // Power delivered back to the network, in kW.
        let delivered = if dt_hours > 0.0 {
            available / dt_hours
        } else {
            0.0
        };
        self.flow_kwh = -available;
        distribute_charge(&mut self.batteries, -available);
        delivered
    }
}

fn distribute_charge(batteries: &mut [Battery], energy: f32) {
    if batteries.is_empty() {
        return;
    }
    let total_capacity: f32 = batteries.iter().map(|battery| battery.capacity_kwh).sum();
    if total_capacity <= 0.0 {
        return;
    }
    for battery in batteries.iter_mut() {
        let share = battery.capacity_kwh / total_capacity;
        battery.charge_kwh = (battery.charge_kwh + energy * share).clamp(0.0, battery.capacity_kwh);
    }
}

/// All networks in the world, keyed by network id.
#[derive(Debug, Default, Resource)]
pub struct PowerNetworks {
    networks: HashMap<u32, Network>,
    next_id: u32,
    /// Maps a member entity to the network it belongs to, per kind.
    membership: HashMap<(NetworkKind, Entity), u32>,
}

impl PowerNetworks {
    /// Creates an empty network and returns its id.
    pub fn create(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.networks.insert(id, Network::default());
        id
    }

    /// Borrows a network.
    pub fn get(&self, id: u32) -> Option<&Network> {
        self.networks.get(&id)
    }

    /// Mutably borrows a network.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut Network> {
        self.networks.get_mut(&id)
    }

    /// Iterates `(id, network)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Network)> {
        self.networks.iter().map(|(id, network)| (*id, network))
    }

    /// Number of networks.
    pub fn len(&self) -> usize {
        self.networks.len()
    }

    /// Whether no network exists.
    pub fn is_empty(&self) -> bool {
        self.networks.is_empty()
    }

    /// Adds a producer to a network.
    pub fn add_producer(&mut self, network: u32, kind: NetworkKind, producer: Producer) {
        self.membership.insert((kind, producer.source), network);
        if let Some(network) = self.networks.get_mut(&network) {
            network.producers.push(producer);
        }
    }

    /// Adds a consumer to a network.
    pub fn add_consumer(&mut self, network: u32, kind: NetworkKind, consumer: Consumer) {
        self.membership.insert((kind, consumer.sink), network);
        if let Some(network) = self.networks.get_mut(&network) {
            network.consumers.push(consumer);
        }
    }

    /// Adds a battery to a network.
    pub fn add_battery(&mut self, network: u32, battery: Battery) {
        self.membership
            .insert((NetworkKind::Electric, battery.source), network);
        if let Some(network) = self.networks.get_mut(&network) {
            network.batteries.push(battery);
        }
    }

    /// The network a member belongs to.
    pub fn network_of(&self, kind: NetworkKind, member: Entity) -> Option<u32> {
        self.membership.get(&(kind, member)).copied()
    }

    /// The satisfaction of the network a member belongs to (1.0 when it has no network).
    pub fn satisfaction_for(&self, kind: NetworkKind, member: Entity) -> f32 {
        self.network_of(kind, member)
            .and_then(|id| self.networks.get(&id))
            .map_or(1.0, |network| network.satisfaction)
    }

    /// Removes every member of a network (used when a network is destroyed).
    pub fn remove_network(&mut self, id: u32) {
        if let Some(network) = self.networks.remove(&id) {
            self.membership.retain(|_, network_id| *network_id != id);
            for consumer in network.consumers {
                self.membership
                    .remove(&(NetworkKind::Electric, consumer.sink));
            }
        }
    }

    /// Solves every network for one tick.
    pub fn solve_all(&mut self, dt: f32) {
        let dt_hours = dt / 3600.0;
        for network in self.networks.values_mut() {
            network.solve(dt_hours);
        }
    }
}

/// Flood-fills connected pipe or cable blocks to build networks.
///
/// `is_conductor` answers whether a block position carries the network (a pipe or a cable), and
/// `members` are the machine entities attached to a conductor position.
pub fn flood_fill_network(
    start: BlockPos,
    is_conductor: &dyn Fn(BlockPos) -> bool,
) -> Vec<BlockPos> {
    let mut visited = vec![start];
    let mut stack = vec![start];
    while let Some(pos) = stack.pop() {
        for face in crate::voxel::BlockFace::ALL {
            let neighbor = face.neighbor(pos);
            if visited.contains(&neighbor) {
                continue;
            }
            if is_conductor(neighbor) {
                visited.push(neighbor);
                stack.push(neighbor);
            }
        }
    }
    visited
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network_with(supply: f32, demand: f32) -> Network {
        Network {
            producers: vec![Producer {
                source: Entity::from_raw_u32(1).unwrap(),
                amount: supply,
            }],
            consumers: vec![Consumer {
                sink: Entity::from_raw_u32(2).unwrap(),
                demand,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn surplus_runs_everything_and_charges() {
        let mut network = network_with(100.0, 40.0);
        network.batteries.push(Battery {
            source: Entity::from_raw_u32(3).unwrap(),
            capacity_kwh: 10.0,
            charge_kwh: 0.0,
        });
        network.solve(1.0 / 3600.0);
        assert_eq!(network.satisfaction, 1.0);
        assert!(network.stored_kwh > 0.0);
    }

    #[test]
    fn brownout_scales_consumers() {
        let mut network = network_with(50.0, 100.0);
        network.solve(1.0 / 3600.0);
        assert!((network.satisfaction - 0.5).abs() < 1e-5);
    }

    #[test]
    fn batteries_cover_a_deficit() {
        let mut network = network_with(50.0, 100.0);
        network.batteries.push(Battery {
            source: Entity::from_raw_u32(3).unwrap(),
            capacity_kwh: 100.0,
            charge_kwh: 50.0,
        });
        // One second of a 50 kW deficit is 50/3600 kWh, comfortably inside the stored energy.
        network.solve(1.0 / 3600.0);
        assert_eq!(network.satisfaction, 1.0);
        assert!(network.stored_kwh < 50.0);
    }

    #[test]
    fn batteries_run_out_and_then_brownout() {
        let mut network = network_with(50.0, 100.0);
        network.batteries.push(Battery {
            source: Entity::from_raw_u32(3).unwrap(),
            capacity_kwh: 100.0,
            charge_kwh: 0.5,
        });
        // One minute at 50 kW of deficit needs 0.83 kWh: more than the battery holds.
        network.solve(60.0 / 3600.0);
        assert!(network.satisfaction < 1.0);
        assert!(network.satisfaction > 0.5);
    }

    #[test]
    fn empty_batteries_do_not_invent_power() {
        let mut network = network_with(0.0, 10.0);
        network.batteries.push(Battery {
            source: Entity::from_raw_u32(3).unwrap(),
            capacity_kwh: 10.0,
            charge_kwh: 0.0,
        });
        network.solve(1.0 / 3600.0);
        assert_eq!(network.satisfaction, 0.0);
    }

    #[test]
    fn flood_fill_finds_a_line_of_pipes() {
        let start = BlockPos::new(0, 0, 0);
        let is_conductor = |pos: BlockPos| pos.x >= 0 && pos.x < 5 && pos.y == 0 && pos.z == 0;
        let found = flood_fill_network(start, &is_conductor);
        assert_eq!(found.len(), 5);
    }
}
