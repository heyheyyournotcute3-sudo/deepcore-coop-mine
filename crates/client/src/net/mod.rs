//! The client's network session: connecting, receiving chunks and world updates, prediction.

use std::net::SocketAddr;

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use bevy_replicon_renet::RenetClient;
use bevy_replicon_renet::netcode::{ClientAuthentication, NetcodeClientTransport};
use bevy_replicon_renet::RenetChannelsExt;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::protocol::{
    BlockChanges, ChatLine, ChunkData, Handshake, InventoryUpdate, MachineScreen, MiningFeedback,
    NetworkStats, PROTOCOL_VERSION, ServerNotice, TechState,
};
use deepcore_shared::voxel::{BlockPos, Chunk, ChunkPos, LoadedChunks};
use tracing::{debug, info, warn};

use crate::ClientConfig;

/// Registers the network session.
pub struct ClientNetPlugin;

impl Plugin for ClientNetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SessionState>()
            .init_resource::<ReceivedChunks>()
            .add_systems(Startup, start_connection)
            .add_systems(
                Update,
                (
                    send_handshake_when_connected,
                    receive_chunks,
                    receive_block_changes,
                    receive_notices,
                    receive_tech_state,
                    receive_chat,
                    receive_inventory,
                    receive_machine_screen,
                    receive_mining_feedback,
                    track_local_vitals,
                )
                    .run_if(in_state(ClientState::Connected)),
            );
    }
}

/// The local player's view of the session.
#[derive(Debug, Resource, Default)]
pub struct SessionState {
    /// Whether the handshake has been accepted.
    pub welcomed: bool,
    /// The world seed the server reported.
    pub seed: u64,
    /// The server's name.
    pub server_name: String,
    /// The last connection error, for the menu.
    pub error: Option<String>,
    /// The local player entity, once replicated.
    pub local_player: Option<Entity>,
    /// The tech tree as last received.
    pub tech: TechState,
    /// The last inventory the server sent.
    pub inventory: Option<InventoryUpdate>,
    /// The last machine screen opened.
    pub machine_screen: Option<MachineScreen>,
    /// The last mining feedback.
    pub mining_feedback: Option<MiningFeedback>,
    /// Chat lines.
    pub chat: Vec<ChatLine>,
    /// Notifications, newest last.
    pub notifications: Vec<(String, u8, f32)>,
    /// Network stats for the debug overlay.
    pub stats: Option<NetworkStats>,
    /// The local player's vitals as replicated by the server.
    pub vitals: deepcore_shared::hazards::Vitals,
    /// The local player's dose rate, sampled by the client from nearby blocks.
    pub dose_rate: f32,
}

impl SessionState {
    /// The local player's health.
    pub fn health(&self) -> f32 {
        self.vitals.health
    }

    /// The local player's oxygen.
    pub fn oxygen(&self) -> f32 {
        self.vitals.oxygen
    }

    /// The local player's radiation dose.
    pub fn dose(&self) -> f32 {
        self.vitals.dose
    }

    /// The local player's dose rate, for the Geiger counter.
    pub fn dose_rate(&self) -> f32 {
        self.dose_rate
    }
}

/// Counts chunks received, for the loading screen.
#[derive(Debug, Resource, Default)]
pub struct ReceivedChunks {
    /// Number of chunks received.
    pub count: usize,
}

fn start_connection(
    mut commands: Commands,
    config: Res<ClientConfig>,
    channels: Res<RepliconChannels>,
    content: Res<ContentRegistry>,
) {
    let Some(address) = config.connect.clone() else {
        info!("no server configured; the menu will offer Host and Join");
        return;
    };
    let server_addr: SocketAddr = match address.parse() {
        Ok(addr) => addr,
        Err(error) => {
            warn!("invalid server address {address}: {error}");
            return;
        }
    };
    let mut connection = ConnectionConfig {
        server_channels_config: channels.server_configs(),
        client_channels_config: channels.client_configs(),
        ..Default::default()
    };
    for channel in &mut connection.server_channels_config {
        channel.max_memory_usage_bytes = 8 * 1024 * 1024;
    }
    let client = RenetClient::new(connection);
    let authentication = ClientAuthentication::Unsecure {
        protocol_id: crate::protocol_id(),
        client_id: rand_id(),
        server_addr,
        user_data: None,
    };
    let socket = match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => socket,
        Err(error) => {
            warn!("cannot bind client socket: {error}");
            return;
        }
    };
    let transport = match NetcodeClientTransport::new(std::time::Duration::ZERO, authentication, socket)
    {
        Ok(transport) => transport,
        Err(error) => {
            warn!("cannot create client transport: {error}");
            return;
        }
    };
    info!("connecting to {server_addr}");
    commands.insert_resource(client);
    commands.insert_resource(transport);
    commands.insert_resource(ExpectedContentHash(content.content_hash));
}

use bevy_replicon_renet::renet::ConnectionConfig;

/// The content hash the server must match.
#[derive(Debug, Resource)]
pub struct ExpectedContentHash(pub u64);

fn send_handshake_when_connected(
    mut sent: Local<bool>,
    config: Res<ClientConfig>,
    content: Res<ContentRegistry>,
    mut handshakes: MessageWriter<Handshake>,
) {
    if *sent {
        return;
    }
    *sent = true;
    handshakes.write(Handshake {
        protocol: PROTOCOL_VERSION,
        content_hash: content.content_hash,
        name: config.player_name(),
        token: config.token(),
    });
    info!("handshake sent");
}

/// Receives chunk data and inserts it into the client's chunk store.
pub fn receive_chunks(
    mut messages: MessageReader<ChunkData>,
    mut chunks: ResMut<LoadedChunks>,
    mut received: ResMut<ReceivedChunks>,
) {
    let mut count = 0usize;
    for message in messages.read() {
        count += 1;
        if message.unload {
            chunks.remove(message.pos);
            continue;
        }
        match decode_chunk(&message.payload) {
            Some(chunk) => {
                chunks.insert(message.pos, chunk);
                received.count += 1;
            }
            None => warn!("dropping corrupt chunk at {:?}", message.pos),
        }
    }
    if count > 0 {
        debug!("received {count} chunk messages");
    }
}

/// Decodes a chunk payload written by the server.
pub fn decode_chunk(payload: &[u8]) -> Option<Chunk> {
    let raw = zstd::decode_all(payload).ok()?;
    let (palette, _bits, packed): (Vec<u16>, u32, Vec<u8>) = postcard::from_bytes(&raw).ok()?;
    let mut indices = vec![0u16; deepcore_shared::voxel::CHUNK_VOLUME];
    for (index, slot) in indices.iter_mut().enumerate() {
        let byte = index * 2;
        let mut value: u16 = 0;
        for offset in 0..2 {
            value |= u16::from(*packed.get(byte + offset).unwrap_or(&0)) << (8 * offset);
        }
        *slot = value;
    }
    Some(Chunk::from_palette(palette, &indices))
}

fn receive_block_changes(
    mut messages: MessageReader<BlockChanges>,
    mut chunks: ResMut<LoadedChunks>,
) {
    for message in messages.read() {
        for (pos, block) in &message.changes {
            chunks.set_block(*pos, *block);
        }
    }
}

fn receive_notices(
    mut messages: MessageReader<ServerNotice>,
    mut session: ResMut<SessionState>,
) {
    for notice in messages.read() {
        match notice {
            ServerNotice::Welcome {
                seed,
                server_name,
                ..
            } => {
                session.welcomed = true;
                session.seed = *seed;
                session.server_name = server_name.clone();
                info!("welcomed by {server_name} (seed {seed})");
            }
            ServerNotice::Refused { reason } => {
                session.error = Some(reason.clone());
                warn!("refused: {reason}");
            }
            ServerNotice::Notification { text, severity } => {
                session.notifications.push((text.clone(), *severity, 0.0));
                if session.notifications.len() > 8 {
                    session.notifications.remove(0);
                }
            }
        }
    }
}

fn receive_tech_state(mut messages: MessageReader<TechState>, mut session: ResMut<SessionState>) {
    for state in messages.read() {
        session.tech = state.clone();
    }
}

fn receive_chat(mut messages: MessageReader<ChatLine>, mut session: ResMut<SessionState>) {
    for line in messages.read() {
        session.chat.push(line.clone());
        if session.chat.len() > 100 {
            session.chat.remove(0);
        }
    }
}

fn receive_inventory(
    mut messages: MessageReader<InventoryUpdate>,
    mut session: ResMut<SessionState>,
) {
    for update in messages.read() {
        session.inventory = Some(update.clone());
    }
}

fn receive_machine_screen(
    mut messages: MessageReader<MachineScreen>,
    mut session: ResMut<SessionState>,
) {
    for screen in messages.read() {
        session.machine_screen = Some(screen.clone());
    }
}

fn receive_mining_feedback(
    mut messages: MessageReader<MiningFeedback>,
    mut session: ResMut<SessionState>,
) {
    for feedback in messages.read() {
        session.mining_feedback = Some(feedback.clone());
    }
}

/// Copies the replicated vitals of the local player into the session, and samples the dose rate
/// from the blocks around it so the Geiger counter reacts before the server's dose updates.
fn track_local_vitals(
    players: Query<(
        &deepcore_shared::protocol::PlayerVitals,
        &deepcore_shared::protocol::NetworkPosition,
        Option<&deepcore_shared::protocol::LocalOwner>,
    )>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
    mut session: ResMut<SessionState>,
) {
    for (vitals, position, owner) in &players {
        if owner.is_none() {
            continue;
        }
        session.vitals = vitals.vitals;
        let eye = position.position + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
        let eye_block = BlockPos::from_vec3(eye);
        let mut sources = Vec::new();
        for dy in -4..=4i32 {
            for dz in -4..=4i32 {
                for dx in -4..=4i32 {
                    let pos = eye_block.offset(dx, dy, dz);
                    let Some(id) = chunks.block(pos) else {
                        continue;
                    };
                    let Some(block) = content.block(deepcore_shared::content::ContentId(id)) else {
                        continue;
                    };
                    if let Some(radiation) = block.radiation {
                        let distance = Vec3::new(dx as f32, dy as f32, dz as f32).length();
                        sources.push((radiation, distance));
                    }
                }
            }
        }
        session.dose_rate = deepcore_shared::hazards::source_dose(&sources, 0.0);
    }
}

fn rand_id() -> u64 {
    use rand::Rng;
    rand::rng().random()
}

/// Helper for other modules: a block position from a world-space point.
pub fn block_at(position: Vec3) -> BlockPos {
    BlockPos::from_vec3(position)
}

/// Helper: whether the chunk is loaded on the client.
pub fn chunk_loaded(chunks: &LoadedChunks, pos: BlockPos) -> bool {
    chunks.contains(pos.chunk()) && pos.in_bounds()
}

/// Debug helper: the chunk position of a point.
pub fn chunk_of(position: Vec3) -> ChunkPos {
    BlockPos::from_vec3(position).chunk()
}
