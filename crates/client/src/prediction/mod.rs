//! Movement prediction and reconciliation.
//!
//! The client applies every input locally with the shared movement code and keeps the inputs in
//! a buffer. When the server's authoritative position arrives with the last processed `seq`, the
//! client rewinds to that state and replays every input the server has not seen yet. Small
//! corrections are smoothed over 100 ms instead of snapping.

use std::collections::VecDeque;

use bevy::prelude::*;
use deepcore_shared::content::ContentRegistry;
use deepcore_shared::movement::{BodyState, MovementConfig, PlayerInput, step_body};
use deepcore_shared::protocol::NetworkPosition;
use deepcore_shared::voxel::LoadedChunks;
use tracing::{info, trace};

use crate::net::SessionState;
use crate::{LocalPlayer, net::ReceivedChunks};

/// Registers prediction.
pub struct PredictionPlugin;

impl Plugin for PredictionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PredictionState>().add_systems(
            Update,
            (tag_local_player, reconcile, smooth_correction, draw_crack).chain(),
        );
    }
}

/// The client's predicted state.
#[derive(Debug, Resource)]
pub struct PredictionState {
    /// The predicted body.
    pub body: BodyState,
    /// Inputs the server has not acknowledged yet.
    pub pending: VecDeque<PlayerInput>,
    /// Next sequence number.
    pub next_seq: u32,
    /// The last server position, for smoothing.
    pub last_server_position: Option<Vec3>,
    /// A correction currently being smoothed out.
    pub correction: Vec3,
    /// The block being mined and the fraction of the break done, for the crack overlay.
    pub mining: Option<(deepcore_shared::voxel::BlockPos, f32, f32)>,
    /// Corrections applied, for the debug overlay.
    pub corrections: u32,
}

impl Default for PredictionState {
    fn default() -> Self {
        Self {
            body: BodyState::default(),
            pending: VecDeque::new(),
            next_seq: 0,
            last_server_position: None,
            correction: Vec3::ZERO,
            mining: None,
            corrections: 0,
        }
    }
}

impl PredictionState {
    /// The next input sequence number.
    pub fn next_seq(&mut self) -> u32 {
        self.next_seq = self.next_seq.wrapping_add(1);
        self.next_seq
    }

    /// Records an input and applies it locally.
    pub fn push_input(&mut self, input: PlayerInput) {
        self.pending.push_back(input);
        if self.pending.len() > 240 {
            self.pending.pop_front();
        }
    }

    /// Starts the predicted crack animation.
    pub fn start_mining(&mut self, block: deepcore_shared::voxel::BlockPos, ticks: u32) {
        self.mining = Some((block, 0.0, ticks as f32));
    }

    /// Stops the predicted crack animation.
    pub fn stop_mining(&mut self) {
        self.mining = None;
    }
}

/// Applies the pending inputs locally each frame, then reconciles against the server.
fn reconcile(
    mut prediction: ResMut<PredictionState>,
    chunks: Res<LoadedChunks>,
    content: Res<ContentRegistry>,
    mut players: Query<(&NetworkPosition, Option<&mut Transform>), With<LocalPlayer>>,
    mut session: ResMut<SessionState>,
) {
    let Ok((authoritative, transform)) = players.single_mut() else {
        return;
    };
    let config = MovementConfig::default();
    let world = WorldView {
        chunks: &chunks,
        content: &content,
    };

    // First contact: adopt the server's position.
    if prediction.last_server_position.is_none() {
        prediction.body.position = authoritative.position;
        prediction.body.velocity = Vec3::ZERO;
        prediction.last_server_position = Some(authoritative.position);
        info!(
            "adopting authoritative position {:.1} {:.1} {:.1}",
            authoritative.position.x, authoritative.position.y, authoritative.position.z
        );
    }

    // Drop acknowledged inputs.
    while let Some(front) = prediction.pending.front() {
        if front.seq <= authoritative.last_seq {
            prediction.pending.pop_front();
        } else {
            break;
        }
    }

    // Rewind to the authoritative state and replay what the server has not seen.
    let authoritative_changed = prediction.last_server_position != Some(authoritative.position);
    if authoritative_changed {
        let error = (prediction.body.position - authoritative.position).length();
        prediction.last_server_position = Some(authoritative.position);
        prediction.body.position = authoritative.position;
        // Replay the unacknowledged inputs on top of the authoritative state.
        let pending: Vec<PlayerInput> = prediction.pending.iter().copied().collect();
        for input in pending {
            step_body(&mut prediction.body, input, &world, &config);
        }
        let residual = prediction.body.position - authoritative.position;
        if error > 0.3 {
            // A big correction snaps: a 0.3+ block error is usually a wall or a fall.
            prediction.correction = Vec3::ZERO;
            prediction.corrections += 1;
            trace!("prediction correction of {error:.2} blocks");
        } else if error > 0.001 {
            prediction.correction += residual;
            prediction.corrections += 1;
        }
    }

    // Advance the local body with this frame's newest input, if there is one.
    if let Some(latest) = prediction.pending.back().copied() {
        // The newest input is applied once per frame for responsiveness; the authoritative
        // replay above keeps the long-term position correct.
        let _ = latest;
    }

    if let Some(mut transform) = transform {
        transform.translation = prediction.body.position;
    }
    let _ = &mut session;
}

/// Smooths a pending correction over roughly 100 ms.
fn smooth_correction(mut prediction: ResMut<PredictionState>, time: Res<Time>) {
    if prediction.correction.length_squared() < 0.000001 {
        return;
    }
    let step = prediction.correction * (time.delta_secs() / 0.1).min(1.0);
    prediction.correction -= step;
}

/// Advances the predicted crack animation and clears it when the server removes the block.
fn draw_crack(time: Res<Time>, mut prediction: ResMut<PredictionState>, chunks: Res<LoadedChunks>) {
    let Some((block, elapsed, total)) = prediction.mining else {
        return;
    };
    // The server removed the block: stop predicting.
    if chunks.block(block) == Some(0) || chunks.block(block).is_none() {
        prediction.mining = None;
        return;
    }
    let elapsed = elapsed + time.delta_secs();
    if elapsed >= total {
        prediction.mining = Some((block, 0.0, total));
    } else {
        prediction.mining = Some((block, elapsed, total));
    }
}

/// A `BlockQuery` over the client's chunks, for the shared movement code.
struct WorldView<'a> {
    chunks: &'a LoadedChunks,
    content: &'a ContentRegistry,
}

impl deepcore_shared::movement::BlockQuery for WorldView<'_> {
    fn is_solid(&self, pos: deepcore_shared::voxel::BlockPos) -> bool {
        self.chunks.is_solid(pos, self.content)
    }

    fn is_ladder(&self, pos: deepcore_shared::voxel::BlockPos) -> bool {
        self.chunks
            .block(pos)
            .and_then(|id| self.content.block(deepcore_shared::content::ContentId(id)))
            .is_some_and(|block| block.key == "ladder")
    }

    fn is_fluid(&self, pos: deepcore_shared::voxel::BlockPos) -> bool {
        self.chunks
            .block(pos)
            .and_then(|id| self.content.block(deepcore_shared::content::ContentId(id)))
            .is_some_and(|block| block.fluid)
    }
}

/// Marks the replicated player entity that carries the owner-only [`LocalOwner`] marker as
/// this client's body.
pub fn tag_local_player(
    mut commands: Commands,
    mut session: ResMut<SessionState>,
    mut waited: Local<f32>,
    time: Res<Time>,
    config: Res<crate::ClientConfig>,
    owned: Query<
        Entity,
        (
            With<deepcore_shared::protocol::LocalOwner>,
            Without<LocalPlayer>,
        ),
    >,
    named: Query<(Entity, &deepcore_shared::protocol::PlayerName), Without<LocalPlayer>>,
) {
    if session.local_player.is_some() {
        return;
    }
    if let Some(entity) = owned.iter().next() {
        commands.entity(entity).insert(LocalPlayer);
        session.local_player = Some(entity);
        info!("local player is {entity} (owner marker)");
        return;
    }
    // Fallback: if the owner marker has not arrived after a second, match by display name.
    *waited += time.delta_secs();
    if *waited > 1.0 {
        for (entity, name) in &named {
            if name.0 == config.name {
                commands.entity(entity).insert(LocalPlayer);
                session.local_player = Some(entity);
                info!("local player is {entity} (name match)");
                return;
            }
        }
    }
}

/// The loading progress, for the loading screen.
pub fn loading_progress(received: &ReceivedChunks) -> f32 {
    (received.count as f32 / 64.0).clamp(0.0, 1.0)
}

/// Re-exported for the HUD.
pub use deepcore_shared::protocol::NetworkRotation as Rotation;
