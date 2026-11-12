//! Deterministic world generation.
//!
//! Generation is a pure function of `(seed, chunk position)` plus the content registry. Two
//! machines with the same seed produce bit-identical chunks, which is what lets the server skip
//! saving unmodified chunks and lets a client trust anything the server does not resend.
//!
//! Structure, top to bottom:
//! 1. **Layers** fill the column with the base block, secondary fillers and the surface.
//! 2. **Caves** carve tunnels and caverns with 3D noise, per-layer density.
//! 3. **Ore veins** replace stone with ore in blobs, sized and spaced by the layer's ore table.
//! 4. **Pockets** add water and gas pockets for the flooding and gas hazards.

use bevy::prelude::*;
use noise::{NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::content::{ContentId, ContentRegistry, LayerHazard};
use crate::voxel::{
    BEDROCK_ID, BlockPos, CHUNK_SIZE, Chunk, ChunkPos, LoadedChunks, SURFACE_Y, WORLD_HEIGHT,
    WORLD_HORIZONTAL,
};

/// Registers world generation resources.
pub struct WorldgenPlugin;

impl Plugin for WorldgenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldSeed>();
    }
}

/// The world seed. The same seed always produces the same world.
#[derive(Debug, Resource, Clone, Copy)]
pub struct WorldSeed(pub u64);

impl Default for WorldSeed {
    fn default() -> Self {
        Self(0x0DEE9C04E)
    }
}

/// Deterministic noise field set, rebuilt per generator instance.
pub struct WorldGenerator {
    /// The seed.
    pub seed: u64,
    /// Cave tunnel noise.
    cave_a: Perlin,
    /// Second cave tunnel noise (the tunnels are the intersection of two fields).
    cave_b: Perlin,
    /// Cavern noise.
    cavern: Perlin,
    /// Fill blend noise, for limestone lenses and shale bands.
    blend: Perlin,
}

impl WorldGenerator {
    /// Builds a generator for a seed.
    pub fn new(seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5EED_C0DE);
        Self {
            seed,
            cave_a: Perlin::new(rng.random()),
            cave_b: Perlin::new(rng.random()),
            cavern: Perlin::new(rng.random()),
            blend: Perlin::new(rng.random()),
        }
    }

    /// Generates one chunk.
    pub fn generate_chunk(&self, pos: ChunkPos, content: &ContentRegistry) -> Chunk {
        let origin = pos.origin();
        // Start from air; every column below the surface is then filled.
        let mut indices = vec![0u16; crate::voxel::CHUNK_VOLUME];
        let mut palette: Vec<u16> = vec![0];

        let intern = |palette: &mut Vec<u16>, block: ContentId| -> u16 {
            if let Some(slot) = palette.iter().position(|entry| *entry == block.0) {
                return slot as u16;
            }
            palette.push(block.0);
            (palette.len() - 1) as u16
        };

        for local_y in 0..CHUNK_SIZE {
            let y = origin.y + local_y as i32;
            if y < 0 || y >= WORLD_HEIGHT {
                continue;
            }
            let depth = (SURFACE_Y - y).max(0) as u32;
            let Some(layer) = content.layer_at_depth(depth) else {
                continue;
            };
            for local_z in 0..CHUNK_SIZE {
                for local_x in 0..CHUNK_SIZE {
                    let x = origin.x + local_x as i32;
                    let z = origin.z + local_z as i32;
                    let block = self.column_block(x, y, z, depth, layer, content);
                    let slot = intern(&mut palette, block);
                    indices[crate::voxel::local_index(bevy::math::UVec3::new(
                        local_x as u32,
                        local_y as u32,
                        local_z as u32,
                    ))] = slot;
                }
            }
        }

        Chunk::from_palette(palette, &indices)
    }

    /// The block at a world position, before caves and ores are applied.
    fn column_block(
        &self,
        x: i32,
        y: i32,
        z: i32,
        depth: u32,
        layer: &crate::content::Layer,
        content: &ContentRegistry,
    ) -> ContentId {
        // Unbreakable shell: the bottom of the world and the four outer walls.
        if y <= 0
            || y >= WORLD_HEIGHT - 1
            || x <= 0
            || z <= 0
            || x >= WORLD_HORIZONTAL - 1
            || z >= WORLD_HORIZONTAL - 1
        {
            return ContentId(BEDROCK_ID);
        }

        let fx = x as f64;
        let fy = y as f64;
        let fz = z as f64;

        // --- caves ---------------------------------------------------------------------
        let cave_density = layer.cave_density as f64;
        if cave_density > 0.0 && depth > 2 {
            let tunnel_a = self.cave_a.get([fx * 0.018, fy * 0.028, fz * 0.018]);
            let tunnel_b = self.cave_b.get([fx * 0.018 + 41.0, fy * 0.028, fz * 0.018 - 17.0]);
            // Tunnels: where both fields are near zero we get a tube-like intersection.
            let tunnel = tunnel_a.abs() + tunnel_b.abs() < 0.085 / cave_density.sqrt();
            // Caverns: large blobby voids, rarer.
            let cavern = self.cavern.get([fx * 0.012, fy * 0.02, fz * 0.012]) > 0.62 - 0.12 * cave_density;
            if tunnel || cavern {
                // Caves are flooded only in layers that declare the flooding hazard; the water
                // itself is placed by the pocket pass.
                return ContentId(0);
            }
        }

        // --- surface -------------------------------------------------------------------
        if depth == 0 {
            return self.surface_block(x, z, content);
        }

        // --- ores ----------------------------------------------------------------------
        if let Some(ore) = self.ore_at(x, y, z, layer, content) {
            return ore;
        }

        // --- base block with fillers ---------------------------------------------------
        let blend = self.blend.get([fx * 0.02, fy * 0.02, fz * 0.02]);
        let mut accumulated = 0.0f32;
        for (filler, share) in &layer.fillers {
            accumulated += *share;
            if blend as f32 + 0.5 < accumulated {
                return *filler;
            }
        }
        layer.base_block
    }

    /// Surface blocks: dirt/grass with sand patches near water level.
    fn surface_block(&self, x: i32, z: i32, content: &ContentRegistry) -> ContentId {
        let patch = self.blend.get([x as f64 * 0.05, 3.0, z as f64 * 0.05]);
        if patch > 0.45 {
            content
                .block_by_key("sand")
                .map(|block| block.id)
                .unwrap_or_else(|| {
                    content
                        .layer_at_depth(0)
                        .map(|layer| layer.base_block)
                        .unwrap_or(ContentId(2))
                })
        } else {
            content
                .layer_at_depth(0)
                .map(|layer| layer.base_block)
                .unwrap_or(ContentId(2))
        }
    }

    /// Ore veins: blobs placed on a jittered lattice, sized and spaced per ore.
    ///
    /// The lattice approach makes veins deterministic without scanning the whole layer: for a
    /// given block we look at the lattice cells around it and test whether any blob reaches.
    fn ore_at(
        &self,
        x: i32,
        y: i32,
        z: i32,
        layer: &crate::content::Layer,
        _content: &ContentRegistry,
    ) -> Option<ContentId> {
        for (ore_block, frequency, (min_size, max_size)) in &layer.ores {
            if *frequency <= 0.0 {
                continue;
            }
            // Cell size shrinks as frequency rises: rarer ores have sparser, larger-spaced cells.
            let cell = ((1.0 / frequency.max(0.0001)).cbrt() * 9.0).clamp(12.0, 90.0) as i32;
            let cx = x.div_euclid(cell);
            let cy = y.div_euclid(cell);
            let cz = z.div_euclid(cell);
            // A vein's home cell is the jittered lattice point; check neighbouring cells too so
            // a blob that spans a cell border is still found.
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let cell_pos = (cx + dx, cy + dy, cz + dz);
                        if let Some(ore) = self.vein_at_cell(
                            cell_pos,
                            (cell, *min_size, *max_size, *ore_block),
                            (x, y, z),
                        ) {
                            return Some(ore);
                        }
                    }
                }
            }
        }
        None
    }

    fn vein_at_cell(
        &self,
        cell_pos: (i32, i32, i32),
        params: (i32, u32, u32, ContentId),
        target: (i32, i32, i32),
    ) -> Option<ContentId> {
        let (cell, min_size, max_size, ore) = params;
        let mut rng = ChaCha8Rng::seed_from_u64(
            self.seed
                ^ (cell_pos.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (cell_pos.1 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
                ^ (cell_pos.2 as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
                ^ (ore.0 as u64) << 32,
        );
        // Only some cells host a vein at all.
        if rng.random::<f32>() > 0.55 {
            return None;
        }
        let center = (
            cell_pos.0 * cell + rng.random_range(0..cell),
            cell_pos.1 * cell + rng.random_range(0..cell),
            cell_pos.2 * cell + rng.random_range(0..cell),
        );
        let size = rng.random_range(min_size..=max_size) as f32;
        let radius = (size / 4.19).cbrt().max(1.0);
        let dx = (target.0 - center.0) as f32;
        let dy = (target.1 - center.1) as f32;
        let dz = (target.2 - center.2) as f32;
        if dx * dx + dy * dy + dz * dz <= radius * radius {
            Some(ore)
        } else {
            None
        }
    }

    /// Whether a layer declares a hazard.
    pub fn layer_has_flooding(layer: &crate::content::Layer) -> Option<(f32, u32)> {
        layer.hazards.iter().find_map(|hazard| match hazard {
            LayerHazard::Flooding {
                pocket_chance,
                max_volume,
            } => Some((*pocket_chance, *max_volume)),
            _ => None,
        })
    }

    /// Whether a layer declares a gas hazard.
    pub fn layer_has_gas(layer: &crate::content::Layer) -> Option<(f32, f32)> {
        layer.hazards.iter().find_map(|hazard| match hazard {
            LayerHazard::Gas {
                pocket_chance,
                oxygen_drain,
            } => Some((*pocket_chance, *oxygen_drain)),
            _ => None,
        })
    }
}

/// Generates a chunk and inserts it into `chunks`.
pub fn generate_into(
    generator: &WorldGenerator,
    content: &ContentRegistry,
    chunks: &mut LoadedChunks,
    pos: ChunkPos,
) {
    let chunk = generator.generate_chunk(pos, content);
    chunks.insert(pos, chunk);
}

/// A comfortable spawn point on the surface at the centre of the map.
pub fn spawn_position(content: &ContentRegistry) -> BlockPos {
    let centre = WORLD_HORIZONTAL / 2;
    let mut y = SURFACE_Y;
    while y < WORLD_HEIGHT - 2 {
        let depth = (SURFACE_Y - y).max(0) as u32;
        if content.layer_at_depth(depth).is_some() {
            break;
        }
        y += 1;
    }
    BlockPos::new(centre, SURFACE_Y + 1, centre)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::pos::CHUNK_VOLUME;

    fn test_content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    #[test]
    fn generation_is_deterministic() {
        let content = test_content();
        let generator = WorldGenerator::new(42);
        let a = generator.generate_chunk(ChunkPos::new(5, 3, 7), &content);
        let b = WorldGenerator::new(42).generate_chunk(ChunkPos::new(5, 3, 7), &content);
        assert_eq!(a.palette(), b.palette());
        assert_eq!(a.packed(), b.packed());
        assert_eq!(a.solid_count(), b.solid_count());
    }

    #[test]
    fn different_seeds_differ() {
        let content = test_content();
        let a = WorldGenerator::new(1).generate_chunk(ChunkPos::new(5, 3, 7), &content);
        let b = WorldGenerator::new(2).generate_chunk(ChunkPos::new(5, 3, 7), &content);
        assert_ne!(a.packed(), b.packed());
    }

    #[test]
    fn chunk_is_not_uniformly_empty_or_full() {
        let content = test_content();
        let generator = WorldGenerator::new(9);
        // A chunk in the middle of layer 2 should be mostly solid with some caves.
        let chunk = generator.generate_chunk(ChunkPos::new(16, 12, 16), &content);
        let solid = chunk.solid_count();
        assert!(
            solid > CHUNK_VOLUME as u32 / 2,
            "expected mostly solid rock, got {solid}"
        );
        assert!(solid < CHUNK_VOLUME as u32, "expected some caves");
    }

    #[test]
    fn bedrock_shell_is_unbreakable() {
        let content = test_content();
        let generator = WorldGenerator::new(3);
        let chunk = generator.generate_chunk(ChunkPos::new(0, 0, 0), &content);
        assert_eq!(chunk.get(bevy::math::UVec3::new(0, 0, 0)), BEDROCK_ID);
    }
}
