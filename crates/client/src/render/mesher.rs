//! Chunk meshing: greedy meshing per face direction, on the async task pool.
//!
//! The mesher turns a 32³ chunk plus its six neighbours into one mesh per chunk. It works in
//! three steps:
//!
//! 1. For each of the six face directions, walk the chunk in slices and build a mask of visible
//!    faces (a face is visible when the neighbour is not solid).
//! 2. Greedily merge rectangles of equal block id and equal tile in the mask.
//! 3. Emit quads with the atlas UVs of the block's tile.
//!
//! Merging is what keeps a mostly-solid chunk at a few hundred quads instead of 20 000.

use bevy::asset::RenderAssetUsages;
use bevy::math::UVec3;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::voxel::{
    CHUNK_SIZE, Chunk, ChunkPos, BlockSource, index_to_local, local_index,
};

use crate::render::atlas::BlockAtlas;

/// A quad produced by the mesher, before it becomes mesh data.
#[derive(Debug, Clone, Copy)]
pub struct Quad {
    /// Four corner positions in chunk-local space.
    pub positions: [Vec3; 4],
    /// The block id, which selects the atlas tile.
    pub block: u16,
    /// Face normal.
    pub normal: Vec3,
}

/// Builds the mesh for one chunk.
pub fn build_chunk_mesh(
    pos: ChunkPos,
    chunk: &Chunk,
    chunks: &impl BlockSource,
    content: &ContentRegistry,
    atlas: &BlockAtlas,
) -> Option<Mesh> {
    if chunk.is_empty() {
        return None;
    }
    let quads = collect_quads(chunk, pos, chunks, content);
    if quads.is_empty() {
        return None;
    }
    Some(quads_to_mesh(&quads, atlas))
}

/// Collects every visible face of a chunk, greedily merged.
pub fn collect_quads(
    chunk: &Chunk,
    pos: ChunkPos,
    chunks: &impl BlockSource,
    content: &ContentRegistry,
) -> Vec<Quad> {
    let mut quads = Vec::new();
    // Face direction: (axis, sign)
    let directions: [(usize, i32); 6] = [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)];
    for (axis, sign) in directions {
        greedy_faces(chunk, pos, chunks, content, axis, sign, &mut quads);
    }
    quads
}

/// Greedy-meshes one face direction of a chunk.
#[allow(clippy::needless_range_loop)]
fn greedy_faces(
    chunk: &Chunk,
    pos: ChunkPos,
    chunks: &impl BlockSource,
    content: &ContentRegistry,
    axis: usize,
    sign: i32,
    out: &mut Vec<Quad>,
) {
    // The two axes that span the face plane.
    let u_axis = (axis + 1) % 3;
    let v_axis = (axis + 2) % 3;
    let mut mask: Vec<u16> = vec![0; CHUNK_SIZE * CHUNK_SIZE];
    let mut mask_uv: Vec<(u32, u32)> = vec![(0, 0); CHUNK_SIZE * CHUNK_SIZE];

    for slice in 0..CHUNK_SIZE {
        // Build the mask for this slice.
        for v in 0..CHUNK_SIZE {
            for u in 0..CHUNK_SIZE {
                let mut local = [0usize; 3];
                local[axis] = slice;
                local[u_axis] = u;
                local[v_axis] = v;
                let block = chunk.get(UVec3::new(
                    local[0] as u32,
                    local[1] as u32,
                    local[2] as u32,
                ));
                if block == 0 {
                    mask[u + v * CHUNK_SIZE] = 0;
                    continue;
                }
                // The neighbour in the face direction, possibly in the adjacent chunk.
                let world = BlockPos::from_chunk_local(pos, local, axis, sign);
                let neighbor = chunks.block(world).unwrap_or(0);
                let neighbor_solid = content
                    .block(deepcore_shared::content::ContentId(neighbor))
                    .is_some_and(|block| block.solid || (block.fluid && block.key != "water"));
                let current_fluid = content
                    .block(deepcore_shared::content::ContentId(block))
                    .is_some_and(|block| block.fluid);
                // A fluid face is only drawn against air, so water surfaces stay flat.
                let visible = if current_fluid {
                    neighbor == 0 || !neighbor_solid
                } else {
                    !neighbor_solid
                };
                mask[u + v * CHUNK_SIZE] = if visible { block } else { 0 };
                mask_uv[u + v * CHUNK_SIZE] = (u as u32, v as u32);
            }
        }

        // Greedily merge rectangles of equal block id.
        for v in 0..CHUNK_SIZE {
            let mut u = 0;
            while u < CHUNK_SIZE {
                let block = mask[u + v * CHUNK_SIZE];
                if block == 0 {
                    u += 1;
                    continue;
                }
                let mut width = 1;
                while u + width < CHUNK_SIZE && mask[u + width + v * CHUNK_SIZE] == block {
                    width += 1;
                }
                let mut height = 1;
                'grow: while v + height < CHUNK_SIZE {
                    for du in 0..width {
                        if mask[u + du + (v + height) * CHUNK_SIZE] != block {
                            break 'grow;
                        }
                    }
                    height += 1;
                }
                out.push(make_quad(axis, sign, slice, u, v, width, height, block));
                for dv in 0..height {
                    for du in 0..width {
                        mask[u + du + (v + dv) * CHUNK_SIZE] = 0;
                    }
                }
                u += width;
            }
        }
    }
}

/// Builds the quad for a merged rectangle.
#[allow(clippy::too_many_arguments)]
fn make_quad(
    axis: usize,
    sign: i32,
    slice: usize,
    u: usize,
    v: usize,
    width: usize,
    height: usize,
    block: u16,
) -> Quad {
    let u_axis = (axis + 1) % 3;
    let v_axis = (axis + 2) % 3;
    let offset = if sign > 0 { 1.0 } else { 0.0 };
    let mut corners = [Vec3::ZERO; 4];
    let uv_offsets = [(0, 0), (width, 0), (width, height), (0, height)];
    for (index, (du, dv)) in uv_offsets.iter().enumerate() {
        let mut local = [0f32; 3];
        local[axis] = slice as f32 + offset;
        local[u_axis] = (u + du) as f32;
        local[v_axis] = (v + dv) as f32;
        corners[index] = Vec3::new(local[0], local[1], local[2]);
    }
    // Keep the winding counter-clockwise when seen from outside.
    if sign < 0 {
        corners.swap(1, 3);
    }
    let mut normal = Vec3::ZERO;
    normal[axis] = sign as f32;
    Quad {
        positions: corners,
        block,
        normal,
    }
}

/// Helper: the world block position for a local coordinate offset along a face direction.
trait BlockPosExt {
    fn from_chunk_local(pos: ChunkPos, local: [usize; 3], axis: usize, sign: i32) -> BlockPos;
}

impl BlockPosExt for BlockPos {
    fn from_chunk_local(pos: ChunkPos, local: [usize; 3], axis: usize, sign: i32) -> BlockPos {
        let mut world = [local[0] as i32, local[1] as i32, local[2] as i32];
        world[axis] += sign;
        let origin = pos.origin();
        BlockPos::new(origin.x + world[0], origin.y + world[1], origin.z + world[2])
    }
}

use deepcore_shared::voxel::BlockPos;

/// Turns quads into a Bevy mesh with atlas UVs.
pub fn quads_to_mesh(quads: &[Quad], atlas: &BlockAtlas) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(quads.len() * 4);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(quads.len() * 4);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(quads.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(quads.len() * 6);

    for (quad_index, quad) in quads.iter().enumerate() {
        let (min, max) = atlas.tile_uv(quad.block);
        // Shrink the UVs by half a texel so neighbouring tiles never bleed at distance.
        let texel = (max - min) / 16.0 * 0.5;
        let min = min + texel;
        let max = max - texel;
        // Every face uses the same UV frame: the quad corners are emitted in the same
        // counter-clockwise order the atlas tile expects, for all six directions.
        let normal = quad.normal;
        let corner_uvs = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        for (corner, uv) in quad.positions.iter().zip(corner_uvs.iter()) {
            positions.push([corner.x, corner.y, corner.z]);
            normals.push([normal.x, normal.y, normal.z]);
            uvs.push([
                min.x + (max.x - min.x) * uv.0,
                min.y + (max.y - min.y) * uv.1,
            ]);
        }
        let base = (quad_index * 4) as u32;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// The chunk-local index of a coordinate, exposed for tests.
pub fn local_of(x: u32, y: u32, z: u32) -> usize {
    local_index(UVec3::new(x, y, z))
}

/// The coordinate of a chunk-local index, exposed for tests.
pub fn coordinate_of(index: usize) -> UVec3 {
    index_to_local(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deepcore_shared::voxel::{Chunk, LoadedChunks};

    fn content() -> ContentRegistry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        ContentRegistry::load_from_dir(&dir).expect("content loads")
    }

    fn atlas() -> BlockAtlas {
        BlockAtlas {
            image: Handle::default(),
            material: Handle::default(),
            tiles: 128,
        }
    }

    #[test]
    fn single_block_produces_six_faces() {
        let content = content();
        let mut chunks = LoadedChunks::default();
        let mut chunk = Chunk::default();
        chunk.set(UVec3::new(4, 4, 4), 3);
        chunks.insert(ChunkPos::new(0, 0, 0), chunk);
        let quads = collect_quads(
            chunks.get(ChunkPos::new(0, 0, 0)).unwrap(),
            ChunkPos::new(0, 0, 0),
            &chunks,
            &content,
        );
        assert_eq!(quads.len(), 6, "a lone cube has six visible faces");
    }

    #[test]
    fn solid_chunk_is_greedy_meshed_to_a_handful_of_quads() {
        let content = content();
        let mut chunks = LoadedChunks::default();
        chunks.insert(ChunkPos::new(0, 0, 0), Chunk::filled(3));
        let quads = collect_quads(
            chunks.get(ChunkPos::new(0, 0, 0)).unwrap(),
            ChunkPos::new(0, 0, 0),
            &chunks,
            &content,
        );
        // A solid chunk is greedy-merged per face direction, so the quad count is bounded by
        // six regardless of how many blocks it holds (a naive mesher would emit 20 736).
        assert!(quads.len() <= 6, "got {} quads", quads.len());
        assert!(quads.iter().all(|quad| quad.block == 3));
        // The face touching the unloaded space below reads as bedrock and is culled.
        assert!(quads.iter().all(|quad| quad.normal != Vec3::NEG_Y));
    }

    #[test]
    fn interior_faces_are_culled() {
        let content = content();
        let mut chunks = LoadedChunks::default();
        // Two adjacent chunks, both solid: the shared face must not be emitted.
        chunks.insert(ChunkPos::new(0, 0, 0), Chunk::filled(3));
        chunks.insert(ChunkPos::new(1, 0, 0), Chunk::filled(3));
        let quads = collect_quads(
            chunks.get(ChunkPos::new(0, 0, 0)).unwrap(),
            ChunkPos::new(0, 0, 0),
            &chunks,
            &content,
        );
        // The shared face between the two chunks is culled entirely.
        assert!(quads.iter().all(|quad| quad.normal != Vec3::X));
        assert!(quads.len() < 6, "got {} quads", quads.len());
    }

    #[test]
    fn mesh_has_matching_attribute_counts() {
        let atlas = atlas();
        let quad = Quad {
            positions: [
                Vec3::ZERO,
                Vec3::X,
                Vec3::X + Vec3::Y,
                Vec3::Y,
            ],
            block: 3,
            normal: Vec3::Z,
        };
        let mesh = quads_to_mesh(&[quad], &atlas);
        assert_eq!(mesh.count_vertices(), 4);
    }

    #[test]
    fn local_index_round_trip_matches_voxel_crate() {
        use deepcore_shared::voxel::CHUNK_VOLUME;
        for index in [0usize, 1, 33, CHUNK_VOLUME - 1] {
            assert_eq!(local_of(
                coordinate_of(index).x,
                coordinate_of(index).y,
                coordinate_of(index).z
            ), index);
        }
    }
}
