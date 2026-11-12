//! Networking: the renet transport, the handshake, and per-client chunk visibility.
//!
//! The handshake is deliberately small. On connect the server creates a `PendingJoin` for the
//! client. The client's first message must be a [`Handshake`] with a matching protocol version
//! and content hash; only then does the server spawn the player and start streaming chunks.

use std::collections::HashMap;
use std::time::Duration;

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use bevy_replicon_renet::RenetServer as RenetServerResource;
use bevy_replicon_renet::netcode::{
    ClientAuthentication, NetcodeServerTransport, ServerAuthentication, ServerConfig,
};
use bevy_replicon_renet::renet::RenetClient;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::protocol::{ChunkData, NetworkPosition};
use deepcore_shared::protocol::{
    ClientId, ConnectedClient, DisconnectRequest, FromClient, SendTargets, ToClients,
};
use deepcore_shared::protocol::{Handshake, PROTOCOL_VERSION, ServerNotice};
use deepcore_shared::voxel::{BlockPos, ChunkPos, LoadedChunks};
use tracing::{debug, info, warn};

use crate::sim::player::spawn_player_for_client;
use crate::world::{INTEREST_HORIZONTAL, INTEREST_VERTICAL};
use crate::{ServerConfigResource, connection_config, persistence::encode_chunk, protocol_id};

/// Registers the transport and the handshake.
pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        // Replicon and its renet backend are added by `main` before the shared plugins, so the
        // replication registries exist when the protocol registers its components.
        app.init_resource::<PendingJoins>();
        app.init_resource::<SentChunks>();
        // `LocalOwner` only reaches the owning client: each player entity carries the marker and
        // each client entity carries the player it owns, so the filter matches exactly one pair.
        app.add_visibility_filter::<deepcore_shared::protocol::LocalOwner>();
        app.add_systems(Startup, start_server);
        app.add_systems(
            PreUpdate,
            (receive_handshakes.run_if(in_state(ServerState::Running)),)
                .after(ServerSystems::Receive),
        );
        app.add_systems(
            Update,
            (
                handle_connected_clients.run_if(in_state(ServerState::Running)),
                stream_chunks.run_if(in_state(ServerState::Running)),
            ),
        );
    }
}

/// Clients that connected at the transport level but have not completed the handshake.
#[derive(Debug, Default, Resource)]
pub struct PendingJoins {
    /// `client entity → time of connection`.
    pub waiting: HashMap<Entity, Duration>,
}

fn start_server(
    mut commands: Commands,
    config: Res<ServerConfigResource>,
    channels: Res<RepliconChannels>,
    time: Res<Time<Real>>,
) {
    let server_config = ServerConfig {
        current_time: time.elapsed(),
        max_clients: deepcore_shared::protocol::MAX_PLAYERS,
        protocol_id: protocol_id(),
        public_addresses: vec![config.bind_address()],
        authentication: ServerAuthentication::Unsecure,
    };
    let socket = match std::net::UdpSocket::bind(config.bind_address()) {
        Ok(socket) => socket,
        Err(error) => {
            crate::fatal(&format!("cannot bind UDP port {}: {error}", config.0.port));
        }
    };
    let transport = match NetcodeServerTransport::new(server_config, socket) {
        Ok(transport) => transport,
        Err(error) => crate::fatal(&format!("cannot create netcode transport: {error}")),
    };
    info!(
        "listening on {} (UDP, protocol {PROTOCOL_VERSION})",
        config.bind_address()
    );
    commands.insert_resource(RenetServerResource::new(connection_config(&channels)));
    commands.insert_resource(transport);
}

/// Handles new transport connections: they start as pending until the handshake arrives.
fn handle_connected_clients(
    mut commands: Commands,
    mut joins: ResMut<PendingJoins>,
    added: Query<Entity, Added<ConnectedClient>>,
    time: Res<Time<Real>>,
) {
    for entity in &added {
        info!("client {entity} connected, waiting for handshake");
        joins.waiting.insert(entity, time.elapsed());
        commands.entity(entity).insert(PendingHandshake);
    }
}

/// Marker: the client is connected but has not completed the game handshake.
#[derive(Debug, Component)]
pub struct PendingHandshake;

/// Reads the handshake message and spawns the player when it is valid.
fn receive_handshakes(
    mut commands: Commands,
    mut messages: MessageReader<FromClient<Handshake>>,
    mut notices: MessageWriter<ToClients<ServerNotice>>,
    mut joins: ResMut<PendingJoins>,
    content: Res<ContentRegistry>,
    config: Res<ServerConfigResource>,
    clients: Query<&ConnectedClient>,
) {
    for FromClient { client_id, message } in messages.read() {
        let Some(client_entity) = client_id.entity() else {
            continue;
        };
        if clients.get(client_entity).is_err() {
            continue;
        }

        if message.protocol != PROTOCOL_VERSION {
            refuse(
                &mut commands,
                &mut notices,
                client_entity,
                format!(
                    "Protocol mismatch: server {} client {}",
                    PROTOCOL_VERSION, message.protocol
                ),
            );
            continue;
        }
        if message.content_hash != content.content_hash {
            refuse(
                &mut commands,
                &mut notices,
                client_entity,
                "Content mismatch".to_owned(),
            );
            continue;
        }
        let name = sanitise_name(&message.name);
        info!("handshake from {name} accepted");
        spawn_player_for_client(&mut commands, &content, &config, client_entity, name);
        joins.waiting.remove(&client_entity);
    }
}

fn refuse(
    commands: &mut Commands,
    notices: &mut MessageWriter<ToClients<ServerNotice>>,
    client: Entity,
    reason: String,
) {
    warn!("refusing client {client}: {reason}");
    notices.write(ToClients {
        targets: SendTargets::Single(ClientId::Client(client)),
        message: ServerNotice::Refused { reason },
    });
    commands.write_message(DisconnectRequest { client });
}

/// Sends chunk data to each client for chunks it has not received yet.
///
/// Chunks are sent nearest-first and only within the interest radius, which is the same radius
/// the server generates and the client meshes. The payload is palette-packed and zstd-compressed.
pub fn stream_chunks(
    players: Query<(&OwnedByPlayer, &NetworkPosition)>,
    chunks: Res<LoadedChunks>,
    mut sent: ResMut<SentChunks>,
    mut writer: MessageWriter<ToClients<ChunkData>>,
) {
    for (owner, position) in &players {
        let centre = BlockPos::from_vec3(position.position).chunk();
        let mut candidates: Vec<ChunkPos> = chunks
            .positions()
            .filter(|pos| {
                (pos.x - centre.x).abs() <= INTEREST_HORIZONTAL
                    && (pos.z - centre.z).abs() <= INTEREST_HORIZONTAL
                    && (pos.y - centre.y).abs() <= INTEREST_VERTICAL
            })
            .collect();
        candidates.sort_by_key(|pos| pos.distance(centre));
        let already = sent.by_client.entry(owner.0).or_default();
        // Nearest chunks first; the budget is per client per frame and generous enough to
        // clear the loading screen in a few seconds on a LAN.
        let mut budget = CHUNKS_PER_TICK;
        for pos in candidates {
            if budget == 0 {
                break;
            }
            if !already.insert(pos) {
                continue;
            }
            let Some(chunk) = chunks.get(pos) else {
                continue;
            };
            let Some(payload) = encode_chunk(chunk) else {
                continue;
            };
            debug!(
                "streaming chunk {pos:?} ({} bytes) to {}",
                payload.len(),
                owner.0
            );
            writer.write(ToClients {
                targets: SendTargets::Single(ClientId::Client(owner.0)),
                message: ChunkData {
                    pos,
                    payload,
                    unload: false,
                },
            });
            budget -= 1;
        }
    }
}

/// Chunks each client receives per frame, so a burst never starves player updates.
pub const CHUNKS_PER_TICK: usize = 12;

/// The chunks already sent to each client.
#[derive(Debug, Default, Resource)]
pub struct SentChunks {
    /// `client entity → chunk set`.
    pub by_client: HashMap<Entity, std::collections::HashSet<ChunkPos>>,
}

/// The client that owns a player entity.
#[derive(Debug, Component)]
pub struct OwnedByPlayer(pub Entity);

/// Keeps a display name printable and short.
fn sanitise_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(24)
        .collect();
    if cleaned.trim().is_empty() {
        "Miner".to_owned()
    } else {
        cleaned.trim().to_owned()
    }
}

/// Builds a renet client for the "host game" path or a real join.
pub fn client_transport(
    channels: &RepliconChannels,
    server_addr: std::net::SocketAddr,
    user_data: Option<[u8; 256]>,
) -> (RenetClient, ClientAuthentication) {
    let config = connection_config(channels);
    let client = RenetClient::new(config);
    let authentication = ClientAuthentication::Unsecure {
        protocol_id: protocol_id(),
        client_id: rand_client_id(),
        server_addr,
        user_data,
    };
    (client, authentication)
}

fn rand_client_id() -> u64 {
    use rand::Rng;
    let mut rng = rand::rng();
    rng.random()
}
