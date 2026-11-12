//! `deepcore-shared` — every rule of Deepcore.
//!
//! This crate is pure game logic: voxel storage, world generation, movement, inventory,
//! crafting, machines, power, hazards and the network protocol. It contains no rendering and
//! no sockets, so the same code runs on the authoritative server and on the client.

pub mod content;
pub mod crafting;
pub mod hazards;
pub mod inventory;
pub mod movement;
pub mod power;
pub mod protocol;
pub mod voxel;
pub mod worldgen;

use bevy::prelude::*;

/// Registers every shared, engine-side plugin. Called by both binaries.
pub struct SharedPlugin;

impl Plugin for SharedPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            content::ContentPlugin,
            voxel::VoxelPlugin,
            worldgen::WorldgenPlugin,
            movement::MovementPlugin,
            inventory::InventoryPlugin,
            crafting::CraftingPlugin,
            power::PowerPlugin,
            hazards::HazardsPlugin,
        ));
    }
}

/// Deterministic 32-bit hash of a byte slice, used for content hashing and ids.
///
/// FNV-1a: stable across platforms and Rust versions, which matters because the hash is part
/// of the network handshake.
pub fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::stable_hash;

    #[test]
    fn hash_is_stable_and_different() {
        assert_eq!(stable_hash(b"deepcore"), stable_hash(b"deepcore"));
        assert_ne!(stable_hash(b"deepcore"), stable_hash(b"deepcorf"));
    }
}
