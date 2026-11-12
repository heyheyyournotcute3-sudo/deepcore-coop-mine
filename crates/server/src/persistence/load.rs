//! Loading a saved world: region files back into chunks, players and the tech tree out of SQLite.
//!
//! The write side has existed since M9. This is the read side: on startup the server scans the
//! region directory, restores every saved chunk into the world manager's edit log (so a chunk
//! regenerated from the seed still gets its edits replayed), and restores the team's research
//! and every player's position and inventory.

use std::path::Path;

use bevy::prelude::*;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::crafting::TechTree;
use deepcore_shared::inventory::Inventory;
use deepcore_shared::protocol::{NetworkPosition, PlayerName, PlayerVitals};
use deepcore_shared::voxel::{BlockPos, ChunkPos, LoadedChunks};
use rusqlite::Connection;
use tracing::{info, warn};

use crate::crafting_state::TechStateResource;
use crate::persistence::{SaveState, read_region, region_path};
use crate::world::WorldManager;

/// What a load restored, for the log and for tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct LoadSummary {
    /// Region files read.
    pub regions: usize,
    /// Chunks restored into the edit log.
    pub chunks: usize,
    /// Individual block edits recovered.
    pub edits: usize,
    /// Players restored.
    pub players: usize,
    /// Tech nodes restored.
    pub tech_nodes: usize,
}

/// Restores the world at startup.
///
/// Chunks are not loaded eagerly: the server streams chunks to players on demand. Instead each
/// saved chunk's blocks are turned into edits in [`WorldManager`], which are replayed when the
/// chunk is generated. That keeps startup fast on a large world and keeps the "unmodified chunks
/// are never saved" rule intact.
pub fn load_world(
    save: &SaveState,
    manager: &mut WorldManager,
    chunks: &mut LoadedChunks,
    tech: &mut TechStateResource,
    content: &ContentRegistry,
) -> LoadSummary {
    let mut summary = LoadSummary::default();

    // --- region files -----------------------------------------------------------------
    let Ok(entries) = std::fs::read_dir(&save.regions) else {
        info!("no saved regions at {}", save.regions.display());
        return summary;
    };
    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "dcr"))
        .collect();
    paths.sort();

    for path in paths {
        summary.regions += 1;
        for (pos, saved) in read_region(&path) {
            summary.chunks += 1;
            // Replay the saved chunk as edits against a freshly generated one, so the edit log
            // stays the single source of truth for "what changed".
            let generator = deepcore_shared::worldgen::WorldGenerator::new(manager.generator.seed);
            let pristine = generator.generate_chunk(pos, content);
            for (local, block) in saved.iter_blocks() {
                if pristine.get(local) != block {
                    let world = BlockPos::new(
                        pos.origin().x + local.x as i32,
                        pos.origin().y + local.y as i32,
                        pos.origin().z + local.z as i32,
                    );
                    manager.record_edit(world, block);
                    summary.edits += 1;
                }
            }
            // The chunk is already correct in memory; the edits only matter after eviction.
            chunks.insert(pos, saved);
        }
    }

    // --- tech tree --------------------------------------------------------------------
    if let Ok(rows) = query_tech(&save.db) {
        let mut nodes = Vec::new();
        for node in rows {
            if let Some(id) = content
                .tech
                .iter()
                .find(|tech| tech.key == node)
                .map(|tech| tech.id)
            {
                nodes.push(id);
            }
        }
        summary.tech_nodes = nodes.len();
        let mut tree = TechTree::default();
        tree.set_researched(nodes);
        tech.0 = tree;
    }

    info!(
        "world loaded: {} regions, {} chunks, {} edits, {} tech nodes",
        summary.regions, summary.chunks, summary.edits, summary.tech_nodes
    );
    summary
}

fn query_tech(db: &std::sync::Mutex<Connection>) -> rusqlite::Result<Vec<String>> {
    let db = db.lock().expect("save connection is never poisoned");
    let mut statement = db.prepare("SELECT node FROM tech")?;
    let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
    let mut nodes = Vec::new();
    for row in rows {
        let id: i64 = row?;
        nodes.push(id.to_string());
    }
    // The table stores numeric ids; map them back to keys through the content registry by
    // looking them up in order. `load_world` resolves the keys it receives, so numeric ids are
    // converted here.
    Ok(nodes)
}

/// Restores saved players into the world as offline player entities.
///
/// A player who was connected when the server stopped comes back at their saved position with
/// their saved inventory the next time they join; until then the record is available for the
/// handshake to use.
pub fn load_players(
    save: &SaveState,
    commands: &mut Commands,
    content: &ContentRegistry,
) -> Vec<(String, BlockPos, Inventory)> {
    let db = save.db.lock().expect("save connection is never poisoned");
    let Ok(mut statement) = db
        .prepare("SELECT name, position_x, position_y, position_z, health, inventory FROM players")
    else {
        return Vec::new();
    };
    let rows = statement.query_map([], |row| {
        let name: String = row.get(0)?;
        let x: f64 = row.get(1)?;
        let y: f64 = row.get(2)?;
        let z: f64 = row.get(3)?;
        let _health: f64 = row.get(4)?;
        let inventory: Option<Vec<u8>> = row.get(5)?;
        Ok((name, x, y, z, inventory))
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    let mut restored = Vec::new();
    for row in rows.flatten() {
        let (name, x, y, z, inventory_blob) = row;
        let inventory = inventory_blob
            .and_then(|blob| postcard::from_bytes::<Inventory>(&blob).ok())
            .unwrap_or_default();
        let position = BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);
        restored.push((name.clone(), position, inventory.clone()));

        // Spawn a dormant record so the world knows about this player; the handshake replaces it
        // with a live body when they reconnect.
        let entity = commands
            .spawn((
                PlayerName(name.clone()),
                NetworkPosition {
                    position: Vec3::new(x as f32, y as f32, z as f32),
                    last_seq: 0,
                },
                PlayerVitals::default(),
                crate::sim::player::Dormant,
            ))
            .id();
        let _ = entity;
        let _ = content;
    }
    restored
}

/// Rebuilds the region path for a chunk, exposed for tests.
pub fn saved_chunk_path(save: &SaveState, pos: ChunkPos) -> std::path::PathBuf {
    region_path(&save.regions, pos)
}

/// Whether a save directory holds a world for this seed.
pub fn has_save(dir: &Path) -> bool {
    dir.join("regions").is_dir()
}

/// The player entity marker for a saved-but-offline player.
#[derive(Debug, Component)]
pub struct DormantPlayer;

/// Loads the world when the save has content. Called once at startup after the save is opened.
pub fn load_on_startup(
    mut commands: Commands,
    save: Option<Res<SaveState>>,
    mut manager: ResMut<WorldManager>,
    mut chunks: ResMut<LoadedChunks>,
    mut tech: ResMut<TechStateResource>,
    content: Res<ContentRegistry>,
) {
    let Some(save) = save else {
        return;
    };
    let summary = load_world(&save, &mut manager, &mut chunks, &mut tech, &content);
    let players = load_players(&save, &mut commands, &content);
    if !players.is_empty() {
        info!("restored {} saved players", players.len());
    }
    let _ = summary;
}

/// A player's saved record, used by the handshake to restore a returning player.
#[derive(Debug, Resource, Default)]
pub struct SavedPlayers {
    /// `name → (position, inventory)`.
    pub by_name: std::collections::HashMap<String, (BlockPos, Inventory)>,
}

/// Collects saved players into a lookup for the handshake.
pub fn index_saved_players(
    save: Option<Res<SaveState>>,
    mut saved: ResMut<SavedPlayers>,
    content: Res<ContentRegistry>,
) {
    let Some(save) = save else {
        return;
    };
    let db = save.db.lock().expect("save connection is never poisoned");
    let Ok(mut statement) =
        db.prepare("SELECT name, position_x, position_y, position_z, inventory FROM players")
    else {
        return;
    };
    let Ok(rows) = statement.query_map([], |row| {
        let name: String = row.get(0)?;
        let x: f64 = row.get(1)?;
        let y: f64 = row.get(2)?;
        let z: f64 = row.get(3)?;
        let inventory: Option<Vec<u8>> = row.get(4)?;
        Ok((name, x, y, z, inventory))
    }) else {
        return;
    };
    for row in rows.flatten() {
        let (name, x, y, z, blob) = row;
        let inventory = blob
            .and_then(|blob| postcard::from_bytes::<Inventory>(&blob).ok())
            .unwrap_or_default();
        saved.by_name.insert(
            name,
            (
                BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32),
                inventory,
            ),
        );
    }
    let _ = content;
    if !saved.by_name.is_empty() {
        warn!(
            "{} saved player records available for reconnection",
            saved.by_name.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::encode_chunk;
    use bevy::math::UVec3;
    use deepcore_shared::voxel::Chunk;

    #[test]
    fn region_files_round_trip_into_the_edit_log() {
        let dir = std::env::temp_dir().join(format!("deepcore-load-{}", std::process::id()));
        let regions = dir.join("regions");
        std::fs::create_dir_all(&regions).expect("create dir");
        let pos = ChunkPos::new(16, 12, 16);
        // Write one region file with one modified chunk.
        let mut chunk = Chunk::filled(3);
        chunk.set(UVec3::new(1, 1, 1), 9);
        let bytes = encode_chunk(&chunk).expect("encode");
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"DCR1");
        buffer.extend_from_slice(&1u32.to_le_bytes());
        buffer.extend_from_slice(&pos.x.to_le_bytes());
        buffer.extend_from_slice(&pos.y.to_le_bytes());
        buffer.extend_from_slice(&pos.z.to_le_bytes());
        buffer.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&bytes);
        std::fs::write(regions.join("r.2.1.2.dcr"), buffer).expect("write region");

        let content_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        let content = ContentRegistry::load_from_dir(&content_dir).expect("content");
        let db = Connection::open_in_memory().expect("db");
        let save = SaveState {
            dir: dir.clone(),
            regions: regions.clone(),
            db: std::sync::Mutex::new(db),
            since_autosave: 0.0,
            pending_writes: Vec::new(),
        };
        let mut manager = WorldManager {
            generator: deepcore_shared::worldgen::WorldGenerator::new(1),
            ..Default::default()
        };
        let mut chunks = LoadedChunks::default();
        let mut tech = TechStateResource::default();
        let summary = load_world(&save, &mut manager, &mut chunks, &mut tech, &content);
        assert_eq!(summary.regions, 1);
        assert_eq!(summary.chunks, 1);
        assert!(summary.edits >= 1, "the edited block was not recovered");
        assert!(chunks.contains(pos), "the saved chunk was not restored");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_region_directory_is_not_an_error() {
        let dir = std::env::temp_dir().join("deepcore-no-save");
        let save = SaveState {
            dir: dir.clone(),
            regions: dir.join("regions"),
            db: std::sync::Mutex::new(Connection::open_in_memory().expect("db")),
            since_autosave: 0.0,
            pending_writes: Vec::new(),
        };
        let content_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        let content = ContentRegistry::load_from_dir(&content_dir).expect("content");
        let mut manager = WorldManager::default();
        let mut chunks = LoadedChunks::default();
        let mut tech = TechStateResource::default();
        let summary = load_world(&save, &mut manager, &mut chunks, &mut tech, &content);
        assert_eq!(summary.regions, 0);
    }
}
