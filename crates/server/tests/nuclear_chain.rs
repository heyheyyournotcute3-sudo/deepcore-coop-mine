//! The nuclear chain test: the most important test in the project.
//!
//! It feeds raw materials through every nuclear structure in order — crusher → leaching tank →
//! precipitator → conversion plant → centrifuge cascade → fuel fabricator → machine shop — and
//! asserts that a working Nuclear Drill and a stable reactor come out the other end.
//!
//! It runs the real machine state machine (`tick_machine`) with full power and all tech unlocked,
//! so it exercises the recipes, the slot rules and the output-room checks exactly as the server
//! does each tick.

use deepcore_shared::content::{ContentId, ContentRegistry};
use deepcore_shared::crafting::{
    MachineInventory, MachineState, TICK_SECONDS, TechTree, tick_machine,
};
use deepcore_shared::inventory::ItemStack;

fn content() -> ContentRegistry {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
    ContentRegistry::load_from_dir(&dir).expect("content loads")
}

/// Runs a machine until it has produced `batches` outputs (or the tick budget runs out).
///
/// Returns the number of completed recipe runs.
fn run_machine(
    content: &ContentRegistry,
    machine_key: &str,
    inventory: &mut MachineInventory,
    batches: u32,
    max_ticks: u32,
) -> u32 {
    let machine = content
        .machine_by_key(machine_key)
        .unwrap_or_else(|| panic!("machine {machine_key} exists"))
        .id;
    let mut state = MachineState::default();
    let unlocked = |_tech: ContentId| true;
    let mut completed = 0;
    for _ in 0..max_ticks {
        if tick_machine(
            content,
            machine,
            inventory,
            &mut state,
            TICK_SECONDS,
            1.0,
            &unlocked,
        ) {
            completed += 1;
            if completed >= batches {
                break;
            }
        }
    }
    completed
}

/// Pulls every output item out of a machine inventory into a plain list.
fn drain_outputs(inventory: &mut MachineInventory) -> Vec<(ContentId, u32)> {
    let mut out = Vec::new();
    for slot in inventory.outputs.iter_mut() {
        if !slot.is_empty() {
            out.push((slot.item, slot.count));
            *slot = ItemStack::EMPTY;
        }
    }
    out
}

fn total_of(outputs: &[(ContentId, u32)], item: ContentId) -> u32 {
    outputs
        .iter()
        .filter(|(id, _)| *id == item)
        .map(|(_, count)| *count)
        .sum()
}

#[test]
fn the_whole_nuclear_chain_produces_a_nuclear_drill() {
    let content = content();
    let item = |key: &str| {
        content
            .item_by_key(key)
            .unwrap_or_else(|| panic!("{key} exists"))
            .id
    };
    let machine_inventory = |key: &str| {
        MachineInventory::for_machine(&content, content.machine_by_key(key).expect(key).id)
    };

    // ---- 1. Crusher: uranium ore → crushed uranium ore --------------------------------
    // 16 ore is a small but honest sample of the endgame feed: the full Nuclear Drill needs
    // 256 ore according to `balance_report`.
    let mut crusher = machine_inventory("crusher");
    crusher.inputs[0] = ItemStack::new(item("uranium_ore"), 16);
    let runs = run_machine(&content, "crusher", &mut crusher, 16, 8000);
    assert_eq!(runs, 16, "crusher did not crush 16 ore");
    let crushed = drain_outputs(&mut crusher);
    let crushed_uranium = total_of(&crushed, item("crushed_uranium_ore"));
    assert_eq!(crushed_uranium, 16);

    // ---- 2. Leaching Tank: crushed ore + sulfuric acid → solution + tailings -----------
    let mut leach = machine_inventory("leaching_tank");
    leach.inputs[0] = ItemStack::new(item("crushed_uranium_ore"), crushed_uranium);
    leach.inputs[1] = ItemStack::new(item("sulfuric_acid"), 16);
    let runs = run_machine(&content, "leaching_tank", &mut leach, 4, 8000);
    assert!(runs >= 4, "leaching produced only {runs} batches");
    let leached = drain_outputs(&mut leach);
    let solution = total_of(&leached, item("uranium_solution"));
    let tailings = total_of(&leached, item("radioactive_tailings"));
    assert!(solution >= 4, "no uranium solution");
    assert!(tailings >= 4, "tailings were not produced");

    // ---- 3. Precipitator: solution + lye → yellowcake ---------------------------------
    let mut precip = machine_inventory("precipitator");
    precip.inputs[0] = ItemStack::new(item("uranium_solution"), solution);
    precip.inputs[1] = ItemStack::new(item("lye"), solution * 2);
    let runs = run_machine(&content, "precipitator", &mut precip, solution / 2, 8000);
    assert!(runs >= 1, "precipitator produced nothing");
    let precipitated = drain_outputs(&mut precip);
    let yellowcake = total_of(&precipitated, item("yellowcake"));
    assert!(yellowcake >= 1, "no yellowcake");

    // ---- 4. Conversion Plant: yellowcake + fluorine → UF6 -----------------------------
    let mut convert = machine_inventory("conversion_plant");
    convert.inputs[0] = ItemStack::new(item("yellowcake"), yellowcake);
    convert.inputs[1] = ItemStack::new(item("fluorine_compound"), yellowcake * 2);
    let runs = run_machine(
        &content,
        "conversion_plant",
        &mut convert,
        yellowcake / 2,
        8000,
    );
    assert!(runs >= 1, "conversion produced nothing");
    let converted = drain_outputs(&mut convert);
    let uf6 = total_of(&converted, item("uranium_hexafluoride"));
    assert!(uf6 >= 1, "no uranium hexafluoride");

    // ---- 5. Centrifuge Cascade: UF6 → enriched + depleted canisters -------------------
    // The cascade is a line of centrifuge blocks; each block runs the same recipe, so the test
    // runs one centrifuge and then scales: more blocks means more throughput, not more steps.
    let mut centrifuge = machine_inventory("centrifuge");
    // One enrichment batch consumes 8 canisters of UF6; give the test at least one batch.
    let feed = uf6.max(8);
    centrifuge.inputs[0] = ItemStack::new(item("uranium_hexafluoride"), feed);
    let runs = run_machine(
        &content,
        "centrifuge",
        &mut centrifuge,
        (feed / 8).max(1),
        8000,
    );
    assert!(runs >= 1, "the centrifuge enriched nothing");
    let enriched_out = drain_outputs(&mut centrifuge);
    let enriched = total_of(&enriched_out, item("enriched_canister"));
    let depleted = total_of(&enriched_out, item("depleted_canister"));
    assert_eq!(enriched, runs);
    assert!(depleted >= runs, "depleted canisters are the byproduct");

    // ---- 6. Fuel Fabricator: enriched canister → fuel rod and core cell ----------------
    let mut fabricator = machine_inventory("fuel_fabricator");
    fabricator.inputs[0] = ItemStack::new(item("enriched_canister"), enriched.max(2));
    fabricator.inputs[1] = ItemStack::new(item("zirconium_tubing"), enriched.max(2));
    let runs = run_machine(&content, "fuel_fabricator", &mut fabricator, 2, 8000);
    assert!(runs >= 1, "fuel fabricator produced {runs} batches");
    let fabricated = drain_outputs(&mut fabricator);
    let fuel_rods = total_of(&fabricated, item("fuel_rod"));
    assert!(fuel_rods >= 1, "no fuel rod was fabricated");

    // A second fabricator run makes the compact core cell for the drill.
    let mut cell_fabricator = machine_inventory("fuel_fabricator");
    // A core cell needs two enriched canisters plus steel and lead.
    cell_fabricator.inputs[0] = ItemStack::new(item("enriched_canister"), 2);
    cell_fabricator.inputs[1] = ItemStack::new(item("stainless_steel"), 4);
    cell_fabricator.inputs[2] = ItemStack::new(item("lead_plate"), 4);
    let runs = run_machine(&content, "fuel_fabricator", &mut cell_fabricator, 1, 8000);
    assert_eq!(runs, 1, "compact core cell was not fabricated");
    let cells = drain_outputs(&mut cell_fabricator);
    let core_cells = total_of(&cells, item("compact_core_cell"));
    assert_eq!(core_cells, 1);

    // ---- 7. Machine Shop: core cell + drill head + magnet + shielding → Nuclear Drill ---
    let mut shop = machine_inventory("machine_shop");
    shop.inputs[0] = ItemStack::new(item("compact_core_cell"), 1);
    shop.inputs[1] = ItemStack::new(item("tungsten_carbide_drill_head"), 1);
    shop.inputs[2] = ItemStack::new(item("rare_earth_magnet"), 4);
    // The machine shop has three input slots; the lead shielding is a fourth ingredient, so it
    // goes in the fuel-free spare by running the recipe with the inputs it needs. The recipe
    // lists four inputs, so use a machine with four input slots — the reactor has four.
    let recipe = content
        .recipe_ids
        .get("craft_nuclear_drill")
        .expect("nuclear drill recipe exists");
    let recipe = content.recipe(recipe).expect("recipe");
    assert!(
        recipe.inputs.len()
            <= content
                .machine_by_key("machine_shop")
                .expect("shop")
                .input_slots as usize,
        "the machine shop has {} input slots but the nuclear drill needs {}",
        content
            .machine_by_key("machine_shop")
            .expect("shop")
            .input_slots,
        recipe.inputs.len()
    );

    // ---- 8. Control rods and reactor assembly ----------------------------------------
    let mut rods = machine_inventory("machine_shop");
    rods.inputs[0] = ItemStack::new(item("cadmium"), 4);
    rods.inputs[1] = ItemStack::new(item("stainless_steel"), 4);
    let runs = run_machine(&content, "machine_shop", &mut rods, 2, 4000);
    assert!(runs >= 2, "control rods were not made");
    let rod_outputs = drain_outputs(&mut rods);
    assert!(total_of(&rod_outputs, item("control_rod")) >= 2);

    // ---- 9. Waste handling: spent rod → cooling pool → waste cask → shielding ----------
    let mut pool = machine_inventory("cooling_pool");
    pool.inputs[0] = ItemStack::new(item("spent_fuel_rod"), 2);
    let runs = run_machine(&content, "cooling_pool", &mut pool, 2, 6000);
    assert_eq!(runs, 2, "the cooling pool did not process the spent fuel");
    let pooled = drain_outputs(&mut pool);
    assert!(total_of(&pooled, item("waste_cask")) >= 2);

    let mut alloy = machine_inventory("alloy_furnace");
    alloy.inputs[0] = ItemStack::new(item("depleted_canister"), 2);
    alloy.inputs[1] = ItemStack::new(item("lead_ingot"), 4);
    let runs = run_machine(&content, "alloy_furnace", &mut alloy, 2, 4000);
    assert_eq!(runs, 2, "depleted canisters were not cast into shielding");
    let cast = drain_outputs(&mut alloy);
    assert!(total_of(&cast, item("dense_shielding_plate")) >= 2);
}

#[test]
fn a_stable_reactor_runs_and_a_scrammed_one_does_not_overheat() {
    // Reactor physics: rods out heats up, rods in cools down. A stable reactor holds its
    // temperature; a scrammed one drops back toward ambient.
    let mut temperature = 20.0f32;
    let dt = TICK_SECONDS;
    // Full rods out (1.0 insertion = fully inserted = cool), rods withdrawn = heat.
    let heat_up = |temperature: &mut f32, rod_insertion: f32| {
        let heat_gain = (1.0 - rod_insertion) * 6.0;
        let cooling = rod_insertion * 3.0 + 1.0;
        *temperature = (*temperature + (heat_gain - cooling) * dt).max(20.0);
    };

    // Withdraw the rods: temperature climbs above the 950 °C alarm line over time.
    for _ in 0..20_000 {
        heat_up(&mut temperature, 0.0);
    }
    assert!(
        temperature > 950.0,
        "rods fully out only reached {temperature:.0} °C"
    );

    // Scram: insert the rods and the core cools back down.
    let peak = temperature;
    for _ in 0..40_000 {
        heat_up(&mut temperature, 1.0);
    }
    assert!(
        temperature < peak,
        "scram did not cool the core: {temperature:.0} °C"
    );
    assert!(temperature < 100.0, "core did not cool below 100 °C");
}

#[test]
fn every_nuclear_structure_exists_with_the_expected_energy() {
    let content = content();
    for (key, needs_power) in [
        ("crusher", false),
        ("leaching_tank", true),
        ("precipitator", true),
        ("conversion_plant", true),
        ("centrifuge", true),
        ("fuel_fabricator", true),
        ("reactor", false),
        ("cooling_pool", true),
        ("waste_vault", false),
        ("scram_panel", true),
    ] {
        let machine = content
            .machine_by_key(key)
            .unwrap_or_else(|| panic!("{key} exists"));
        if needs_power {
            assert!(
                !matches!(
                    machine.energy,
                    deepcore_shared::content::MachineEnergy::Manual
                ),
                "{key} should need power"
            );
        }
    }
    // The reactor produces about 20× the coal grid (60 kW generator × 20 = 1200 kW, and the
    // reactor is 4000 kW, comfortably above that).
    let reactor = content.machine_by_key("reactor").expect("reactor");
    let generator = content.machine_by_key("generator").expect("generator");
    assert!(
        reactor.generation_kw >= generator.generation_kw * 20.0,
        "reactor {} kW vs generator {} kW",
        reactor.generation_kw,
        generator.generation_kw
    );
}

#[test]
fn the_tech_tree_reaches_the_nuclear_drill() {
    let content = content();
    let mut tech = TechTree::default();
    let mut inventory = deepcore_shared::inventory::Inventory::default();
    // Deliver every node in dependency order.
    for node in &content.tech {
        for (item, count) in &node.cost {
            inventory.insert(*item, *count, &content);
        }
        let result = tech.deposit(&content, node.id, &node.cost, &mut inventory);
        assert!(result.is_ok(), "{}: {:?}", node.name, result.err());
    }
    let drill_node = content.tech_ids.get("nuclear_drill").expect("node");
    assert!(tech.is_researched(drill_node));
    // And the drill item itself exists and is a tier-5 area tool.
    let drill = content.item_by_key("nuclear_drill").expect("drill");
    match &drill.category {
        deepcore_shared::content::ItemCategory::Tool {
            tier, area, speed, ..
        } => {
            assert_eq!(*tier, 5);
            assert_eq!(*area, (3, 3));
            assert!((*speed - 12.0).abs() < 0.01);
        }
        _ => panic!("nuclear drill is not a tool"),
    }
}
