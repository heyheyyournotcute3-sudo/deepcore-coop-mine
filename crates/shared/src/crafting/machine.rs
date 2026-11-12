//! Recipe matching and the machine processing state machine.
//!
//! A machine is a block entity with input slots, output slots, an optional fuel slot, a current
//! recipe and a progress value. The rules here are pure: the server calls them each tick, the
//! client calls the same functions to predict the progress bar.

use crate::content::{ContentId, ContentRegistry, ItemCategory, MachineEnergy, Recipe};
use crate::inventory::ItemStack;

/// Why a machine is not running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleReason {
    /// No recipe can be started with the current inputs.
    NoRecipe,
    /// A recipe matched but an output slot is full.
    OutputFull,
    /// The machine has no power, steam or fuel.
    NoPower,
    /// The machine is waiting for a specific input.
    MissingInput(ContentId),
    /// The machine is a cascade that is too short.
    CascadeTooShort,
    /// The machine finished its work and is idle by design.
    Idle,
}

impl IdleReason {
    /// Human-readable text for the machine UI.
    pub fn message(&self, content: &ContentRegistry) -> String {
        match self {
            IdleReason::NoRecipe => "No recipe".into(),
            IdleReason::OutputFull => "Output full".into(),
            IdleReason::NoPower => "No power".into(),
            IdleReason::MissingInput(item) => format!(
                "Missing {}",
                content.item(*item).map_or("input", |item| item.name.as_str())
            ),
            IdleReason::CascadeTooShort => "Cascade too short".into(),
            IdleReason::Idle => "Idle".into(),
        }
    }
}

/// The running state of a machine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MachineState {
    /// The recipe currently running, if any.
    pub recipe: Option<ContentId>,
    /// Progress in seconds.
    pub progress: f32,
    /// Total duration of the current recipe in seconds.
    pub duration: f32,
    /// Why the machine is idle (only meaningful when `recipe` is `None`).
    pub idle: IdleReason,
    /// The machine ran this tick.
    pub running: bool,
    /// Fuel remaining in seconds.
    pub fuel_seconds: f32,
}

impl Default for MachineState {
    fn default() -> Self {
        Self {
            recipe: None,
            progress: 0.0,
            duration: 0.0,
            idle: IdleReason::Idle,
            running: false,
            fuel_seconds: 0.0,
        }
    }
}

impl MachineState {
    /// Fraction complete, 0..1, for the UI.
    pub fn fraction(&self) -> f32 {
        if self.duration <= 0.0 {
            0.0
        } else {
            (self.progress / self.duration).clamp(0.0, 1.0)
        }
    }
}

/// A machine's inventory, in the flat slot layout the wire format uses.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MachineInventory {
    /// Input slots.
    pub inputs: Vec<ItemStack>,
    /// Output slots.
    pub outputs: Vec<ItemStack>,
    /// Fuel slots.
    pub fuel: Vec<ItemStack>,
    /// Upgrade module slots (max two).
    pub modules: Vec<ItemStack>,
}

impl MachineInventory {
    /// Builds an inventory with the right slot counts for a machine.
    pub fn for_machine(content: &ContentRegistry, machine: ContentId) -> Self {
        let def = content.machine(machine);
        let input_slots = def.map_or(1, |machine| machine.input_slots.max(1)) as usize;
        let output_slots = def.map_or(1, |machine| machine.output_slots.max(1)) as usize;
        let fuel_slots = match def.map(|machine| machine.energy) {
            Some(MachineEnergy::Fuel { slots }) => slots as usize,
            _ => 0,
        };
        Self {
            inputs: vec![ItemStack::EMPTY; input_slots],
            outputs: vec![ItemStack::EMPTY; output_slots],
            fuel: vec![ItemStack::EMPTY; fuel_slots],
            modules: vec![ItemStack::EMPTY; 2],
        }
    }

    /// Counts of an item across the input slots.
    pub fn input_count(&self, item: ContentId) -> u32 {
        self.inputs
            .iter()
            .filter(|stack| stack.item == item)
            .map(|stack| stack.count)
            .sum()
    }

    /// Whether the output slots can hold `recipe`'s outputs.
    pub fn has_output_room(&self, recipe: &Recipe, content: &ContentRegistry) -> bool {
        let mut simulated = self.outputs.clone();
        for (item, count) in &recipe.outputs {
            let max_stack = content.item(*item).map_or(64, |item| item.stack);
            let mut remaining = *count;
            for stack in simulated.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if stack.item == *item && stack.count < max_stack {
                    let room = max_stack - stack.count;
                    let moved = room.min(remaining);
                    stack.count += moved;
                    remaining -= moved;
                }
            }
            for stack in simulated.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if stack.is_empty() {
                    let moved = max_stack.min(remaining);
                    *stack = ItemStack::new(*item, moved);
                    remaining -= moved;
                }
            }
            if remaining > 0 {
                return false;
            }
        }
        true
    }

    /// Consumes the recipe's inputs. Assumes [`Self::can_consume`] returned true.
    pub fn consume(&mut self, recipe: &Recipe) {
        for (item, count) in &recipe.inputs {
            let mut remaining = *count;
            for stack in self.inputs.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if stack.item == *item && !stack.is_empty() {
                    let taken = remaining.min(stack.count);
                    stack.count -= taken;
                    remaining -= taken;
                    if stack.count == 0 {
                        *stack = ItemStack::EMPTY;
                    }
                }
            }
        }
    }

    /// Whether the inputs hold every ingredient of the recipe.
    pub fn can_consume(&self, recipe: &Recipe) -> bool {
        recipe
            .inputs
            .iter()
            .all(|(item, count)| self.input_count(*item) >= *count)
    }

    /// Produces the recipe's outputs. Assumes [`Self::has_output_room`] returned true.
    pub fn produce(&mut self, recipe: &Recipe, content: &ContentRegistry) {
        for (item, count) in &recipe.outputs {
            let max_stack = content.item(*item).map_or(64, |item| item.stack);
            let mut remaining = *count;
            for stack in self.outputs.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if stack.item == *item && stack.count < max_stack {
                    let moved = (max_stack - stack.count).min(remaining);
                    stack.count += moved;
                    remaining -= moved;
                }
            }
            for stack in self.outputs.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if stack.is_empty() {
                    let moved = max_stack.min(remaining);
                    *stack = ItemStack::new(*item, moved);
                    remaining -= moved;
                }
            }
        }
    }

    /// Module speed and power multipliers from the installed modules.
    pub fn module_multipliers(&self, content: &ContentRegistry) -> (f32, f32) {
        let mut speed = 1.0;
        let mut power = 1.0;
        for stack in &self.modules {
            if stack.is_empty() {
                continue;
            }
            match content.item(stack.item).map(|item| &item.key[..]) {
                Some("module_speed") => {
                    speed += 0.5;
                    power += 0.8;
                }
                Some("module_efficiency") => {
                    power = (power - 0.3_f32).max(0.1);
                }
                _ => {}
            }
        }
        (speed, power)
    }

    /// Total item count, for save diffing.
    pub fn is_empty(&self) -> bool {
        self.inputs
            .iter()
            .chain(self.outputs.iter())
            .chain(self.fuel.iter())
            .chain(self.modules.iter())
            .all(|stack| stack.is_empty())
    }
}

/// Finds the best recipe a machine can start with the given inputs.
///
/// "Best" means the first recipe in content order whose inputs are satisfied and whose tech is
/// unlocked; content order is deliberate so that the file controls priority.
pub fn match_recipe<'a>(
    content: &'a ContentRegistry,
    machine: ContentId,
    inventory: &MachineInventory,
    unlocked: &dyn Fn(ContentId) -> bool,
) -> Option<&'a Recipe> {
    let candidates = content.recipes_by_machine.get(&machine)?;
    candidates.iter().find_map(|recipe_id| {
        let recipe = content.recipe(*recipe_id)?;
        if let Some(tech) = recipe.tech
            && !unlocked(tech)
        {
            return None;
        }
        if inventory.can_consume(recipe) && inventory.has_output_room(recipe, content) {
            Some(recipe)
        } else {
            None
        }
    })
}

/// Advances a machine by `dt` seconds.
///
/// `power_fraction` is 1.0 for full power, less during a brownout, 0.0 when unpowered.
/// `speed_multiplier` comes from upgrade modules.
///
/// Returns whether the machine produced output this tick.
pub fn tick_machine(
    content: &ContentRegistry,
    machine: ContentId,
    inventory: &mut MachineInventory,
    state: &mut MachineState,
    dt: f32,
    power_fraction: f32,
    unlocked: &dyn Fn(ContentId) -> bool,
) -> bool {
    let (module_speed, _) = inventory.module_multipliers(content);
    let def = content.machine(machine);
    let is_fuel = matches!(def.map(|machine| machine.energy), Some(MachineEnergy::Fuel { .. }));

    // Fuel machines burn their own fuel; everything else needs external power.
    if is_fuel && state.recipe.is_none() && state.fuel_seconds <= 0.0 {
        burn_fuel(content, inventory, state);
    }
    let powered = if is_fuel {
        state.fuel_seconds > 0.0
    } else {
        power_fraction > 0.0
    };

    // Keep the current recipe or pick a new one.
    if state.recipe.is_none() {
        match match_recipe(content, machine, inventory, unlocked) {
            Some(recipe) => {
                state.recipe = Some(recipe.id);
                state.progress = 0.0;
                state.duration = (recipe.time / (module_speed * power_fraction.max(0.01))).max(0.01);
                state.running = false;
            }
            None => {
                state.idle = diagnose(content, machine, inventory, unlocked);
                state.running = false;
                return false;
            }
        }
    }

    let Some(recipe_id) = state.recipe else {
        return false;
    };
    let Some(recipe) = content.recipe(recipe_id) else {
        state.recipe = None;
        return false;
    };

    if !powered {
        state.idle = IdleReason::NoPower;
        state.running = false;
        return false;
    }
    if !inventory.can_consume(recipe) || !inventory.has_output_room(recipe, content) {
        // The recipe became impossible (a player took an input): drop it and re-match.
        state.recipe = None;
        state.progress = 0.0;
        state.running = false;
        state.idle = diagnose(content, machine, inventory, unlocked);
        return false;
    }

    let effective = dt * power_fraction * module_speed;
    state.progress += effective;
    state.running = true;
    state.idle = IdleReason::Idle;

    if state.progress >= state.duration {
        inventory.consume(recipe);
        inventory.produce(recipe, content);
        if is_fuel {
            state.fuel_seconds = (state.fuel_seconds - state.duration).max(0.0);
        }
        state.recipe = None;
        state.progress = 0.0;
        state.duration = 0.0;
        state.running = false;
        return true;
    }
    false
}

fn burn_fuel(content: &ContentRegistry, inventory: &mut MachineInventory, state: &mut MachineState) {
    for slot in inventory.fuel.iter_mut() {
        if slot.is_empty() {
            continue;
        }
        if let Some(item) = content.item(slot.item)
            && let ItemCategory::Fuel { burn_seconds, .. } = item.category
        {
            state.fuel_seconds += burn_seconds;
            slot.count -= 1;
            if slot.count == 0 {
                *slot = ItemStack::EMPTY;
            }
            return;
        }
    }
}

/// Works out the most useful idle reason to show the player.
fn diagnose(
    content: &ContentRegistry,
    machine: ContentId,
    inventory: &MachineInventory,
    unlocked: &dyn Fn(ContentId) -> bool,
) -> IdleReason {
    let Some(candidates) = content.recipes_by_machine.get(&machine) else {
        return IdleReason::NoRecipe;
    };
    let mut saw_room = false;
    let mut missing: Option<ContentId> = None;
    for recipe_id in candidates {
        let Some(recipe) = content.recipe(*recipe_id) else {
            continue;
        };
        if let Some(tech) = recipe.tech
            && !unlocked(tech)
        {
            continue;
        }
        if !inventory.has_output_room(recipe, content) {
            saw_room = true;
            continue;
        }
        for (item, count) in &recipe.inputs {
            if inventory.input_count(*item) < *count {
                missing = Some(*item);
                break;
            }
        }
    }
    if let Some(item) = missing {
        IdleReason::MissingInput(item)
    } else if saw_room {
        IdleReason::OutputFull
    } else {
        IdleReason::NoRecipe
    }
}

/// The power a machine draws in kW from the electric network.
pub fn machine_draw_kw(content: &ContentRegistry, machine: ContentId, recipe: Option<ContentId>) -> f32 {
    let Some(def) = content.machine(machine) else {
        return 0.0;
    };
    let base = match def.energy {
        MachineEnergy::Electric { kw } => kw,
        _ => 0.0,
    };
    let recipe_draw = recipe
        .and_then(|recipe| content.recipe(recipe))
        .map_or(0.0, |recipe| recipe.power_kw);
    base.max(recipe_draw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    #[test]
    fn furnace_smelts_iron_into_ingot() {
        let content = content();
        let furnace = content.machine_by_key("stone_furnace").expect("furnace").id;
        let raw_iron = content.item_by_key("raw_iron").expect("raw iron").id;
        let coal = content.item_by_key("coal").expect("coal").id;
        let iron_ingot = content.item_by_key("iron_ingot").expect("ingot").id;

        let mut inventory = MachineInventory::for_machine(&content, furnace);
        inventory.inputs[0] = ItemStack::new(raw_iron, 2);
        inventory.fuel[0] = ItemStack::new(coal, 2);
        let mut state = MachineState::default();

        let unlocked = |_tech: ContentId| true;
        let mut produced = 0;
        for _ in 0..1000 {
            if tick_machine(&content, furnace, &mut inventory, &mut state, 0.1, 1.0, &unlocked) {
                produced += 1;
            }
            if produced == 2 {
                break;
            }
        }
        assert_eq!(produced, 2);
        assert_eq!(
            inventory.outputs.iter().map(|stack| stack.count).sum::<u32>(),
            2
        );
        assert_eq!(inventory.outputs[0].item, iron_ingot);
    }

    #[test]
    fn no_power_reports_no_power() {
        let content = content();
        let crusher = content.machine_by_key("crusher").expect("crusher").id;
        let raw_iron = content.item_by_key("raw_iron").expect("raw iron").id;
        let mut inventory = MachineInventory::for_machine(&content, crusher);
        inventory.inputs[0] = ItemStack::new(raw_iron, 2);
        let mut state = MachineState::default();
        let unlocked = |_tech: ContentId| true;
        tick_machine(&content, crusher, &mut inventory, &mut state, 0.1, 0.0, &unlocked);
        assert_eq!(state.idle, IdleReason::NoPower);
    }

    #[test]
    fn locked_recipe_does_not_start() {
        let content = content();
        let furnace = content.machine_by_key("stone_furnace").expect("furnace").id;
        let raw_iron = content.item_by_key("raw_iron").expect("raw iron").id;
        let mut inventory = MachineInventory::for_machine(&content, furnace);
        inventory.inputs[0] = ItemStack::new(raw_iron, 1);
        inventory.fuel[0] = ItemStack::new(content.item_by_key("coal").unwrap().id, 1);
        let mut state = MachineState::default();
        let locked = |_tech: ContentId| false;
        for _ in 0..100 {
            tick_machine(&content, furnace, &mut inventory, &mut state, 0.1, 1.0, &locked);
        }
        assert!(state.recipe.is_none());
        assert_eq!(inventory.inputs[0].count, 1);
    }

    #[test]
    fn output_full_blocks_progress() {
        let content = content();
        let furnace = content.machine_by_key("stone_furnace").expect("furnace").id;
        let raw_iron = content.item_by_key("raw_iron").expect("raw iron").id;
        let iron_ingot = content.item_by_key("iron_ingot").unwrap().id;
        let mut inventory = MachineInventory::for_machine(&content, furnace);
        inventory.inputs[0] = ItemStack::new(raw_iron, 1);
        inventory.fuel[0] = ItemStack::new(content.item_by_key("coal").unwrap().id, 1);
        let max = content.item(iron_ingot).unwrap().stack;
        for slot in inventory.outputs.iter_mut() {
            *slot = ItemStack::new(iron_ingot, max);
        }
        let mut state = MachineState::default();
        let unlocked = |_tech: ContentId| true;
        for _ in 0..100 {
            tick_machine(&content, furnace, &mut inventory, &mut state, 0.1, 1.0, &unlocked);
        }
        assert_eq!(state.idle, IdleReason::OutputFull);
    }
}
