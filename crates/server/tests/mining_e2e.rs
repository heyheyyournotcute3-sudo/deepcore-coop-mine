//! End-to-end intent tests: a client intent travels through the real server systems and the
//! world changes.
//!
//! This is the closest thing to "a player clicked and a block disappeared" that runs without a
//! window: the intent is written as a message, `PreUpdate` handlers process it, `FixedUpdate`
//! advances the mining timer, and the assertions check the block, the drops and the broadcast.

use bevy::ecs::message::Messages;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_replicon::prelude::{ClientId, FromClient, Replicated, ToClients};
use deepcore_shared::SharedPlugin;
use deepcore_shared::content::{ContentDir, ContentPlugin, ContentRegistry};
use deepcore_shared::inventory::ItemStack;
use deepcore_shared::protocol::{
    BlockChanges, ClientIntent, NetworkPosition, PlayerName, ProtocolPlugin,
};
use deepcore_shared::voxel::{BlockFace, BlockPos, LoadedChunks};
use deepcore_shared::worldgen::{WorldGenerator, spawn_position};

use deepcore_server::ServerArgs;
use deepcore_server::ServerConfigResource;
use deepcore_server::crafting_state::{TechStateDirty, TechStateResource};
use deepcore_server::sim::SimPlugin;
use deepcore_server::sim::machine::{BlockEntityIndex, MachinePowerDemand};
use deepcore_server::sim::mining::PendingBlockChanges;
use deepcore_server::sim::player::{HandshakedPlayer, Player, PlayerState};
use deepcore_server::world::WorldManager;
use deepcore_shared::power::PowerNetworks;

/// A headless app with the whole server simulation and replicon's shared half.
fn app_with_simulation(seed: u64) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin));
    app.insert_resource(Time::<Fixed>::from_hz(30.0));
    let content_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
    app.insert_resource(ContentDir(content_dir.clone()));
    app.add_plugins(ContentPlugin);
    // Replicon's shared half registers channels and replication rules without needing a
    // transport; the server half would try to drain packets from a renet server.
    app.add_plugins(bevy_replicon::shared::RepliconSharedPlugin::default());
    app.add_plugins((SharedPlugin, ProtocolPlugin));
    app.insert_resource(ServerConfigResource(ServerArgs {
        port: 0,
        seed,
        content: content_dir.to_string_lossy().into_owned(),
        save_dir: "target/test-saves".to_owned(),
        simulate_latency: 0,
        simulate_loss: 0,
        bots: 0,
        name: "test".to_owned(),
        lobby: String::new(),
        require_account: false,
        public_host: None,
        run_seconds: 0,
    }));
    app.init_resource::<WorldManager>();
    app.init_resource::<PendingBlockChanges>();
    app.init_resource::<BlockEntityIndex>();
    app.init_resource::<MachinePowerDemand>();
    app.init_resource::<PowerNetworks>();
    app.init_resource::<TechStateResource>();
    app.init_resource::<TechStateDirty>();
    app.init_resource::<deepcore_server::sim::objectives::ObjectiveState>();
    app.init_resource::<deepcore_server::sim::logistics::Conveyors>();
    app.init_resource::<deepcore_shared::inventory::PendingInventoryUpdates>();
    app.add_plugins(SimPlugin);
    // The intent handlers run only while the server is in `Running`; a headless test has no
    // transport to drive the state machine, so put the app in that state directly.
    app.insert_resource(bevy::state::state::State::new(
        bevy_replicon::prelude::ServerState::Running,
    ));
    app
}

/// Spawns a player wired the way the handshake does it, at a known position.
fn spawn_player(app: &mut App, position: BlockPos, name: &str) -> Entity {
    let content = app.world().resource::<ContentRegistry>().clone();
    let mut inventory = deepcore_shared::inventory::Inventory::default();
    let pickaxe = content.item_by_key("steel_pickaxe").expect("pickaxe").id;
    inventory.hotbar[0] = ItemStack::new(pickaxe, 1);
    let body = deepcore_shared::movement::BodyState {
        position: position.center(),
        on_ground: true,
        ..Default::default()
    };
    let client = app.world_mut().spawn_empty().id();
    let player = app
        .world_mut()
        .spawn((
            Player,
            PlayerName(name.to_owned()),
            NetworkPosition {
                position: body.position,
                last_seq: 0,
            },
            PlayerState {
                body,
                inventory,
                gear: Default::default(),
                vitals: Default::default(),
                last_seq: 0,
                input_queue: Vec::new(),
                tool_charge: 0,
                dead: false,
                death_crate: None,
                fall_start_y: None,
            },
            deepcore_server::net::OwnedByPlayer(client),
            Replicated,
        ))
        .id();
    app.world_mut().entity_mut(client).insert((
        HandshakedPlayer(player),
        deepcore_server::sim::player::OwnsPlayer(player),
    ));
    player
}

/// Sends an intent as if it came from the player's client.
fn send_intent(app: &mut App, player: Entity, message: ClientIntent) {
    let client = app
        .world()
        .get::<deepcore_server::net::OwnedByPlayer>(player)
        .expect("owner")
        .0;
    app.world_mut()
        .resource_mut::<Messages<FromClient<ClientIntent>>>()
        .write(FromClient {
            client_id: ClientId::Client(client),
            message,
        });
}

/// Loads the chunks around a position so the world can be read and written there.
fn load_around(app: &mut App, centre: BlockPos, radius: i32) {
    let content = app.world().resource::<ContentRegistry>().clone();
    let seed = app.world().resource::<ServerConfigResource>().0.seed;
    let generator = WorldGenerator::new(seed);
    let centre_chunk = centre.chunk();
    let mut chunks = app
        .world_mut()
        .remove_resource::<LoadedChunks>()
        .unwrap_or_default();
    for dy in -radius..=radius {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = centre_chunk.offset(dx, dy, dz);
                if pos.in_bounds() && !chunks.contains(pos) {
                    chunks.insert(pos, generator.generate_chunk(pos, &content));
                }
            }
        }
    }
    app.world_mut().insert_resource(chunks);
}

/// Writes a block and records the edit, as a real world change would.
fn set_block(app: &mut App, pos: BlockPos, block: u16) {
    let mut chunks = app
        .world_mut()
        .remove_resource::<LoadedChunks>()
        .unwrap_or_default();
    chunks.set_block(pos, block);
    app.world_mut().insert_resource(chunks);
    let mut manager = app
        .world_mut()
        .remove_resource::<WorldManager>()
        .unwrap_or_default();
    manager.record_edit(pos, block);
    app.world_mut().insert_resource(manager);
}

/// Runs one frame: `Update` (which handles intents) and then exactly one fixed tick.
///
/// A headless app's real delta is ~0, so `FixedUpdate` would never run on its own; running
/// `FixedMain` directly is deterministic and instant, and is exactly what the schedule does when
/// the accumulator crosses the timestep.
fn step(app: &mut App) {
    app.update();
    app.world_mut()
        .run_schedule(bevy::app::FixedMain);
}

/// The block at a position.
fn block_at(app: &App, pos: BlockPos) -> u16 {
    app.world()
        .resource::<LoadedChunks>()
        .block(pos)
        .unwrap_or(u16::MAX)
}

#[test]
fn a_start_mining_intent_breaks_a_block_and_grants_the_drop() {
    let mut app = app_with_simulation(11);
    let content = app.world().resource::<ContentRegistry>().clone();
    let spawn = spawn_position(&content);
    load_around(&mut app, spawn, 1);
    let player = spawn_player(&mut app, spawn, "Miner");

    let target = spawn.offset(0, -1, 0);
    let dirt = content.block_by_key("dirt").expect("dirt").id;
    let dirt_item = content.item_by_key("dirt").expect("dirt item").id;
    set_block(&mut app, target, dirt.0);
    assert_eq!(block_at(&app, target), dirt.0);

    send_intent(
        &mut app,
        player,
        ClientIntent::StartMining {
            pos: target,
            face: BlockFace::Up,
        },
    );

    // Dirt has hardness 0.5: 15 ticks with a tier-1 pickaxe at 30 Hz. Run well past that.
    for _ in 0..90 {
        step(&mut app);
    }

    assert_eq!(
        block_at(&app, target),
        content.air.0,
        "the block was not removed"
    );
    let state = app.world().get::<PlayerState>(player).expect("state");
    assert!(
        state.inventory.count_of(dirt_item) >= 1,
        "the drop was not granted"
    );
}

#[test]
fn a_too_hard_intent_does_not_break_the_block() {
    let mut app = app_with_simulation(12);
    let content = app.world().resource::<ContentRegistry>().clone();
    let spawn = spawn_position(&content);
    load_around(&mut app, spawn, 1);
    let player = spawn_player(&mut app, spawn, "Miner");
    let granite = content.block_by_key("granite").expect("granite").id;
    let target = spawn.offset(0, -1, 0);
    set_block(&mut app, target, granite.0);

    send_intent(
        &mut app,
        player,
        ClientIntent::StartMining {
            pos: target,
            face: BlockFace::Up,
        },
    );
    for _ in 0..60 {
        step(&mut app);
    }

    assert_eq!(
        block_at(&app, target),
        granite.0,
        "a tier-1 pickaxe broke granite"
    );
}

#[test]
fn a_place_block_intent_puts_a_block_in_the_world() {
    let mut app = app_with_simulation(13);
    let content = app.world().resource::<ContentRegistry>().clone();
    let spawn = spawn_position(&content);
    load_around(&mut app, spawn, 1);
    let player = spawn_player(&mut app, spawn, "Miner");

    let dirt_item = content.item_by_key("dirt").expect("dirt item").id;
    {
        let mut state = app
            .world_mut()
            .get_mut::<PlayerState>(player)
            .expect("state");
        state.inventory.hotbar[1] = ItemStack::new(dirt_item, 5);
    }

    let ground = spawn.offset(2, -1, 0);
    send_intent(
        &mut app,
        player,
        ClientIntent::PlaceBlock {
            pos: ground,
            face: BlockFace::Up,
            slot: 1,
            facing: 0,
        },
    );
    for _ in 0..30 {
        step(&mut app);
    }

    let placed = spawn.offset(2, 0, 0);
    let dirt_block = content.block_by_key("dirt").expect("dirt").id;
    assert_eq!(block_at(&app, placed), dirt_block.0, "no block was placed");
    let state = app.world().get::<PlayerState>(player).expect("state");
    assert_eq!(
        state.inventory.count_of(dirt_item),
        4,
        "the placed block was not consumed"
    );
}

#[test]
fn mining_out_of_reach_is_refused() {
    let mut app = app_with_simulation(15);
    let content = app.world().resource::<ContentRegistry>().clone();
    let spawn = spawn_position(&content);
    load_around(&mut app, spawn, 2);
    let player = spawn_player(&mut app, spawn, "Miner");
    let dirt = content.block_by_key("dirt").expect("dirt").id;
    // 12 blocks away is well past the 5-block reach.
    let target = spawn.offset(12, 0, 0);
    set_block(&mut app, target, dirt.0);
    send_intent(
        &mut app,
        player,
        ClientIntent::StartMining {
            pos: target,
            face: BlockFace::Up,
        },
    );
    for _ in 0..90 {
        step(&mut app);
    }
    assert_eq!(
        block_at(&app, target),
        dirt.0,
        "an out-of-reach block was mined"
    );
}

#[test]
fn block_changes_reach_the_client_message_queue() {
    let mut app = app_with_simulation(16);
    let content = app.world().resource::<ContentRegistry>().clone();
    let spawn = spawn_position(&content);
    load_around(&mut app, spawn, 1);
    let player = spawn_player(&mut app, spawn, "Miner");
    let dirt = content.block_by_key("dirt").expect("dirt").id;
    let target = spawn.offset(0, -1, 0);
    set_block(&mut app, target, dirt.0);

    send_intent(
        &mut app,
        player,
        ClientIntent::StartMining {
            pos: target,
            face: BlockFace::Up,
        },
    );
    // Run until the block is gone, then one more tick so the flush system sends it.
    let mut removed = false;
    for _ in 0..120 {
        step(&mut app);
        if block_at(&app, target) == content.air.0 {
            removed = true;
            step(&mut app);
            break;
        }
    }
    assert!(removed, "the block was never mined");

    // The queued change names the mined block.
    let messages = app.world().resource::<Messages<ToClients<BlockChanges>>>();
    let mut cursor = messages.get_cursor();
    let mut found = false;
    for message in cursor.read(messages) {
        if message
            .message
            .changes
            .iter()
            .any(|(pos, block)| *pos == target && *block == content.air.0)
        {
            found = true;
        }
    }
    assert!(found, "the block change was never broadcast");
}
