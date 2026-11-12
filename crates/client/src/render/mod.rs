//! Chunk rendering: material setup, async meshing, and entity lifecycle.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::voxel::{ChunkPos, LoadedChunks};

pub mod atlas;
pub mod ghost;
pub mod mesher;

use crate::render::atlas::{BlockAtlas, build_atlas_image};
use crate::render::mesher::build_chunk_mesh;

/// Registers the render systems.
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkMeshes>()
            .init_resource::<ChunkRenderSettings>()
            .add_systems(Startup, setup_atlas)
            .add_systems(
                Update,
                (
                    queue_chunk_meshes,
                    collect_chunk_meshes,
                    rebuild_dirty_chunks,
                )
                    .chain(),
            )
            .add_plugins(ghost::GhostPlugin);
    }
}

/// How far the client meshes, in chunks. Matches the server's interest radius.
#[derive(Debug, Resource, Clone, Copy)]
pub struct ChunkRenderSettings {
    /// Horizontal radius.
    pub horizontal: i32,
    /// Vertical radius.
    pub vertical: i32,
    /// Rebuild the mesh when the chunk revision changes.
    pub track_revisions: bool,
}

impl Default for ChunkRenderSettings {
    fn default() -> Self {
        Self {
            horizontal: 6,
            vertical: 4,
            track_revisions: true,
        }
    }
}

/// The meshes spawned for each chunk.
#[derive(Debug, Default, Resource)]
pub struct ChunkMeshes {
    /// `chunk → (entity, revision)`.
    pub meshes: HashMap<ChunkPos, (Entity, u32)>,
    /// In-flight mesh tasks.
    pub pending: Vec<(ChunkPos, u32, Task<Option<Mesh>>)>,
    /// Chunks being meshed right now.
    pub in_flight: std::collections::HashSet<ChunkPos>,
}

/// A mesh entity that follows its chunk position.
#[derive(Debug, Component)]
pub struct ChunkMeshEntity {
    /// The chunk this mesh belongs to.
    pub chunk: ChunkPos,
}

fn setup_atlas(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    content: Res<ContentRegistry>,
) {
    let image = images.add(build_atlas_image(&content));
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(image.clone()),
        perceptual_roughness: 0.92,
        metallic: 0.0,
        ..default()
    });
    info!("procedural atlas built for {} blocks", content.blocks.len());
    commands.insert_resource(BlockAtlas {
        image,
        material,
        tiles: content.blocks.len() as u32,
    });
}

/// Queues meshing for chunks that are loaded but have no mesh (or a stale one).
fn queue_chunk_meshes(
    mut meshes: ResMut<ChunkMeshes>,
    chunks: Res<LoadedChunks>,
    settings: Res<ChunkRenderSettings>,
    content: Res<ContentRegistry>,
    atlas: Option<Res<BlockAtlas>>,
    camera: Query<&Transform, With<Camera3d>>,
) {
    let Some(atlas) = atlas else {
        return;
    };
    let centre = camera
        .iter()
        .next()
        .map(|transform| deepcore_shared::voxel::BlockPos::from_vec3(transform.translation).chunk())
        .unwrap_or(ChunkPos::new(0, 0, 0));

    let mut candidates: Vec<(ChunkPos, u32)> = Vec::new();
    for pos in chunks.positions() {
        if (pos.x - centre.x).abs() > settings.horizontal
            || (pos.z - centre.z).abs() > settings.horizontal
            || (pos.y - centre.y).abs() > settings.vertical
        {
            continue;
        }
        let revision = chunks.get(pos).map_or(0, |chunk| chunk.revision());
        match meshes.meshes.get(&pos) {
            Some((_, existing)) if !settings.track_revisions || *existing == revision => continue,
            _ => {}
        }
        if meshes.in_flight.contains(&pos) {
            continue;
        }
        candidates.push((pos, revision));
    }
    candidates.sort_by_key(|(pos, _)| pos.distance(centre));
    candidates.truncate(6);

    let pool = AsyncComputeTaskPool::get();
    for (pos, revision) in candidates {
        let chunks_snapshot = clone_for_meshing(&chunks);
        let content = content.clone();
        let atlas = atlas.clone();
        let Some(chunk) = chunks.get(pos).cloned() else {
            continue;
        };
        meshes.in_flight.insert(pos);
        let task =
            pool.spawn(
                async move { build_chunk_mesh(pos, &chunk, &chunks_snapshot, &content, &atlas) },
            );
        meshes.pending.push((pos, revision, task));
    }
}

/// Collects finished mesh tasks and spawns or updates the mesh entities.
fn collect_chunk_meshes(
    mut commands: Commands,
    mut meshes: ResMut<ChunkMeshes>,
    mut mesh_assets: ResMut<Assets<Mesh>>,
    atlas: Option<Res<BlockAtlas>>,
) {
    if meshes.pending.is_empty() {
        return;
    }
    let Some(atlas) = atlas else {
        return;
    };
    let pending = std::mem::take(&mut meshes.pending);
    let mut still = Vec::with_capacity(pending.len());
    for (pos, revision, mut task) in pending {
        match block_on(poll_once(&mut task)) {
            Some(mesh) => {
                meshes.in_flight.remove(&pos);
                let origin = pos.origin();
                let translation = Vec3::new(origin.x as f32, origin.y as f32, origin.z as f32);
                match mesh {
                    Some(mesh) => {
                        let handle = mesh_assets.add(mesh);
                        match meshes.meshes.get(&pos).map(|(entity, _)| *entity) {
                            Some(entity) => {
                                commands.entity(entity).insert(Mesh3d(handle));
                                meshes.meshes.insert(pos, (entity, revision));
                            }
                            None => {
                                let entity = commands
                                    .spawn((
                                        Mesh3d(handle),
                                        MeshMaterial3d(atlas.material.clone()),
                                        Transform::from_translation(translation),
                                        ChunkMeshEntity { chunk: pos },
                                    ))
                                    .id();
                                meshes.meshes.insert(pos, (entity, revision));
                            }
                        }
                    }
                    None => {
                        if let Some((entity, _)) = meshes.meshes.remove(&pos) {
                            commands.entity(entity).despawn();
                        }
                    }
                }
            }
            None => still.push((pos, revision, task)),
        }
    }
    meshes.pending = still;
}

/// Re-queues chunks whose blocks changed, and despawns meshes for unloaded chunks.
fn rebuild_dirty_chunks(
    mut commands: Commands,
    mut meshes: ResMut<ChunkMeshes>,
    chunks: Res<LoadedChunks>,
) {
    let stale: Vec<ChunkPos> = meshes
        .meshes
        .keys()
        .copied()
        .filter(|pos| !chunks.contains(*pos))
        .collect();
    for pos in stale {
        if let Some((entity, _)) = meshes.meshes.remove(&pos) {
            commands.entity(entity).despawn();
        }
    }
}

/// A cheap snapshot of the chunk store for meshing tasks.
///
/// Meshing needs neighbouring chunks for face culling, so the snapshot copies the chunks in the
/// client's view. Chunks are `Vec<u8>` payloads, so this is a memcpy of a few hundred KiB.
pub struct ChunkSnapshot {
    chunks: HashMap<ChunkPos, deepcore_shared::voxel::Chunk>,
}

impl ChunkSnapshot {
    /// Reads a block from the snapshot.
    pub fn block(&self, pos: deepcore_shared::voxel::BlockPos) -> Option<u16> {
        if pos.y >= deepcore_shared::voxel::WORLD_HEIGHT {
            return Some(0);
        }
        if !pos.in_bounds() {
            return Some(deepcore_shared::voxel::BEDROCK_ID);
        }
        self.chunks
            .get(&pos.chunk())
            .map(|chunk| chunk.get(pos.local()))
    }

    /// Borrows a chunk.
    pub fn get(&self, pos: ChunkPos) -> Option<&deepcore_shared::voxel::Chunk> {
        self.chunks.get(&pos)
    }

    /// Iterates chunk positions.
    pub fn positions(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.chunks.keys().copied()
    }
}

impl deepcore_shared::voxel::BlockSource for ChunkSnapshot {
    fn block(&self, pos: deepcore_shared::voxel::BlockPos) -> Option<u16> {
        ChunkSnapshot::block(self, pos)
    }
}

/// Snapshots the loaded chunks for a meshing task.
pub fn clone_for_meshing(chunks: &LoadedChunks) -> ChunkSnapshot {
    let mut map = HashMap::with_capacity(chunks.len());
    for pos in chunks.positions() {
        if let Some(chunk) = chunks.get(pos) {
            map.insert(pos, chunk.clone());
        }
    }
    ChunkSnapshot { chunks: map }
}
