//! Deserialised shapes of the RON content files, before ids are resolved.
//!
//! Every struct here mirrors a file in `assets/data/`. String ids are kept as `String` so the
//! validator can produce readable errors; [`super::registry::ContentRegistry::build`] resolves
//! them into numeric ids exactly once.

use serde::Deserialize;

/// How a block is drawn by the procedural atlas generator.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum Pattern {
    /// Flat colour, no detail.
    Solid,
    /// Stone-like speckle noise.
    Speckle,
    /// Bright flecks of `ore` colour over the base colour.
    OreFleck((f32, f32, f32)),
    /// Dark organic grain (wood, dirt).
    Grain,
    /// Thin diagonal veins (marble, gneiss).
    Veins,
    /// Uniform glow with a soft pulse (uranium, core crystals, lamps).
    Glow,
    /// Small repeating rectangles (bricks, casing).
    Bricks,
    /// Horizontal banding (shale, basalt columns).
    Bands,
    /// Liquid surface.
    Fluid,
}

/// One block definition from `blocks.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockDef {
    /// Unique string id.
    pub id: String,
    /// Seconds to break with a tier-1 tool at 1.0× speed.
    pub hardness: f32,
    /// Minimum tool tier that can break the block at all.
    pub min_tier: u8,
    /// Item drops as `(item id, count)`.
    #[serde(default)]
    pub drops: Vec<(String, u32)>,
    /// Base colour, linear sRGB 0..1.
    pub color: (f32, f32, f32),
    /// Atlas pattern.
    pub pattern: Pattern,
    /// Emissive strength 0..1.
    #[serde(default)]
    pub emissive: f32,
    /// Radiation emitted by the block while exposed.
    #[serde(default)]
    pub radiation: Option<RadiationDef>,
    /// Blocks player movement.
    #[serde(default = "yes")]
    pub solid: bool,
    /// The block is a fluid (water, steam) and spreads.
    #[serde(default)]
    pub fluid: bool,
    /// Light level emitted by the block, 0..15.
    #[serde(default)]
    pub light: u8,
    /// Layer this block is associated with, for the atlas and the depth meter.
    #[serde(default)]
    pub layer_hint: Option<u8>,
    /// Machine this block places, if any.
    #[serde(default)]
    pub machine: Option<String>,
    /// The block cannot be broken by any tool (world bounds).
    #[serde(default)]
    pub unbreakable: bool,
}

fn yes() -> bool {
    true
}

/// Radiation emitted by a block or an item.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RadiationDef {
    /// Radius in blocks.
    pub radius: f32,
    /// Dose per second at the source.
    pub dose_per_sec: f32,
}

/// Item category, carrying the extra data tools need.
#[derive(Debug, Clone, Deserialize)]
pub enum ItemCategory {
    /// Plain material.
    Material,
    /// A tool or drill.
    Tool {
        /// Minimum block tier the tool can break.
        tier: u8,
        /// Speed multiplier in the break-time formula.
        speed: f32,
        /// Mining area as `(width, height)` on the targeted face.
        area: (u8, u8),
        /// Energy source.
        energy: EnergySource,
        /// Reach in blocks.
        #[serde(default = "default_reach")]
        reach: f32,
    },
    /// A placeable machine kit (the machine to place).
    MachineKit(String),
    /// A placeable block.
    Block(String),
    /// Armour / hazard gear, worn in a gear slot.
    Gear {
        /// Slot the gear occupies.
        slot: GearSlot,
        /// Radiation dose reduction, 0..1.
        #[serde(default)]
        radiation_shielding: f32,
        /// Maximum body heat offset in °C.
        #[serde(default)]
        heat_resistance: f32,
        /// Oxygen use multiplier while worn, 0..1.
        #[serde(default = "one")]
        oxygen_use: f32,
        /// Gas damage multiplier while worn, 0..1.
        #[serde(default = "one")]
        gas_protection: f32,
    },
    /// A fuel item.
    Fuel {
        /// Seconds of burn time in a machine with one fuel slot.
        burn_seconds: f32,
        /// Heat units produced per second while burning.
        heat: f32,
    },
    /// A consumable canister or core cell.
    Charge {
        /// Energy stored.
        capacity: u32,
    },
}

fn default_reach() -> f32 {
    5.0
}

fn one() -> f32 {
    1.0
}

/// Where a tool gets its energy.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum EnergySource {
    /// Hand powered.
    None,
    /// Steam canister, refilled at a boiler.
    Steam {
        /// Canister capacity.
        capacity: u32,
    },
    /// Battery, recharged at a charging station.
    Battery {
        /// Battery capacity.
        capacity: u32,
    },
    /// Nuclear core cell with a fixed number of block breaks.
    CoreCell {
        /// Block breaks per cell.
        breaks: u32,
    },
}

/// Gear slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum GearSlot {
    /// Suit.
    Suit,
    /// Mask.
    Mask,
    /// Pocket instrument.
    Instrument,
}

/// One item definition from `items.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct ItemDef {
    /// Unique string id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Max stack size.
    pub stack: u32,
    /// Category and category data.
    pub category: ItemCategory,
    /// Radiation emitted while carried.
    #[serde(default)]
    pub radiation: Option<RadiationDef>,
    /// A system that consumes this item outside of a recipe (`"reactor"`, `"vault"`, ...).
    ///
    /// Fuels and charges are consumed by machines and tools; this field covers the rest so the
    /// validator can prove that no item is a dead end.
    #[serde(default)]
    pub sink: Option<String>,
}

/// One recipe from `recipes.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct RecipeDef {
    /// Unique string id.
    pub id: String,
    /// Machine that runs it, or `"hand"` for the workbench.
    pub machine: String,
    /// Seconds at 1× speed.
    pub time: f32,
    /// Electric draw in kW (0 for fuel/steam machines that draw from their own fuel).
    #[serde(default)]
    pub power_kw: f32,
    /// Steam units per second consumed.
    #[serde(default)]
    pub steam: f32,
    /// Input items.
    pub inputs: Vec<(String, u32)>,
    /// Output items.
    pub outputs: Vec<(String, u32)>,
    /// Tech node required, if any.
    #[serde(default)]
    pub tech: Option<String>,
}

/// How a machine is powered.
#[derive(Debug, Clone, Copy, Deserialize)]
pub enum MachineEnergy {
    /// Burns fuel from its own fuel slot.
    Fuel { slots: u8 },
    /// Draws steam from a connected pipe network.
    Steam { units_per_sec: f32 },
    /// Draws electricity from a connected cable network.
    Electric { kw: f32 },
    /// Runs on muscle power.
    Manual,
}

/// One machine definition from `machines.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct MachineDef {
    /// Unique string id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Footprint in blocks, `(x, y, z)`.
    pub size: (u8, u8, u8),
    /// Energy source.
    pub energy: MachineEnergy,
    /// Number of input slots.
    pub input_slots: u8,
    /// Number of output slots.
    pub output_slots: u8,
    /// Items needed to build it.
    pub build_cost: Vec<(String, u32)>,
    /// Tech node required to build it.
    #[serde(default)]
    pub tech: Option<String>,
    /// Cascade configuration for centrifuges.
    #[serde(default)]
    pub cascade: Option<CascadeDef>,
    /// Whether the machine accepts upgrade modules.
    #[serde(default)]
    pub modules: bool,
    /// Maximum stored steam, for buffers.
    #[serde(default)]
    pub steam_buffer: f32,
    /// Maximum stored energy in kWh, for batteries.
    #[serde(default)]
    pub battery_kwh: f32,
    /// Energy produced in kW, for generators.
    #[serde(default)]
    pub generation_kw: f32,
    /// The machine is a multi-block structure assembled from a controller block.
    #[serde(default)]
    pub multiblock: bool,
}

/// Centrifuge cascade limits.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct CascadeDef {
    /// Minimum blocks in a cascade.
    pub min: u8,
    /// Maximum blocks in a cascade.
    pub max: u8,
}

/// One tech tree node from `tech_tree.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct TechNodeDef {
    /// Unique string id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Era index, 0..=5.
    pub era: u8,
    /// Parent node ids.
    #[serde(default)]
    pub parents: Vec<String>,
    /// Items that must be delivered to the Research Desk.
    #[serde(default)]
    pub cost: Vec<(String, u32)>,
    /// Content ids unlocked by this node.
    #[serde(default)]
    pub unlocks: Vec<String>,
}

/// Hazard settings for a layer.
#[derive(Debug, Clone, Copy, Deserialize)]
pub enum LayerHazard {
    /// Water pockets that flood when breached.
    Flooding {
        /// Chance per chunk of a pocket.
        pocket_chance: f32,
        /// Maximum pocket volume in blocks.
        max_volume: u32,
    },
    /// Methane pockets.
    Gas {
        /// Chance per chunk of a pocket.
        pocket_chance: f32,
        /// Oxygen drained per second inside a pocket.
        oxygen_drain: f32,
    },
    /// Rising body heat.
    Heat {
        /// Heat added per second at the top of the layer.
        base_rate: f32,
        /// Extra heat per second per block of depth.
        depth_rate: f32,
    },
    /// Ceiling collapses.
    CaveIn {
        /// Maximum unsupported span in blocks.
        max_span: u8,
    },
    /// Ambient radiation from the rock itself.
    Radiation {
        /// Dose per second in this layer.
        dose_per_sec: f32,
    },
}

/// One ore entry in a layer's table.
#[derive(Debug, Clone, Deserialize)]
pub struct OreEntry {
    /// Block id.
    pub block: String,
    /// Vein frequency, 0..1 (roughly the share of chunks that contain a vein).
    pub frequency: f32,
    /// Vein size range in blocks.
    pub vein_size: (u32, u32),
}

/// One layer from `layers.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct LayerDef {
    /// Unique string id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Depth band below the surface, `(top, bottom)`.
    pub depth: (u32, u32),
    /// The filler block.
    pub base_block: String,
    /// Secondary filler blocks with their share, e.g. limestone lenses.
    #[serde(default)]
    pub fillers: Vec<(String, f32)>,
    /// Ore table.
    #[serde(default)]
    pub ores: Vec<OreEntry>,
    /// Hazards present in this layer.
    #[serde(default)]
    pub hazards: Vec<LayerHazard>,
    /// Ambient rock temperature in °C.
    #[serde(default)]
    pub ambient_temp: f32,
    /// Minimum tool tier to dig this layer's base block (mirrors the block table).
    #[serde(default)]
    pub min_tier: u8,
    /// Cave density multiplier.
    #[serde(default = "one")]
    pub cave_density: f32,
}

/// Hazard tuning from `hazards.ron`.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct HazardTuning {
    /// Radiation dose above which the screen grains.
    pub dose_grain: f32,
    /// Radiation dose above which health regeneration stops.
    pub dose_no_regen: f32,
    /// Radiation dose above which health drains.
    pub dose_damage: f32,
    /// Damage per second at `dose_damage` and beyond.
    pub dose_damage_rate: f32,
    /// Dose removed per second by a Decon Shower.
    pub decon_rate: f32,
    /// Natural dose decay per second.
    pub dose_decay: f32,
    /// Body heat at which health drains.
    pub heat_limit: f32,
    /// Heat lost per second in cool air.
    pub heat_cooling: f32,
    /// Oxygen capacity.
    pub oxygen_capacity: f32,
    /// Oxygen drained per second while underwater.
    pub oxygen_drain_water: f32,
    /// Oxygen drained per second in methane.
    pub oxygen_drain_gas: f32,
    /// Oxygen regenerated per second in clean air.
    pub oxygen_regen: f32,
    /// Health lost per second with no oxygen.
    pub suffocation_damage: f32,
    /// Methane ignited by an open flame: burst damage.
    pub gas_ignition_damage: f32,
    /// Seconds a radiation zone left by a meltdown lasts.
    pub meltdown_zone_seconds: f32,
    /// Player health.
    pub max_health: f32,
    /// Player stamina.
    pub max_stamina: f32,
    /// Stamina drained per second while sprinting.
    pub sprint_stamina: f32,
    /// Stamina drained per second while climbing.
    pub climb_stamina: f32,
    /// Stamina regenerated per second while resting.
    pub stamina_regen: f32,
    /// Fall damage per block above the safe height.
    pub fall_damage_per_block: f32,
    /// Fall height that causes no damage.
    pub safe_fall_blocks: f32,
}
