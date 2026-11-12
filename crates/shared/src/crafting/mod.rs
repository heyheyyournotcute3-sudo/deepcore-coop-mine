//! Crafting: hand recipes at the workbench, and the tech tree that gates everything else.

use bevy::prelude::*;

use crate::content::{ContentId, ContentRegistry};
use crate::inventory::Inventory;

pub mod machine;
pub mod mining;

pub use machine::{
    IdleReason, MachineInventory, MachineState, machine_draw_kw, match_recipe, tick_machine,
};
pub use mining::{
    DRILL_REACH, MiningVerdict, REACH, TICK_HZ, TICK_SECONDS, ToolStats, area_blocks,
    break_time_seconds, break_time_ticks, evaluate_mining, placement_position,
};

/// Registers crafting resources.
pub struct CraftingPlugin;

impl Plugin for CraftingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TechTree>();
    }
}

/// The team-shared tech tree state.
#[derive(Debug, Default, Resource, Clone)]
pub struct TechTree {
    /// Nodes that have been researched.
    researched: Vec<ContentId>,
    /// Items delivered to each in-progress node.
    deposits: Vec<(ContentId, Vec<(ContentId, u32)>)>,
}

impl TechTree {
    /// Whether a node is researched.
    pub fn is_researched(&self, node: ContentId) -> bool {
        self.researched.contains(&node)
    }

    /// Whether a node's parents are all researched (so it may be deposited into).
    pub fn is_available(&self, content: &ContentRegistry, node: ContentId) -> bool {
        if self.is_researched(node) {
            return false;
        }
        content
            .tech(node)
            .map(|tech| {
                tech.parents
                    .iter()
                    .all(|parent| self.is_researched(*parent))
            })
            .unwrap_or(false)
    }

    /// Whether content is unlocked: either it needs no tech, or its tech is researched.
    pub fn unlocks_content(&self, tech: Option<ContentId>) -> bool {
        match tech {
            None => true,
            Some(tech) => self.is_researched(tech),
        }
    }

    /// The set of unlocked content ids, for the client's UI.
    pub fn unlocked_set(&self) -> Vec<ContentId> {
        self.researched.clone()
    }

    /// Researched nodes.
    pub fn researched(&self) -> &[ContentId] {
        &self.researched
    }

    /// Marks a node researched (used by the save loader).
    pub fn set_researched(&mut self, nodes: Vec<ContentId>) {
        self.researched = nodes;
    }

    /// Items already delivered toward a node.
    pub fn deposited(&self, node: ContentId, item: ContentId) -> u32 {
        self.deposits
            .iter()
            .find(|(node_id, _)| *node_id == node)
            .and_then(|(_, items)| {
                items
                    .iter()
                    .find(|(item_id, _)| *item_id == item)
                    .map(|(_, count)| *count)
            })
            .unwrap_or(0)
    }

    /// Delivers items toward a node, taking them out of the player's inventory.
    ///
    /// Returns `Ok(true)` when the node completed, `Ok(false)` when the deposit was accepted but
    /// the node is not finished, and `Err` with a reason when the deposit is invalid.
    pub fn deposit(
        &mut self,
        content: &ContentRegistry,
        node: ContentId,
        offered: &[(ContentId, u32)],
        inventory: &mut Inventory,
    ) -> Result<bool, String> {
        let Some(tech) = content.tech(node) else {
            return Err("unknown tech node".into());
        };
        if self.is_researched(node) {
            return Err(format!("{} is already researched", tech.name));
        }
        if !self.is_available(content, node) {
            return Err(format!("{} needs its parent research first", tech.name));
        }
        for (item, count) in offered {
            if inventory.count_of(*item) < *count {
                return Err(format!(
                    "not enough {}",
                    content
                        .item(*item)
                        .map_or("item", |item| item.name.as_str())
                ));
            }
        }
        for (item, count) in offered {
            inventory.remove(*item, *count);
            let entry = self
                .deposits
                .iter_mut()
                .find(|(node_id, _)| *node_id == node);
            match entry {
                Some((_, items)) => match items.iter_mut().find(|(item_id, _)| item_id == item) {
                    Some((_, total)) => *total += count,
                    None => items.push((*item, *count)),
                },
                None => self.deposits.push((node, vec![(*item, *count)])),
            }
        }
        let complete = tech
            .cost
            .iter()
            .all(|(item, count)| self.deposited(node, *item) >= *count);
        if complete {
            self.researched.push(node);
            self.deposits.retain(|(node_id, _)| *node_id != node);
        }
        Ok(complete)
    }

    /// Total progress of a node, 0..1.
    pub fn progress(&self, content: &ContentRegistry, node: ContentId) -> f32 {
        let Some(tech) = content.tech(node) else {
            return 0.0;
        };
        if self.is_researched(node) {
            return 1.0;
        }
        if tech.cost.is_empty() {
            return 0.0;
        }
        let total: u32 = tech.cost.iter().map(|(_, count)| *count).sum();
        if total == 0 {
            return 0.0;
        }
        let delivered: u32 = tech
            .cost
            .iter()
            .map(|(item, count)| self.deposited(node, *item).min(*count))
            .sum();
        delivered as f32 / total as f32
    }
}

/// The outcome of a hand-crafting attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CraftError {
    /// No recipe with that id exists in the hand machine.
    UnknownRecipe,
    /// The recipe is locked behind research.
    Locked(String),
    /// The player does not have the inputs.
    MissingItems(String),
    /// There is no room for the outputs.
    NoRoom,
}

/// Crafting at the workbench: instant, takes the inputs, gives the outputs.
pub fn craft_hand(
    content: &ContentRegistry,
    tech: &TechTree,
    recipe: ContentId,
    count: u32,
    inventory: &mut Inventory,
) -> Result<u32, CraftError> {
    let def = content.recipe(recipe).ok_or(CraftError::UnknownRecipe)?;
    if def.machine != content.hand_machine {
        return Err(CraftError::UnknownRecipe);
    }
    if !tech.unlocks_content(def.tech) {
        return Err(CraftError::Locked(
            def.tech
                .and_then(|tech| content.tech(tech))
                .map_or_else(|| "research".into(), |tech| tech.name.clone()),
        ));
    }
    let batches = count.max(1);
    for (item, needed) in &def.inputs {
        let required = needed * batches;
        if inventory.count_of(*item) < required {
            return Err(CraftError::MissingItems(
                content
                    .item(*item)
                    .map_or_else(|| "item".into(), |item| item.name.clone()),
            ));
        }
    }
    for (item, produced) in &def.outputs {
        if !inventory.can_insert(*item, produced * batches, content) {
            return Err(CraftError::NoRoom);
        }
    }
    for (item, needed) in &def.inputs {
        inventory.remove(*item, needed * batches);
    }
    for (item, produced) in &def.outputs {
        inventory.insert(*item, produced * batches, content);
    }
    Ok(batches)
}

/// Whether the player can craft a recipe right now, without doing it.
pub fn can_craft(
    content: &ContentRegistry,
    tech: &TechTree,
    recipe: ContentId,
    count: u32,
    inventory: &Inventory,
) -> bool {
    let Some(def) = content.recipe(recipe) else {
        return false;
    };
    if !tech.unlocks_content(def.tech) {
        return false;
    }
    def.inputs
        .iter()
        .all(|(item, needed)| inventory.count_of(*item) >= needed * count.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    /// A tech tree with the root node researched, which is the state a new team starts in
    /// once the first Workbench is built.
    fn researched_root(content: &ContentRegistry) -> TechTree {
        let mut tech = TechTree::default();
        let root = content.tech_ids.get("hand_tools").expect("root node");
        tech.set_researched(vec![root]);
        tech
    }

    #[test]
    fn hand_craft_consumes_and_produces() {
        let content = content();
        let tech = researched_root(&content);
        let recipe = content
            .recipe_ids
            .get("craft_workbench")
            .expect("workbench recipe");
        let def = content.recipe(recipe).unwrap();
        let mut inventory = Inventory::default();
        for (item, count) in &def.inputs {
            inventory.insert(*item, *count + 5, &content);
        }
        let crafted = craft_hand(&content, &tech, recipe, 1, &mut inventory).expect("craft works");
        assert_eq!(crafted, 1);
        for (item, count) in &def.outputs {
            assert!(inventory.count_of(*item) >= *count);
        }
    }

    #[test]
    fn locked_recipe_is_refused() {
        let content = content();
        let tech = TechTree::default();
        let recipe = content
            .recipe_ids
            .get("craft_kit_blast_furnace")
            .expect("recipe exists");
        let def = content.recipe(recipe).unwrap();
        assert!(def.tech.is_some());
        let mut inventory = Inventory::default();
        for (item, count) in &def.inputs {
            inventory.insert(*item, *count, &content);
        }
        let error = craft_hand(&content, &tech, recipe, 1, &mut inventory).unwrap_err();
        assert!(matches!(error, CraftError::Locked(_)));
    }

    #[test]
    fn research_requires_parents_and_items() {
        let content = content();
        let mut tech = TechTree::default();
        let smelting = content.tech_ids.get("smelting").expect("smelting node");
        let node = content.tech(smelting).unwrap();
        let mut inventory = Inventory::default();
        // Parents missing.
        let error = tech.deposit(&content, smelting, &node.cost, &mut inventory);
        assert!(error.is_err());

        // Research the root first.
        let root = content.tech_ids.get("hand_tools").expect("root node");
        let root_def = content.tech(root).unwrap();
        for (item, count) in &root_def.cost {
            inventory.insert(*item, *count, &content);
        }
        let done = tech
            .deposit(&content, root, &root_def.cost, &mut inventory)
            .expect("root deposits");
        assert!(done);

        // Now smelting is available but needs its items.
        for (item, count) in &node.cost {
            inventory.insert(*item, *count, &content);
        }
        let done = tech
            .deposit(&content, smelting, &node.cost, &mut inventory)
            .expect("smelting deposits");
        assert!(done);
        assert!(tech.is_researched(smelting));
    }
}
