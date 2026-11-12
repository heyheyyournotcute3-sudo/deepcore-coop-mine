//! The runtime content registry: resolved, validated, indexed content.
//!
//! Built once at startup from the RON files. The server and the client both build it; the
//! handshake compares [`ContentRegistry::content_hash`] so a client with different content is
//! refused.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::ids::{ContentId, IdTable};
use super::model::*;

/// A block, resolved.
#[derive(Debug, Clone)]
pub struct Block {
    /// Numeric id, equal to the palette index.
    pub id: ContentId,
    /// String id from the file.
    pub key: String,
    /// Seconds to break with a tier-1 tool at 1.0× speed.
    pub hardness: f32,
    /// Minimum tool tier.
    pub min_tier: u8,
    /// Resolved drops.
    pub drops: Vec<(ContentId, u32)>,
    /// Atlas colour.
    pub color: (f32, f32, f32),
    /// Atlas pattern.
    pub pattern: Pattern,
    /// Emissive strength.
    pub emissive: f32,
    /// Radiation emitted.
    pub radiation: Option<RadiationDef>,
    /// Blocks movement.
    pub solid: bool,
    /// Is a fluid.
    pub fluid: bool,
    /// Light emission 0..15.
    pub light: u8,
    /// Machine placed by this block.
    pub machine: Option<ContentId>,
    /// Cannot be broken.
    pub unbreakable: bool,
}

/// An item, resolved.
#[derive(Debug, Clone)]
pub struct Item {
    /// Numeric id.
    pub id: ContentId,
    /// String id from the file.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Max stack.
    pub stack: u32,
    /// Category.
    pub category: ItemCategory,
    /// Radiation emitted while carried.
    pub radiation: Option<RadiationDef>,
    /// Block placed by this item, if any.
    pub places_block: Option<ContentId>,
    /// A non-recipe system that consumes the item, if any.
    pub sink: Option<String>,
}

/// A recipe, resolved and bucketed by machine.
#[derive(Debug, Clone)]
pub struct Recipe {
    /// Numeric id.
    pub id: ContentId,
    /// String id from the file.
    pub key: String,
    /// Machine id (`"hand"` is interned as a machine id too).
    pub machine: ContentId,
    /// Seconds at 1× speed.
    pub time: f32,
    /// Electric draw in kW.
    pub power_kw: f32,
    /// Steam draw per second.
    pub steam: f32,
    /// Inputs.
    pub inputs: Vec<(ContentId, u32)>,
    /// Outputs.
    pub outputs: Vec<(ContentId, u32)>,
    /// Required tech node.
    pub tech: Option<ContentId>,
}

/// A machine, resolved.
#[derive(Debug, Clone)]
pub struct Machine {
    /// Numeric id.
    pub id: ContentId,
    /// String id.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Footprint.
    pub size: (u8, u8, u8),
    /// Energy source.
    pub energy: MachineEnergy,
    /// Input slots.
    pub input_slots: u8,
    /// Output slots.
    pub output_slots: u8,
    /// Build cost.
    pub build_cost: Vec<(ContentId, u32)>,
    /// Required tech node.
    pub tech: Option<ContentId>,
    /// Cascade limits.
    pub cascade: Option<CascadeDef>,
    /// Accepts modules.
    pub modules: bool,
    /// Steam buffer.
    pub steam_buffer: f32,
    /// Battery capacity kWh.
    pub battery_kwh: f32,
    /// Generation kW.
    pub generation_kw: f32,
    /// Multi-block structure.
    pub multiblock: bool,
}

/// A tech node, resolved.
#[derive(Debug, Clone)]
pub struct TechNode {
    /// Numeric id.
    pub id: ContentId,
    /// String id.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Era.
    pub era: u8,
    /// Parents.
    pub parents: Vec<ContentId>,
    /// Delivery cost.
    pub cost: Vec<(ContentId, u32)>,
    /// Unlocked content ids (machines, items, blocks, recipes).
    pub unlocks: Vec<ContentId>,
}

/// A layer, resolved.
#[derive(Debug, Clone)]
pub struct Layer {
    /// String id.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Depth band.
    pub depth: (u32, u32),
    /// Base block.
    pub base_block: ContentId,
    /// Secondary fillers with shares.
    pub fillers: Vec<(ContentId, f32)>,
    /// Ore table.
    pub ores: Vec<(ContentId, f32, (u32, u32))>,
    /// Hazards.
    pub hazards: Vec<LayerHazard>,
    /// Ambient temperature.
    pub ambient_temp: f32,
    /// Minimum tier for the base block.
    pub min_tier: u8,
    /// Cave density multiplier.
    pub cave_density: f32,
}

/// Errors produced while loading or validating content.
#[derive(Debug, Error)]
pub enum ContentError {
    /// A file could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// A file did not parse.
    #[error("cannot parse {path}: {source}")]
    Parse {
        /// Path that failed.
        path: PathBuf,
        /// RON error.
        source: Box<ron::error::SpannedError>,
    },
    /// The content is structurally invalid.
    #[error("invalid content: {0}")]
    Invalid(String),
}

/// The loaded, validated content.
#[derive(Debug, Default, Clone, bevy::prelude::Resource)]
pub struct ContentRegistry {
    /// Block table (id order == palette order).
    pub blocks: Vec<Block>,
    /// Item table.
    pub items: Vec<Item>,
    /// Recipe table.
    pub recipes: Vec<Recipe>,
    /// Machine table.
    pub machines: Vec<Machine>,
    /// Tech node table.
    pub tech: Vec<TechNode>,
    /// Layers, ordered by depth.
    pub layers: Vec<Layer>,
    /// Hazard tuning.
    pub hazards: Option<HazardTuning>,
    /// Recipes indexed by machine id.
    pub recipes_by_machine: HashMap<ContentId, Vec<ContentId>>,
    /// Recipes indexed by the item they produce (first output).
    pub recipes_by_output: HashMap<ContentId, Vec<ContentId>>,
    /// Block ids by string key.
    pub block_ids: IdTable,
    /// Item ids by string key.
    pub item_ids: IdTable,
    /// Machine ids by string key.
    pub machine_ids: IdTable,
    /// Recipe ids by string key.
    pub recipe_ids: IdTable,
    /// Tech ids by string key.
    pub tech_ids: IdTable,
    /// Hash of every content file, for the handshake.
    pub content_hash: u64,
    /// The special `"hand"` machine id.
    pub hand_machine: ContentId,
    /// The air block id.
    pub air: ContentId,
    /// The bedrock block id.
    pub bedrock: ContentId,
}

impl ContentRegistry {
    /// Loads every content file from `dir`.
    pub fn load_from_dir(dir: &Path) -> Result<Self, ContentError> {
        let blocks: Vec<BlockDef> = read_ron(&dir.join("blocks.ron"))?;
        let items: Vec<ItemDef> = read_ron(&dir.join("items.ron"))?;
        let recipes: Vec<RecipeDef> = read_ron(&dir.join("recipes.ron"))?;
        let machines: Vec<MachineDef> = read_ron(&dir.join("machines.ron"))?;
        let tech: Vec<TechNodeDef> = read_ron(&dir.join("tech_tree.ron"))?;
        let layers: Vec<LayerDef> = read_ron(&dir.join("layers.ron"))?;
        let hazards: HazardTuning = read_ron(&dir.join("hazards.ron"))?;

        let mut hasher = crate::stable_hash(b"");
        for name in [
            "blocks.ron",
            "items.ron",
            "recipes.ron",
            "machines.ron",
            "tech_tree.ron",
            "layers.ron",
            "hazards.ron",
        ] {
            let path = dir.join(name);
            let bytes = std::fs::read(&path).map_err(|source| ContentError::Io { path, source })?;
            hasher ^= crate::stable_hash(&bytes);
            hasher = hasher.wrapping_mul(0x0000_0100_0000_01b3);
        }

        let mut registry = Self::build(blocks, items, recipes, machines, tech, layers, hazards)?;
        registry.content_hash = hasher;
        Ok(registry)
    }

    /// Resolves string ids, builds indexes and validates every cross reference.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        block_defs: Vec<BlockDef>,
        item_defs: Vec<ItemDef>,
        recipe_defs: Vec<RecipeDef>,
        machine_defs: Vec<MachineDef>,
        tech_defs: Vec<TechNodeDef>,
        mut layer_defs: Vec<LayerDef>,
        hazards: HazardTuning,
    ) -> Result<Self, ContentError> {
        let mut registry = Self::default();

        // --- machines and tech first: everything else may reference them ----------------
        for def in &machine_defs {
            registry.machine_ids.intern(&def.id);
        }
        registry.hand_machine = registry.machine_ids.intern("hand");

        for def in &tech_defs {
            registry.tech_ids.intern(&def.id);
        }

        // --- blocks: id order is the palette order, so air and bedrock are pinned first ---
        registry.block_ids.intern("air");
        registry.block_ids.intern("bedrock");
        for def in &block_defs {
            registry.block_ids.intern(&def.id);
        }
        registry.air = registry.block_ids.get("air").expect("interned above");
        registry.bedrock = registry.block_ids.get("bedrock").expect("interned above");

        // --- items ----------------------------------------------------------------------
        for def in &item_defs {
            registry.item_ids.intern(&def.id);
        }

        // Blocks, resolved.
        for def in &block_defs {
            let id = registry.block_ids.get(&def.id).expect("interned above");
            let drops = def
                .drops
                .iter()
                .map(|(item, count)| {
                    let item_id = registry.item_ids.get(item).ok_or_else(|| {
                        ContentError::Invalid(format!(
                            "block `{}` drops unknown item `{item}`",
                            def.id
                        ))
                    })?;
                    Ok((item_id, *count))
                })
                .collect::<Result<Vec<_>, ContentError>>()?;
            let machine = match &def.machine {
                Some(machine) => Some(registry.machine_ids.get(machine).ok_or_else(|| {
                    ContentError::Invalid(format!(
                        "block `{}` refers to unknown machine `{machine}`",
                        def.id
                    ))
                })?),
                None => None,
            };
            registry.blocks.push(Block {
                id,
                key: def.id.clone(),
                hardness: def.hardness,
                min_tier: def.min_tier,
                drops,
                color: def.color,
                pattern: def.pattern,
                emissive: def.emissive,
                radiation: def.radiation,
                solid: def.solid,
                fluid: def.fluid,
                light: def.light,
                machine,
                unbreakable: def.unbreakable,
            });
        }
        // Air and bedrock are synthetic: they exist even if a file omits them.
        registry.ensure_synthetic_blocks();

        // Items, resolved.
        for def in &item_defs {
            let id = registry.item_ids.get(&def.id).expect("interned above");
            let places_block = match &def.category {
                ItemCategory::MachineKit(machine) => {
                    let machine_id = registry.machine_ids.get(machine).ok_or_else(|| {
                        ContentError::Invalid(format!(
                            "item `{}` is a kit for unknown machine `{machine}`",
                            def.id
                        ))
                    })?;
                    registry.block_for_machine(machine_id)
                }
                ItemCategory::Block(block) => Some(registry.block_ids.get(block).ok_or_else(|| {
                    ContentError::Invalid(format!(
                        "item `{}` places unknown block `{block}`",
                        def.id
                    ))
                })?),
                _ => None,
            };
            registry.items.push(Item {
                id,
                key: def.id.clone(),
                name: def.name.clone(),
                stack: def.stack,
                category: def.category.clone(),
                radiation: def.radiation,
                places_block,
                sink: def.sink.clone(),
            });
        }

        // Machines, resolved.
        for def in &machine_defs {
            let id = registry.machine_ids.get(&def.id).expect("interned above");
            let build_cost = registry.resolve_items(&def.build_cost, "machine", &def.id)?;
            let tech = match &def.tech {
                Some(tech) => Some(registry.tech_ids.get(tech).ok_or_else(|| {
                    ContentError::Invalid(format!(
                        "machine `{}` requires unknown tech `{tech}`",
                        def.id
                    ))
                })?),
                None => None,
            };
            registry.machines.push(Machine {
                id,
                key: def.id.clone(),
                name: def.name.clone(),
                size: def.size,
                energy: def.energy,
                input_slots: def.input_slots,
                output_slots: def.output_slots,
                build_cost,
                tech,
                cascade: def.cascade,
                modules: def.modules,
                steam_buffer: def.steam_buffer,
                battery_kwh: def.battery_kwh,
                generation_kw: def.generation_kw,
                multiblock: def.multiblock,
            });
        }

        // Recipes, resolved.
        for def in &recipe_defs {
            let id = registry.recipe_ids.intern(&def.id);
            let machine = registry.machine_ids.get(&def.machine).ok_or_else(|| {
                ContentError::Invalid(format!(
                    "recipe `{}` runs in unknown machine `{}`",
                    def.id, def.machine
                ))
            })?;
            let inputs = registry.resolve_items(&def.inputs, "recipe input", &def.id)?;
            let outputs = registry.resolve_items(&def.outputs, "recipe output", &def.id)?;
            let tech = match &def.tech {
                Some(tech) => Some(registry.tech_ids.get(tech).ok_or_else(|| {
                    ContentError::Invalid(format!(
                        "recipe `{}` requires unknown tech `{tech}`",
                        def.id
                    ))
                })?),
                None => None,
            };
            registry.recipes.push(Recipe {
                id,
                key: def.id.clone(),
                machine,
                time: def.time,
                power_kw: def.power_kw,
                steam: def.steam,
                inputs,
                outputs,
                tech,
            });
        }

        // Tech nodes, resolved.
        for def in &tech_defs {
            let id = registry.tech_ids.get(&def.id).expect("interned above");
            let parents = def
                .parents
                .iter()
                .map(|parent| {
                    registry.tech_ids.get(parent).ok_or_else(|| {
                        ContentError::Invalid(format!(
                            "tech `{}` has unknown parent `{parent}`",
                            def.id
                        ))
                    })
                })
                .collect::<Result<Vec<_>, ContentError>>()?;
            let cost = registry.resolve_items(&def.cost, "tech cost", &def.id)?;
            let unlocks = def
                .unlocks
                .iter()
                .map(|unlock| {
                    registry
                        .machine_ids
                        .get(unlock)
                        .or_else(|| registry.item_ids.get(unlock))
                        .or_else(|| registry.block_ids.get(unlock))
                        .ok_or_else(|| {
                            ContentError::Invalid(format!(
                                "tech `{}` unlocks unknown content `{unlock}`",
                                def.id
                            ))
                        })
                })
                .collect::<Result<Vec<_>, ContentError>>()?;
            registry.tech.push(TechNode {
                id,
                key: def.id.clone(),
                name: def.name.clone(),
                era: def.era,
                parents,
                cost,
                unlocks,
            });
        }

        // Layers, resolved and ordered by depth.
        layer_defs.sort_by_key(|layer| layer.depth.0);
        for def in &layer_defs {
            let base_block = registry.block_ids.get(&def.base_block).ok_or_else(|| {
                ContentError::Invalid(format!(
                    "layer `{}` has unknown base block `{}`",
                    def.id, def.base_block
                ))
            })?;
            let fillers = def
                .fillers
                .iter()
                .map(|(block, share)| {
                    let block = registry.block_ids.get(block).ok_or_else(|| {
                        ContentError::Invalid(format!(
                            "layer `{}` has unknown filler block `{block}`",
                            def.id
                        ))
                    })?;
                    Ok((block, *share))
                })
                .collect::<Result<Vec<_>, ContentError>>()?;
            let ores = def
                .ores
                .iter()
                .map(|ore| {
                    let block = registry.block_ids.get(&ore.block).ok_or_else(|| {
                        ContentError::Invalid(format!(
                            "layer `{}` has unknown ore block `{}`",
                            def.id, ore.block
                        ))
                    })?;
                    Ok((block, ore.frequency, ore.vein_size))
                })
                .collect::<Result<Vec<_>, ContentError>>()?;
            registry.layers.push(Layer {
                key: def.id.clone(),
                name: def.name.clone(),
                depth: def.depth,
                base_block,
                fillers,
                ores,
                hazards: def.hazards.clone(),
                ambient_temp: def.ambient_temp,
                min_tier: def.min_tier,
                cave_density: def.cave_density,
            });
        }

        registry.hazards = Some(hazards);

        // Indexes.
        for recipe in &registry.recipes {
            registry
                .recipes_by_machine
                .entry(recipe.machine)
                .or_default()
                .push(recipe.id);
            if let Some((output, _)) = recipe.outputs.first() {
                registry
                    .recipes_by_output
                    .entry(*output)
                    .or_default()
                    .push(recipe.id);
            }
        }

        registry.validate()?;
        Ok(registry)
    }

    fn ensure_synthetic_blocks(&mut self) {
        let air = self.air;
        let bedrock = self.bedrock;
        if !self.blocks.iter().any(|block| block.id == air) {
            self.blocks.push(Block {
                id: air,
                key: "air".into(),
                hardness: 0.0,
                min_tier: 0,
                drops: Vec::new(),
                color: (0.0, 0.0, 0.0),
                pattern: Pattern::Solid,
                emissive: 0.0,
                radiation: None,
                solid: false,
                fluid: false,
                light: 0,
                machine: None,
                unbreakable: false,
            });
        }
        if !self.blocks.iter().any(|block| block.id == bedrock) {
            self.blocks.push(Block {
                id: bedrock,
                key: "bedrock".into(),
                hardness: f32::INFINITY,
                min_tier: u8::MAX,
                drops: Vec::new(),
                color: (0.09, 0.09, 0.11),
                pattern: Pattern::Speckle,
                emissive: 0.0,
                radiation: None,
                solid: true,
                fluid: false,
                light: 0,
                machine: None,
                unbreakable: true,
            });
        }
        self.blocks.sort_by_key(|block| block.id.0);
    }

    fn resolve_items(
        &self,
        list: &[(String, u32)],
        what: &str,
        owner: &str,
    ) -> Result<Vec<(ContentId, u32)>, ContentError> {
        list.iter()
            .map(|(item, count)| {
                let id = self.item_ids.get(item).ok_or_else(|| {
                    ContentError::Invalid(format!("{what} of `{owner}` uses unknown item `{item}`"))
                })?;
                Ok((id, *count))
            })
            .collect()
    }

    /// Structural validation, run after everything is resolved.
    fn validate(&self) -> Result<(), ContentError> {
        let mut problems: Vec<String> = Vec::new();

        // Every recipe needs a machine that exists (the `hand` pseudo-machine included).
        for recipe in &self.recipes {
            if self.machine(recipe.machine).is_none() && recipe.machine != self.hand_machine {
                problems.push(format!(
                    "recipe `{}` runs in `{}`, which is not a machine",
                    recipe.key,
                    self.machine_ids.name(recipe.machine).unwrap_or("?")
                ));
            }
            if recipe.outputs.is_empty() {
                problems.push(format!("recipe `{}` has no outputs", recipe.key));
            }
            if recipe.inputs.is_empty() && recipe.machine != self.hand_machine {
                problems.push(format!("recipe `{}` has no inputs", recipe.key));
            }
        }

        // Every machine with a build cost must be buildable: a machine kit item must exist.
        for machine in &self.machines {
            let has_kit = self
                .items
                .iter()
                .any(|item| item.places_block.and_then(|b| self.block(b)).and_then(|b| b.machine) == Some(machine.id));
            if !has_kit && machine.build_cost.is_empty() {
                problems.push(format!(
                    "machine `{}` has no build cost and no kit item",
                    machine.key
                ));
            }
        }

        // Tech parents must exist and form a DAG.
        let mut visiting = vec![0u8; self.tech.len()];
        for node in &self.tech {
            for parent in &node.parents {
                if *parent == node.id {
                    problems.push(format!("tech `{}` is its own parent", node.key));
                }
                if self.tech(*parent).is_none() {
                    problems.push(format!(
                        "tech `{}` has a parent that does not exist",
                        node.key
                    ));
                }
            }
        }
        for node in &self.tech {
            if let Err(cycle) = self.check_acyclic(node.id, &mut visiting) {
                problems.push(format!("tech tree has a cycle through `{cycle}`"));
                break;
            }
        }

        // Every tech node must be reachable from an era-0 root.
        let mut roots = 0;
        for node in &self.tech {
            if node.parents.is_empty() {
                roots += 1;
            }
        }
        if roots == 0 && !self.tech.is_empty() {
            problems.push("tech tree has no root node".into());
        }

        // Every ore block must have a use: a recipe input, a build cost, a tech cost, or be a
        // block that is placed directly.
        let mut used: HashMap<ContentId, bool> = HashMap::new();
        for recipe in &self.recipes {
            for (item, _) in &recipe.inputs {
                used.insert(*item, true);
            }
        }
        for machine in &self.machines {
            for (item, _) in &machine.build_cost {
                used.insert(*item, true);
            }
        }
        for node in &self.tech {
            for (item, _) in &node.cost {
                used.insert(*item, true);
            }
        }
        for item in &self.items {
            if item.places_block.is_some() {
                used.insert(item.id, true);
            }
            if matches!(item.category, ItemCategory::Tool { .. } | ItemCategory::Gear { .. }) {
                used.insert(item.id, true);
            }
            // Fuels, charges and items with a declared system sink are consumed by systems.
            if item.sink.is_some()
                || matches!(
                    item.category,
                    ItemCategory::Fuel { .. } | ItemCategory::Charge { .. }
                )
            {
                used.insert(item.id, true);
            }
            // Upgrade modules are consumed by machine module slots.
            if item.key.starts_with("module_") {
                used.insert(item.id, true);
            }
        }
        for item in &self.items {
            if !used.contains_key(&item.id) {
                problems.push(format!("item `{}` has no use (dead end)", item.key));
            }
        }
        for block in &self.blocks {
            if block.id == self.air {
                continue;
            }
            if block.drops.is_empty() && !block.fluid && !block.unbreakable {
                problems.push(format!("block `{}` drops nothing", block.key));
            }
        }

        // Layers must be contiguous and cover the world.
        let mut expected_top = 0u32;
        for layer in &self.layers {
            if layer.depth.0 != expected_top {
                problems.push(format!(
                    "layer `{}` starts at depth {} but the previous layer ends at {expected_top}",
                    layer.key, layer.depth.0
                ));
            }
            expected_top = layer.depth.1;
            for (block, _, _) in &layer.ores {
                if self.block(*block).is_none() {
                    problems.push(format!("layer `{}` has an ore with no block", layer.key));
                }
            }
        }
        if expected_top == 0 {
            problems.push("no layers defined".into());
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(ContentError::Invalid(problems.join("\n  - ")))
        }
    }

    fn check_acyclic(&self, start: ContentId, visiting: &mut [u8]) -> Result<(), String> {
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            match visiting[node.index()] {
                2 => continue,
                1 => {
                    return Err(self
                        .tech_ids
                        .name(node)
                        .unwrap_or("?")
                        .to_owned());
                }
                _ => {}
            }
            visiting[node.index()] = 1;
            if let Some(tech) = self.tech(node) {
                for parent in &tech.parents {
                    stack.push(*parent);
                }
            }
        }
        // Second pass marks everything reachable from `start` as done.
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            if visiting[node.index()] == 2 {
                continue;
            }
            visiting[node.index()] = 2;
            if let Some(tech) = self.tech(node) {
                for parent in &tech.parents {
                    stack.push(*parent);
                }
            }
        }
        Ok(())
    }

    /// Looks a block up by id.
    pub fn block(&self, id: ContentId) -> Option<&Block> {
        match self.blocks.get(id.index()) {
            Some(block) if block.id == id => Some(block),
            _ => self.blocks.iter().find(|block| block.id == id),
        }
    }

    /// Looks an item up by id.
    pub fn item(&self, id: ContentId) -> Option<&Item> {
        match self.items.get(id.index()) {
            Some(item) if item.id == id => Some(item),
            _ => self.items.iter().find(|item| item.id == id),
        }
    }

    /// Looks a recipe up by id.
    pub fn recipe(&self, id: ContentId) -> Option<&Recipe> {
        match self.recipes.get(id.index()) {
            Some(recipe) if recipe.id == id => Some(recipe),
            _ => self.recipes.iter().find(|recipe| recipe.id == id),
        }
    }

    /// Looks a machine up by id.
    pub fn machine(&self, id: ContentId) -> Option<&Machine> {
        match self.machines.get(id.index()) {
            Some(machine) if machine.id == id => Some(machine),
            _ => self.machines.iter().find(|machine| machine.id == id),
        }
    }

    /// Looks a tech node up by id.
    pub fn tech(&self, id: ContentId) -> Option<&TechNode> {
        match self.tech.get(id.index()) {
            Some(tech) if tech.id == id => Some(tech),
            _ => self.tech.iter().find(|tech| tech.id == id),
        }
    }

    /// Looks a block up by string key.
    pub fn block_by_key(&self, key: &str) -> Option<&Block> {
        self.block_ids.get(key).and_then(|id| self.block(id))
    }

    /// Looks an item up by string key.
    pub fn item_by_key(&self, key: &str) -> Option<&Item> {
        self.item_ids.get(key).and_then(|id| self.item(id))
    }

    /// Looks a machine up by string key.
    pub fn machine_by_key(&self, key: &str) -> Option<&Machine> {
        self.machine_ids.get(key).and_then(|id| self.machine(id))
    }

    /// Returns the block that places the given machine.
    pub fn block_for_machine(&self, machine: ContentId) -> Option<ContentId> {
        self.blocks
            .iter()
            .find(|block| block.machine == Some(machine))
            .map(|block| block.id)
    }

    /// The layer containing `depth` blocks below the surface.
    pub fn layer_at_depth(&self, depth: u32) -> Option<&Layer> {
        self.layers
            .iter()
            .find(|layer| depth >= layer.depth.0 && depth < layer.depth.1)
    }

    /// Total world depth, in blocks.
    pub fn world_depth(&self) -> u32 {
        self.layers.last().map_or(0, |layer| layer.depth.1)
    }

    /// Hazard tuning, falling back to the embedded defaults when a test builds a registry
    /// without a hazards file.
    pub fn hazard_tuning(&self) -> &HazardTuning {
        static DEFAULT: std::sync::OnceLock<HazardTuning> = std::sync::OnceLock::new();
        self.hazards.as_ref().unwrap_or_else(|| {
            DEFAULT.get_or_init(|| {
                ron::from_str(include_str!("../../../../assets/data/hazards.ron"))
                    .expect("embedded hazards.ron parses")
            })
        })
    }
}

fn read_ron<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ContentError> {
    let text = std::fs::read_to_string(path).map_err(|source| ContentError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    ron::from_str(&text).map_err(|source| ContentError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}
