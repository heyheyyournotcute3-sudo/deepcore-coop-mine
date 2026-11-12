//! Persistence: modified chunks to region files, players and the tech tree to SQLite.
//!
//! The read side lives in [`load`].
//!
//! The rule is that unmodified chunks are never written: they regenerate from the seed. A chunk
//! is written when it is evicted after an edit or on autosave, and every write goes to a temp
//! file that is then renamed, so a crash cannot leave a half-written region behind.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use bevy::ecs::message::Messages;
use bevy::prelude::*;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::voxel::{ChunkPos, LoadedChunks};
use rusqlite::Connection;
use tracing::{error, info, warn};

pub mod load;

use crate::ServerConfigResource;
use crate::crafting_state::TechStateResource;
use crate::world::WorldManager;

/// Registers persistence systems.
pub struct PersistencePlugin;

impl Plugin for PersistencePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, open_save);
        // The loader runs after `open_save` so the connection exists.
        app.init_resource::<load::SavedPlayers>();
        app.add_systems(
            Startup,
            (
                load::load_on_startup.after(open_save),
                load::index_saved_players.after(open_save),
            ),
        );
        app.add_systems(
            Update,
            (autosave, save_on_exit)
                .run_if(resource_exists::<SaveState>)
                .run_if(resource_exists::<Messages<AppExit>>),
        );
        app.add_systems(Last, flush_region_writes);
    }
}

/// The open save database and paths.
#[derive(Debug, Resource)]
pub struct SaveState {
    /// Save directory.
    pub dir: PathBuf,
    /// Region directory.
    pub regions: PathBuf,
    /// SQLite connection for players, machines and tech.
    ///
    /// `rusqlite::Connection` is `Send` but not `Sync`; the `Mutex` makes the resource legal in
    /// a multithreaded Bevy app. All access is from the main schedule, so it is never contended.
    pub db: std::sync::Mutex<Connection>,
    /// Seconds since the last autosave.
    pub since_autosave: f32,
    /// Pending region writes: `(path, bytes)`.
    pub pending_writes: Vec<(PathBuf, Vec<u8>)>,
}

/// Region files hold 8×8×8 chunks.
pub const REGION_CHUNKS: i32 = 8;

/// Autosave interval in seconds.
pub const AUTOSAVE_SECONDS: f32 = 300.0;

fn open_save(
    mut commands: Commands,
    config: Res<ServerConfigResource>,
    content: Res<ContentRegistry>,
) {
    let dir = PathBuf::from(&config.0.save_dir).join(format!("world-{:016x}", config.0.seed));
    let regions = dir.join("regions");
    if let Err(error) = fs::create_dir_all(&regions) {
        warn!(
            "cannot create save directory {}: {error}",
            regions.display()
        );
        return;
    }
    let db_path = dir.join("deepcore.sqlite");
    let db = match Connection::open(&db_path) {
        Ok(db) => db,
        Err(error) => {
            warn!("cannot open {}: {error}", db_path.display());
            return;
        }
    };
    if let Err(error) = init_schema(&db) {
        error!("cannot initialise save schema: {error}");
        return;
    }
    info!("saves at {}", dir.display());
    commands.insert_resource(SaveState {
        dir,
        regions,
        db: std::sync::Mutex::new(db),
        since_autosave: 0.0,
        pending_writes: Vec::new(),
    });
    let _ = &content;
}

fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS players (
            name TEXT PRIMARY KEY,
            position_x REAL NOT NULL,
            position_y REAL NOT NULL,
            position_z REAL NOT NULL,
            health REAL NOT NULL,
            inventory BLOB
         );
         CREATE TABLE IF NOT EXISTS tech (
            node TEXT PRIMARY KEY
         );
         CREATE TABLE IF NOT EXISTS machines (
            position_x INTEGER NOT NULL,
            position_y INTEGER NOT NULL,
            position_z INTEGER NOT NULL,
            machine TEXT NOT NULL,
            facing INTEGER NOT NULL,
            state BLOB,
            PRIMARY KEY (position_x, position_y, position_z)
         );",
    )
}

/// The region file path for a chunk.
pub fn region_path(regions: &Path, pos: ChunkPos) -> PathBuf {
    let rx = pos.x.div_euclid(REGION_CHUNKS);
    let ry = pos.y.div_euclid(REGION_CHUNKS);
    let rz = pos.z.div_euclid(REGION_CHUNKS);
    regions.join(format!("r.{rx}.{ry}.{rz}.dcr"))
}

/// Serialises one chunk: palette + packed data + zstd.
pub fn encode_chunk(chunk: &deepcore_shared::voxel::Chunk) -> Option<Vec<u8>> {
    let raw = postcard::to_allocvec(&(
        chunk.palette().to_vec(),
        chunk.bits(),
        chunk.packed().to_vec(),
    ))
    .ok()?;
    zstd::encode_all(&raw[..], 3).ok()
}

/// Decodes a chunk written by [`encode_chunk`].
pub fn decode_chunk(bytes: &[u8]) -> Option<deepcore_shared::voxel::Chunk> {
    let raw = zstd::decode_all(bytes).ok()?;
    let (palette, _bits, packed): (Vec<u16>, u32, Vec<u8>) = postcard::from_bytes(&raw).ok()?;
    let chunk = deepcore_shared::voxel::Chunk::default();
    let mut indices = vec![0u16; deepcore_shared::voxel::CHUNK_VOLUME];
    let bits = 16usize;
    for (index, slot) in indices.iter_mut().enumerate() {
        let bit_offset = index * bits;
        let byte = bit_offset / 8;
        let mut value: u16 = 0;
        for offset in 0..2 {
            value |= u16::from(*packed.get(byte + offset).unwrap_or(&0)) << (8 * offset);
        }
        *slot = value;
    }
    let _ = chunk;
    Some(deepcore_shared::voxel::Chunk::from_palette(
        palette, &indices,
    ))
}

/// Autosaves modified chunks, players and the tech tree every five minutes.
pub fn autosave(
    time: Res<Time<Real>>,
    mut save: ResMut<SaveState>,
    mut manager: ResMut<WorldManager>,
    chunks: Res<LoadedChunks>,
    tech: Res<TechStateResource>,
    players: Query<(
        &deepcore_shared::protocol::PlayerName,
        &deepcore_shared::protocol::NetworkPosition,
        &crate::sim::player::PlayerState,
    )>,
) {
    save.since_autosave += time.delta_secs();
    if save.since_autosave < AUTOSAVE_SECONDS {
        return;
    }
    save.since_autosave = 0.0;
    save_world(&mut save, &mut manager, &chunks, &tech, &players);
}

fn save_on_exit(
    mut exits: MessageReader<AppExit>,
    mut save: ResMut<SaveState>,
    mut manager: ResMut<WorldManager>,
    chunks: Res<LoadedChunks>,
    tech: Res<TechStateResource>,
    players: Query<(
        &deepcore_shared::protocol::PlayerName,
        &deepcore_shared::protocol::NetworkPosition,
        &crate::sim::player::PlayerState,
    )>,
) {
    if exits.read().next().is_none() {
        return;
    }
    info!("clean shutdown: saving");
    save_world(&mut save, &mut manager, &chunks, &tech, &players);
    // Flush immediately on exit.
    for (path, bytes) in std::mem::take(&mut save.pending_writes) {
        write_atomic(&path, &bytes);
    }
}

/// Writes dirty chunks, players and the tech tree.
pub fn save_world(
    save: &mut SaveState,
    manager: &mut WorldManager,
    chunks: &LoadedChunks,
    tech: &TechStateResource,
    players: &Query<(
        &deepcore_shared::protocol::PlayerName,
        &deepcore_shared::protocol::NetworkPosition,
        &crate::sim::player::PlayerState,
    )>,
) {
    let dirty: Vec<ChunkPos> = manager.dirty.iter().copied().collect();
    let mut per_region: HashMap<PathBuf, Vec<(ChunkPos, Vec<u8>)>> = HashMap::new();
    for pos in dirty {
        let Some(chunk) = chunks.get(pos) else {
            continue;
        };
        let Some(bytes) = encode_chunk(chunk) else {
            continue;
        };
        per_region
            .entry(region_path(&save.regions, pos))
            .or_default()
            .push((pos, bytes));
    }
    for (path, entries) in per_region {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"DCR1");
        buffer.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (pos, bytes) in entries {
            buffer.extend_from_slice(&pos.x.to_le_bytes());
            buffer.extend_from_slice(&pos.y.to_le_bytes());
            buffer.extend_from_slice(&pos.z.to_le_bytes());
            buffer.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buffer.extend_from_slice(&bytes);
        }
        save.pending_writes.push((path, buffer));
    }
    manager.dirty.clear();

    let db = save
        .db
        .get_mut()
        .expect("save connection is never poisoned");
    for (name, position, state) in players {
        let _ = db.execute(
            "INSERT INTO players (name, position_x, position_y, position_z, health, inventory)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(name) DO UPDATE SET
                position_x = excluded.position_x,
                position_y = excluded.position_y,
                position_z = excluded.position_z,
                health = excluded.health,
                inventory = excluded.inventory",
            rusqlite::params![
                name.0,
                position.position.x,
                position.position.y,
                position.position.z,
                state.vitals.health,
                postcard::to_allocvec(&state.inventory).ok(),
            ],
        );
    }
    let _ = db.execute("DELETE FROM tech", []);
    for node in tech.0.researched() {
        let _ = db.execute("INSERT INTO tech (node) VALUES (?1)", [node.0 as i64]);
    }
    info!(
        "autosave: {} regions queued, {} players, {} tech nodes",
        save.pending_writes.len(),
        players.iter().count(),
        tech.0.researched().len()
    );
}

/// Writes queued region files on a background task pool.
pub fn flush_region_writes(mut save: ResMut<SaveState>) {
    if save.pending_writes.is_empty() {
        return;
    }
    let writes = std::mem::take(&mut save.pending_writes);
    bevy::tasks::AsyncComputeTaskPool::get()
        .spawn(async move {
            for (path, bytes) in writes {
                write_atomic(&path, &bytes);
            }
        })
        .detach();
}

/// Writes to a temp file and renames, so a crash never leaves a partial file.
pub fn write_atomic(path: &Path, bytes: &[u8]) {
    let temp = path.with_extension("tmp");
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if let Err(error) = result {
        warn!("cannot write {}: {error}", path.display());
    }
}

/// Loads a region file, returning every chunk it holds.
pub fn read_region(path: &Path) -> Vec<(ChunkPos, deepcore_shared::voxel::Chunk)> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    if bytes.len() < 8 || &bytes[0..4] != b"DCR1" {
        return Vec::new();
    }
    let count = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let mut cursor = 8;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if cursor + 16 > bytes.len() {
            break;
        }
        let x = i32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap_or([0; 4]));
        let y = i32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap_or([0; 4]));
        let z = i32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap_or([0; 4]));
        let len = u32::from_le_bytes(bytes[cursor + 12..cursor + 16].try_into().unwrap_or([0; 4]))
            as usize;
        cursor += 16;
        if cursor + len > bytes.len() {
            break;
        }
        if let Some(chunk) = decode_chunk(&bytes[cursor..cursor + len]) {
            out.push((ChunkPos::new(x, y, z), chunk));
        }
        cursor += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::UVec3;
    use deepcore_shared::voxel::{CHUNK_VOLUME, Chunk};

    #[test]
    fn chunk_round_trips_through_the_region_format() {
        let mut chunk = Chunk::filled(4);
        chunk.set(UVec3::new(3, 5, 7), 12);
        let encoded = encode_chunk(&chunk).expect("encodes");
        let decoded = decode_chunk(&encoded).expect("decodes");
        assert_eq!(decoded.get(UVec3::new(3, 5, 7)), 12);
        assert_eq!(decoded.get(UVec3::new(0, 0, 0)), 4);
        assert_eq!(decoded.solid_count(), CHUNK_VOLUME as u32);
    }

    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join("deepcore-test-write");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("r.0.0.0.dcr");
        write_atomic(&path, b"hello");
        assert_eq!(fs::read(&path).expect("readable"), b"hello");
        assert!(!path.with_extension("tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
