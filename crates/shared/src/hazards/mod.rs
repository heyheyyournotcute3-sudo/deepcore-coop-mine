//! Hazards: radiation dose, body heat, oxygen and gas.
//!
//! All of these are pure functions of the environment and the player's gear, so the server can
//! run them authoritatively and the client can show the same numbers without guessing.

use bevy::prelude::*;

use crate::content::{ContentId, ContentRegistry, GearSlot, ItemCategory, RadiationDef};

/// Registers hazard resources.
pub struct HazardsPlugin;

impl Plugin for HazardsPlugin {
    fn build(&self, _app: &mut App) {}
}

/// A player's vitals.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Vitals {
    /// Health.
    pub health: f32,
    /// Stamina.
    pub stamina: f32,
    /// Oxygen.
    pub oxygen: f32,
    /// Body heat in °C above the ambient comfort level.
    pub body_heat: f32,
    /// Radiation dose.
    pub dose: f32,
}

impl Default for Vitals {
    fn default() -> Self {
        Self {
            health: 100.0,
            stamina: 100.0,
            oxygen: 100.0,
            body_heat: 0.0,
            dose: 0.0,
        }
    }
}

/// The gear a player is wearing, by slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WornGear {
    /// Suit slot.
    pub suit: Option<ContentId>,
    /// Mask slot.
    pub mask: Option<ContentId>,
    /// Instrument slot.
    pub instrument: Option<ContentId>,
}

impl WornGear {
    /// The gear in a slot.
    pub fn get(&self, slot: GearSlot) -> Option<ContentId> {
        match slot {
            GearSlot::Suit => self.suit,
            GearSlot::Mask => self.mask,
            GearSlot::Instrument => self.instrument,
        }
    }

    /// Total radiation shielding, capped at 0.95.
    pub fn radiation_shielding(&self, content: &ContentRegistry) -> f32 {
        let mut shielding = 0.0f32;
        for slot in [GearSlot::Suit, GearSlot::Mask, GearSlot::Instrument] {
            if let Some(item) = self.get(slot)
                && let Some(ItemCategory::Gear {
                    radiation_shielding, ..
                }) = content.item(item).map(|item| &item.category)
            {
                // Shielding combines multiplicatively so two suits do not stack to immunity.
                shielding = 1.0 - (1.0 - shielding) * (1.0 - radiation_shielding);
            }
        }
        shielding.clamp(0.0, 0.95)
    }

    /// Heat resistance in °C.
    pub fn heat_resistance(&self, content: &ContentRegistry) -> f32 {
        let mut total = 0.0;
        for slot in [GearSlot::Suit, GearSlot::Mask, GearSlot::Instrument] {
            if let Some(item) = self.get(slot)
                && let Some(ItemCategory::Gear {
                    heat_resistance, ..
                }) = content.item(item).map(|item| &item.category)
            {
                total += heat_resistance;
            }
        }
        total
    }

    /// Gas protection multiplier, 0..1.
    pub fn gas_protection(&self, content: &ContentRegistry) -> f32 {
        let mut protection = 1.0f32;
        for slot in [GearSlot::Suit, GearSlot::Mask, GearSlot::Instrument] {
            if let Some(item) = self.get(slot)
                && let Some(ItemCategory::Gear {
                    gas_protection, ..
                }) = content.item(item).map(|item| &item.category)
            {
                protection *= gas_protection;
            }
        }
        protection.clamp(0.0, 1.0)
    }
}

/// Dose from a point source, falling off with distance.
///
/// A source at distance `d` contributes `dose_per_sec` at zero distance and nothing beyond
/// `radius`, with a linear falloff. Several sources add up.
pub fn source_dose(sources: &[(RadiationDef, f32)], shielding: f32) -> f32 {
    let raw: f32 = sources
        .iter()
        .map(|(def, distance)| {
            if *distance >= def.radius {
                0.0
            } else {
                def.dose_per_sec * (1.0 - distance / def.radius)
            }
        })
        .sum();
    raw * (1.0 - shielding.clamp(0.0, 0.95))
}

/// Applies one hazard tick to a player's vitals.
///
/// `environment` describes what the player is standing in this tick.
pub fn tick_vitals(
    content: &ContentRegistry,
    vitals: &mut Vitals,
    gear: &WornGear,
    environment: &Environment,
    dt: f32,
) -> VitalEffects {
    let tuning = content.hazard_tuning();
    let shielding = gear.radiation_shielding(content);
    let gas_protection = gear.gas_protection(content);
    let heat_resistance = gear.heat_resistance(content);

    let mut effects = VitalEffects::default();

    // --- radiation -------------------------------------------------------------------
    let dose = source_dose(&environment.radiation, shielding) * dt;
    vitals.dose = (vitals.dose + dose - tuning.dose_decay * dt).max(0.0);
    if environment.decon {
        vitals.dose = (vitals.dose - tuning.decon_rate * dt).max(0.0);
    }
    if vitals.dose >= tuning.dose_damage {
        let damage = tuning.dose_damage_rate * dt;
        vitals.health -= damage;
        effects.damage += damage;
    }

    // --- oxygen ----------------------------------------------------------------------
    let drain = if environment.underwater {
        tuning.oxygen_drain_water
    } else if environment.gas > 0.0 {
        tuning.oxygen_drain_gas * environment.gas * gas_protection
    } else {
        0.0
    };
    if drain > 0.0 {
        vitals.oxygen = (vitals.oxygen - drain * dt).clamp(0.0, tuning.oxygen_capacity);
    } else {
        vitals.oxygen = (vitals.oxygen + tuning.oxygen_regen * dt).min(tuning.oxygen_capacity);
    }
    if vitals.oxygen <= 0.0 {
        let damage = tuning.suffocation_damage * dt;
        vitals.health -= damage;
        effects.damage += damage;
        effects.suffocating = true;
    }

    // --- heat ------------------------------------------------------------------------
    let heating = environment.ambient_heat - heat_resistance;
    if heating > 0.0 {
        vitals.body_heat += heating * dt;
    } else {
        vitals.body_heat = (vitals.body_heat - tuning.heat_cooling * dt).max(0.0);
    }
    if vitals.body_heat > tuning.heat_limit {
        let damage = tuning.heat_limit.max(1.0) * 0.01 * dt;
        vitals.health -= damage;
        effects.damage += damage;
        effects.overheated = true;
    }

    // --- regeneration ----------------------------------------------------------------
    if vitals.health > 0.0 && vitals.health < tuning.max_health && vitals.dose < tuning.dose_no_regen {
        let regen = if effects.damage <= 0.0 { 0.6 } else { 0.0 };
        vitals.health = (vitals.health + regen * dt).min(tuning.max_health);
    }

    // --- stamina ---------------------------------------------------------------------
    if environment.sprinting {
        vitals.stamina = (vitals.stamina - tuning.sprint_stamina * dt).max(0.0);
    } else if environment.climbing {
        vitals.stamina = (vitals.stamina - tuning.climb_stamina * dt).max(0.0);
    } else {
        vitals.stamina = (vitals.stamina + tuning.stamina_regen * dt).min(tuning.max_stamina);
    }

    vitals.health = vitals.health.clamp(0.0, tuning.max_health);
    effects.alive = vitals.health > 0.0;
    effects
}

/// What the player is standing in this tick.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// Radiation sources with their distance to the player.
    pub radiation: Vec<(RadiationDef, f32)>,
    /// Ambient heat in °C.
    pub ambient_heat: f32,
    /// Gas concentration 0..1.
    pub gas: f32,
    /// The player's head is under water.
    pub underwater: bool,
    /// The player is sprinting.
    pub sprinting: bool,
    /// The player is climbing.
    pub climbing: bool,
    /// The player is in a Decon Shower.
    pub decon: bool,
}

/// The effects a hazard tick had, for HUD feedback.
#[derive(Debug, Clone, Copy, Default)]
pub struct VitalEffects {
    /// Damage dealt this tick.
    pub damage: f32,
    /// The player is out of oxygen.
    pub suffocating: bool,
    /// The player is overheating.
    pub overheated: bool,
    /// The player is still alive.
    pub alive: bool,
}

/// Fall damage for a fall of `blocks`.
pub fn fall_damage(content: &ContentRegistry, blocks: f32) -> f32 {
    let tuning = content.hazard_tuning();
    let excess = blocks - tuning.safe_fall_blocks;
    if excess <= 0.0 {
        0.0
    } else {
        excess * tuning.fall_damage_per_block
    }
}

/// Geiger clicks per second for a dose rate, used by the HUD.
pub fn geiger_rate(dose_per_sec: f32) -> f32 {
    (dose_per_sec * 8.0).clamp(0.0, 40.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    #[test]
    fn dose_falls_off_with_distance() {
        let source = RadiationDef {
            radius: 4.0,
            dose_per_sec: 0.5,
        };
        assert!((source_dose(&[(source, 0.0)], 0.0) - 0.5).abs() < 1e-6);
        assert!((source_dose(&[(source, 2.0)], 0.0) - 0.25).abs() < 1e-6);
        assert_eq!(source_dose(&[(source, 5.0)], 0.0), 0.0);
    }

    #[test]
    fn hazmat_suit_cuts_dose_by_eighty_percent() {
        let content = content();
        let hazmat = content
            .item_by_key("hazmat_suit")
            .expect("hazmat suit exists")
            .id;
        let gear = WornGear {
            suit: Some(hazmat),
            ..Default::default()
        };
        let shielding = gear.radiation_shielding(&content);
        assert!((shielding - 0.8).abs() < 0.01, "shielding = {shielding}");
    }

    #[test]
    fn high_dose_drains_health() {
        let content = content();
        let mut vitals = Vitals {
            dose: 700.0,
            ..Default::default()
        };
        let environment = Environment::default();
        let effects = tick_vitals(&content, &mut vitals, &WornGear::default(), &environment, 1.0);
        assert!(vitals.health < 100.0);
        assert!(effects.damage > 0.0);
    }

    #[test]
    fn underwater_drains_oxygen_and_then_damages() {
        let content = content();
        let mut vitals = Vitals::default();
        let environment = Environment {
            underwater: true,
            ..Default::default()
        };
        // 100 ticks of 0.1 s at 6 oxygen/s drains 60 of the 100 units of oxygen.
        for _ in 0..100 {
            tick_vitals(&content, &mut vitals, &WornGear::default(), &environment, 0.1);
        }
        assert!(vitals.oxygen < 50.0, "oxygen = {}", vitals.oxygen);
    }

    #[test]
    fn clean_air_regenerates_health_and_oxygen() {
        let content = content();
        let mut vitals = Vitals {
            health: 50.0,
            oxygen: 20.0,
            ..Default::default()
        };
        let environment = Environment::default();
        for _ in 0..60 {
            tick_vitals(&content, &mut vitals, &WornGear::default(), &environment, 0.1);
        }
        assert!(vitals.health > 50.0);
        assert!(vitals.oxygen > 20.0);
    }
}
