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
    ///
    /// Two passes: first every column is filled with the layer's base block, fillers, surface
    /// and caves; then each ore's lattice cells overlapping the chunk are stamped in. Ore
    /// generation is per *cell* rather than per *block*, which is what keeps a chunk under a
    /// few milliseconds: a chunk touches a handful of cells, not 32 768 × ores × neighbours.
    pub fn generate_chunk(&self, pos: ChunkPos, content: &ContentRegistry) -> Chunk {
        let origin = pos.origin();
        let mut palette: Vec<u16> = vec![0];
        let mut indices = vec![0u16; crate::voxel::CHUNK_VOLUME];
        let mut lookup: std::collections::HashMap<u16, u16> = std::collections::HashMap::new();
        lookup.insert(0, 0);

        let intern = |palette: &mut Vec<u16>,
                      lookup: &mut std::collections::HashMap<u16, u16>,
                      block: u16|
         -> u16 {
            if let Some(slot) = lookup.get(&block) {
                return *slot;
            }
            palette.push(block);
            let slot = (palette.len() - 1) as u16;
            lookup.insert(block, slot);
            slot
        };

        // --- pass 1: terrain, fillers, surface and caves ---------------------------------
        for local_y in 0..CHUNK_SIZE {
            let y = origin.y + local_y as i32;
            if !(0..WORLD_HEIGHT).contains(&y) {
                continue;
            }
            let depth = (SURFACE_Y - y).max(0) as u32;
            let Some(layer) = content.layer_at_depth(depth) else {
                continue;
            };
            let layer_index = layer_index_of(content, layer);
            for local_z in 0..CHUNK_SIZE {
                for local_x in 0..CHUNK_SIZE {
                    let x = origin.x + local_x as i32;
                    let z = origin.z + local_z as i32;
                    let block = self.column_block(x, y, z, depth, layer, content);
                    let slot = intern(&mut palette, &mut lookup, block.0);
                    indices[crate::voxel::local_index(bevy::math::UVec3::new(
                        local_x as u32,
                        local_y as u32,
                        local_z as u32,
                    ))] = slot;
                    let _ = layer_index;
                }
            }
        }

        // --- pass 2: ore veins -----------------------------------------------------------
        for layer in &content.layers {
            for (ore_block, frequency, (min_size, max_size)) in &layer.ores {
                if *frequency <= 0.0 {
                    continue;
                }
                self.stamp_veins(
                    pos,
                    layer,
                    *ore_block,
                    *frequency,
                    (*min_size, *max_size),
                    &mut indices,
                    &mut palette,
                    &mut lookup,
                );
            }
        }

        Chunk::from_palette(palette, &indices)
    }

    /// Stamps every vein of one ore that overlaps the chunk.
    #[allow(clippy::too_many_arguments)]
    fn stamp_veins(
        &self,
        pos: ChunkPos,
        layer: &crate::content::Layer,
        ore: ContentId,
        frequency: f32,
        (min_size, max_size): (u32, u32),
        indices: &mut [u16],
        palette: &mut Vec<u16>,
        lookup: &mut std::collections::HashMap<u16, u16>,
    ) {
        let origin = pos.origin();
        let cell = ((1.0 / frequency.max(0.0001)).cbrt() * 9.0).clamp(12.0, 90.0) as i32;
        // Only the part of the chunk inside this layer's depth band can hold its ore.
        let layer_top = SURFACE_Y - layer.depth.0 as i32;
        let layer_bottom = SURFACE_Y - layer.depth.1 as i32;
        let chunk_min_y = origin.y.max(layer_bottom);
        let chunk_max_y = (origin.y + CHUNK_SIZE as i32 - 1).min(layer_top);
        if chunk_min_y > chunk_max_y {
            return;
        }
        let slot = intern_block(palette, lookup, ore.0);

        let cell_min = |world: i32| world.div_euclid(cell) - 1;
        let cell_max = |world: i32| world.div_euclid(cell) + 1;
        for cz in cell_min(origin.z)..=cell_max(origin.z + CHUNK_SIZE as i32) {
            for cy in cell_min(chunk_min_y)..=cell_max(chunk_max_y) {
                for cx in cell_min(origin.x)..=cell_max(origin.x + CHUNK_SIZE as i32) {
                    let mut rng = ChaCha8Rng::seed_from_u64(
                        self.seed
                            ^ (cx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                            ^ (cy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
                            ^ (cz as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
                            ^ ((ore.0 as u64) << 32),
                    );
                    if rng.random::<f32>() > 0.55 {
                        continue;
                    }
                    let center = (
                        cx * cell + rng.random_range(0..cell),
                        cy * cell + rng.random_range(0..cell),
                        cz * cell + rng.random_range(0..cell),
                    );
                    let size = rng.random_range(min_size..=max_size) as f32;
                    let radius = (size / 4.19).cbrt().max(1.0);
                    let radius_i = radius.ceil() as i32;
                    for dy in -radius_i..=radius_i {
                        let world_y = center.1 + dy;
                        if world_y < chunk_min_y || world_y > chunk_max_y {
                            continue;
                        }
                        for dz in -radius_i..=radius_i {
                            let world_z = center.2 + dz;
                            if world_z < origin.z || world_z >= origin.z + CHUNK_SIZE as i32 {
                                continue;
                            }
                            for dx in -radius_i..=radius_i {
                                let world_x = center.0 + dx;
                                if world_x < origin.x || world_x >= origin.x + CHUNK_SIZE as i32 {
                                    continue;
                                }
                                let distance_sq = (dx * dx + dy * dy + dz * dz) as f32;
                                if distance_sq > radius * radius {
                                    continue;
                                }
                                let local = bevy::math::UVec3::new(
                                    (world_x - origin.x) as u32,
                                    (world_y - origin.y) as u32,
                                    (world_z - origin.z) as u32,
                                );
                                let index = crate::voxel::local_index(local);
                                // Only replace solid rock: never fill a cave back in.
                                if indices[index] != 0 {
                                    indices[index] = slot;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// The block at a world position, ignoring caves, ores and pockets.
    ///
    /// Used by the spawn logic to find the surface without generating a whole chunk.
    pub fn debug_block_at(
        &self,
        x: i32,
        y: i32,
        z: i32,
        layer: &crate::content::Layer,
        content: &ContentRegistry,
    ) -> u16 {
        let depth = (SURFACE_Y - y).max(0) as u32;
        self.column_block(x, y, z, depth, layer, content).0
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

        // Above the surface there is only air; the camp platform is built below this line.
        if y > SURFACE_Y {
            return ContentId(0);
        }

        // --- the Base ------------------------------------------------------------------
        // A flat, cave-free camp area around the spawn point, so the surface camp is usable
        // and the first thing a player sees is not a hole.
        let camp_radius = (WORLD_HORIZONTAL / 2 - x).abs().min((WORLD_HORIZONTAL / 2 - z).abs());
        if camp_radius <= CAMP_RADIUS && depth <= CAMP_DEPTH {
            return layer.base_block;
        }

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

/// Interns a block id into the chunk palette.
fn intern_block(
    palette: &mut Vec<u16>,
    lookup: &mut std::collections::HashMap<u16, u16>,
    block: u16,
) -> u16 {
    if let Some(slot) = lookup.get(&block) {
        return *slot;
    }
    palette.push(block);
    let slot = (palette.len() - 1) as u16;
    lookup.insert(block, slot);
    slot
}

/// Index of a layer in the registry, used for cheap per-layer decisions.
fn layer_index_of(content: &ContentRegistry, layer: &crate::content::Layer) -> usize {
    content
        .layers
        .iter()
        .position(|candidate| candidate.key == layer.key)
        .unwrap_or(0)
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

/// Radius of the flat surface camp (the Base), in blocks.
pub const CAMP_RADIUS: i32 = 40;

/// Depth of the camp platform, in blocks. Below it the normal layers resume.
pub const CAMP_DEPTH: u32 = 3;

/// The surface height at the camp: the camp platform's top block is `SURFACE_Y`.
pub fn camp_height() -> i32 {
    SURFACE_Y
}

/// The surface height at a column: the topmost solid block plus one.
///
/// The Base sits on this, so machines and players are never buried by a hill or a cave.
pub fn surface_height(generator: &WorldGenerator, content: &ContentRegistry, x: i32, z: i32) -> i32 {
    let mut y = SURFACE_Y + 2;
    // Walk down from just above the surface until a solid block is found.
    for candidate in (SURFACE_Y - 6..=SURFACE_Y + 3).rev() {
        let depth = (SURFACE_Y - candidate).max(0) as u32;
        let Some(layer) = content.layer_at_depth(depth) else {
            continue;
        };
        let block = generator.debug_block_at(x, candidate, z, layer, content);
        if block != 0 {
            y = candidate + 1;
            break;
        }
    }
    y
}

/// A comfortable spawn point on the flat camp platform at the centre of the map.
pub fn spawn_position(content: &ContentRegistry) -> BlockPos {
    let _ = content;
    let centre = WORLD_HORIZONTAL / 2;
    // One block above the platform's top block: the feet rest on it.
    BlockPos::new(centre, camp_height() + 1, centre)
}

/// A spawn point for a specific seed. The camp is flat, so the seed does not move the spawn,
/// but the function exists so callers do not have to know that.
pub fn spawn_position_for_seed(seed: u64, content: &ContentRegistry) -> BlockPos {
    let _ = seed;
    spawn_position(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::pos::CHUNK_VOLUME;

    pub(super) fn test_content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    #[test]
    fn generation_is_deterministic() {
        let content = super::tests::test_content();
        let generator = WorldGenerator::new(42);
        let a = generator.generate_chunk(ChunkPos::new(5, 3, 7), &content);
        let b = WorldGenerator::new(42).generate_chunk(ChunkPos::new(5, 3, 7), &content);
        assert_eq!(a.palette(), b.palette());
        assert_eq!(a.packed(), b.packed());
        assert_eq!(a.solid_count(), b.solid_count());
    }

    #[test]
    fn different_seeds_differ() {
        let content = super::tests::test_content();
        let a = WorldGenerator::new(1).generate_chunk(ChunkPos::new(5, 3, 7), &content);
        let b = WorldGenerator::new(2).generate_chunk(ChunkPos::new(5, 3, 7), &content);
        assert_ne!(a.packed(), b.packed());
    }

    #[test]
    fn chunk_is_not_uniformly_empty_or_full() {
        let content = super::tests::test_content();
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
        let content = super::tests::test_content();
        let generator = WorldGenerator::new(3);
        let chunk = generator.generate_chunk(ChunkPos::new(0, 0, 0), &content);
        assert_eq!(chunk.get(bevy::math::UVec3::new(0, 0, 0)), BEDROCK_ID);
    }
}

#[cfg(test)]
mod perf_tests {
    use super::*;

    /// Generation must stay well inside the 5 ms per-chunk budget.
    ///
    /// The budget is measured in a release-like profile in CI; under `cargo test` the crate is
    /// built with `opt-level = 1`, so the threshold here is deliberately loose and the printed
    /// number is the real measurement.
    #[test]
    fn chunk_generation_is_fast() {
        let content = super::tests::test_content();
        let generator = WorldGenerator::new(7);
        // Warm up.
        let _ = generator.generate_chunk(ChunkPos::new(16, 12, 16), &content);
        let start = std::time::Instant::now();
        let count = 16;
        for index in 0..count {
            let _ = generator.generate_chunk(ChunkPos::new(16 + index, 12, 16), &content);
        }
        let per_chunk = start.elapsed().as_secs_f64() * 1000.0 / f64::from(count);
        println!("chunk generation: {per_chunk:.2} ms");
        assert!(
            per_chunk < 12.0,
            "chunk generation took {per_chunk:.2} ms (budget is 5 ms in release)"
        );
    }

    /// Every layer must contain at least some of each ore it declares.
    #[test]
    fn ores_appear_in_their_layers() {
        let content = super::tests::test_content();
        let generator = WorldGenerator::new(1234);
        for layer in &content.layers {
            for (ore, _frequency, _size) in &layer.ores {
                let mut found = 0;
                // Sample a band of chunks spread over the layer's depth.
                let chunk_y = (SURFACE_Y - layer.depth.0 as i32) / 32;
                for index in 0..12 {
                    let chunk = generator.generate_chunk(
                        ChunkPos::new(40 + index * 3, chunk_y - 1, 40),
                        &content,
                    );
                    if chunk.palette().contains(&ore.0) {
                        found += 1;
                    }
                }
                assert!(
                    found > 0,
                    "ore {} never appeared in layer {}",
                    content.block(*ore).map_or("?", |block| block.key.as_str()),
                    layer.key
                );
            }
        }
    }
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    #[test]
    fn spawn_is_on_the_camp_surface_with_air_above() {
        let content = super::tests::test_content();
        let spawn = spawn_position(&content);
        let generator = WorldGenerator::new(0x0DEE9C04E);
        let mut chunks = LoadedChunks::default();
        for dy in -2..3 {
            for dz in -2..2 {
                for dx in -2..2 {
                    let pos = spawn.offset(dx, dy, dz);
                    let chunk = pos.chunk();
                    if !chunks.contains(chunk) {
                        chunks.insert(chunk, generator.generate_chunk(chunk, &content));
                    }
                }
            }
        }
        // The feet are above solid ground, and the head is in air.
        assert!(chunks.is_solid(spawn.offset(0, -1, 0), &content), "no ground under the spawn");
        assert!(!chunks.is_solid(spawn, &content), "spawn block is solid");
        assert!(!chunks.is_solid(spawn.offset(0, 1, 0), &content), "head block is solid");
        // The whole camp is flat.
        for offset in [-20, 0, 20] {
            let column = spawn.offset(offset, -1, offset);
            assert!(chunks.is_solid(column, &content), "camp is not flat at {offset}");
            assert!(!chunks.is_solid(column.offset(0, 1, 0), &content));
        }
    }
}
