//! Headless integration tests: build a server `App` with no window, script intents, step the
//! schedule and assert what actually happened in the world.
//!
//! These tests are the safety net for the whole project. They run the real systems: mining,
//! inventory, crafting, machine ticks and the tech tree.

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use deepcore_shared::SharedPlugin;
use deepcore_shared::content::{ContentDir, ContentId, ContentPlugin, ContentRegistry};
use deepcore_shared::crafting::{MiningVerdict, TICK_SECONDS, ToolStats, evaluate_mining};
use deepcore_shared::inventory::ItemStack;
use deepcore_shared::protocol::NetworkPosition;
use deepcore_shared::voxel::{BlockPos, LoadedChunks};
use deepcore_shared::worldgen::{WorldGenerator, spawn_position};

use deepcore_server::crafting_state::TechStateResource;
use deepcore_server::sim::machine::{BlockEntityIndex, MachineEntity};
use deepcore_server::sim::mining::{PendingBlockChanges, break_block, validate_mining};
use deepcore_server::sim::player::{Player, PlayerState, spawn_player_for_client};
use deepcore_server::world::WorldManager;
use deepcore_server::{ServerArgs, ServerConfigResource};

/// Builds a headless server app with content, the shared rules and the world manager.
pub fn test_app(seed: u64) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin));
    app.insert_resource(Time::<Fixed>::from_hz(30.0));
    // Tests run with the crate directory as the working directory, so resolve the content
    // relative to the crate manifest instead.
    let content_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
    app.insert_resource(ContentDir(content_dir.clone()));
    app.add_plugins(ContentPlugin);
    app.add_plugins(SharedPlugin);
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
    }));
    app.init_resource::<WorldManager>();
    app.init_resource::<PendingBlockChanges>();
    app.init_resource::<BlockEntityIndex>();
    app.init_resource::<TechStateResource>();
    app
}

/// Generates the chunks around a position so the test can read and write blocks there.
pub fn load_around(app: &mut App, centre: BlockPos, radius: i32) {
    let content = app.world().resource::<ContentRegistry>().clone();
    let seed = app.world().resource::<ServerConfigResource>().0.seed;
    let generator = WorldGenerator::new(seed);
    let centre_chunk = centre.chunk();
    let mut chunks = app
        .world_mut()
        .remove_resource::<LoadedChunks>()
        .expect("chunks exist");
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

/// Spawns a player at `position` with a steel pickaxe in slot 0.
pub fn spawn_test_player(app: &mut App, position: BlockPos) -> Entity {
    let content = app.world().resource::<ContentRegistry>().clone();
    let config = app.world().resource::<ServerConfigResource>().clone();
    let client = app.world_mut().spawn_empty().id();
    {
        let mut commands = app.world_mut().commands();
        spawn_player_for_client(&mut commands, &content, &config, client, "Test".into());
    }
    app.update();
    // Find the player that was spawned and move it to the requested position.
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .iter(app.world())
        .next()
        .expect("player spawned");
    {
        let mut state = app
            .world_mut()
            .get_mut::<PlayerState>(player)
            .expect("player state");
        state.body.position = position.center();
        state.body.on_ground = true;
    }
    {
        let mut net = app
            .world_mut()
            .get_mut::<NetworkPosition>(player)
            .expect("network position");
        net.position = position.center();
    }
    player
}

/// Reads a block from the world.
pub fn block_at(app: &App, pos: BlockPos) -> u16 {
    app.world()
        .resource::<LoadedChunks>()
        .block(pos)
        .unwrap_or(u16::MAX)
}

/// Writes a block into the world and records the edit.
pub fn set_block(app: &mut App, pos: BlockPos, block: u16) {
    app.world_mut()
        .resource_mut::<LoadedChunks>()
        .set_block(pos, block);
    app.world_mut()
        .resource_mut::<WorldManager>()
        .record_edit(pos, block);
}

#[test]
fn player_spawns_on_the_camp_with_a_pickaxe() {
    let mut app = test_app(1);
    let content = app.world().resource::<ContentRegistry>().clone();
    load_around(&mut app, spawn_position(&content), 1);
    let player = spawn_test_player(&mut app, spawn_position(&content));
    let state = app.world().get::<PlayerState>(player).expect("state");
    let pickaxe = content.item_by_key("steel_pickaxe").expect("pickaxe").id;
    assert_eq!(state.inventory.hotbar[0].item, pickaxe);
    assert_eq!(state.inventory.hotbar[0].count, 1);
    assert_eq!(state.vitals.health, 100.0);
    assert!(!state.dead);
}

#[test]
fn mining_a_block_grants_its_drops() {
    let mut app = test_app(2);
    let content = app.world().resource::<ContentRegistry>().clone();
    let spawn = spawn_position(&content);
    load_around(&mut app, spawn, 1);
    let player = spawn_test_player(&mut app, spawn);

    // Put a known block directly under the player's feet and mine it.
    let target = spawn.offset(0, -1, 0);
    let dirt = content.block_by_key("dirt").expect("dirt").id;
    let dirt_item = content.item_by_key("dirt").expect("dirt item").id;
    set_block(&mut app, target, dirt.0);

    let state = app
        .world()
        .get::<PlayerState>(player)
        .expect("state")
        .clone();
    let (verdict, blocks, _) = validate_mining(
        &content,
        app.world().resource::<LoadedChunks>(),
        &state,
        spawn.center(),
        target,
        deepcore_shared::voxel::BlockFace::Up,
    );
    assert!(matches!(verdict, MiningVerdict::CanMine { .. }));
    assert!(!blocks.is_empty());

    // Break it through the real helper.
    let mut chunks = app
        .world_mut()
        .remove_resource::<LoadedChunks>()
        .expect("chunks exist");
    let mut manager = app
        .world_mut()
        .remove_resource::<WorldManager>()
        .expect("manager exists");
    let mut changes = app
        .world_mut()
        .remove_resource::<PendingBlockChanges>()
        .expect("changes exist");
    let mut state = state;
    break_block(
        &content,
        &mut chunks,
        &mut manager,
        &mut state,
        target,
        &mut changes,
    );
    app.world_mut().insert_resource(chunks);
    app.world_mut().insert_resource(manager);
    app.world_mut().insert_resource(changes);

    assert_eq!(
        block_at(&app, target),
        content.air.0,
        "block was not removed"
    );
    assert!(
        state.inventory.count_of(dirt_item) >= 1,
        "dirt was not granted"
    );
}

#[test]
fn a_tier_one_pickaxe_cannot_mine_granite() {
    let mut app = test_app(3);
    let content = app.world().resource::<ContentRegistry>().clone();
    let spawn = spawn_position(&content);
    load_around(&mut app, spawn, 1);
    let player = spawn_test_player(&mut app, spawn);
    let granite = content.block_by_key("granite").expect("granite").id;
    let target = spawn.offset(1, -1, 0);
    set_block(&mut app, target, granite.0);
    let state = app
        .world()
        .get::<PlayerState>(player)
        .expect("state")
        .clone();
    let (verdict, _, _) = validate_mining(
        &content,
        app.world().resource::<LoadedChunks>(),
        &state,
        spawn.center(),
        target,
        deepcore_shared::voxel::BlockFace::Up,
    );
    assert_eq!(verdict, MiningVerdict::TooHard { required_tier: 2 });
}

#[test]
fn a_tier_two_pickaxe_mines_granite() {
    let content_app = test_app(4);
    let content = content_app.world().resource::<ContentRegistry>().clone();
    let granite = content.block_by_key("granite").expect("granite").id;
    let tool = ToolStats {
        tier: 2,
        speed: 1.6,
        area: (1, 1),
        reach: 5.0,
    };
    assert!(matches!(
        evaluate_mining(granite, &tool, &content, 1.0),
        MiningVerdict::CanMine { .. }
    ));
}

#[test]
fn bedrock_cannot_be_mined_by_the_nuclear_drill() {
    let app = test_app(5);
    let content = app.world().resource::<ContentRegistry>().clone();
    let drill = content.item_by_key("nuclear_drill").expect("drill").id;
    let tool = ToolStats::from_item(drill, &content);
    assert_eq!(tool.tier, 5);
    assert_eq!(tool.area, (3, 3));
    assert_eq!(
        evaluate_mining(
            ContentId(deepcore_shared::voxel::BEDROCK_ID),
            &tool,
            &content,
            1.0
        ),
        MiningVerdict::Unbreakable
    );
}

#[test]
fn placing_a_furnace_kit_spawns_a_machine() {
    let mut app = test_app(6);
    let content = app.world().resource::<ContentRegistry>().clone();
    let spawn = spawn_position(&content);
    load_around(&mut app, spawn, 1);
    let _player = spawn_test_player(&mut app, spawn);

    let furnace = content.machine_by_key("stone_furnace").expect("furnace").id;
    let anchor = spawn.offset(3, 0, 0);
    let mut chunks = app
        .world_mut()
        .remove_resource::<LoadedChunks>()
        .expect("chunks exist");
    let mut manager = app
        .world_mut()
        .remove_resource::<WorldManager>()
        .expect("manager exists");
    let mut index = app
        .world_mut()
        .remove_resource::<BlockEntityIndex>()
        .expect("index exists");
    let mut commands = app.world_mut().commands();
    let _entity = deepcore_server::sim::machine::place_machine(
        &mut commands,
        &content,
        &mut chunks,
        &mut manager,
        &mut index,
        furnace,
        anchor,
        0,
    )
    .expect("machine placed");
    let entity = _entity;
    app.world_mut().insert_resource(chunks);
    app.world_mut().insert_resource(manager);
    app.world_mut().insert_resource(index);
    app.update();

    let machine = app
        .world()
        .get::<MachineEntity>(entity)
        .expect("machine entity");
    assert_eq!(machine.machine, furnace);
    assert_eq!(machine.position, anchor);
    // The footprint is 1 × 2 × 1 for a stone furnace, so the block above is filled too.
    let block = content.block_for_machine(furnace).expect("machine block");
    assert_eq!(block_at(&app, anchor), block.0);
    assert_eq!(block_at(&app, anchor.offset(0, 1, 0)), block.0);
}

#[test]
fn a_furnace_smelts_iron_from_raw_ore_and_coal() {
    let app = test_app(7);
    let content = app.world().resource::<ContentRegistry>().clone();
    let furnace = content.machine_by_key("stone_furnace").expect("furnace").id;
    let raw_iron = content.item_by_key("raw_iron").expect("raw iron").id;
    let coal = content.item_by_key("coal").expect("coal").id;
    let iron_ingot = content.item_by_key("iron_ingot").expect("ingot").id;

    let mut inventory = deepcore_shared::crafting::MachineInventory::for_machine(&content, furnace);
    inventory.inputs[0] = ItemStack::new(raw_iron, 2);
    inventory.fuel[0] = ItemStack::new(coal, 2);
    let mut state = deepcore_shared::crafting::MachineState::default();
    let unlocked = |_tech: ContentId| true;
    let mut produced = 0;
    // 20 in-game seconds at 30 Hz.
    for _ in 0..600 {
        if deepcore_shared::crafting::tick_machine(
            &content,
            furnace,
            &mut inventory,
            &mut state,
            TICK_SECONDS,
            1.0,
            &unlocked,
        ) {
            produced += 1;
        }
        if produced == 2 {
            break;
        }
    }
    assert_eq!(produced, 2);
    let ingots: u32 = inventory
        .outputs
        .iter()
        .filter(|stack| stack.item == iron_ingot)
        .map(|stack| stack.count)
        .sum();
    assert_eq!(ingots, 2);
}

#[test]
fn a_full_tech_era_can_be_researched_in_order() {
    let app = test_app(8);
    let content = app.world().resource::<ContentRegistry>().clone();
    let mut tech = deepcore_shared::crafting::TechTree::default();
    let mut inventory = deepcore_shared::inventory::Inventory::default();

    // Walk the tree in content order; every node must be reachable once its parents are done.
    for node in &content.tech {
        for (item, count) in &node.cost {
            inventory.insert(*item, *count, &content);
        }
        let result = tech.deposit(&content, node.id, &node.cost, &mut inventory);
        assert!(
            result.is_ok(),
            "tech node {} could not be researched: {:?}",
            node.name,
            result.err()
        );
        assert!(tech.is_researched(node.id), "{} not researched", node.name);
    }
    assert_eq!(tech.researched().len(), content.tech.len());
}

#[test]
fn every_era_has_a_tool_that_can_reach_the_next_layer() {
    let app = test_app(9);
    let content = app.world().resource::<ContentRegistry>().clone();
    // Each layer's base block must be minable by the tool its era unlocks.
    let tools = [
        ("steel_pickaxe", 0usize),
        ("hardened_steel_pickaxe", 1),
        ("steam_drill", 2),
        ("tungsten_carbide_drill", 3),
        ("nuclear_drill", 4),
    ];
    for (index, layer) in content.layers.iter().enumerate() {
        // The layer's own base block must be minable by a tool from the previous era or this
        // one; the first layer is reachable with the starting pickaxe.
        let needed = layer.min_tier;
        let tool_index = tools
            .iter()
            .position(|(key, _)| {
                content
                    .item_by_key(key)
                    .map(|item| match &item.category {
                        deepcore_shared::content::ItemCategory::Tool { tier, .. } => {
                            *tier >= needed
                        }
                        _ => false,
                    })
                    .unwrap_or(false)
            })
            .expect("a tool exists for every layer");
        assert!(
            tool_index <= index + 1,
            "layer {} needs tier {needed}, which only a late tool reaches",
            layer.name
        );
    }
}
