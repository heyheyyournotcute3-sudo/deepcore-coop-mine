//! The server's world: chunk generation workers, the chunk cache, and the edit log.
//!
//! Chunks are generated on `AsyncComputeTaskPool` workers, never on the main thread. The cache
//! keeps chunks in memory around the players; a chunk that is evicted but has been modified is
//! written to the region store by the persistence plugin.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::voxel::{BlockPos, CHUNK_SIZE, Chunk, ChunkPos, LoadedChunks};
use deepcore_shared::worldgen::WorldGenerator;
use tracing::{debug, trace};

use crate::ServerConfigResource;

/// Registers world systems.
pub struct ServerWorldPlugin;

impl Plugin for ServerWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldManager>();
        app.add_systems(Startup, setup_world);
        app.add_systems(
            Update,
            (
                queue_missing_chunks,
                collect_generated_chunks,
                evict_far_chunks,
                debug_world_state,
            )
                .chain()
                .run_if(resource_exists::<ContentRegistry>),
        );
    }
}

/// Where chunk generation requests come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkRequest {
    /// A player needs the chunk to play.
    Player,
    /// The chunk is being generated for a save, a test, or the map.
    System,
}

/// Owns the generator, the in-flight generation tasks and the edit log.
#[derive(Resource)]
pub struct WorldManager {
    /// The deterministic generator.
    pub generator: WorldGenerator,
    /// Chunks that have been modified and must be saved.
    pub modified: HashSet<ChunkPos>,
    /// In-flight generation tasks.
    pub pending: Vec<(ChunkPos, Task<Chunk>)>,
    /// Chunks currently requested, so we do not queue the same chunk twice.
    pub queued: HashSet<ChunkPos>,
    /// Positions that have been edited since the last persistence flush.
    pub dirty: HashSet<ChunkPos>,
    /// Block edits, replayed onto chunks when they are generated.
    pub edits: HashMap<ChunkPos, Vec<(usize, u16)>>,
    /// Chunks to be unloaded from the client, per client, flushed each tick.
    pub unload_queue: VecDeque<(Entity, ChunkPos)>,
}

impl Default for WorldManager {
    fn default() -> Self {
        Self {
            generator: WorldGenerator::new(0),
            modified: HashSet::new(),
            pending: Vec::new(),
            queued: HashSet::new(),
            dirty: HashSet::new(),
            edits: HashMap::new(),
            unload_queue: VecDeque::new(),
        }
    }
}

impl WorldManager {
    /// Records an edit so it survives eviction and reload.
    pub fn record_edit(&mut self, pos: BlockPos, block: u16) {
        let chunk = pos.chunk();
        let local = pos.local();
        let index = deepcore_shared::voxel::local_index(local);
        self.edits.entry(chunk).or_default().push((index, block));
        self.modified.insert(chunk);
        self.dirty.insert(chunk);
    }

    /// Whether a chunk has been modified and needs saving.
    pub fn is_modified(&self, pos: ChunkPos) -> bool {
        self.modified.contains(&pos)
    }
}

fn setup_world(
    mut manager: ResMut<WorldManager>,
    config: Res<ServerConfigResource>,
    content: Res<ContentRegistry>,
) {
    manager.generator = WorldGenerator::new(config.0.seed);
    info!(
        "world seed {} — {} layers, {} blocks deep",
        config.0.seed,
        content.layers.len(),
        content.world_depth()
    );
}

/// Queues generation for every chunk within the interest radius of a loaded player.
///
/// The interest radius matches the client's view distance, so the server never sends a chunk
/// the client would not mesh.
fn queue_missing_chunks(
    mut manager: ResMut<WorldManager>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
    players: Query<&deepcore_shared::protocol::NetworkPosition>,
) {
    if players.is_empty() {
        return;
    }
    let mut wanted: HashSet<ChunkPos> = HashSet::new();
    for position in &players {
        let centre = BlockPos::new(
            position.position.x.floor() as i32,
            position.position.y.floor() as i32,
            position.position.z.floor() as i32,
        )
        .chunk();
        for dy in -INTEREST_VERTICAL..=INTEREST_VERTICAL {
            for dz in -INTEREST_HORIZONTAL..=INTEREST_HORIZONTAL {
                for dx in -INTEREST_HORIZONTAL..=INTEREST_HORIZONTAL {
                    let pos = centre.offset(dx, dy, dz);
                    if pos.in_bounds() {
                        wanted.insert(pos);
                    }
                }
            }
        }
    }
    let pool = AsyncComputeTaskPool::get();
    for pos in wanted {
        if chunks.contains(pos) || manager.queued.contains(&pos) {
            continue;
        }
        let generator = WorldGenerator::new(manager.generator.seed);
        let content = content.clone();
        let edits = manager.edits.get(&pos).cloned().unwrap_or_default();
        manager.queued.insert(pos);
        let task = pool.spawn(async move {
            let mut chunk = generator.generate_chunk(pos, &content);
            for (index, block) in edits {
                let local = deepcore_shared::voxel::index_to_local(index);
                chunk.set(local, block);
            }
            chunk
        });
        manager.pending.push((pos, task));
    }
}

/// Collects finished generation tasks into the chunk store.
fn collect_generated_chunks(mut manager: ResMut<WorldManager>, mut chunks: ResMut<LoadedChunks>) {
    if manager.pending.is_empty() {
        return;
    }
    let mut still_pending = Vec::with_capacity(manager.pending.len());
    let mut finished = Vec::new();
    for (pos, mut task) in manager.pending.drain(..) {
        match block_on(poll_once(&mut task)) {
            Some(chunk) => finished.push((pos, chunk)),
            None => still_pending.push((pos, task)),
        }
    }
    manager.pending = still_pending;
    for (pos, chunk) in finished {
        manager.queued.remove(&pos);
        trace!("generated chunk {pos:?}");
        chunks.insert(pos, chunk);
    }
}

/// Drops chunks nobody is near any more, so memory stays bounded on long sessions.
fn evict_far_chunks(
    manager: Res<WorldManager>,
    mut chunks: ResMut<LoadedChunks>,
    players: Query<&deepcore_shared::protocol::NetworkPosition>,
) {
    if players.is_empty() || chunks.len() < 4096 {
        return;
    }
    let centres: Vec<ChunkPos> = players
        .iter()
        .map(|position| {
            BlockPos::new(
                position.position.x.floor() as i32,
                position.position.y.floor() as i32,
                position.position.z.floor() as i32,
            )
            .chunk()
        })
        .collect();
    let far: Vec<ChunkPos> = chunks
        .positions()
        .filter(|pos| {
            centres
                .iter()
                .all(|centre| centre.distance(*pos) > INTEREST_HORIZONTAL + 2)
        })
        .collect();
    for pos in far {
        if manager.dirty.contains(&pos) {
            // The persistence plugin will save it before it leaves memory.
            continue;
        }
        chunks.remove(pos);
    }
}

/// Periodic visibility into streaming, logged at debug level once a second.
fn debug_world_state(
    manager: Res<WorldManager>,
    chunks: Res<LoadedChunks>,
    players: Query<&deepcore_shared::protocol::NetworkPosition>,
    time: Res<Time>,
    mut last: Local<f32>,
) {
    *last += time.delta_secs();
    if *last < 1.0 {
        return;
    }
    *last = 0.0;
    debug!(
        "world: {} players, {} chunks loaded, {} generating, {} queued",
        players.iter().count(),
        chunks.len(),
        manager.pending.len(),
        manager.queued.len()
    );
}

/// Horizontal interest radius, in chunks. The client's view radius is the same.
pub const INTEREST_HORIZONTAL: i32 = 6;

/// Vertical interest radius, in chunks.
pub const INTEREST_VERTICAL: i32 = 4;

/// Blocks across a chunk, re-exported for readability in this module.
pub const CHUNK_BLOCKS: i32 = CHUNK_SIZE as i32;
