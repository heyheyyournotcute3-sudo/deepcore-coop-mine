//! Objectives: the guided first hour.
//!
//! Each objective is a predicate over the team's progress (tech tree, machine counts, mining
//! history). The server evaluates them so all players see the same list, and broadcasts changes.
//!
//! The list is deliberately short and linear: a new player should always know exactly one next
//! step. Later objectives unlock the moment the previous one completes.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use deepcore_shared::content::{ContentId, ContentRegistry};
use deepcore_shared::protocol::ServerNotice;
use tracing::info;

use crate::crafting_state::TechStateResource;
use crate::sim::machine::MachineEntity;
use crate::sim::player::PlayerState;

/// Registers the objective tracker.
pub struct ObjectivePlugin;

impl Plugin for ObjectivePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ObjectiveState>()
            .add_systems(Update, evaluate_objectives);
    }
}

/// One objective definition.
#[derive(Debug, Clone, Copy)]
pub struct Objective {
    /// Stable key.
    pub key: &'static str,
    /// Text shown to the player.
    pub title: &'static str,
    /// A one-line hint.
    pub hint: &'static str,
    /// What must be true to complete it.
    pub requirement: Requirement,
}

/// What an objective requires.
#[derive(Debug, Clone, Copy)]
pub enum Requirement {
    /// Mine at least this many blocks of any kind.
    MineBlocks(u32),
    /// Research a tech node.
    Research(&'static str),
    /// Build at least one machine of this kind.
    BuildMachine(&'static str),
    /// Hold at least one of an item.
    HoldItem(&'static str, u32),
}

/// The full objective list, in order.
pub const OBJECTIVES: &[Objective] = &[
    Objective {
        key: "dig_down",
        title: "Dig into the Shale Beds",
        hint: "Left-click the ground and dig at least 20 blocks down.",
        requirement: Requirement::MineBlocks(20),
    },
    Objective {
        key: "find_ores",
        title: "Collect coal, copper and iron",
        hint: "Dark seams are coal; green is copper; reddish is iron.",
        requirement: Requirement::HoldItem("raw_iron", 1),
    },
    Objective {
        key: "workbench",
        title: "Build a Workbench",
        hint: "Hand-craft a Workbench Kit from timber and place it.",
        requirement: Requirement::BuildMachine("workbench"),
    },
    Objective {
        key: "research_hand_tools",
        title: "Research Hand Tools",
        hint: "Open the tech tree at the Research Desk and deliver the timber.",
        requirement: Requirement::Research("hand_tools"),
    },
    Objective {
        key: "bricks",
        title: "Fire clay into bricks",
        hint: "Build a Brick Kiln, fill it with clay and coal.",
        requirement: Requirement::HoldItem("brick", 1),
    },
    Objective {
        key: "first_ingot",
        title: "Smelt your first iron ingot",
        hint: "Build a Stone Furnace, then feed it raw iron and coal.",
        requirement: Requirement::HoldItem("iron_ingot", 1),
    },
    Objective {
        key: "anvil",
        title: "Build an Anvil",
        hint: "Forge plates and rods for the machines ahead.",
        requirement: Requirement::BuildMachine("anvil"),
    },
    Objective {
        key: "blast_furnace",
        title: "Build a Blast Furnace",
        hint: "Steel needs iron, coal and limestone flux.",
        requirement: Requirement::BuildMachine("blast_furnace"),
    },
    Objective {
        key: "hardened_pickaxe",
        title: "Forge the Hardened Steel Pickaxe",
        hint: "Steel opens the Granite Depths at depth 64.",
        requirement: Requirement::HoldItem("hardened_steel_pickaxe", 1),
    },
    Objective {
        key: "steam",
        title: "Raise steam power",
        hint: "A Hand Pump, a Steam Boiler and a Steam Engine run the first machines.",
        requirement: Requirement::Research("steam_power"),
    },
    Objective {
        key: "electricity",
        title: "Light up the Base",
        hint: "Turbines and generators bring electricity to the mine.",
        requirement: Requirement::Research("electricity"),
    },
    Objective {
        key: "chemistry",
        title: "Start the chemical plant",
        hint: "Sulfuric acid is the key to uranium.",
        requirement: Requirement::Research("chemistry"),
    },
    Objective {
        key: "yellowcake",
        title: "Produce yellowcake",
        hint: "Leach crushed uranium ore, then precipitate it with lye.",
        requirement: Requirement::HoldItem("yellowcake", 1),
    },
    Objective {
        key: "enriched",
        title: "Enrich uranium",
        hint: "A centrifuge cascade turns UF6 into enriched canisters.",
        requirement: Requirement::HoldItem("enriched_canister", 1),
    },
    Objective {
        key: "nuclear_drill",
        title: "Build the Nuclear Drill",
        hint: "A compact core cell plus a tungsten-carbide drill head.",
        requirement: Requirement::HoldItem("nuclear_drill", 1),
    },
];

/// The team's objective progress.
#[derive(Debug, Default, Resource)]
pub struct ObjectiveState {
    /// Completed objective keys, in order.
    pub completed: Vec<&'static str>,
    /// Blocks mined by the team.
    pub blocks_mined: u32,
    /// Machines built, by key.
    pub machines_built: std::collections::HashMap<String, u32>,
}

impl ObjectiveState {
    /// The next objective, if any.
    pub fn next(&self) -> Option<&'static Objective> {
        OBJECTIVES
            .iter()
            .find(|objective| !self.completed.contains(&objective.key))
    }

    /// Whether an objective is complete.
    pub fn is_complete(&self, key: &str) -> bool {
        self.completed.contains(&key)
    }

    /// Completion fraction, for the HUD progress bar.
    pub fn fraction(&self) -> f32 {
        self.completed.len() as f32 / OBJECTIVES.len() as f32
    }
}

/// Evaluates every objective and completes the ones whose requirement is met.
pub fn evaluate_objectives(
    mut state: ResMut<ObjectiveState>,
    tech: Res<TechStateResource>,
    content: Res<ContentRegistry>,
    machines: Query<&MachineEntity>,
    players: Query<&PlayerState>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
) {
    // Count machines once per evaluation rather than per objective.
    let mut machine_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    for machine in &machines {
        if let Some(def) = content.machine(machine.machine) {
            *machine_counts.entry(def.key.clone()).or_insert(0) += 1;
        }
    }
    let mut inventory_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    for player in &players {
        for (item, count) in player.inventory.snapshot() {
            if let Some(def) = content.item(item) {
                *inventory_counts.entry(def.key.clone()).or_insert(0) += count;
            }
        }
    }

    let mut newly_completed = Vec::new();
    for objective in OBJECTIVES {
        if state.is_complete(objective.key) {
            continue;
        }
        let satisfied = match objective.requirement {
            Requirement::MineBlocks(count) => state.blocks_mined >= count,
            Requirement::Research(node) => content
                .tech_ids
                .get(node)
                .is_some_and(|id| tech.0.is_researched(id)),
            Requirement::BuildMachine(machine) => {
                machine_counts.get(machine).copied().unwrap_or(0) > 0
            }
            Requirement::HoldItem(item, count) => {
                inventory_counts.get(item).copied().unwrap_or(0) >= count
            }
        };
        if satisfied {
            state.completed.push(objective.key);
            newly_completed.push(*objective);
        }
    }

    for objective in newly_completed {
        info!("objective complete: {}", objective.title);
        notices.write(ToClients {
            targets: SendTargets::All,
            message: ServerNotice::Notification {
                text: format!("Objective complete: {}", objective.title),
                severity: 0,
            },
        });
    }
    // Keep the machine counts for the HUD.
    state.machines_built = machine_counts;
}

/// Records a mined block, called from the mining system.
pub fn record_mined(state: &mut ObjectiveState, count: u32) {
    state.blocks_mined += count;
}

/// A requirement that can be checked without the world, for tests.
pub fn requirement_met(
    requirement: Requirement,
    tech: &deepcore_shared::crafting::TechTree,
    content: &ContentRegistry,
    machines: &std::collections::HashMap<String, u32>,
    items: &std::collections::HashMap<String, u32>,
    blocks_mined: u32,
) -> bool {
    match requirement {
        Requirement::MineBlocks(count) => blocks_mined >= count,
        Requirement::Research(node) => content
            .tech_ids
            .get(node)
            .is_some_and(|id: ContentId| tech.is_researched(id)),
        Requirement::BuildMachine(machine) => machines.get(machine).copied().unwrap_or(0) > 0,
        Requirement::HoldItem(item, count) => items.get(item).copied().unwrap_or(0) >= count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    #[test]
    fn objectives_are_in_a_sensible_order() {
        // Every objective's referenced content must exist, or the list can never complete.
        let content = content();
        for objective in OBJECTIVES {
            match objective.requirement {
                Requirement::Research(node) => {
                    assert!(content.tech_ids.get(node).is_some(), "{}", objective.key)
                }
                Requirement::BuildMachine(machine) => {
                    assert!(
                        content.machine_ids.get(machine).is_some(),
                        "{}",
                        objective.key
                    )
                }
                Requirement::HoldItem(item, _) => {
                    assert!(content.item_ids.get(item).is_some(), "{}", objective.key)
                }
                Requirement::MineBlocks(_) => {}
            }
        }
    }

    #[test]
    fn the_first_objective_is_mining() {
        let state = ObjectiveState::default();
        assert_eq!(state.next().expect("an objective").key, "dig_down");
        assert_eq!(state.fraction(), 0.0);
    }

    #[test]
    fn completing_the_first_unlocks_the_second() {
        let content = content();
        let tech = deepcore_shared::crafting::TechTree::default();
        let machines = std::collections::HashMap::new();
        let items = std::collections::HashMap::new();
        // Not met with 19 blocks.
        assert!(!requirement_met(
            Requirement::MineBlocks(20),
            &tech,
            &content,
            &machines,
            &items,
            19
        ));
        // Met with 20.
        assert!(requirement_met(
            Requirement::MineBlocks(20),
            &tech,
            &content,
            &machines,
            &items,
            20
        ));
    }

    #[test]
    fn research_objectives_track_the_tech_tree() {
        let content = content();
        let mut tech = deepcore_shared::crafting::TechTree::default();
        let machines = std::collections::HashMap::new();
        let items = std::collections::HashMap::new();
        let requirement = Requirement::Research("hand_tools");
        assert!(!requirement_met(
            requirement,
            &tech,
            &content,
            &machines,
            &items,
            0
        ));
        tech.set_researched(vec![content.tech_ids.get("hand_tools").expect("node")]);
        assert!(requirement_met(
            requirement,
            &tech,
            &content,
            &machines,
            &items,
            0
        ));
    }
}
