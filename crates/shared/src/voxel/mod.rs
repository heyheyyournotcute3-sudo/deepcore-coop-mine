//! The voxel world: a sparse set of loaded chunks plus block access across chunk borders.

use std::collections::HashMap;

use bevy::prelude::*;

pub mod chunk;
pub mod pos;

pub use chunk::Chunk;
pub use pos::{
    BlockFace, BlockPos, CHUNK_SIZE, CHUNK_VOLUME, ChunkPos, SURFACE_Y, WORLD_HEIGHT,
    WORLD_HORIZONTAL, index_to_local, local_index,
};

/// Registers the voxel resource containers.
pub struct VoxelPlugin;

impl Plugin for VoxelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadedChunks>();
    }
}

/// The chunks currently in memory on this process, keyed by chunk position.
#[derive(Debug, Default, Resource)]
pub struct LoadedChunks {
    chunks: HashMap<ChunkPos, Chunk>,
}

impl LoadedChunks {
    /// Inserts or replaces a chunk.
    pub fn insert(&mut self, pos: ChunkPos, chunk: Chunk) {
        self.chunks.insert(pos, chunk);
    }

    /// Removes a chunk.
    pub fn remove(&mut self, pos: ChunkPos) -> Option<Chunk> {
        self.chunks.remove(&pos)
    }

    /// Borrows a chunk.
    pub fn get(&self, pos: ChunkPos) -> Option<&Chunk> {
        self.chunks.get(&pos)
    }

    /// Mutably borrows a chunk.
    pub fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut Chunk> {
        self.chunks.get_mut(&pos)
    }

    /// Whether the chunk is loaded.
    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.chunks.contains_key(&pos)
    }

    /// Number of loaded chunks.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether no chunk is loaded.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Iterates loaded chunk positions.
    pub fn positions(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.chunks.keys().copied()
    }

    /// Reads a block, returning `None` when its chunk is not loaded.
    ///
    /// Outside the world box the answer is bedrock below/around and air above, so callers do
    /// not need bounds checks.
    pub fn block(&self, pos: BlockPos) -> Option<u16> {
        if pos.y >= WORLD_HEIGHT {
            return Some(0);
        }
        if !pos.in_bounds() {
            return Some(BEDROCK_ID);
        }
        let chunk = self.chunks.get(&pos.chunk())?;
        Some(chunk.get(pos.local()))
    }

    /// Writes a block. Returns `false` when the chunk is not loaded.
    pub fn set_block(&mut self, pos: BlockPos, block: u16) -> bool {
        if !pos.in_bounds() {
            return false;
        }
        let Some(chunk) = self.chunks.get_mut(&pos.chunk()) else {
            return false;
        };
        chunk.set(pos.local(), block);
        true
    }

    /// Whether a block at `pos` blocks movement. Unloaded chunks count as solid so players
    /// cannot fall through the world while chunks stream in.
    pub fn is_solid(&self, pos: BlockPos, registry: &crate::content::ContentRegistry) -> bool {
        match self.block(pos) {
            Some(id) => registry
                .block(crate::content::ContentId(id))
                .map(|block| block.solid)
                .unwrap_or(false),
            None => true,
        }
    }
}

/// A read-only view of blocks, used by the mesher and the movement collision code.
pub trait BlockSource {
    /// Reads a block, returning `None` when the chunk is not loaded.
    fn block(&self, pos: BlockPos) -> Option<u16>;
}

impl BlockSource for LoadedChunks {
    fn block(&self, pos: BlockPos) -> Option<u16> {
        LoadedChunks::block(self, pos)
    }
}

/// The id of the unbreakable bedrock block. `1` is pinned by the registry (`air` is `0`).
pub const BEDROCK_ID: u16 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_chunk_access() {
        let mut chunks = LoadedChunks::default();
        chunks.insert(ChunkPos::new(0, 0, 0), Chunk::filled(5));
        chunks.insert(ChunkPos::new(1, 0, 0), Chunk::filled(6));
        assert_eq!(chunks.block(BlockPos::new(31, 1, 1)), Some(5));
        assert_eq!(chunks.block(BlockPos::new(32, 1, 1)), Some(6));
        assert_eq!(chunks.block(BlockPos::new(1, 1, 40)), None);
    }

    #[test]
    fn out_of_bounds_is_bedrock_or_air() {
        let chunks = LoadedChunks::default();
        assert_eq!(chunks.block(BlockPos::new(-1, 4, 4)), Some(BEDROCK_ID));
        assert_eq!(chunks.block(BlockPos::new(4, WORLD_HEIGHT, 4)), Some(0));
    }
}
