//! The headless authoritative Deepcore server, as a library.
//!
//! The `deepcore-server` binary is a thin wrapper around [`run`], so integration tests can build
//! the same app without opening sockets.
//!
//! Runs Bevy with `MinimalPlugins` (no window, no renderer), a fixed 30 Hz simulation in
//! `FixedUpdate`, and the replicon/renet networking stack. Every gameplay rule lives on this
//! side: the server owns the world, the inventories, the machines, the power networks and the
//! hazards, and it validates every client intent before acting on it.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_replicon::prelude::*;
use bevy_replicon_renet::renet::ConnectionConfig;
use bevy_replicon_renet::{RenetChannelsExt, RepliconRenetPlugins};
use clap::Parser;
use deepcore_shared::content::{ContentDir, ContentPlugin};
use deepcore_shared::protocol::{DEFAULT_PORT, PROTOCOL_VERSION};
use deepcore_shared::voxel::LoadedChunks;
use deepcore_shared::{SharedPlugin, content::ContentRegistry, protocol::ProtocolPlugin};
use tracing::{error, info};

pub mod crafting_state;
pub mod net;
pub mod persistence;
pub mod sim;
pub mod world;

/// Command line options for the dedicated server.
#[derive(Debug, Parser, Clone)]
#[command(name = "deepcore-server", about = "Deepcore dedicated server")]
pub struct ServerArgs {
    /// Port to listen on.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
    /// World seed. The same seed always generates the same world.
    #[arg(long, default_value_t = 0x0DEE9C04E)]
    pub seed: u64,
    /// Directory holding the RON content.
    #[arg(long, default_value = "assets/data")]
    pub content: String,
    /// Directory for saves.
    #[arg(long, default_value = "saves")]
    pub save_dir: String,
    /// Simulated one-way latency, in milliseconds.
    #[arg(long, default_value_t = 0)]
    pub simulate_latency: u64,
    /// Simulated packet loss, in percent.
    #[arg(long, default_value_t = 0)]
    pub simulate_loss: u8,
    /// Number of headless bot clients to connect for load and desync testing.
    #[arg(long, default_value_t = 0)]
    pub bots: u32,
    /// Server name shown in the server browser.
    #[arg(long, default_value = "Deepcore")]
    pub name: String,
    /// Lobby service URL, empty for an open LAN server.
    #[arg(long, default_value = "")]
    pub lobby: String,
    /// Refuse players without a valid lobby account.
    #[arg(long, default_value_t = false)]
    pub require_account: bool,
    /// Public host name advertised to the lobby.
    #[arg(long)]
    pub public_host: Option<String>,
    /// Exit after this many seconds (used by the restart test and by smoke checks).
    #[arg(long, default_value_t = 0)]
    pub run_seconds: u64,
}

/// The protocol id used by renet's netcode: changing the protocol version changes the id, so
/// mismatched builds cannot even complete a transport handshake.
pub fn protocol_id() -> u64 {
    0x0DEE_C04E_0000_0000 | u64::from(PROTOCOL_VERSION)
}

/// Runs the dedicated server.
pub fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,deepcore_server=debug".into()),
        )
        .init();

    let args = ServerArgs::parse();
    info!(
        "Deepcore server starting: port {} seed {} protocol {}",
        args.port, args.seed, PROTOCOL_VERSION
    );

    let mut app = App::new();
    // `MinimalPlugins` already includes `LogPlugin`, and the subscriber above is ours, so the
    // app only needs the scheduler and the state machine.
    app.add_plugins((MinimalPlugins, StatesPlugin));

    // The simulation runs at exactly 30 Hz and replicon's tick is aligned with it.
    app.insert_resource(Time::<Fixed>::from_hz(
        deepcore_shared::crafting::TICK_HZ as f64,
    ));
    app.insert_resource(Time::<Virtual>::default());

    // Order matters: content first (registries), then replicon (replication registries),
    // then the shared rules and the protocol that registers replicated components.
    app.insert_resource(ContentDir(std::path::PathBuf::from(&args.content)));
    app.add_plugins(ContentPlugin);
    app.add_plugins((RepliconPlugins, RepliconRenetPlugins));
    app.add_plugins((SharedPlugin, ProtocolPlugin));

    app.insert_resource(ServerConfigResource(args.clone()));
    app.init_resource::<world::WorldManager>();
    app.add_plugins((
        net::NetPlugin,
        net::lobby::LobbyPlugin,
        world::ServerWorldPlugin,
        sim::SimPlugin,
        persistence::PersistencePlugin,
    ));
    app.add_message::<AppExit>();
    if args.run_seconds > 0 {
        app.add_systems(Update, exit_after_delay);
    }

    app.run();
    Ok(())
}

/// The parsed server arguments, available to every plugin.
#[derive(Debug, Resource, Clone)]
pub struct ServerConfigResource(pub ServerArgs);

impl ServerConfigResource {
    /// The socket address the server binds to.
    pub fn bind_address(&self) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), self.0.port)
    }
}

/// Builds the renet connection configuration from replicon's channel list.
pub fn connection_config(channels: &RepliconChannels) -> ConnectionConfig {
    let mut config = ConnectionConfig {
        server_channels_config: channels.server_configs(),
        client_channels_config: channels.client_configs(),
        ..Default::default()
    };
    // Chunk streaming is bursty: give the reliable channels room for several chunks per tick.
    for channel in &mut config.server_channels_config {
        channel.max_memory_usage_bytes = 8 * 1024 * 1024;
    }
    config
}

/// Builds a headless server `App` for tests: no sockets, no window, plugins only.
///
/// Tests drive the simulation by inserting intents and stepping `FixedUpdate` directly.
pub fn build_headless_app(seed: u64, content_dir: &str) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin));
    app.insert_resource(Time::<Fixed>::from_hz(
        deepcore_shared::crafting::TICK_HZ as f64,
    ));
    app.insert_resource(ContentDir(std::path::PathBuf::from(content_dir)));
    app.add_plugins((ContentPlugin, SharedPlugin));
    app.insert_resource(ServerConfigResource(ServerArgs {
        port: 0,
        seed,
        content: content_dir.to_owned(),
        save_dir: "saves-test".to_owned(),
        simulate_latency: 0,
        simulate_loss: 0,
        bots: 0,
        name: "test".to_owned(),
        lobby: String::new(),
        require_account: false,
        public_host: None,
        run_seconds: 0,
    }));
    app.init_resource::<world::WorldManager>();
    app.add_plugins((world::ServerWorldPlugin, sim::SimPlugin));
    app
}

/// Logs a fatal configuration error and exits, used when a socket cannot be bound.
pub fn fatal(message: &str) -> ! {
    error!("{message}");
    std::process::exit(1)
}

/// Exits the process after `--run-seconds`, sending `AppExit` so the save-on-exit path runs.
fn exit_after_delay(
    time: Res<Time<Real>>,
    config: Res<ServerConfigResource>,
    mut exits: MessageWriter<AppExit>,
    mut sent: Local<bool>,
) {
    if *sent {
        return;
    }
    if time.elapsed_secs() >= config.0.run_seconds as f32 {
        *sent = true;
        info!("run time reached, shutting down cleanly");
        exits.write(AppExit::Success);
    }
}

/// A small helper so tests and the binary agree on the simulation timestep.
pub fn tick_duration() -> Duration {
    Duration::from_secs_f32(deepcore_shared::crafting::TICK_SECONDS)
}

/// Convenience for plugins: the content registry, which every system needs.
pub type Content<'w> = Res<'w, ContentRegistry>;

/// Convenience for plugins: the loaded chunks resource.
pub type Chunks<'w> = Res<'w, LoadedChunks>;
