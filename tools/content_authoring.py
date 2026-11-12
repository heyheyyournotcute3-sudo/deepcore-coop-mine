#!/usr/bin/env python3
"""One-off authoring aid for Deepcore's RON content.

The RON files in assets/data are the single source of truth at runtime; nothing in the game
reads this script. It exists so the initial content set (which is large and highly
cross-referenced) was generated consistently, and it can be re-run to regenerate the files if
you prefer editing tables to editing RON by hand.

Usage: python3 tools/content_authoring.py
"""

import os

OUT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets", "data")

# --------------------------------------------------------------------------------------------
# Machines
# --------------------------------------------------------------------------------------------
# id, name, size, energy, in, out, build_cost, tech, extras
MACHINES = [
    ("workbench", "Workbench", (1, 1, 1), "Manual", 9, 1, [("timber", 10)], "hand_tools", {}),
    ("storage_crate", "Storage Crate", (1, 1, 1), "Manual", 27, 27, [("timber", 8)], "hand_tools", {}),
    ("research_desk", "Research Desk", (1, 1, 1), "Manual", 1, 1, [("timber", 12), ("stone", 8)], "hand_tools", {}),
    ("brick_kiln", "Brick Kiln", (1, 2, 1), 'Fuel { slots: 1 }', 2, 2, [("clay", 20), ("stone", 10)], "brick_making", {}),
    ("stone_furnace", "Stone Furnace", (1, 2, 1), 'Fuel { slots: 1 }', 2, 2, [("stone", 20), ("brick", 8)], "smelting", {}),
    ("anvil", "Anvil", (1, 1, 1), "Manual", 3, 3, [("iron_ingot", 6), ("stone", 4)], "smelting", {}),
    ("blast_furnace", "Blast Furnace", (2, 3, 2), 'Fuel { slots: 1 }', 3, 2, [("fire_brick", 40), ("iron_plate", 12), ("bronze_gear", 4)], "steelmaking", {}),
    ("hand_pump", "Hand Pump", (1, 1, 1), "Manual", 1, 1, [("bronze_ingot", 4), ("copper_pipe", 2)], "steam_power", {}),
    ("steam_boiler", "Steam Boiler", (2, 2, 2), 'Fuel { slots: 1 }', 2, 1, [("steel_plate", 12), ("copper_pipe", 6), ("bronze_gear", 2)], "steam_power", {"steam_buffer": "600.0"}),
    ("steam_engine", "Steam Engine", (2, 2, 2), 'Steam { units_per_sec: 6.0 }', 1, 1, [("steel_plate", 10), ("bronze_gear", 6), ("copper_pipe", 4)], "steam_power", {"generation_kw": "40.0"}),
    ("crusher", "Crusher", (2, 2, 2), 'Steam { units_per_sec: 4.0 }', 2, 2, [("steel_plate", 8), ("iron_rod", 6), ("bronze_gear", 4)], "ore_processing", {}),
    ("ore_washer", "Ore Washer", (2, 2, 2), 'Steam { units_per_sec: 3.0 }', 2, 2, [("steel_plate", 6), ("copper_pipe", 8), ("brick", 12)], "ore_processing", {}),
    ("mine_lift", "Mine Lift", (2, 2, 2), 'Steam { units_per_sec: 5.0 }', 2, 2, [("steel_plate", 14), ("bronze_gear", 8), ("iron_rod", 10)], "logistics", {}),
    ("steam_turbine", "Steam Turbine", (3, 2, 2), 'Steam { units_per_sec: 10.0 }', 1, 1, [("steel_plate", 20), ("turbine_blade", 6), ("bronze_gear", 4)], "electricity", {"generation_kw": "120.0"}),
    ("generator", "Generator", (2, 2, 2), 'Steam { units_per_sec: 4.0 }', 1, 1, [("steel_plate", 10), ("copper_wire", 16), ("bronze_gear", 6)], "electricity", {"generation_kw": "60.0"}),
    ("battery_bank", "Battery Bank", (2, 2, 2), 'Electric { kw: 0.0 }', 1, 1, [("battery_pack", 8), ("lead_plate", 8), ("copper_wire", 12)], "energy_storage", {"battery_kwh": "200.0"}),
    ("arc_furnace", "Arc Furnace", (2, 2, 2), 'Electric { kw: 60.0 }', 3, 3, [("steel_plate", 16), ("graphite_electrode", 4), ("circuit_mk1", 2)], "arc_smelting", {"modules": "true"}),
    ("electrolysis_cell", "Electrolysis Cell", (2, 2, 2), 'Electric { kw: 45.0 }', 2, 3, [("steel_plate", 12), ("copper_wire", 20), ("glass_block_item", 4)], "electrolysis", {"modules": "true"}),
    ("circuit_assembler", "Circuit Assembler", (2, 2, 2), 'Electric { kw: 35.0 }', 3, 2, [("steel_plate", 10), ("circuit_mk1", 2), ("copper_wire", 24)], "circuitry", {"modules": "true"}),
    ("charging_station", "Charging Station", (1, 2, 1), 'Electric { kw: 20.0 }', 2, 2, [("steel_plate", 8), ("copper_wire", 18), ("battery_pack", 2)], "circuitry", {}),
    ("vent_fan", "Vent Fan", (1, 1, 1), 'Electric { kw: 15.0 }', 1, 1, [("steel_plate", 6), ("turbine_blade", 2), ("copper_wire", 8)], "ventilation", {}),
    ("electric_pump", "Electric Pump", (1, 1, 1), 'Electric { kw: 20.0 }', 1, 1, [("steel_plate", 6), ("copper_pipe", 6), ("circuit_mk1", 1)], "ventilation", {}),
    ("chemical_plant", "Chemical Plant", (3, 3, 2), 'Electric { kw: 80.0 }', 3, 3, [("stainless_steel", 12), ("glass_block_item", 6), ("circuit_mk2", 2)], "chemistry", {"modules": "true"}),
    ("alloy_furnace", "Alloy Furnace", (2, 3, 2), 'Electric { kw: 90.0 }', 3, 2, [("fire_brick", 30), ("steel_plate", 16), ("graphite_electrode", 6)], "advanced_alloys", {"modules": "true"}),
    ("machine_shop", "Machine Shop", (3, 2, 2), 'Electric { kw: 70.0 }', 3, 3, [("stainless_steel", 10), ("tungsten_carbide", 4), ("circuit_mk2", 3)], "precision_machining", {"modules": "true"}),
    ("rare_earth_separator", "Rare-Earth Separator", (2, 2, 2), 'Electric { kw: 110.0 }', 2, 2, [("stainless_steel", 8), ("circuit_mk2", 2), ("centrifuge_rotor", 2)], "rare_earths", {"modules": "true"}),
    ("decon_shower", "Decon Shower", (1, 2, 1), 'Electric { kw: 25.0 }', 1, 1, [("lead_plate", 8), ("copper_pipe", 6), ("circuit_mk1", 1)], "hazard_gear", {}),
    ("leaching_tank", "Leaching Tank", (2, 2, 2), 'Electric { kw: 40.0 }', 2, 2, [("stainless_steel", 10), ("lead_plate", 6), ("circuit_mk2", 2)], "uranium_milling", {"modules": "true"}),
    ("precipitator", "Precipitator", (2, 2, 2), 'Electric { kw: 35.0 }', 2, 2, [("stainless_steel", 8), ("glass_block_item", 4), ("circuit_mk2", 2)], "uranium_milling", {"modules": "true"}),
    ("conversion_plant", "Conversion Plant", (3, 2, 2), 'Electric { kw: 120.0 }', 3, 2, [("stainless_steel", 14), ("fluorine_compound", 8), ("circuit_mk2", 3)], "conversion", {"modules": "true"}),
    ("centrifuge", "Centrifuge", (1, 3, 1), 'Electric { kw: 150.0 }', 1, 2, [("stainless_rotor", 2), ("rare_earth_magnet", 4), ("circuit_mk2", 2)], "enrichment", {"cascade": "Some((min: 4, max: 16))", "modules": "true"}),
    ("fuel_fabricator", "Fuel Fabricator", (2, 2, 2), 'Electric { kw: 100.0 }', 3, 2, [("stainless_steel", 12), ("zirconium_tubing", 6), ("circuit_mk2", 3)], "fuel_fabrication", {"modules": "true"}),
    ("reactor", "Reactor", (5, 5, 5), 'Electric { kw: 0.0 }', 4, 2, [("reactor_casing", 40), ("lead_shielding", 24), ("graphite_moderator", 16), ("control_rod", 4)], "reactor_engineering", {"generation_kw": "4000.0", "multiblock": "true"}),
    ("scram_panel", "Scram Panel", (1, 1, 1), 'Electric { kw: 2.0 }', 1, 1, [("stainless_steel", 4), ("circuit_mk2", 1), ("cadmium", 4)], "reactor_engineering", {}),
    ("cooling_pool", "Cooling Pool", (3, 3, 1), 'Electric { kw: 30.0 }', 2, 2, [("stainless_steel", 10), ("lead_plate", 10), ("copper_pipe", 12)], "waste_handling", {"modules": "true"}),
    ("waste_vault", "Waste Vault", (3, 3, 2), "Manual", 12, 12, [("lead_shielding", 16), ("stainless_steel", 8)], "waste_handling", {}),
]

# --------------------------------------------------------------------------------------------
# Tech tree
# --------------------------------------------------------------------------------------------
TECH = [
    ("hand_tools", "Hand Tools", 0, [], [("timber", 10)],
     ["workbench", "storage_crate", "research_desk", "ladder", "timber_support", "oil_lamp"]),
    ("brick_making", "Brick Making", 1, ["hand_tools"], [("clay", 20), ("timber", 10)],
     ["brick_kiln", "brick", "fire_brick"]),
    ("smelting", "Smelting", 1, ["hand_tools"], [("coal", 10), ("raw_copper", 10), ("stone", 10)],
     ["stone_furnace", "anvil", "copper_ingot", "tin_ingot", "iron_ingot"]),
    ("alloying", "Bronze Alloying", 1, ["smelting"], [("copper_ingot", 6), ("tin_ingot", 6)],
     ["bronze_ingot"]),
    ("steelmaking", "Steelmaking", 1, ["smelting", "brick_making"],
     [("iron_ingot", 12), ("coal", 20), ("limestone", 10)],
     ["blast_furnace", "steel_ingot", "steel_plate", "hardened_steel_pickaxe"]),
    ("steam_power", "Steam Power", 2, ["steelmaking"],
     [("steel_plate", 20), ("copper_pipe", 10), ("bronze_gear", 5)],
     ["steam_boiler", "steam_engine", "hand_pump", "copper_pipe_block"]),
    ("ore_processing", "Ore Processing", 2, ["steam_power"],
     [("steel_plate", 12), ("bronze_gear", 6)],
     ["crusher", "ore_washer"]),
    ("logistics", "Logistics", 2, ["steam_power"],
     [("steel_plate", 10), ("iron_rod", 12)],
     ["mine_lift", "rail", "conveyor"]),
    ("steam_drill", "Steam Drill", 2, ["ore_processing", "alloying"],
     [("steel_ingot", 10), ("bronze_gear", 8), ("copper_pipe", 6)],
     ["steam_drill", "steam_canister"]),
    ("electricity", "Electricity", 3, ["steam_drill"],
     [("steel_plate", 24), ("copper_wire", 20), ("bronze_gear", 6)],
     ["steam_turbine", "generator", "cable_block"]),
    ("arc_smelting", "Arc Smelting", 3, ["electricity"],
     [("steel_plate", 12), ("graphite", 8)],
     ["arc_furnace", "graphite_electrode"]),
    ("electrolysis", "Electrolysis", 3, ["electricity"],
     [("steel_plate", 10), ("salt", 12)],
     ["electrolysis_cell", "aluminium_ingot", "chlorine", "lye"]),
    ("circuitry", "Circuitry", 3, ["electricity"],
     [("silicon", 8), ("copper_wire", 12)],
     ["circuit_assembler", "circuit_mk1", "charging_station"]),
    ("energy_storage", "Energy Storage", 3, ["circuitry"],
     [("lead_plate", 10), ("sulfuric_acid", 6)],
     ["battery_bank", "battery_pack"]),
    ("ventilation", "Ventilation", 3, ["electricity"],
     [("steel_plate", 8), ("circuit_mk1", 2)],
     ["vent_fan", "electric_pump"]),
    ("ore_scanning", "Ore Scanning", 3, ["circuitry"],
     [("circuit_mk1", 4), ("quartz", 10)],
     ["ore_scanner"]),
    ("chemistry", "Chemistry", 4, ["arc_smelting", "electrolysis"],
     [("sulfuric_acid", 8), ("fluorite", 10), ("stainless_steel", 6)],
     ["chemical_plant", "sulfuric_acid", "fluorine_compound"]),
    ("advanced_alloys", "Advanced Alloys", 4, ["chemistry"],
     [("nickel_ingot", 8), ("chromium_ingot", 8), ("steel_plate", 16)],
     ["alloy_furnace", "stainless_steel", "tungsten_carbide"]),
    ("precision_machining", "Precision Machining", 4, ["advanced_alloys"],
     [("stainless_steel", 10), ("tungsten_carbide", 6)],
     ["machine_shop", "bearing", "turbine_blade", "centrifuge_rotor", "stainless_rotor"]),
    ("rare_earths", "Rare Earths", 4, ["precision_machining"],
     [("monazite", 12), ("sulfuric_acid", 6)],
     ["rare_earth_separator", "rare_earth_magnet"]),
    ("hazard_gear", "Hazard Gear", 4, ["chemistry"],
     [("lead_plate", 12), ("circuit_mk1", 2), ("glass_block_item", 4)],
     ["decon_shower", "cooling_suit", "respirator", "hazmat_suit", "geiger_counter"]),
    ("tungsten_drill", "Tungsten Drill", 4, ["precision_machining", "hazard_gear"],
     [("tungsten_carbide", 8), ("stainless_steel", 8), ("rare_earth_magnet", 2)],
     ["tungsten_carbide_drill"]),
    ("uranium_milling", "Uranium Milling", 5, ["chemistry"],
     [("stainless_steel", 10), ("sulfuric_acid", 10)],
     ["leaching_tank", "precipitator", "yellowcake"]),
    ("conversion", "Conversion", 5, ["uranium_milling", "chemistry"],
     [("fluorine_compound", 8), ("yellowcake", 6)],
     ["conversion_plant", "uranium_hexafluoride"]),
    ("enrichment", "Enrichment", 5, ["conversion", "rare_earths"],
     [("rare_earth_magnet", 8), ("stainless_rotor", 4), ("circuit_mk2", 4)],
     ["centrifuge", "enriched_canister"]),
    ("fuel_fabrication", "Fuel Fabrication", 5, ["enrichment"],
     [("zirconium_tubing", 6), ("stainless_steel", 10)],
     ["fuel_fabricator", "fuel_rod"]),
    ("control_rods", "Control Rods", 5, ["fuel_fabrication"],
     [("cadmium", 8), ("stainless_steel", 6)],
     ["control_rod"]),
    ("reactor_engineering", "Reactor Engineering", 5, ["fuel_fabrication", "precision_machining"],
     [("reactor_casing", 20), ("graphite_moderator", 8), ("control_rod", 4), ("core_crystal", 2)],
     ["reactor", "scram_panel"]),
    ("waste_handling", "Waste Handling", 5, ["reactor_engineering"],
     [("lead_shielding", 12), ("stainless_steel", 8)],
     ["cooling_pool", "waste_vault", "waste_cask", "dense_shielding_plate"]),
    ("nuclear_drill", "Nuclear Drill", 5, ["reactor_engineering", "precision_machining"],
     [("compact_core_cell", 1), ("tungsten_carbide_drill_head", 1), ("rare_earth_magnet", 4), ("lead_shielding", 8)],
     ["nuclear_drill"]),
]

# --------------------------------------------------------------------------------------------
# Items
# --------------------------------------------------------------------------------------------
# (id, name, stack, category)
ITEMS = [
    # --- terrain block items -----------------------------------------------------------
    ("dirt", "Dirt", 64, 'Block("dirt")'),
    ("gravel", "Gravel", 64, 'Block("gravel")'),
    ("stone", "Stone", 64, 'Block("stone")'),
    ("obsidian", "Obsidian", 64, 'Block("obsidian")'),
    ("ladder", "Ladder", 64, 'Block("ladder")'),
    ("timber_support", "Timber Support", 64, 'Block("timber_support")'),
    ("oil_lamp", "Oil Lamp", 16, 'Block("oil_lamp")'),
    ("brick_block", "Brick Block", 64, 'Block("brick_block")'),
    ("glass_block_item", "Glass", 64, 'Block("glass_block")'),
    ("copper_pipe_block", "Copper Pipe", 64, 'Block("copper_pipe_block")'),
    ("cable_block", "Cable", 64, 'Block("cable_block")'),
    ("conveyor", "Conveyor", 64, 'Block("conveyor")'),
    ("rail", "Rail", 64, 'Block("rail")'),
    ("lead_shielding", "Lead Shielding", 64, 'Block("lead_shielding")'),
    ("reactor_casing", "Reactor Casing", 64, 'Block("reactor_casing")'),
    ("graphite_moderator", "Graphite Moderator", 64, 'Block("graphite_moderator")'),
    # --- raw materials -----------------------------------------------------------------
    ("timber", "Timber", 64, "Material"),
    ("clay", "Clay", 64, "Material"),
    ("sand", "Sand", 64, "Material"),
    ("limestone", "Limestone", 64, "Material"),
    ("coal", "Coal", 64, 'Fuel(burn_seconds: 80.0, heat: 1.0)'),
    ("raw_copper", "Raw Copper", 64, "Material"),
    ("raw_tin", "Raw Tin", 64, "Material"),
    ("raw_iron", "Raw Iron", 64, "Material"),
    ("raw_lead", "Raw Lead", 64, "Material"),
    ("raw_zinc", "Raw Zinc", 64, "Material"),
    ("sulfur", "Sulfur", 64, "Material"),
    ("quartz", "Quartz", 64, "Material"),
    ("raw_bauxite", "Raw Bauxite", 64, "Material"),
    ("salt", "Rock Salt", 64, "Material"),
    ("raw_nickel", "Raw Nickel", 64, "Material"),
    ("raw_tungsten", "Raw Tungsten", 64, "Material"),
    ("raw_chromium", "Raw Chromium", 64, "Material"),
    ("graphite", "Graphite", 64, "Material"),
    ("fluorite", "Fluorite", 64, "Material"),
    ("raw_zircon", "Raw Zircon", 64, "Material"),
    ("raw_gold", "Raw Gold", 64, "Material"),
    ("uranium_ore", "Uranium Ore", 64, "Material"),
    ("monazite", "Monazite Sand", 64, "Material"),
    ("core_crystal", "Core Crystal", 16, "Material"),
    # --- smelting and metallurgy --------------------------------------------------------
    ("brick", "Brick", 64, "Material"),
    ("fire_brick", "Fire Brick", 64, "Material"),
    ("glass", "Glass Pane", 64, "Material"),
    ("copper_ingot", "Copper Ingot", 64, "Material"),
    ("tin_ingot", "Tin Ingot", 64, "Material"),
    ("bronze_ingot", "Bronze Ingot", 64, "Material"),
    ("iron_ingot", "Iron Ingot", 64, "Material"),
    ("lead_ingot", "Lead Ingot", 64, "Material"),
    ("zinc_ingot", "Zinc Ingot", 64, "Material"),
    ("steel_ingot", "Steel Ingot", 64, "Material"),
    ("aluminium_ingot", "Aluminium Ingot", 64, "Material"),
    ("nickel_ingot", "Nickel Ingot", 64, "Material"),
    ("chromium_ingot", "Chromium Ingot", 64, "Material"),
    ("tungsten_ingot", "Tungsten Ingot", 64, "Material"),
    ("gold_ingot", "Gold Ingot", 64, "Material"),
    ("stainless_steel", "Stainless Steel", 64, "Material"),
    ("tungsten_carbide", "Tungsten Carbide", 64, "Material"),
    ("tungsten_carbide_drill_head", "Tungsten-Carbide Drill Head", 4, "Material"),
    ("silicon", "Silicon", 64, "Material"),
    ("cadmium", "Cadmium", 64, "Material"),
    ("chlorine", "Chlorine", 64, "Material"),
    ("lye", "Lye", 64, "Material"),
    ("sulfuric_acid", "Sulfuric Acid", 64, "Material"),
    ("fluorine_compound", "Fluorine Compound", 64, "Material"),
    ("rare_earth_magnet", "Rare-Earth Magnet", 64, "Material"),
    # --- parts --------------------------------------------------------------------------
    ("copper_wire", "Copper Wire", 64, "Material"),
    ("copper_pipe", "Copper Pipe", 64, "Material"),
    ("iron_plate", "Iron Plate", 64, "Material"),
    ("steel_plate", "Steel Plate", 64, "Material"),
    ("lead_plate", "Lead Plate", 64, "Material"),
    ("bronze_gear", "Bronze Gear", 64, "Material"),
    ("iron_rod", "Iron Rod", 64, "Material"),
    ("steel_rod", "Steel Rod", 64, "Material"),
    ("screw", "Screw", 64, "Material"),
    ("bearing", "Bearing", 64, "Material"),
    ("turbine_blade", "Turbine Blade", 64, "Material"),
    ("centrifuge_rotor", "Centrifuge Rotor", 64, "Material"),
    ("stainless_rotor", "Stainless Rotor", 64, "Material"),
    ("zirconium_tubing", "Zirconium Tubing", 64, "Material"),
    ("graphite_electrode", "Graphite Electrode", 64, "Material"),
    ("circuit_mk1", "Circuit Mk1", 64, "Material"),
    ("circuit_mk2", "Circuit Mk2", 64, "Material"),
    ("battery_pack", "Battery Pack", 16, "Material"),
    # --- nuclear ------------------------------------------------------------------------
    ("crushed_uranium_ore", "Crushed Uranium Ore", 64, "Material"),
    ("uranium_solution", "Uranium Solution", 64, "Material"),
    ("radioactive_tailings", "Radioactive Tailings", 64, "Material"),
    ("yellowcake", "Yellowcake", 64, "Material"),
    ("uranium_hexafluoride", "Uranium Hexafluoride", 64, "Material"),
    ("enriched_canister", "Enriched Canister", 16, "Material"),
    ("depleted_canister", "Depleted Canister", 16, "Material"),
    ("fuel_rod", "Fuel Rod", 16, 'Material'),  # sink: reactor
    ("spent_fuel_rod", "Spent Fuel Rod", 16, "Material"),
    ("compact_core_cell", "Compact Core Cell", 4, "Material"),
    ("spent_core_cell", "Spent Core Cell", 4, "Material"),
    ("control_rod", "Control Rod", 16, "Material"),
    ("waste_cask", "Waste Cask", 16, "Material"),  # sink: waste vault
    ("dense_shielding_plate", "Dense Shielding Plate", 64, "Material"),
    # --- tools --------------------------------------------------------------------------
    ("steel_pickaxe", "Steel Pickaxe", 1,
     'Tool(tier: 1, speed: 1.0, area: (1, 1), energy: None)'),
    ("hardened_steel_pickaxe", "Hardened Steel Pickaxe", 1,
     'Tool(tier: 2, speed: 1.6, area: (1, 1), energy: None)'),
    ("steam_drill", "Steam Drill", 1,
     'Tool(tier: 3, speed: 3.0, area: (1, 1), energy: Steam(capacity: 300), reach: 6.0)'),
    ("tungsten_carbide_drill", "Tungsten-Carbide Drill", 1,
     'Tool(tier: 4, speed: 5.0, area: (1, 1), energy: Battery(capacity: 900), reach: 6.0)'),
    ("nuclear_drill", "Nuclear Drill", 1,
     'Tool(tier: 5, speed: 12.0, area: (3, 3), energy: CoreCell(breaks: 2000), reach: 6.0)'),
    # --- charges ------------------------------------------------------------------------
    ("steam_canister", "Steam Canister", 8, "Charge(capacity: 300)"),
    ("charged_battery", "Charged Battery", 8, "Charge(capacity: 900)"),
    # --- gear ---------------------------------------------------------------------------
    ("cooling_suit", "Cooling Suit", 1,
     'Gear(slot: Suit, heat_resistance: 60.0, oxygen_use: 1.0, gas_protection: 1.0)'),
    ("respirator", "Respirator", 1,
     'Gear(slot: Mask, gas_protection: 0.15)'),
    ("hazmat_suit", "Lead-Lined Hazmat Suit", 1,
     'Gear(slot: Suit, radiation_shielding: 0.8, heat_resistance: 20.0, oxygen_use: 0.9, gas_protection: 0.3)'),
    ("geiger_counter", "Geiger Counter", 1, 'Gear(slot: Instrument, radiation_shielding: 0.0)'),
    ("headlamp_mk2", "Headlamp Mk2", 1, 'Gear(slot: Instrument, radiation_shielding: 0.0)'),
    ("ore_scanner", "Ore Scanner", 1, 'Gear(slot: Instrument, radiation_shielding: 0.0)'),
    # --- upgrade modules ----------------------------------------------------------------
    ("module_speed", "Speed Module", 8, "Material"),
    ("module_efficiency", "Efficiency Module", 8, "Material"),
]

# Machine kits are generated from MACHINES.
for machine_id, name, *_rest in MACHINES:
    ITEMS.append((f"kit_{machine_id}", f"{name} Kit", 4, f'MachineKit("{machine_id}")'))

# --------------------------------------------------------------------------------------------
# Blocks
# --------------------------------------------------------------------------------------------
BLOCKS = []


def block(id, hardness, min_tier, drops, color, pattern, **extra):
    BLOCKS.append(dict(id=id, hardness=hardness, min_tier=min_tier, drops=drops,
                       color=color, pattern=pattern, **extra))


# terrain
block("dirt", 0.5, 1, [("dirt", 1)], (0.36, 0.26, 0.17), "Grain", layer_hint="Some(1)")
block("gravel", 0.6, 1, [("gravel", 1)], (0.42, 0.40, 0.38), "Speckle", layer_hint="Some(1)")
block("clay", 0.7, 1, [("clay", 2)], (0.55, 0.45, 0.36), "Speckle", layer_hint="Some(1)")
block("sand", 0.5, 1, [("sand", 1)], (0.78, 0.72, 0.52), "Speckle", layer_hint="Some(1)")
block("stone", 1.5, 1, [("stone", 1)], (0.45, 0.45, 0.46), "Speckle", layer_hint="Some(2)")
block("limestone", 1.8, 1, [("limestone", 2)], (0.72, 0.70, 0.62), "Bands", layer_hint="Some(2)")
block("shale", 1.6, 1, [("stone", 1)], (0.30, 0.31, 0.33), "Bands", layer_hint="Some(2)")
block("granite", 2.5, 2, [("stone", 1)], (0.55, 0.50, 0.48), "Speckle", layer_hint="Some(3)")
block("basalt", 3.2, 3, [("stone", 1)], (0.22, 0.22, 0.25), "Bands", layer_hint="Some(4)")
block("gneiss", 4.0, 4, [("stone", 1)], (0.38, 0.36, 0.40), "Veins", layer_hint="Some(5)")
block("obsidian", 5.0, 4, [("obsidian", 1)], (0.10, 0.09, 0.14), "Glow", layer_hint="Some(5)")
block("mantle_rock", 6.0, 5, [("stone", 1)], (0.30, 0.12, 0.10), "Speckle", layer_hint="Some(6)")

# ores
block("coal_seam", 1.6, 1, [("coal", 2)], (0.18, 0.18, 0.18), "OreFleck((0.08, 0.08, 0.08))", layer_hint="Some(1)")
block("malachite", 1.8, 1, [("raw_copper", 1)], (0.35, 0.42, 0.38), "OreFleck((0.20, 0.75, 0.55))", layer_hint="Some(2)")
block("cassiterite", 2.0, 1, [("raw_tin", 1)], (0.44, 0.44, 0.42), "OreFleck((0.75, 0.75, 0.78))", layer_hint="Some(2)")
block("hematite", 2.2, 1, [("raw_iron", 1)], (0.48, 0.42, 0.40), "OreFleck((0.70, 0.35, 0.25))", layer_hint="Some(2)")
block("galena", 2.6, 2, [("raw_lead", 1)], (0.40, 0.42, 0.46), "OreFleck((0.65, 0.68, 0.75))", layer_hint="Some(3)")
block("sphalerite", 2.6, 2, [("raw_zinc", 1)], (0.42, 0.40, 0.36), "OreFleck((0.85, 0.80, 0.45))", layer_hint="Some(3)")
block("sulfur_deposit", 2.0, 2, [("sulfur", 2)], (0.62, 0.58, 0.28), "OreFleck((0.95, 0.90, 0.20))", layer_hint="Some(3)")
block("quartz_block", 2.4, 2, [("quartz", 1)], (0.70, 0.72, 0.74), "OreFleck((0.95, 0.95, 1.0))", layer_hint="Some(3)")
block("bauxite", 2.3, 2, [("raw_bauxite", 1)], (0.55, 0.45, 0.36), "OreFleck((0.85, 0.60, 0.40))", layer_hint="Some(3)")
block("rock_salt", 1.9, 2, [("salt", 2)], (0.82, 0.80, 0.78), "OreFleck((0.95, 0.95, 0.98))", layer_hint="Some(3)")
block("pentlandite", 3.4, 3, [("raw_nickel", 1)], (0.35, 0.33, 0.30), "OreFleck((0.70, 0.75, 0.55))", layer_hint="Some(4)")
block("wolframite", 4.2, 3, [("raw_tungsten", 1)], (0.28, 0.26, 0.26), "OreFleck((0.55, 0.50, 0.60))", layer_hint="Some(4)")
block("chromite", 4.0, 3, [("raw_chromium", 1)], (0.24, 0.22, 0.24), "OreFleck((0.80, 0.75, 0.85))", layer_hint="Some(4)")
block("graphite_vein", 3.0, 3, [("graphite", 2)], (0.14, 0.14, 0.15), "OreFleck((0.35, 0.35, 0.38))", layer_hint="Some(4)")
block("fluorite_block", 3.6, 3, [("fluorite", 1)], (0.30, 0.45, 0.50), "OreFleck((0.35, 0.95, 0.85))", layer_hint="Some(4)", emissive="0.15")
block("zircon_block", 4.4, 3, [("raw_zircon", 1)], (0.48, 0.38, 0.32), "OreFleck((0.90, 0.70, 0.45))", layer_hint="Some(4)")
block("gold_vein", 4.0, 3, [("raw_gold", 1)], (0.40, 0.36, 0.26), "OreFleck((1.0, 0.82, 0.20))", layer_hint="Some(4)")
block("pitchblende", 6.0, 4, [("uranium_ore", 1)], (0.15, 0.18, 0.12),
      "OreFleck((0.40, 0.90, 0.30))", emissive="0.3", layer_hint="Some(5)",
      radiation="Some((radius: 4.0, dose_per_sec: 0.5))")
block("monazite_sand", 4.6, 4, [("monazite", 2)], (0.50, 0.44, 0.34), "OreFleck((0.85, 0.55, 0.75))", layer_hint="Some(5)")
block("core_crystal", 7.0, 5, [("core_crystal", 1)], (0.55, 0.20, 0.65), "Glow",
      emissive="0.8", layer_hint="Some(6)",
      radiation="Some((radius: 3.0, dose_per_sec: 0.2))")

# placed blocks
block("ladder", 0.4, 1, [("ladder", 1)], (0.55, 0.40, 0.22), "Grain", solid="false")
block("timber_support", 1.2, 1, [("timber_support", 1)], (0.45, 0.32, 0.18), "Grain")
block("oil_lamp", 0.3, 1, [("oil_lamp", 1)], (1.0, 0.85, 0.45), "Glow", light="14", solid="false", emissive="0.9")
block("brick_block", 1.6, 1, [("brick_block", 1)], (0.62, 0.30, 0.22), "Bricks")
block("glass_block", 0.4, 1, [("glass_block_item", 1)], (0.75, 0.85, 0.90), "Solid", solid="true")
block("water", 0.0, 0, [], (0.12, 0.32, 0.62), "Fluid", solid="false", fluid="true")
block("copper_pipe_block", 0.8, 1, [("copper_pipe_block", 1)], (0.75, 0.45, 0.25), "Solid", solid="false")
block("cable_block", 0.6, 1, [("cable_block", 1)], (0.30, 0.30, 0.34), "Solid", solid="false")
block("conveyor", 0.9, 1, [("conveyor", 1)], (0.35, 0.35, 0.38), "Bands", solid="false")
block("rail", 0.5, 1, [("rail", 1)], (0.50, 0.50, 0.55), "Bands", solid="false")
block("lead_shielding", 3.0, 2, [("lead_shielding", 1)], (0.42, 0.44, 0.50), "Bricks")
block("reactor_casing", 4.0, 3, [("reactor_casing", 1)], (0.55, 0.56, 0.60), "Bricks")
block("graphite_moderator", 2.5, 3, [("graphite_moderator", 1)], (0.16, 0.16, 0.18), "Bands")

# machine blocks
for machine_id, name, size, *_rest in MACHINES:
    block(machine_id, 3.0, 1, [(f"kit_{machine_id}", 1)], (0.40, 0.42, 0.46), "Bricks",
          machine=f'Some("{machine_id}")', solid="true")

# --------------------------------------------------------------------------------------------
# Recipes
# --------------------------------------------------------------------------------------------
RECIPES = []


def recipe(id, machine, time, inputs, outputs, tech=None, power=0.0, steam=0.0):
    RECIPES.append(dict(id=id, machine=machine, time=time, inputs=inputs, outputs=outputs,
                        tech=tech, power=power, steam=steam))


# --- hand crafting -------------------------------------------------------------------------
recipe("craft_workbench", "hand", 1.0, [("timber", 10)], [("kit_workbench", 1)], "hand_tools")
recipe("craft_storage_crate", "hand", 1.0, [("timber", 8)], [("kit_storage_crate", 1)], "hand_tools")
recipe("craft_research_desk", "hand", 1.0, [("timber", 12), ("stone", 8)], [("kit_research_desk", 1)], "hand_tools")
recipe("craft_ladder", "hand", 0.5, [("timber", 2)], [("ladder", 4)], "hand_tools")
recipe("craft_timber_support", "hand", 0.5, [("timber", 3)], [("timber_support", 2)], "hand_tools")
recipe("craft_oil_lamp", "hand", 0.5, [("timber", 1), ("clay", 2), ("coal", 1)], [("oil_lamp", 1)], "hand_tools")
recipe("craft_steel_pickaxe", "hand", 2.0, [("iron_ingot", 3), ("timber", 2)], [("steel_pickaxe", 1)], "smelting")
recipe("craft_brick_block", "hand", 0.5, [("brick", 4)], [("brick_block", 1)], "brick_making")
recipe("craft_glass_block", "hand", 0.5, [("glass", 4)], [("glass_block_item", 1)], "smelting")
recipe("craft_copper_pipe_block", "hand", 0.5, [("copper_pipe", 1)], [("copper_pipe_block", 1)], "steam_power")
recipe("craft_cable_block", "hand", 0.5, [("copper_wire", 2)], [("cable_block", 1)], "electricity")
recipe("craft_rail", "hand", 0.5, [("iron_rod", 1), ("timber", 1)], [("rail", 4)], "logistics")
recipe("craft_conveyor", "hand", 1.0, [("iron_plate", 1), ("bronze_gear", 1)], [("conveyor", 1)], "logistics")
recipe("craft_lead_shielding", "hand", 1.0, [("lead_plate", 2)], [("lead_shielding", 1)], "hazard_gear")
recipe("craft_reactor_casing", "hand", 1.0, [("stainless_steel", 2), ("lead_plate", 1)], [("reactor_casing", 1)], "reactor_engineering")
recipe("craft_graphite_moderator", "hand", 1.0, [("graphite", 4)], [("graphite_moderator", 1)], "reactor_engineering")
recipe("craft_hardened_pickaxe", "hand", 3.0, [("steel_ingot", 4), ("timber", 2)], [("hardened_steel_pickaxe", 1)], "steelmaking")
recipe("craft_steam_canister", "hand", 2.0, [("steel_plate", 2), ("copper_pipe", 1)], [("steam_canister", 1)], "steam_drill")
recipe("craft_module_speed", "hand", 2.0, [("circuit_mk1", 1), ("bronze_gear", 2)], [("module_speed", 1)], "circuitry")
recipe("craft_module_efficiency", "hand", 2.0, [("circuit_mk1", 1), ("copper_wire", 4)], [("module_efficiency", 1)], "circuitry")

# --- machine kits (one hand recipe per machine) ----------------------------------------------
for machine_id, name, size, energy, in_slots, out_slots, cost, tech, extras in MACHINES:
    recipe(f"craft_kit_{machine_id}", "hand", 1.5, cost, [(f"kit_{machine_id}", 1)], tech)

# --- Era 1: kiln, furnace, anvil, blast furnace ----------------------------------------------
recipe("fire_brick", "brick_kiln", 4.0, [("clay", 2), ("sand", 1)], [("fire_brick", 1)], "brick_making")
recipe("brick", "brick_kiln", 3.0, [("clay", 2)], [("brick", 1)], "brick_making")
recipe("smelt_copper", "stone_furnace", 5.0, [("raw_copper", 1)], [("copper_ingot", 1)], "smelting")
recipe("smelt_tin", "stone_furnace", 5.0, [("raw_tin", 1)], [("tin_ingot", 1)], "smelting")
recipe("smelt_iron", "stone_furnace", 6.0, [("raw_iron", 1)], [("iron_ingot", 1)], "smelting")
recipe("smelt_lead", "stone_furnace", 6.0, [("raw_lead", 1)], [("lead_ingot", 1)], "smelting")
recipe("smelt_zinc", "stone_furnace", 6.0, [("raw_zinc", 1)], [("zinc_ingot", 1)], "smelting")
recipe("smelt_gold", "stone_furnace", 8.0, [("raw_gold", 1)], [("gold_ingot", 1)], "smelting")
recipe("smelt_glass", "stone_furnace", 4.0, [("sand", 2), ("coal", 1)], [("glass", 1)], "smelting")
recipe("alloy_bronze", "stone_furnace", 8.0, [("copper_ingot", 3), ("tin_ingot", 1)], [("bronze_ingot", 4)], "alloying")
recipe("refine_zinc", "stone_furnace", 10.0, [("zinc_ingot", 4), ("sulfur", 1)], [("cadmium", 1)], "smelting")
recipe("forge_iron_plate", "anvil", 4.0, [("iron_ingot", 1)], [("iron_plate", 1)], "smelting")
recipe("forge_lead_plate", "anvil", 4.0, [("lead_ingot", 1)], [("lead_plate", 1)], "smelting")
recipe("forge_copper_pipe", "anvil", 3.0, [("copper_ingot", 1)], [("copper_pipe", 2)], "steam_power")
recipe("forge_copper_wire", "anvil", 2.0, [("copper_ingot", 1)], [("copper_wire", 4)], "electricity")
recipe("forge_iron_rod", "anvil", 3.0, [("iron_ingot", 1)], [("iron_rod", 2)], "smelting")
recipe("forge_bronze_gear", "anvil", 4.0, [("bronze_ingot", 2)], [("bronze_gear", 1)], "alloying")
recipe("forge_screw", "anvil", 2.0, [("iron_rod", 1)], [("screw", 4)], "smelting")
recipe("steel", "blast_furnace", 12.0, [("iron_ingot", 2), ("coal", 2), ("limestone", 1)], [("steel_ingot", 2)], "steelmaking")
recipe("forge_steel_plate", "anvil", 5.0, [("steel_ingot", 1)], [("steel_plate", 1)], "steelmaking")
recipe("forge_steel_rod", "anvil", 4.0, [("steel_ingot", 1)], [("steel_rod", 2)], "steelmaking")

# --- Era 2: steam, ore processing, logistics --------------------------------------------------
# The Crusher and Ore Washer are the yield ladder: 1 ingot per ore becomes 1.5, then 2.
recipe("crush_copper", "crusher", 5.0, [("raw_copper", 2)], [("copper_ingot", 3)], "ore_processing")
recipe("crush_iron", "crusher", 6.0, [("raw_iron", 2)], [("iron_ingot", 3)], "ore_processing")
recipe("crush_lead", "crusher", 6.0, [("raw_lead", 2)], [("lead_ingot", 3)], "ore_processing")
recipe("crush_zinc", "crusher", 6.0, [("raw_zinc", 2)], [("zinc_ingot", 3)], "ore_processing")
recipe("crush_uranium", "crusher", 8.0, [("uranium_ore", 1)], [("crushed_uranium_ore", 1)], "uranium_milling")
recipe("wash_copper", "ore_washer", 4.0, [("raw_copper", 2), ("sand", 1)], [("copper_ingot", 4)], "ore_processing")
recipe("wash_iron", "ore_washer", 5.0, [("raw_iron", 2), ("sand", 1)], [("iron_ingot", 4)], "ore_processing")
recipe("wash_lead", "ore_washer", 5.0, [("raw_lead", 2), ("sand", 1)], [("lead_ingot", 4)], "ore_processing")
recipe("wash_zinc", "ore_washer", 5.0, [("raw_zinc", 2), ("sand", 1)], [("zinc_ingot", 4)], "ore_processing")
recipe("craft_steam_drill", "anvil", 12.0, [("steel_ingot", 6), ("bronze_gear", 4), ("copper_pipe", 4)], [("steam_drill", 1)], "steam_drill")
recipe("refill_canister", "steam_boiler", 6.0, [("steam_canister", 1)], [("steam_canister", 1)], "steam_drill")

# --- Era 3: electricity ------------------------------------------------------------------------
recipe("arc_smelt_steel", "arc_furnace", 6.0, [("iron_ingot", 2), ("coal", 1)], [("steel_ingot", 2)], "arc_smelting", power=40.0)
recipe("arc_silicon", "arc_furnace", 8.0, [("quartz", 3), ("coal", 1)], [("silicon", 1)], "arc_smelting", power=50.0)
recipe("arc_tungsten", "arc_furnace", 10.0, [("raw_tungsten", 2), ("coal", 2)], [("tungsten_ingot", 1)], "arc_smelting", power=60.0)
recipe("arc_nickel", "arc_furnace", 8.0, [("raw_nickel", 2), ("coal", 1)], [("nickel_ingot", 1)], "arc_smelting", power=55.0)
recipe("arc_chromium", "arc_furnace", 9.0, [("raw_chromium", 2), ("coal", 2)], [("chromium_ingot", 1)], "arc_smelting", power=60.0)
recipe("electrolyse_aluminium", "electrolysis_cell", 12.0, [("raw_bauxite", 2), ("coal", 1)], [("aluminium_ingot", 1)], "electrolysis", power=45.0)
recipe("electrolyse_salt", "electrolysis_cell", 8.0, [("salt", 2)], [("chlorine", 1), ("lye", 1)], "electrolysis", power=40.0)
recipe("assemble_circuit_mk1", "circuit_assembler", 8.0, [("silicon", 1), ("copper_wire", 2), ("gold_ingot", 1)], [("circuit_mk1", 1)], "circuitry", power=30.0)
recipe("assemble_circuit_mk2", "circuit_assembler", 14.0, [("silicon", 2), ("circuit_mk1", 1), ("gold_ingot", 2)], [("circuit_mk2", 1)], "circuitry", power=45.0)
recipe("assemble_battery", "circuit_assembler", 10.0, [("lead_plate", 2), ("sulfuric_acid", 1), ("copper_wire", 4)], [("battery_pack", 1)], "energy_storage", power=35.0)
recipe("charge_battery", "charging_station", 6.0, [("battery_pack", 1)], [("charged_battery", 1)], "energy_storage", power=25.0)
recipe("craft_geiger_counter", "circuit_assembler", 12.0, [("circuit_mk1", 1), ("glass", 1), ("copper_wire", 2)], [("geiger_counter", 1)], "hazard_gear", power=30.0)
recipe("craft_headlamp_mk2", "circuit_assembler", 10.0, [("circuit_mk1", 1), ("glass", 1), ("battery_pack", 1)], [("headlamp_mk2", 1)], "circuitry", power=25.0)
recipe("craft_ore_scanner", "circuit_assembler", 16.0, [("circuit_mk1", 2), ("quartz", 4), ("copper_wire", 6)], [("ore_scanner", 1)], "ore_scanning", power=35.0)

# --- Era 4: chemistry and deep metallurgy -------------------------------------------------------
recipe("make_sulfuric_acid", "chemical_plant", 10.0, [("sulfur", 2), ("salt", 1)], [("sulfuric_acid", 1)], "chemistry", power=50.0)
recipe("make_fluorine", "chemical_plant", 12.0, [("fluorite", 2), ("sulfuric_acid", 1)], [("fluorine_compound", 1)], "chemistry", power=60.0)
recipe("bleach_steel", "chemical_plant", 14.0, [("stainless_steel", 2), ("chlorine", 1)], [("stainless_steel", 3)], "chemistry", power=55.0)
recipe("solidify_tailings", "chemical_plant", 20.0, [("radioactive_tailings", 2), ("lead_ingot", 1)], [("waste_cask", 1)], "waste_handling", power=60.0)
recipe("make_stainless", "alloy_furnace", 16.0, [("steel_ingot", 3), ("nickel_ingot", 1), ("chromium_ingot", 1)], [("stainless_steel", 2)], "advanced_alloys", power=70.0)
recipe("make_tungsten_carbide", "alloy_furnace", 18.0, [("tungsten_ingot", 2), ("graphite", 2)], [("tungsten_carbide", 1)], "advanced_alloys", power=80.0)
recipe("cast_shielding", "alloy_furnace", 14.0, [("depleted_canister", 1), ("lead_ingot", 2)], [("dense_shielding_plate", 1)], "waste_handling", power=50.0)
recipe("dense_reactor_casing", "alloy_furnace", 20.0, [("reactor_casing", 2), ("dense_shielding_plate", 1)], [("reactor_casing", 3)], "waste_handling", power=70.0)
recipe("make_bearing", "machine_shop", 10.0, [("steel_rod", 2), ("bronze_gear", 1), ("screw", 2)], [("bearing", 1)], "precision_machining", power=45.0)
recipe("make_turbine_blade", "machine_shop", 14.0, [("stainless_steel", 1), ("bearing", 1)], [("turbine_blade", 1)], "precision_machining", power=55.0)
recipe("make_centrifuge_rotor", "machine_shop", 16.0, [("stainless_steel", 2), ("bearing", 1)], [("centrifuge_rotor", 1)], "precision_machining", power=60.0)
recipe("make_stainless_rotor", "machine_shop", 18.0, [("stainless_steel", 3), ("centrifuge_rotor", 1)], [("stainless_rotor", 1)], "precision_machining", power=70.0)
recipe("make_drill_head", "machine_shop", 20.0, [("tungsten_carbide", 2), ("stainless_steel", 1)], [("tungsten_carbide_drill_head", 1)], "precision_machining", power=75.0)
recipe("make_zirconium_tubing", "machine_shop", 14.0, [("raw_zircon", 2), ("stainless_steel", 1)], [("zirconium_tubing", 1)], "precision_machining", power=60.0)
recipe("make_control_rod", "machine_shop", 16.0, [("cadmium", 2), ("stainless_steel", 2)], [("control_rod", 1)], "control_rods", power=60.0)
recipe("machine_iron_plate", "machine_shop", 3.0, [("iron_ingot", 2)], [("iron_plate", 3)], "precision_machining", power=30.0)
recipe("machine_steel_plate", "machine_shop", 4.0, [("steel_ingot", 2)], [("steel_plate", 3)], "precision_machining", power=35.0)
recipe("make_graphite_electrode", "machine_shop", 8.0, [("graphite", 4), ("steel_rod", 1)], [("graphite_electrode", 1)], "arc_smelting", power=40.0)
recipe("separate_rare_earths", "rare_earth_separator", 20.0, [("monazite", 3), ("sulfuric_acid", 1)], [("rare_earth_magnet", 1)], "rare_earths", power=90.0)
recipe("craft_cooling_suit", "machine_shop", 12.0, [("aluminium_ingot", 4), ("copper_pipe", 2)], [("cooling_suit", 1)], "hazard_gear", power=30.0)
recipe("craft_respirator", "machine_shop", 10.0, [("glass", 2), ("copper_wire", 2)], [("respirator", 1)], "hazard_gear", power=25.0)
recipe("craft_hazmat_suit", "machine_shop", 18.0, [("lead_plate", 6), ("glass", 2), ("stainless_steel", 2)], [("hazmat_suit", 1)], "hazard_gear", power=40.0)
recipe("craft_tungsten_drill", "machine_shop", 24.0, [("tungsten_carbide", 6), ("stainless_steel", 4), ("battery_pack", 2)], [("tungsten_carbide_drill", 1)], "tungsten_drill", power=80.0)

# --- Era 5: the nuclear chain ---------------------------------------------------------------------
recipe("leach_uranium", "leaching_tank", 20.0, [("crushed_uranium_ore", 2), ("sulfuric_acid", 1)],
       [("uranium_solution", 1), ("radioactive_tailings", 1)], "uranium_milling", power=40.0)
recipe("precipitate_uranium", "precipitator", 16.0, [("uranium_solution", 1), ("lye", 1)],
       [("yellowcake", 1)], "uranium_milling", power=35.0)
recipe("convert_uranium", "conversion_plant", 24.0, [("yellowcake", 1), ("fluorine_compound", 1)],
       [("uranium_hexafluoride", 1)], "conversion", power=110.0)
recipe("enrich_uranium", "centrifuge", 30.0, [("uranium_hexafluoride", 1)],
       [("enriched_canister", 1), ("depleted_canister", 1)], "enrichment", power=150.0)
recipe("fabricate_fuel_rod", "fuel_fabricator", 20.0, [("enriched_canister", 1), ("zirconium_tubing", 1)],
       [("fuel_rod", 1)], "fuel_fabrication", power=100.0)
recipe("fabricate_core_cell", "fuel_fabricator", 30.0, [("enriched_canister", 1), ("stainless_steel", 2), ("lead_plate", 2)],
       [("compact_core_cell", 1)], "fuel_fabrication", power=120.0)
recipe("recycle_spent_fuel", "cooling_pool", 60.0, [("spent_fuel_rod", 1)], [("waste_cask", 1)], "waste_handling", power=25.0)
recipe("recycle_spent_cell", "cooling_pool", 60.0, [("spent_core_cell", 1)], [("waste_cask", 1)], "waste_handling", power=25.0)
recipe("craft_nuclear_drill", "machine_shop", 40.0,
       [("compact_core_cell", 1), ("tungsten_carbide_drill_head", 1), ("rare_earth_magnet", 4), ("lead_shielding", 4)],
       [("nuclear_drill", 1)], "nuclear_drill", power=120.0)


# --------------------------------------------------------------------------------------------
# Layers
# --------------------------------------------------------------------------------------------
LAYERS = [
    dict(id="topsoil", name="Topsoil", depth=(0, 16), base_block="dirt",
         fillers=[("gravel", 0.15), ("clay", 0.15), ("sand", 0.12)],
         ores=[("coal_seam", 0.02, (4, 16)), ("clay", 0.05, (6, 24)), ("sand", 0.04, (6, 20))],
         hazards=[], ambient_temp=12.0, min_tier=1, cave_density=0.4),
    dict(id="shale_beds", name="Shale Beds", depth=(16, 64), base_block="stone",
         fillers=[("limestone", 0.18), ("shale", 0.22)],
         ores=[("coal_seam", 0.030, (6, 28)), ("malachite", 0.022, (4, 18)),
               ("cassiterite", 0.012, (4, 14)), ("hematite", 0.026, (4, 20))],
         hazards=[], ambient_temp=16.0, min_tier=1, cave_density=1.0),
    dict(id="granite_depths", name="Granite Depths", depth=(64, 160), base_block="granite",
         fillers=[("stone", 0.15)],
         ores=[("hematite", 0.020, (6, 22)), ("galena", 0.013, (4, 16)),
               ("sphalerite", 0.012, (4, 16)), ("sulfur_deposit", 0.011, (4, 14)),
               ("quartz_block", 0.012, (4, 14)), ("bauxite", 0.012, (4, 16)),
               ("rock_salt", 0.024, (6, 22)), ("coal_seam", 0.012, (6, 20))],
         hazards=['Flooding(pocket_chance: 0.004, max_volume: 512)'],
         ambient_temp=22.0, min_tier=2, cave_density=1.0),
    dict(id="basalt_furnace", name="Basalt Furnace", depth=(160, 288), base_block="basalt",
         fillers=[("stone", 0.10)],
         ores=[("pentlandite", 0.013, (4, 16)), ("wolframite", 0.008, (3, 12)),
               ("chromite", 0.012, (4, 16)), ("graphite_vein", 0.013, (4, 18)),
               ("fluorite_block", 0.008, (3, 12)), ("zircon_block", 0.008, (3, 12)),
               ("gold_vein", 0.007, (3, 10)), ("sulfur_deposit", 0.010, (4, 14))],
         hazards=['Heat(base_rate: 2.0, depth_rate: 0.02)',
                  'Gas(pocket_chance: 0.005, oxygen_drain: 6.0)'],
         ambient_temp=38.0, min_tier=3, cave_density=0.9),
    dict(id="deep_crust", name="Deep Crust", depth=(288, 448), base_block="gneiss",
         fillers=[("obsidian", 0.12)],
         ores=[("pitchblende", 0.006, (3, 10)), ("monazite_sand", 0.009, (4, 14)),
               ("gold_vein", 0.006, (3, 10)), ("graphite_vein", 0.010, (4, 16)),
               ("wolframite", 0.007, (3, 12))],
         hazards=['Radiation(dose_per_sec: 0.02)',
                  'CaveIn(max_span: 7)',
                  'Heat(base_rate: 4.0, depth_rate: 0.03)'],
         ambient_temp=58.0, min_tier=4, cave_density=0.8),
    dict(id="mantle_edge", name="Mantle Edge", depth=(448, 512), base_block="mantle_rock",
         fillers=[("obsidian", 0.10)],
         ores=[("pitchblende", 0.020, (6, 24)), ("core_crystal", 0.004, (2, 6))],
         hazards=['Radiation(dose_per_sec: 0.08)',
                  'Heat(base_rate: 8.0, depth_rate: 0.05)',
                  'CaveIn(max_span: 6)'],
         ambient_temp=95.0, min_tier=5, cave_density=0.6),
]

# --------------------------------------------------------------------------------------------
# Emit RON
# --------------------------------------------------------------------------------------------
def machine_energy(energy):
    """Energy is a tuple field, so RON needs the struct variant in parens."""
    if energy.endswith("}"):
        name, _, body = energy.partition(" ")
        return f"{name}(" + body.strip().removeprefix("{").removesuffix("}").strip() + ")"
    return energy


def fmt_f(v):
    return f"{float(v)}"


def fmt_list(items):
    return "[" + ", ".join(f'("{i}", {c})' for i, c in items) + "]"


def emit_blocks():
    out = ["// Deepcore blocks. Generated content; the file is the runtime source of truth.", "["]
    for b in BLOCKS:
        parts = [f'id: "{b["id"]}"', f'hardness: {fmt_f(b["hardness"])}', f'min_tier: {b["min_tier"]}',
                 f'drops: {fmt_list(b["drops"])}',
                 f'color: ({b["color"][0]}, {b["color"][1]}, {b["color"][2]})',
                 f'pattern: {b["pattern"]}']
        for key in ("emissive", "radiation", "solid", "fluid", "light", "layer_hint", "machine", "unbreakable"):
            if key in b:
                parts.append(f"{key}: {b[key]}")
        out.append("    (" + ", ".join(parts) + "),")
    out.append("]")
    return "\n".join(out) + "\n"


def emit_items():
    out = ["// Deepcore items. Generated content; the file is the runtime source of truth.", "["]
    sinks = {"fuel_rod": "reactor", "waste_cask": "waste_vault"}
    for item_id, name, stack, category in ITEMS:
        sink = f', sink: Some("{sinks[item_id]}")' if item_id in sinks else ""
        out.append(f'    (id: "{item_id}", name: "{name}", stack: {stack}, category: {category}{sink}),')
    out.append("]")
    return "\n".join(out) + "\n"


def emit_machines():
    out = ["// Deepcore machines. Generated content; the file is the runtime source of truth.", "["]
    for machine_id, name, size, energy, in_slots, out_slots, cost, tech, extras in MACHINES:
        parts = [f'id: "{machine_id}"', f'name: "{name}"',
                 f"size: ({size[0]}, {size[1]}, {size[2]})", f"energy: {machine_energy(energy)}",
                 f"input_slots: {in_slots}", f"output_slots: {out_slots}",
                 f"build_cost: {fmt_list(cost)}", f'tech: Some("{tech}")']
        for key, value in extras.items():
            parts.append(f"{key}: {value}")
        out.append("    (" + ", ".join(parts) + "),")
    out.append("]")
    return "\n".join(out) + "\n"


def emit_recipes():
    out = ["// Deepcore recipes. Generated content; the file is the runtime source of truth.", "["]
    for r in RECIPES:
        parts = [f'id: "{r["id"]}"', f'machine: "{r["machine"]}"', f'time: {fmt_f(r["time"])}']
        if r["power"]:
            parts.append(f'power_kw: {fmt_f(r["power"])}')
        parts.append(f'inputs: {fmt_list(r["inputs"])}')
        parts.append(f'outputs: {fmt_list(r["outputs"])}')
        if r["tech"]:
            parts.append(f'tech: Some("{r["tech"]}")')
        out.append("    (" + ", ".join(parts) + "),")
    out.append("]")
    return "\n".join(out) + "\n"


def emit_tech():
    out = ["// Deepcore tech tree. Generated content; the file is the runtime source of truth.", "["]
    for tech_id, name, era, parents, cost, unlocks in TECH:
        parents_s = "[" + ", ".join(f'"{p}"' for p in parents) + "]"
        unlocks_s = "[" + ", ".join(f'"{u}"' for u in unlocks) + "]"
        out.append(f'    (id: "{tech_id}", name: "{name}", era: {era}, parents: {parents_s}, '
                   f'cost: {fmt_list(cost)}, unlocks: {unlocks_s}),')
    out.append("]")
    return "\n".join(out) + "\n"


def emit_layers():
    out = ["// Deepcore layers. Generated content; the file is the runtime source of truth.", "["]
    for layer in LAYERS:
        fillers = "[" + ", ".join(f'("{b}", {s})' for b, s in layer["fillers"]) + "]"
        ores = "[" + ", ".join(
            f'(block: "{b}", frequency: {f}, vein_size: ({lo}, {hi}))'
            for b, f, (lo, hi) in layer["ores"]) + "]"
        hazards = "[" + ", ".join(layer["hazards"]) + "]"
        out.append(
            f'    (id: "{layer["id"]}", name: "{layer["name"]}", '
            f'depth: ({layer["depth"][0]}, {layer["depth"][1]}), '
            f'base_block: "{layer["base_block"]}", fillers: {fillers}, ores: {ores}, '
            f'hazards: {hazards}, ambient_temp: {layer["ambient_temp"]}, '
            f'min_tier: {layer["min_tier"]}, cave_density: {layer["cave_density"]}),')
    out.append("]")
    return "\n".join(out) + "\n"


HAZARDS = """// Deepcore hazard tuning. The file is the runtime source of truth.
(
    dose_grain: 100.0,
    dose_no_regen: 300.0,
    dose_damage: 600.0,
    dose_damage_rate: 3.0,
    decon_rate: 25.0,
    dose_decay: 0.4,
    heat_limit: 100.0,
    heat_cooling: 1.5,
    oxygen_capacity: 100.0,
    oxygen_drain_water: 6.0,
    oxygen_drain_gas: 8.0,
    oxygen_regen: 12.0,
    suffocation_damage: 8.0,
    gas_ignition_damage: 35.0,
    meltdown_zone_seconds: 1200.0,
    max_health: 100.0,
    max_stamina: 100.0,
    sprint_stamina: 12.0,
    climb_stamina: 6.0,
    stamina_regen: 10.0,
    fall_damage_per_block: 4.0,
    safe_fall_blocks: 3.0,
)
"""


def main():
    os.makedirs(OUT, exist_ok=True)
    files = {
        "blocks.ron": emit_blocks(),
        "items.ron": emit_items(),
        "machines.ron": emit_machines(),
        "recipes.ron": emit_recipes(),
        "tech_tree.ron": emit_tech(),
        "layers.ron": emit_layers(),
        "hazards.ron": HAZARDS,
    }
    for name, text in files.items():
        with open(os.path.join(OUT, name), "w") as handle:
            handle.write(text)
    print(f"wrote {len(files)} files to {OUT}")
    print(f"  blocks {len(BLOCKS)}  items {len(ITEMS)}  machines {len(MACHINES)}  "
          f"recipes {len(RECIPES)}  tech {len(TECH)}  layers {len(LAYERS)}")


if __name__ == "__main__":
    main()
