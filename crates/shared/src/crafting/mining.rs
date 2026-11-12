//! Mining and placement rules, shared by the client's prediction and the server's authority.

use crate::content::{ContentId, ContentRegistry, ItemCategory};

/// Server tick rate for the simulation.
pub const TICK_HZ: f32 = 30.0;

/// Seconds per server tick.
pub const TICK_SECONDS: f32 = 1.0 / TICK_HZ;

/// Reach for hand tools.
pub const REACH: f32 = 5.0;

/// Reach for drills.
pub const DRILL_REACH: f32 = 6.0;

/// The outcome of asking "can this tool break this block?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiningVerdict {
    /// The block can be mined; `ticks` is the break time in server ticks.
    CanMine {
        /// Break time in server ticks.
        ticks: u32,
    },
    /// The tool tier is too low.
    TooHard {
        /// The tier the block needs.
        required_tier: u8,
    },
    /// The block cannot be broken at all.
    Unbreakable,
    /// There is nothing to mine.
    Empty,
}

/// Break time in seconds from the formula `hardness / (tool_speed × power)`.
pub fn break_time_seconds(hardness: f32, tool_speed: f32, power_multiplier: f32) -> f32 {
    if tool_speed <= 0.0 || power_multiplier <= 0.0 {
        return f32::INFINITY;
    }
    hardness / (tool_speed * power_multiplier)
}

/// Break time rounded up to whole server ticks.
pub fn break_time_ticks(hardness: f32, tool_speed: f32, power_multiplier: f32) -> u32 {
    let seconds = break_time_seconds(hardness, tool_speed, power_multiplier);
    if !seconds.is_finite() {
        return u32::MAX;
    }
    ((seconds / TICK_SECONDS).ceil() as u32).max(1)
}

/// The tier and speed of whatever the player is holding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolStats {
    /// Minimum tier the tool can break.
    pub tier: u8,
    /// Speed multiplier.
    pub speed: f32,
    /// Mining area.
    pub area: (u8, u8),
    /// Reach.
    pub reach: f32,
}

impl ToolStats {
    /// Bare hands: tier 0, cannot break tiered blocks.
    pub const HANDS: Self = Self {
        tier: 0,
        speed: 0.5,
        area: (1, 1),
        reach: REACH,
    };

    /// The starting steel pickaxe, used when the player holds nothing.
    pub const STARTING_PICK: Self = Self {
        tier: 1,
        speed: 1.0,
        area: (1, 1),
        reach: REACH,
    };

    /// Reads the stats of a held item, falling back to hands.
    pub fn from_item(item: ContentId, content: &ContentRegistry) -> Self {
        match content.item(item).map(|item| &item.category) {
            Some(ItemCategory::Tool {
                tier,
                speed,
                area,
                reach,
                ..
            }) => Self {
                tier: *tier,
                speed: *speed,
                area: *area,
                reach: *reach,
            },
            _ => Self::HANDS,
        }
    }

    /// Whether the tool is an area drill.
    pub fn is_drill(&self) -> bool {
        self.area.0 > 1 || self.area.1 > 1
    }
}

/// Checks whether a tool can break a block, and how long it takes.
pub fn evaluate_mining(
    block: ContentId,
    tool: &ToolStats,
    content: &ContentRegistry,
    power_multiplier: f32,
) -> MiningVerdict {
    if block == content.air {
        return MiningVerdict::Empty;
    }
    let Some(def) = content.block(block) else {
        return MiningVerdict::Unbreakable;
    };
    if def.unbreakable {
        return MiningVerdict::Unbreakable;
    }
    if tool.tier < def.min_tier {
        return MiningVerdict::TooHard {
            required_tier: def.min_tier,
        };
    }
    MiningVerdict::CanMine {
        ticks: break_time_ticks(def.hardness, tool.speed, power_multiplier),
    }
}

/// The blocks an area tool breaks, given the targeted block and face.
///
/// The area is applied on the plane of the face: for a side face the area spans the two
/// perpendicular axes, for a floor or ceiling face it spans the horizontal axes.
pub fn area_blocks(
    target: crate::voxel::BlockPos,
    face: crate::voxel::BlockFace,
    area: (u8, u8),
) -> Vec<crate::voxel::BlockPos> {
    use crate::voxel::BlockFace;
    let (width, height) = area;
    if width <= 1 && height <= 1 {
        return vec![target];
    }
    let half_w = (width as i32 - 1) / 2;
    let half_h = (height as i32 - 1) / 2;
    // Pick the two axes perpendicular to the face normal.
    let (axis_u, axis_v) = match face {
        BlockFace::East | BlockFace::West => (bevy::math::IVec3::Z, bevy::math::IVec3::Y),
        BlockFace::Up | BlockFace::Down => (bevy::math::IVec3::X, bevy::math::IVec3::Z),
        BlockFace::South | BlockFace::North => (bevy::math::IVec3::X, bevy::math::IVec3::Y),
    };
    let mut blocks = Vec::with_capacity(width as usize * height as usize);
    for v in -half_h..=half_h {
        for u in -half_w..=half_w {
            let offset = axis_u * u + axis_v * v;
            blocks.push(target.offset(offset.x, offset.y, offset.z));
        }
    }
    blocks
}

/// Whether the player can place a block against `face` of `target`, given the block in hand.
pub fn placement_position(
    target: crate::voxel::BlockPos,
    face: crate::voxel::BlockFace,
) -> crate::voxel::BlockPos {
    face.neighbor(target)
}

/// Amortised wear of a tool per block broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolWear {
    /// The tool consumes one unit of stored energy per block.
    Energy,
    /// The tool consumes nothing.
    None,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::{BlockFace, BlockPos};

    fn content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    #[test]
    fn break_time_formula_matches_spec() {
        // hardness 2.5, tier-1 tool at 1.0× → 2.5 s → 75 ticks at 30 Hz.
        assert_eq!(break_time_ticks(2.5, 1.0, 1.0), 75);
        // A 12× nuclear drill on the same block: 2.5 / 12 = 0.208 s → 7 ticks.
        assert_eq!(break_time_ticks(2.5, 12.0, 1.0), 7);
        // Power multiplier scales it too.
        assert_eq!(break_time_ticks(2.5, 1.0, 2.0), 38);
    }

    #[test]
    fn tier_gate_rejects_steel_pick_on_granite() {
        let content = content();
        let granite = content.block_by_key("granite").expect("granite").id;
        let verdict = evaluate_mining(granite, &ToolStats::STARTING_PICK, &content, 1.0);
        assert_eq!(verdict, MiningVerdict::TooHard { required_tier: 2 });
    }

    #[test]
    fn tier_two_tool_mines_granite() {
        let content = content();
        let granite = content.block_by_key("granite").expect("granite").id;
        let tool = ToolStats {
            tier: 2,
            speed: 1.6,
            area: (1, 1),
            reach: REACH,
        };
        match evaluate_mining(granite, &tool, &content, 1.0) {
            MiningVerdict::CanMine { ticks } => assert!(ticks > 0),
            other => panic!("expected CanMine, got {other:?}"),
        }
    }

    #[test]
    fn bedrock_is_unbreakable() {
        let content = content();
        let tool = ToolStats {
            tier: 5,
            speed: 12.0,
            area: (3, 3),
            reach: DRILL_REACH,
        };
        assert_eq!(
            evaluate_mining(ContentId(crate::voxel::BEDROCK_ID), &tool, &content, 1.0),
            MiningVerdict::Unbreakable
        );
    }

    #[test]
    fn area_blocks_are_a_square_on_the_face_plane() {
        let blocks = area_blocks(BlockPos::new(10, 20, 30), BlockFace::North, (3, 3));
        assert_eq!(blocks.len(), 9);
        assert!(blocks.contains(&BlockPos::new(10, 20, 30)));
        assert!(blocks.contains(&BlockPos::new(9, 19, 30)));
        assert!(blocks.contains(&BlockPos::new(11, 21, 30)));
        assert!(blocks.iter().all(|pos| pos.z == 30));
    }

    #[test]
    fn placement_goes_into_the_face_normal() {
        let target = BlockPos::new(0, 0, 0);
        assert_eq!(
            placement_position(target, BlockFace::Up),
            BlockPos::new(0, 1, 0)
        );
        assert_eq!(
            placement_position(target, BlockFace::North),
            BlockPos::new(0, 0, -1)
        );
    }
}
