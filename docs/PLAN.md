# Deepcore — Plan

## What the game is

Deepcore is a co-op first-person voxel mining game for up to 16 players on one shared,
persistent 1024 × 1024 × 512-block mine. Everyone starts with a steel pickaxe at a surface
camp (the Base). Progression is depth: dig down, haul ore home, build a machine, the machine
unlocks a better tool or material, which lets you dig deeper. Six Eras take the team from a
hand pickaxe to a nuclear reactor and a Nuclear Drill that bores 3 × 3 tunnels.

The loop is always: **dig → haul → build → unlock → dig deeper**.

## Architectural rules (non-negotiable)

1. `shared` holds every game rule (voxels, worldgen, movement, inventory, recipes, machines,
   power, hazards, protocol). It has no rendering and no sockets.
2. `server` decides. Every gameplay rule runs there. The client sends intents only.
3. `client` shows. It predicts its own movement and its own mining crack animation, nothing else.
4. Content is data. Blocks, items, recipes, machines, tech nodes, layers and hazards live in
   `assets/data/*.ron`, are validated at load, and hash-checked during the handshake. Adding an
   ore or a recipe must never touch Rust.
5. One Bevy plugin per feature. Files under 400 lines, named by purpose.
6. No `todo!()`, no empty systems, no "later" comments in shipped code. Deferred work is
   removed from the tree and listed in `PROGRESS.md`.

## Crate layout

```
deepcore/
├── Cargo.toml                  workspace, pinned versions, profiles
├── crates/shared/              all game rules
│   ├── content/                RON loaders → registries (BlockId/ItemId/RecipeId/...)
│   ├── voxel/                  Chunk (palette + bit-packed), BlockPos/ChunkPos maths
│   ├── worldgen/               layers, caves, ore veins, water/gas pockets (seeded, deterministic)
│   ├── movement/               AABB-vs-voxel collision + player controller (shared by both sides)
│   ├── inventory/              stacks, slots, transfer rules
│   ├── crafting/               recipe matching + machine state machine
│   ├── power/                  steam and electric network solvers (incl. brownout)
│   ├── hazards/                radiation, heat, gas, oxygen maths
│   └── protocol/               PROTOCOL_VERSION, replicated components, intents, events, channels
├── crates/server/              headless authoritative server (MinimalPlugins)
│   ├── net/                    renet transport, handshake, visibility
│   ├── world/                  chunk cache, generation workers, persistence
│   └── sim/                    mining validation, machine ticks, power solve, hazards
├── crates/client/              windowed game
│   ├── render/                 async meshing, procedural atlas, lighting, effects
│   ├── input/                  FPS controls, raycast targeting
│   ├── prediction/             local movement prediction + reconciliation
│   ├── ui/                     egui: inventory, machine screens, tech tree, menus
│   ├── hud/                    hotbar, crosshair, vitals, depth meter, Geiger
│   └── audio/                  procedural SFX (no downloaded assets)
├── crates/lobby/               axum + sqlx accounts, tokens, server browser
├── tools/atlas_gen/            builds the block texture atlas from blocks.ron
└── assets/data/                the content, in RON
```

## Pinned stack (checked against crates.io on 2026-11-12)

| Crate | Version | Note |
|---|---|---|
| bevy | 0.20.1 | newest stable; 0.20 added BSN, deprecated `ui::Interaction`/`ui::widgets::Button` |
| bevy_replicon | 0.45.5 | requires bevy ^0.20 |
| bevy_replicon_renet | 0.21.0 | requires bevy_replicon ^0.45, renet 6.0 |
| bevy_egui | 0.43.1 | requires bevy ^0.20, egui 0.36 |
| noise 0.9, rand 0.9, rand_chacha 0.9 | | deterministic worldgen |
| serde 1.0 / ron 0.12 / postcard 1.1 / zstd 0.13 | | content + wire + save compression |
| rusqlite 0.37 (bundled), axum 0.8, sqlx 0.8, argon2 0.5 | | persistence and lobby |

Rust edition 2024, stable toolchain 1.99.

## Simulation model

* Server fixed tick 30 Hz (`Time<Fixed>` at 30 Hz, Bevy `FixedUpdate`). Machines, power and
  hazards tick there; replicon's `ServerTick` is aligned to it.
* Player movement runs in `FixedUpdate` at 60 Hz on both sides from the same pure function, so
  prediction and authority agree bit-for-bit given the same inputs.
* Chunks are 32³. Only modified chunks are persisted; the rest regenerate from the seed.
* Machines are block entities (ECS entities with a `BlockPos`), never voxel data.

## Content model

`blocks.ron` — hardness, min_tier, drops, colour + pattern (for the procedural atlas),
emissive, radiation, flags (solid/liquid/light-emitting).
`items.ron` — stack size, category; tools carry tier/speed/area/energy.
`recipes.ron` — machine, time, power, inputs, outputs, tech gate.
`machines.ron` — size, energy kind (fuel/steam/electric), slot counts, build cost, tech.
`tech_tree.ron` — era, parents, item cost, unlocks.
`layers.ron` — depth band, base block, ore table (frequency + vein size), hazards, ambient temp.
`hazards.ron` — the tuning numbers for darkness, flooding, gas, heat, cave-ins, radiation.

The validator fails the load with a readable error on: unknown item/block/machine/tech
references, recipes no machine can run, tech nodes with missing parents, ores with no use, and
items no recipe produces (dead ends). `balance_report` walks the tech tree and prints raw ore
needed per Era.

## Milestones

| # | Milestone | Done when |
|---|---|---|
| M0 | Workspace, four crates, pinned versions, `scripts/check.sh`; client shows a lit cube, server ticks at 30 Hz | both binaries start, check.sh green |
| M1 | Voxels, palette chunks, deterministic worldgen (6 layers, caves, ores), procedural atlas, greedy meshing, FPS controller, mining/placing, hotbar, depth meter | dig to the bottom of layer 2, blocked by granite with a tier-1 pick |
| M2 | Authoritative server + client, handshake (protocol + content hash), chunk streaming with interest management, replicated players with interpolation, prediction/reconciliation, validated intents, host/join menu, latency sim, bots | two clients see each other dig; responsive at 150 ms; 8 bots for 10 min clean |
| M3 | Content loaders + validator + balance_report, inventory UI, pickups, death crate, workbench crafting, trees/timber, ladders, supports, lamps, darkness | Era 0 playable in co-op, validator clean |
| M4 | Block entities, multi-block ghosts, machine UI, fuel burning, kiln/furnace/anvil/blast furnace, research desk + tech tree UI | team reaches Hardened Steel Pickaxe and layer 3 |
| M5 | Pipes + steam solver, pump/boiler/engine, crusher/washer yields, conveyors, rails/minecart/lift, steam drill, flooding + oxygen | automated ore → ingot line, reach layer 4 |
| M6 | Cables + electric solver, batteries, brownout, turbine/generator/arc furnace/electrolysis/circuit assembler/charger, modules, ore scanner, vent fan, electric pump, gas | base powered, brownouts correct, gas clearable |
| M7 | Chemical plant, alloy furnace, machine shop, rare-earth separator, decon shower, heat + cave-ins, suits/respirator/hazmat/geiger, tungsten drill | reach layer 5 and survive there |
| M8 | Leach → precipitate → convert → centrifuge cascade → fuel, reactor + console + scram + overheat, cooling pool, waste vault, nuclear drill 3×3, radiation | headless nuclear integration test green; 3×3 tunnel into layer 6 in a real session |
| M9 | Region files + SQLite, autosave, crash-safe writes, audio, particles, menus/settings, objective list, balance pass | restart restores world/machines/inventories/tech exactly |
| M10 | Lobby (accounts, argon2, tokens, server registration), server browser, Dockerfiles, docs | fresh machine runs lobby + dedicated server in Docker, two clients log in and play |

## Testing strategy

* Unit tests in `shared` for every pure rule (palette round-trips, worldgen determinism,
  break-time formula, inventory moves with `proptest`, recipe matching, steam/electric solving
  incl. brownouts, radiation maths).
* Headless Bevy `App` tests for the server plugins: script intents, step the schedule, assert
  the block was mined / item crafted / machine produced output.
* One progression integration test per Era; the M8 nuclear chain test is the most important.
* Network convergence tests running server and client apps in one process.

## Risks / decisions taken

* Bevy 0.20 replaced `ui::Interaction`/`Button` with `bevy_ui_widgets` + `Hovered`/`Pressed`;
  the HUD is built from plain `Node` + `Text`, menus from egui, so we avoid the churn.
* `bevy_egui` 0.43 pulls egui 0.36; the inventory and machine screens use egui only.
* No downloaded assets: the atlas is generated at startup from `blocks.ron`, audio is
  synthesised at runtime.
