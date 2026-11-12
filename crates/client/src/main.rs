//! The client binary: window, camera, rendering, controls, HUD and menus.
//!
//! The client predicts only its own movement. Everything else — blocks, machines, inventories,
//! vitals — comes from the server.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_egui::EguiPlugin;
use bevy_replicon::prelude::*;
use bevy_replicon_renet::RepliconRenetPlugins;
use clap::Parser;
use deepcore_shared::content::{ContentDir, ContentPlugin};
use deepcore_shared::protocol::ProtocolPlugin;
use deepcore_shared::{SharedPlugin, content::ContentRegistry};
use tracing::info;

pub mod audio;
pub mod hud;
pub mod input;
pub mod net;
pub mod prediction;
pub mod render;
pub mod ui;

/// Command line options for the client.
#[derive(Debug, Parser, Clone, Resource)]
#[command(name = "deepcore-client", about = "Deepcore client")]
pub struct ClientConfig {
    /// Connect directly to this `host:port` on start.
    #[arg(long)]
    pub connect: Option<String>,
    /// Player display name.
    #[arg(long, default_value = "Miner")]
    pub name: String,
    /// Directory holding the RON content.
    #[arg(long, default_value = "assets/data")]
    pub content: String,
    /// Render distance in chunks.
    #[arg(long, default_value_t = 6)]
    pub render_distance: i32,
    /// Simulated one-way latency in milliseconds.
    #[arg(long, default_value_t = 0)]
    pub simulate_latency: u64,
    /// Simulated packet loss in percent.
    #[arg(long, default_value_t = 0)]
    pub simulate_loss: u8,
    /// Lobby service URL, empty when playing without accounts.
    #[arg(long, default_value = "")]
    pub lobby: String,
}

/// Resolved connection settings, read by the network plugin.
#[derive(Debug, Resource, Clone, Default)]
pub struct ConnectionSettings {
    /// The server to connect to, if any.
    pub connect_address: Option<String>,
}

impl ClientConfig {
    /// The name to send in the handshake.
    pub fn player_name(&self) -> String {
        self.name.clone()
    }

    /// The lobby token, empty without accounts.
    pub fn token(&self) -> String {
        String::new()
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,deepcore_client=debug".into()),
        )
        .init();

    let args = ClientConfig::parse();
    info!("Deepcore client starting");

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Deepcore".into(),
                    resolution: WindowResolution::new(1600, 900),
                    ..default()
                }),
                ..default()
            })
            .set(ImagePlugin::default_nearest()),
        EguiPlugin::default(),
    ));

    app.insert_resource(ConnectionSettings {
        connect_address: args.connect.clone(),
    });
    app.insert_resource(ContentDir(PathBuf::from(&args.content)));
    app.insert_resource(args.clone());
    app.add_plugins(ContentPlugin);
    app.add_plugins((RepliconPlugins, RepliconRenetPlugins));
    app.add_plugins((SharedPlugin, ProtocolPlugin));

    app.add_plugins((
        render::RenderPlugin,
        input::InputPlugin,
        prediction::PredictionPlugin,
        hud::HudPlugin,
        ui::UiPlugin,
        audio::AudioPlugin,
        net::ClientNetPlugin,
    ));

    app.add_systems(Startup, setup_scene);
    app.add_systems(
        Update,
        (
            spawn_player_avatars,
            follow_local_player,
            decay_notifications,
        ),
    );
    app.run();
    Ok(())
}

/// The protocol id used by renet's netcode; must match the server's.
pub fn protocol_id() -> u64 {
    0x0DEE_C04E_0000_0000 | u64::from(deepcore_shared::protocol::PROTOCOL_VERSION)
}

/// The local player, marked so the camera can follow it.
#[derive(Debug, Component)]
pub struct LocalPlayer;

/// The player's headlamp, moved with the camera.
#[derive(Debug, Component)]
pub struct Headlamp;

/// Query filter for the camera transform.
type CameraFilter = (With<Camera3d>, Without<LocalPlayer>, Without<Headlamp>);

/// Query filter for the local player's transform.
type PlayerFilter = (With<LocalPlayer>, Without<Camera3d>, Without<Headlamp>);

/// A simple body mesh for a replicated player, so teammates are visible.
#[derive(Debug, Component)]
pub struct PlayerAvatar;

/// Spawns a body mesh for every replicated player that does not have one yet.
fn spawn_player_avatars(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    players: Query<
        Entity,
        (
            With<deepcore_shared::protocol::PlayerName>,
            Without<PlayerAvatar>,
        ),
    >,
) {
    for entity in &players {
        let body = meshes.add(Capsule3d::new(0.3, 1.2));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.62, 0.30),
            perceptual_roughness: 0.7,
            ..default()
        });
        commands.entity(entity).insert((
            Mesh3d(body),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, 0.9, 0.0),
            PlayerAvatar,
        ));
    }
}

/// Spawns the camera, the sun and the ambient light.
fn setup_scene(mut commands: Commands, content: Res<ContentRegistry>, config: Res<ClientConfig>) {
    let spawn = deepcore_shared::worldgen::spawn_position(&content);
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(spawn.x as f32, spawn.y as f32 + 6.0, spawn.z as f32 + 10.0)
            .looking_at(
                Vec3::new(spawn.x as f32, spawn.y as f32, spawn.z as f32),
                Vec3::Y,
            ),
        Projection::Perspective(PerspectiveProjection {
            fov: 75f32.to_radians(),
            far: 900.0,
            ..default()
        }),
        Msaa::Sample4,
        DistanceFog {
            color: Color::srgba(0.35, 0.40, 0.48, 1.0),
            falloff: FogFalloff::Linear {
                start: 40.0,
                end: (config.render_distance as f32) * 32.0,
            },
            ..default()
        },
    ));
    // The player's headlamp: a spotlight parented to the camera, so the mine is navigable
    // without placed lamps. Placed lamps add their own point lights.
    commands.spawn((
        SpotLight {
            // A headlamp, not a floodlight: bright enough to read a tunnel, not to blow out
            // the wall half a metre away.
            intensity: 320_000.0,
            range: 48.0,
            outer_angle: 0.70,
            inner_angle: 0.30,
            color: Color::srgb(1.0, 0.96, 0.86),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        Headlamp,
    ));
    // Sun.
    commands.spawn((
        DirectionalLight {
            // Daylight on the surface; the mine below is lit by the headlamp and placed lamps.
            illuminance: 22_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 200.0, 0.0).looking_at(Vec3::new(-60.0, 0.0, -40.0), Vec3::Y),
    ));
    // Ambient light is a component in Bevy 0.20; one entity drives the whole scene.
    commands.spawn(AmbientLight {
        color: Color::srgb(0.55, 0.60, 0.72),
        brightness: 1_600.0,
        ..default()
    });
    info!("scene ready: {} blocks of content", content.blocks.len());
}

/// Keeps the camera on the local player's eyes.
fn follow_local_player(
    session: Res<net::SessionState>,
    input: Res<input::LocalInput>,
    mut camera: Query<&mut Transform, CameraFilter>,
    mut lamp: Query<&mut Transform, (With<Headlamp>, Without<Camera3d>)>,
    players: Query<(&Transform, &deepcore_shared::protocol::NetworkRotation), PlayerFilter>,
) {
    let _ = &session;
    let Ok((player, rotation)) = players.single() else {
        return;
    };
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    transform.translation = player.translation + Vec3::Y * deepcore_shared::movement::EYE_HEIGHT;
    // The camera looks where the mouse points, using the predicted input rather than the
    // replicated rotation, so looking around is never delayed by the network.
    transform.rotation = Quat::from_euler(EulerRot::YXZ, input.yaw, -input.pitch, 0.0);
    if let Ok(mut lamp_transform) = lamp.single_mut() {
        lamp_transform.translation = transform.translation;
        lamp_transform.rotation = transform.rotation;
    }
    let _ = rotation;
}

/// Ages HUD notifications out.
fn decay_notifications(time: Res<Time>, mut session: ResMut<net::SessionState>) {
    for (_, _, age) in session.notifications.iter_mut() {
        *age += time.delta_secs();
    }
    session.notifications.retain(|(_, _, age)| *age < 6.0);
}
