//! The wire protocol: version, replicated components, client intents and server events.
//!
//! Everything that travels between client and server is declared here, so the shape of the
//! protocol is readable in one file. [`PROTOCOL_VERSION`] must be bumped whenever any message or
//! replicated component changes shape.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::content::ContentId;
use crate::hazards::{Vitals, WornGear};
use crate::inventory::ItemStack;
use crate::movement::PlayerInput;
use crate::voxel::{BlockFace, BlockPos, ChunkPos};

/// Protocol version. Bump on any change to a message or a replicated component.
pub const PROTOCOL_VERSION: u16 = 1;

/// Maximum players on one server.
pub const MAX_PLAYERS: usize = 16;

/// Default port for the game.
pub const DEFAULT_PORT: u16 = 24_288;

/// Default lobby port.
pub const DEFAULT_LOBBY_PORT: u16 = 24_289;

/// Registers the replicated components, messages and channels.
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // --- replicated components ---------------------------------------------------
        app.replicate::<NetworkPosition>()
            .replicate::<NetworkRotation>()
            .replicate::<NetworkHeld>()
            .replicate::<PlayerName>()
            .replicate::<PlayerVitals>()
            .replicate::<MachineReplica>()
            .replicate::<BlockEntityKind>();

        // --- client → server ---------------------------------------------------------
        app.add_client_message::<ClientIntent>(Channel::Ordered)
            .add_client_message::<PlayerInput>(Channel::Unordered)
            .add_client_message::<Handshake>(Channel::Ordered);

        // --- server → client ---------------------------------------------------------
        app.add_server_message::<ServerNotice>(Channel::Ordered)
            .add_server_message::<ChunkData>(Channel::Ordered)
            .add_server_message::<BlockChanges>(Channel::Ordered)
            .add_server_message::<InventoryUpdate>(Channel::Ordered)
            .add_server_message::<MachineScreen>(Channel::Ordered)
            .add_server_message::<TechState>(Channel::Ordered)
            .add_server_message::<ChatLine>(Channel::Ordered)
            .add_server_message::<MiningFeedback>(Channel::Unordered)
            .add_server_message::<NetworkStats>(Channel::Unordered)
            .add_server_message::<PlayerRoster>(Channel::Ordered);
    }
}

/// Server-authoritative player position.
#[derive(Debug, Clone, Copy, Component, Serialize, Deserialize, PartialEq)]
pub struct NetworkPosition {
    /// Feet position.
    pub position: Vec3,
    /// The last input sequence the server applied.
    pub last_seq: u32,
}

/// Server-authoritative player rotation.
#[derive(Debug, Clone, Copy, Component, Serialize, Deserialize, PartialEq, Default)]
pub struct NetworkRotation {
    /// Yaw in radians.
    pub yaw: f32,
    /// Pitch in radians.
    pub pitch: f32,
}

/// The item a player is holding, for other players' viewmodels.
#[derive(Debug, Clone, Copy, Component, Serialize, Deserialize, PartialEq, Default)]
pub struct NetworkHeld {
    /// Item id, or `ContentId::NONE`.
    pub item: ContentId,
}

/// A player's display name.
#[derive(Debug, Clone, Component, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerName(pub String);

/// Vitals replicated to the owning player only.
#[derive(Debug, Clone, Copy, Component, Serialize, Deserialize, PartialEq)]
pub struct PlayerVitals {
    /// The vitals.
    pub vitals: Vitals,
    /// Worn gear.
    pub gear: WornGear,
}

impl Default for PlayerVitals {
    fn default() -> Self {
        Self {
            vitals: Vitals::default(),
            gear: WornGear::default(),
        }
    }
}

/// A machine, replicated to everyone (coarse state only).
#[derive(Debug, Clone, Copy, Component, Serialize, Deserialize, PartialEq)]
pub struct MachineReplica {
    /// The machine id.
    pub machine: ContentId,
    /// The block position of the machine controller.
    pub position: BlockPos,
    /// Facing, 0..4 quarter turns.
    pub facing: u8,
    /// Whether it is running.
    pub running: bool,
    /// Progress 0..255, quantised.
    pub progress: u8,
    /// Idle reason, quantised to a small code.
    pub idle: u8,
}

/// The kind of block entity, for machines that are not machines (chests, lamps, rails).
#[derive(Debug, Clone, Copy, Component, Serialize, Deserialize, PartialEq)]
pub enum BlockEntityKind {
    /// A storage crate.
    Crate,
    /// A placed lamp.
    Lamp,
    /// A ladder segment (voxel, but tracked for interaction).
    Ladder,
    /// A conveyor.
    Conveyor,
    /// A rail segment.
    Rail,
    /// A machine.
    Machine,
    /// A multi-block controller.
    Multiblock,
}

/// Client → server intents. Every one is validated on the server.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub enum ClientIntent {
    /// Begin mining a block on a face.
    StartMining {
        /// Target block.
        pos: BlockPos,
        /// Target face.
        face: BlockFace,
    },
    /// Stop mining.
    StopMining,
    /// Place the held item against a face.
    PlaceBlock {
        /// Target block.
        pos: BlockPos,
        /// Target face.
        face: BlockFace,
        /// Slot the item comes from.
        slot: u8,
        /// Facing for the placed block, 0..4.
        facing: u8,
    },
    /// Interact with a block entity or item pickup.
    Interact {
        /// Target position.
        pos: BlockPos,
    },
    /// Move items between slots.
    MoveItems {
        /// Source slot encoding.
        from: SlotAddress,
        /// Destination slot encoding.
        to: SlotAddress,
        /// How many, or `None` for all.
        count: Option<u32>,
    },
    /// Craft at the workbench.
    Craft {
        /// Recipe id.
        recipe: ContentId,
        /// Number of batches.
        count: u32,
    },
    /// Choose which recipe a machine should run.
    SetMachineRecipe {
        /// Machine block position.
        machine: BlockPos,
        /// Recipe id, or `ContentId::NONE` for automatic.
        recipe: ContentId,
    },
    /// Deliver items to a research node.
    ResearchDeposit {
        /// Tech node.
        node: ContentId,
        /// Items offered.
        items: Vec<(ContentId, u32)>,
    },
    /// Chat.
    Chat {
        /// The message text.
        text: String,
    },
    /// Respawn after death.
    Respawn,
    /// Select a hotbar slot.
    SelectSlot {
        /// Slot 0..9.
        slot: u8,
    },
    /// Drop the held stack on the ground.
    DropHeld,
    /// Take the contents of a chest into the backpack.
    TakeAll {
        /// Chest position.
        pos: BlockPos,
    },
    /// Open a machine screen.
    OpenMachine {
        /// Machine position.
        pos: BlockPos,
    },
    /// Close the machine screen.
    CloseMachine,
    /// Swap an item into a gear slot.
    EquipGear {
        /// Backpack slot index.
        slot: u8,
    },
    /// Remove an item from a gear slot.
    UnequipGear {
        /// Which gear slot.
        slot: u8,
    },
}

/// A slot address that can cross the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotAddress {
    /// The container.
    pub container: ContainerId,
    /// The slot index inside the container.
    pub index: u8,
}

/// Which container a slot belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerId {
    /// The player's hotbar.
    Hotbar,
    /// The player's backpack.
    Backpack,
    /// The open machine's inputs.
    MachineInput,
    /// The open machine's outputs.
    MachineOutput,
    /// The open machine's fuel.
    MachineFuel,
    /// The open machine's modules.
    MachineModule,
    /// The open chest.
    Chest,
    /// A gear slot.
    Gear,
}

/// The handshake the client sends first.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct Handshake {
    /// Protocol version.
    pub protocol: u16,
    /// Content hash.
    pub content_hash: u64,
    /// Display name.
    pub name: String,
    /// Lobby auth token, empty when playing without the lobby.
    pub token: String,
}

/// Server → client notices: join, kick, content mismatch, notifications.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub enum ServerNotice {
    /// The handshake was accepted.
    Welcome {
        /// Your player entity.
        player: Entity,
        /// The world seed.
        seed: u64,
        /// Server tick rate.
        tick_hz: f32,
        /// Server name.
        server_name: String,
    },
    /// The handshake was refused.
    Refused {
        /// Human-readable reason.
        reason: String,
    },
    /// A general notification for the HUD.
    Notification {
        /// Text.
        text: String,
        /// Severity 0..2 (info, warning, error).
        severity: u8,
    },
}

/// Compressed chunk data.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct ChunkData {
    /// Chunk position.
    pub pos: ChunkPos,
    /// Zstd-compressed postcard payload of `(palette, bits, packed)`.
    pub payload: Vec<u8>,
    /// Whether this message unloads the chunk instead.
    pub unload: bool,
}

/// A batch of block changes for one tick.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct BlockChanges {
    /// `(position, block id)` pairs.
    pub changes: Vec<(BlockPos, u16)>,
}

/// An inventory update for the owning player.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct InventoryUpdate {
    /// Hotbar contents.
    pub hotbar: [ItemStack; 9],
    /// Backpack contents.
    pub backpack: Vec<ItemStack>,
    /// Selected slot.
    pub selected: u8,
    /// Worn gear.
    pub gear: WornGear,
}

/// A machine screen's contents, sent only while a player has it open.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct MachineScreen {
    /// Machine position.
    pub pos: BlockPos,
    /// Machine id.
    pub machine: ContentId,
    /// Inputs.
    pub inputs: Vec<ItemStack>,
    /// Outputs.
    pub outputs: Vec<ItemStack>,
    /// Fuel.
    pub fuel: Vec<ItemStack>,
    /// Modules.
    pub modules: Vec<ItemStack>,
    /// Progress 0..1.
    pub progress: f32,
    /// Idle reason text.
    pub idle: String,
    /// The running recipe, if any.
    pub recipe: Option<ContentId>,
    /// Whether the player may take from the outputs.
    pub accessible: bool,
}

/// The team's tech tree state.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct TechState {
    /// Researched nodes.
    pub researched: Vec<ContentId>,
    /// Partial deposits: `(node, item, count)`.
    pub deposits: Vec<(ContentId, ContentId, u32)>,
}

/// A chat line.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct ChatLine {
    /// Sender name, or empty for system messages.
    pub from: String,
    /// Text.
    pub text: String,
}

/// Feedback about a mining attempt, so the client can show "Too hard".
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct MiningFeedback {
    /// Target block.
    pub pos: BlockPos,
    /// Outcome code: 0 = ok, 1 = too hard, 2 = unbreakable, 3 = out of range, 4 = no line of sight.
    pub outcome: u8,
    /// Required tier for `outcome == 1`.
    pub required_tier: u8,
}

/// Power and steam network stats for the overlay, sent once per second.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct NetworkStats {
    /// Electric networks: `(id, supply kW, demand kW, stored kWh, capacity kWh)`.
    pub electric: Vec<(u32, f32, f32, f32, f32)>,
    /// Steam networks: `(id, supply, demand)`.
    pub steam: Vec<(u32, f32, f32)>,
}

/// The player roster, for the player list UI.
#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct PlayerRoster {
    /// `(name, health fraction, is_dead)` per player.
    pub players: Vec<(String, f32, bool)>,
}

/// Outcome codes used by [`MiningFeedback`].
pub mod mining_outcome {
    /// The block was mined.
    pub const OK: u8 = 0;
    /// The tool tier is too low.
    pub const TOO_HARD: u8 = 1;
    /// The block cannot be broken.
    pub const UNBREAKABLE: u8 = 2;
    /// The block is out of reach.
    pub const OUT_OF_RANGE: u8 = 3;
    /// The block is not visible from the player's eye.
    pub const NO_LINE_OF_SIGHT: u8 = 4;
    /// The block is not loaded.
    pub const NOT_LOADED: u8 = 5;
}

/// Encodes the idle reason as a small code for [`MachineReplica`].
pub fn idle_code(reason: crate::crafting::IdleReason) -> u8 {
    use crate::crafting::IdleReason;
    match reason {
        IdleReason::Idle => 0,
        IdleReason::NoRecipe => 1,
        IdleReason::OutputFull => 2,
        IdleReason::NoPower => 3,
        IdleReason::MissingInput(_) => 4,
        IdleReason::CascadeTooShort => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_through_postcard() {
        let intent = ClientIntent::PlaceBlock {
            pos: BlockPos::new(1, 2, 3),
            face: BlockFace::Up,
            slot: 4,
            facing: 2,
        };
        let bytes = postcard::to_allocvec(&intent).expect("serialises");
        let decoded: ClientIntent = postcard::from_bytes(&bytes).expect("deserialises");
        assert!(matches!(decoded, ClientIntent::PlaceBlock { slot: 4, .. }));
    }

    #[test]
    fn block_changes_round_trip() {
        let changes = BlockChanges {
            changes: vec![(BlockPos::new(-5, 100, 7), 12), (BlockPos::new(0, 0, 0), 0)],
        };
        let bytes = postcard::to_allocvec(&changes).unwrap();
        let decoded: BlockChanges = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.changes.len(), 2);
        assert_eq!(decoded.changes[0].1, 12);
    }

    #[test]
    fn chunk_payload_compresses_and_restores() {
        let chunk = crate::voxel::Chunk::filled(3);
        let raw = postcard::to_allocvec(&(
            chunk.palette().to_vec(),
            chunk.bits(),
            chunk.packed().to_vec(),
        ))
        .unwrap();
        let compressed = zstd::encode_all(&raw[..], 3).unwrap();
        assert!(compressed.len() < raw.len() + 64);
        let restored = zstd::decode_all(&compressed[..]).unwrap();
        let (palette, bits, packed): (Vec<u16>, u32, Vec<u8>) =
            postcard::from_bytes(&restored).unwrap();
        assert_eq!(palette, chunk.palette());
        assert_eq!(bits, chunk.bits());
        assert_eq!(packed, chunk.packed());
    }
}
