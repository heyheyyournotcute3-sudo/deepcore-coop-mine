//! Player movement: a deterministic AABB-versus-voxel controller shared by both sides.
//!
//! The client predicts with this code and the server replays the same inputs, so the two must
//! agree exactly. Rules for that:
//!
//! * Fixed timestep ([`MOVE_DT`]); never use the frame delta.
//! * No randomness, no floating-point accumulation across systems.
//! * Inputs are quantised (`i8` axes) before they reach the controller.
//!
//! The player is an axis-aligned box `0.6 × 1.8 × 0.6` with its origin at the feet. Collision
//! is resolved axis by axis with a small skin, which handles stairs and slopes of blocks
//! without a physics engine.

use bevy::prelude::*;

/// Fixed simulation step for movement, in seconds (60 Hz).
pub const MOVE_DT: f32 = 1.0 / 60.0;

/// Player half-width.
pub const PLAYER_HALF_WIDTH: f32 = 0.3;

/// Player height.
pub const PLAYER_HEIGHT: f32 = 1.8;

/// Eye height above the feet.
pub const EYE_HEIGHT: f32 = 1.62;

/// Gap kept between the player box and a block face, so resting contact is stable.
const SKIN: f32 = 0.001;

/// Gravity in blocks per second squared.
pub const GRAVITY: f32 = 26.0;

/// Terminal fall speed.
pub const TERMINAL_SPEED: f32 = 55.0;

/// Walk speed.
pub const WALK_SPEED: f32 = 4.6;

/// Sprint speed.
pub const SPRINT_SPEED: f32 = 7.2;

/// Crouch speed.
pub const CROUCH_SPEED: f32 = 1.9;

/// Climb speed while on a ladder.
pub const CLIMB_SPEED: f32 = 2.8;

/// Upward speed of a jump.
pub const JUMP_SPEED: f32 = 8.6;

/// The maximum step up a player can walk onto without jumping.
pub const STEP_HEIGHT: f32 = 0.55;

/// The movement model parameters, so a test can vary them.
#[derive(Debug, Clone, Copy)]
pub struct MovementConfig {
    /// Walk speed.
    pub walk: f32,
    /// Sprint speed.
    pub sprint: f32,
    /// Crouch speed.
    pub crouch: f32,
    /// Gravity.
    pub gravity: f32,
    /// Jump impulse.
    pub jump: f32,
    /// Whether the player is allowed to fly (creative/debug only).
    pub fly: bool,
}

impl Default for MovementConfig {
    fn default() -> Self {
        Self {
            walk: WALK_SPEED,
            sprint: SPRINT_SPEED,
            crouch: CROUCH_SPEED,
            gravity: GRAVITY,
            jump: JUMP_SPEED,
            fly: false,
        }
    }
}

/// One frame of player input, quantised so client and server agree bit-for-bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Message, serde::Serialize, serde::Deserialize)]
pub struct PlayerInput {
    /// Sequence number, echoed back by the server for reconciliation.
    pub seq: u32,
    /// Forward axis, -127..=127.
    pub forward: i8,
    /// Right axis, -127..=127.
    pub right: i8,
    /// Jump held.
    pub jump: bool,
    /// Sprint held.
    pub sprint: bool,
    /// Crouch held.
    pub crouch: bool,
    /// Yaw in quantised radians (1/1000 rad).
    pub yaw: i32,
    /// Pitch in quantised radians (1/1000 rad).
    pub pitch: i32,
}

impl PlayerInput {
    /// Yaw in radians.
    pub fn yaw_radians(self) -> f32 {
        self.yaw as f32 / 1000.0
    }

    /// Pitch in radians.
    pub fn pitch_radians(self) -> f32 {
        self.pitch as f32 / 1000.0
    }

    /// Horizontal movement intent as a unit-ish vector in world space.
    pub fn wish_dir(self) -> Vec3 {
        let forward = self.forward as f32 / 127.0;
        let right = self.right as f32 / 127.0;
        let yaw = self.yaw_radians();
        let (sin, cos) = yaw.sin_cos();
        // -Z is forward when yaw is zero.
        let dir = Vec3::new(-sin * forward + cos * right, 0.0, -cos * forward - sin * right);
        if dir.length_squared() > 1.0 {
            dir.normalize()
        } else {
            dir
        }
    }
}

/// The mutable state of a player body, as the movement code sees it.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct BodyState {
    /// Feet position.
    pub position: Vec3,
    /// Velocity in blocks per second.
    pub velocity: Vec3,
    /// The body is resting on a solid block.
    pub on_ground: bool,
    /// The body is inside a ladder volume.
    pub on_ladder: bool,
    /// The body is inside a fluid.
    pub in_fluid: bool,
}

/// Block query used by the collision code.
pub trait BlockQuery {
    /// Whether the block at `pos` blocks movement.
    fn is_solid(&self, pos: crate::voxel::BlockPos) -> bool;
    /// Whether the block at `pos` is a climbable ladder.
    fn is_ladder(&self, pos: crate::voxel::BlockPos) -> bool;
    /// Whether the block at `pos` is a fluid.
    fn is_fluid(&self, pos: crate::voxel::BlockPos) -> bool;
}

/// Steps the body one fixed tick.
///
/// Returns the body state after the step; the caller stores it.
pub fn step_body(
    state: &mut BodyState,
    input: PlayerInput,
    world: &impl BlockQuery,
    config: &MovementConfig,
) {
    let wish = input.wish_dir();
    let speed = if input.crouch {
        config.crouch
    } else if input.sprint {
        config.sprint
    } else {
        config.walk
    };

    state.on_ladder = body_overlaps(state.position, world, |pos| world.is_ladder(pos));
    state.in_fluid = body_overlaps(state.position, world, |pos| world.is_fluid(pos));

    // Horizontal acceleration: snappy on the ground, floaty in the air.
    let accel = if state.on_ground { 60.0 } else { 18.0 };
    let target = wish * speed;
    state.velocity.x = approach(state.velocity.x, target.x, accel * MOVE_DT);
    state.velocity.z = approach(state.velocity.z, target.z, accel * MOVE_DT);

    // Vertical.
    if config.fly {
        let vertical = if input.jump {
            1.0
        } else if input.crouch {
            -1.0
        } else {
            0.0
        };
        state.velocity.y = vertical * speed;
    } else if state.on_ladder {
        state.velocity.y = if input.jump {
            CLIMB_SPEED
        } else if input.crouch {
            -CLIMB_SPEED
        } else {
            0.0
        };
    } else if state.in_fluid {
        state.velocity.y = approach(state.velocity.y, if input.jump { 3.4 } else { -1.8 }, 30.0 * MOVE_DT);
    } else {
        state.velocity.y -= config.gravity * MOVE_DT;
        state.velocity.y = state.velocity.y.max(-TERMINAL_SPEED);
        if input.jump && state.on_ground {
            state.velocity.y = config.jump;
            state.on_ground = false;
        }
    }

    // Integrate, axis by axis, so a blocked axis does not cancel the others.
    move_axis(state, world, Vec3::X);
    move_axis(state, world, Vec3::Y);
    move_axis(state, world, Vec3::Z);

    // Ground probe: a hair below the feet.
    state.on_ground = state.velocity.y <= 0.0001
        && collides(
            world,
            state.position - Vec3::new(0.0, 0.02, 0.0),
        );

    if state.on_ladder {
        state.on_ground = true;
    }
}

fn move_axis(state: &mut BodyState, world: &impl BlockQuery, axis: Vec3) {
    let delta = state.velocity.dot(axis) * MOVE_DT;
    if delta == 0.0 {
        return;
    }
    let step = axis * delta;
    let candidate = state.position + step;
    if !collides(world, candidate) {
        state.position = candidate;
        return;
    }

    // Blocked. On a horizontal axis, try stepping up onto a ledge of at most `STEP_HEIGHT`.
    if axis != Vec3::Y {
        let raised = state.position + Vec3::Y * STEP_HEIGHT;
        let stepped = raised + step;
        if !collides(world, raised) && !collides(world, stepped) {
            state.position = stepped;
            return;
        }
    }

    // Otherwise stop on this axis only; the other axes still move.
    if axis == Vec3::Y {
        state.velocity.y = 0.0;
    } else if axis == Vec3::X {
        state.velocity.x = 0.0;
    } else {
        state.velocity.z = 0.0;
    }
}

fn approach(current: f32, target: f32, max_delta: f32) -> f32 {
    if current < target {
        (current + max_delta).min(target)
    } else {
        (current - max_delta).max(target)
    }
}

/// Whether the player box at `feet` intersects any solid block.
pub fn collides(world: &impl BlockQuery, feet: Vec3) -> bool {
    let min = feet + Vec3::new(-PLAYER_HALF_WIDTH + SKIN, SKIN, -PLAYER_HALF_WIDTH + SKIN);
    let max = feet
        + Vec3::new(
            PLAYER_HALF_WIDTH - SKIN,
            PLAYER_HEIGHT - SKIN,
            PLAYER_HALF_WIDTH - SKIN,
        );
    let start = crate::voxel::BlockPos::new(
        min.x.floor() as i32,
        min.y.floor() as i32,
        min.z.floor() as i32,
    );
    let end = crate::voxel::BlockPos::new(
        max.x.floor() as i32,
        max.y.floor() as i32,
        max.z.floor() as i32,
    );
    for y in start.y..=end.y {
        for z in start.z..=end.z {
            for x in start.x..=end.x {
                if world.is_solid(crate::voxel::BlockPos::new(x, y, z)) {
                    return true;
                }
            }
        }
    }
    false
}

fn body_overlaps(
    feet: Vec3,
    world: &impl BlockQuery,
    predicate: impl Fn(crate::voxel::BlockPos) -> bool,
) -> bool {
    let start = crate::voxel::BlockPos::new(
        (feet.x - PLAYER_HALF_WIDTH).floor() as i32,
        feet.y.floor() as i32,
        (feet.z - PLAYER_HALF_WIDTH).floor() as i32,
    );
    let end = crate::voxel::BlockPos::new(
        (feet.x + PLAYER_HALF_WIDTH).floor() as i32,
        (feet.y + PLAYER_HEIGHT).floor() as i32,
        (feet.z + PLAYER_HALF_WIDTH).floor() as i32,
    );
    let _ = world;
    for y in start.y..=end.y {
        for z in start.z..=end.z {
            for x in start.x..=end.x {
                if predicate(crate::voxel::BlockPos::new(x, y, z)) {
                    return true;
                }
            }
        }
    }
    false
}

/// Registers movement systems that are shared by both binaries.
pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, _app: &mut App) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::BlockPos;

    struct FlatWorld {
        floor_y: i32,
    }

    impl BlockQuery for FlatWorld {
        fn is_solid(&self, pos: BlockPos) -> bool {
            pos.y <= self.floor_y
        }
        fn is_ladder(&self, _pos: BlockPos) -> bool {
            false
        }
        fn is_fluid(&self, _pos: BlockPos) -> bool {
            false
        }
    }

    fn input(forward: i8, jump: bool) -> PlayerInput {
        PlayerInput {
            forward,
            jump,
            ..Default::default()
        }
    }

    #[test]
    fn falls_until_it_lands() {
        let world = FlatWorld { floor_y: 0 };
        let mut body = BodyState {
            position: Vec3::new(0.5, 10.0, 0.5),
            ..Default::default()
        };
        let config = MovementConfig::default();
        for _ in 0..240 {
            step_body(&mut body, PlayerInput::default(), &world, &config);
        }
        assert!(body.on_ground, "body should have landed");
        assert!((body.position.y - 1.0).abs() < 0.05, "y = {}", body.position.y);
    }

    #[test]
    fn walks_forward_at_walk_speed() {
        let world = FlatWorld { floor_y: 0 };
        let mut body = BodyState {
            position: Vec3::new(0.5, 1.0, 0.5),
            ..Default::default()
        };
        let config = MovementConfig::default();
        for _ in 0..60 {
            step_body(&mut body, input(127, false), &world, &config);
        }
        // One second of walking: roughly walk speed, along -Z.
        let travelled = (body.position - Vec3::new(0.5, 1.0, 0.5)).length();
        assert!(
            (travelled - WALK_SPEED).abs() < 1.2,
            "travelled {travelled} in one second"
        );
        assert!(body.position.z < 0.5);
    }

    #[test]
    fn stops_at_a_wall() {
        struct WallWorld;
        impl BlockQuery for WallWorld {
            fn is_solid(&self, pos: BlockPos) -> bool {
                pos.y <= 0 || pos.z <= -3
            }
            fn is_ladder(&self, _pos: BlockPos) -> bool {
                false
            }
            fn is_fluid(&self, _pos: BlockPos) -> bool {
                false
            }
        }
        let mut body = BodyState {
            position: Vec3::new(0.5, 1.0, 0.5),
            ..Default::default()
        };
        let config = MovementConfig::default();
        for _ in 0..180 {
            step_body(&mut body, input(127, false), &WallWorld, &config);
        }
        assert!(body.position.z > -3.0, "walked through the wall: {body:?}");
    }

    #[test]
    fn jump_leaves_the_ground() {
        let world = FlatWorld { floor_y: 0 };
        let mut body = BodyState {
            position: Vec3::new(0.5, 1.0, 0.5),
            ..Default::default()
        };
        let config = MovementConfig::default();
        step_body(&mut body, PlayerInput::default(), &world, &config);
        assert!(body.on_ground);
        step_body(&mut body, input(0, true), &world, &config);
        assert!(body.velocity.y > 0.0);
        let mut peak = body.position.y;
        for _ in 0..60 {
            step_body(&mut body, PlayerInput::default(), &world, &config);
            peak = peak.max(body.position.y);
        }
        assert!(peak > 2.0, "jump only reached {peak}");
    }

    #[test]
    fn prediction_is_deterministic() {
        let world = FlatWorld { floor_y: 0 };
        let config = MovementConfig::default();
        let mut a = BodyState {
            position: Vec3::new(0.5, 5.0, 0.5),
            ..Default::default()
        };
        let mut b = a;
        for tick in 0..120 {
            let i = input(127, tick % 20 == 0);
            step_body(&mut a, i, &world, &config);
            step_body(&mut b, i, &world, &config);
        }
        assert_eq!(a, b);
    }
}
