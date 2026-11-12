//! Block and chunk coordinates.
//!
//! The world is a fixed 1024 × 512 × 1024 block box. `y = 0` is the deepest point, the surface
//! sits at [`SURFACE_Y`], and `y = WORLD_HEIGHT - 1` is the top of the build volume.

use bevy::math::{IVec3, UVec3};
use serde::{Deserialize, Serialize};

/// Chunk edge length in blocks.
pub const CHUNK_SIZE: usize = 32;

/// Number of blocks in a chunk.
pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// World width and depth in blocks.
pub const WORLD_HORIZONTAL: i32 = 1024;

/// World height in blocks.
pub const WORLD_HEIGHT: i32 = 512;

/// Y coordinate of the surface (the Base sits just above this).
pub const SURFACE_Y: i32 = 448;

/// Integer block coordinate in world space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockPos {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate (up).
    pub y: i32,
    /// Z coordinate.
    pub z: i32,
}

impl BlockPos {
    /// Creates a block position.
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Converts to a vector.
    pub fn as_ivec3(self) -> IVec3 {
        IVec3::new(self.x, self.y, self.z)
    }

    /// Converts to an unsigned vector, for local chunk indices.
    pub fn as_uvec3(self) -> UVec3 {
        UVec3::new(self.x as u32, self.y as u32, self.z as u32)
    }

    /// The chunk containing this block.
    pub fn chunk(self) -> ChunkPos {
        ChunkPos {
            x: self.x.div_euclid(CHUNK_SIZE as i32),
            y: self.y.div_euclid(CHUNK_SIZE as i32),
            z: self.z.div_euclid(CHUNK_SIZE as i32),
        }
    }

    /// The block's index inside its chunk.
    pub fn local(self) -> UVec3 {
        UVec3::new(
            self.x.rem_euclid(CHUNK_SIZE as i32) as u32,
            self.y.rem_euclid(CHUNK_SIZE as i32) as u32,
            self.z.rem_euclid(CHUNK_SIZE as i32) as u32,
        )
    }

    /// Centre of the block in world space.
    pub fn center(self) -> bevy::math::Vec3 {
        bevy::math::Vec3::new(self.x as f32 + 0.5, self.y as f32 + 0.5, self.z as f32 + 0.5)
    }

    /// Minimum corner of the block.
    pub fn min(self) -> bevy::math::Vec3 {
        bevy::math::Vec3::new(self.x as f32, self.y as f32, self.z as f32)
    }

    /// Depth below the surface: positive underground, negative above.
    pub fn depth(self) -> i32 {
        SURFACE_Y - self.y
    }

    /// Whether the block is inside the world box.
    pub fn in_bounds(self) -> bool {
        self.y >= 0
            && self.y < WORLD_HEIGHT
            && self.x >= 0
            && self.x < WORLD_HORIZONTAL
            && self.z >= 0
            && self.z < WORLD_HORIZONTAL
    }

    /// Offsets the position.
    pub fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    /// The block position containing a world-space point.
    pub fn from_vec3(position: bevy::math::Vec3) -> Self {
        Self::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        )
    }
}

impl Default for BlockPos {
    fn default() -> Self {
        Self::new(0, 0, 0)
    }
}

/// Integer chunk coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkPos {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
    /// Z coordinate.
    pub z: i32,
}

impl ChunkPos {
    /// Creates a chunk position.
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The block position of the chunk's minimum corner.
    pub fn origin(self) -> BlockPos {
        BlockPos::new(
            self.x * CHUNK_SIZE as i32,
            self.y * CHUNK_SIZE as i32,
            self.z * CHUNK_SIZE as i32,
        )
    }

    /// The world-space centre of the chunk.
    pub fn center(self) -> bevy::math::Vec3 {
        bevy::math::Vec3::new(
            (self.x as f32 + 0.5) * CHUNK_SIZE as f32,
            (self.y as f32 + 0.5) * CHUNK_SIZE as f32,
            (self.z as f32 + 0.5) * CHUNK_SIZE as f32,
        )
    }

    /// Number of chunks across the world horizontally.
    pub const fn horizontal_count() -> i32 {
        WORLD_HORIZONTAL / CHUNK_SIZE as i32
    }

    /// Number of chunks vertically.
    pub const fn vertical_count() -> i32 {
        WORLD_HEIGHT / CHUNK_SIZE as i32
    }

    /// Whether the chunk is inside the world.
    pub fn in_bounds(self) -> bool {
        self.x >= 0
            && self.z >= 0
            && self.y >= 0
            && self.x < Self::horizontal_count()
            && self.z < Self::horizontal_count()
            && self.y < Self::vertical_count()
    }

    /// Whether a block position is inside this chunk.
    pub fn contains(self, pos: BlockPos) -> bool {
        pos.chunk() == self
    }

    /// Offsets the chunk position.
    pub fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    /// Chebyshev distance to another chunk.
    pub fn distance(self, other: ChunkPos) -> i32 {
        (self.x - other.x)
            .abs()
            .max((self.y - other.y).abs())
            .max((self.z - other.z).abs())
    }
}

/// Index of a local block coordinate inside a chunk's flat array.
///
/// The layout is `x + CHUNK_SIZE * (z + CHUNK_SIZE * y)`, chosen so that horizontal slices are
/// contiguous, which keeps the mesher's cache behaviour good.
#[inline]
pub fn local_index(local: UVec3) -> usize {
    (local.x + CHUNK_SIZE as u32 * (local.z + CHUNK_SIZE as u32 * local.y)) as usize
}

/// Inverse of [`local_index`].
#[inline]
pub fn index_to_local(index: usize) -> UVec3 {
    let x = index % CHUNK_SIZE;
    let z = (index / CHUNK_SIZE) % CHUNK_SIZE;
    let y = index / (CHUNK_SIZE * CHUNK_SIZE);
    UVec3::new(x as u32, y as u32, z as u32)
}

/// The six axis-aligned block faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BlockFace {
    /// +X.
    East,
    /// -X.
    West,
    /// +Y.
    #[default]
    Up,
    /// -Y.
    Down,
    /// +Z.
    South,
    /// -Z.
    North,
}

impl BlockFace {
    /// All faces, in the order used by the mesher and the atlas.
    pub const ALL: [BlockFace; 6] = [
        BlockFace::East,
        BlockFace::West,
        BlockFace::Up,
        BlockFace::Down,
        BlockFace::South,
        BlockFace::North,
    ];

    /// Unit normal of the face.
    pub fn normal(self) -> IVec3 {
        match self {
            BlockFace::East => IVec3::X,
            BlockFace::West => IVec3::NEG_X,
            BlockFace::Up => IVec3::Y,
            BlockFace::Down => IVec3::NEG_Y,
            BlockFace::South => IVec3::Z,
            BlockFace::North => IVec3::NEG_Z,
        }
    }

    /// The neighbouring block position across this face.
    pub fn neighbor(self, pos: BlockPos) -> BlockPos {
        let normal = self.normal();
        pos.offset(normal.x, normal.y, normal.z)
    }

    /// The face opposite to this one.
    pub fn opposite(self) -> Self {
        match self {
            BlockFace::East => BlockFace::West,
            BlockFace::West => BlockFace::East,
            BlockFace::Up => BlockFace::Down,
            BlockFace::Down => BlockFace::Up,
            BlockFace::South => BlockFace::North,
            BlockFace::North => BlockFace::South,
        }
    }

    /// Picks the dominant face from a normal vector.
    pub fn from_normal(normal: bevy::math::Vec3) -> Self {
        let abs = normal.abs();
        if abs.x >= abs.y && abs.x >= abs.z {
            if normal.x >= 0.0 {
                BlockFace::East
            } else {
                BlockFace::West
            }
        } else if abs.y >= abs.z {
            if normal.y >= 0.0 {
                BlockFace::Up
            } else {
                BlockFace::Down
            }
        } else if normal.z >= 0.0 {
            BlockFace::South
        } else {
            BlockFace::North
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_index_round_trips() {
        for y in [0usize, 1, 17, 31] {
            for z in [0usize, 5, 31] {
                for x in [0usize, 9, 31] {
                    let local = UVec3::new(x as u32, y as u32, z as u32);
                    assert_eq!(index_to_local(local_index(local)), local);
                }
            }
        }
    }

    #[test]
    fn negative_coordinates_floor_correctly() {
        let pos = BlockPos::new(-1, -33, -32);
        assert_eq!(pos.chunk(), ChunkPos::new(-1, -2, -1));
        assert_eq!(pos.local(), UVec3::new(31, 31, 0));
    }

    #[test]
    fn chunk_origin_matches_local_zero() {
        let chunk = ChunkPos::new(3, 7, 2);
        let origin = chunk.origin();
        assert_eq!(origin.chunk(), chunk);
        assert_eq!(origin.local(), UVec3::ZERO);
    }

    #[test]
    fn faces_are_symmetric() {
        for face in BlockFace::ALL {
            assert_eq!(face.opposite().opposite(), face);
            assert_eq!(face.normal(), -face.opposite().normal());
        }
    }
}
