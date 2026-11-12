# PROGRESS

Deepcore — co-op 3D voxel mining game in Rust (Bevy). This file is the hand-off note: a future
session (possibly a different model) must be able to continue from this file alone.

**Last updated:** M2 complete (multiplayer foundation working end to end).

---

## How to build and run

```bash
# one-time: system libraries for Bevy on Linux
sudo apt-get install -y libasound2-dev libudev-dev libx11-dev libxkbcommon-dev \
    libwayland-dev libvulkan-dev pkg-config

# quality gate (fmt + clippy -D warnings + tests)
./scripts/check.sh

# server (headless, 30 Hz authoritative)
cargo run -p deepcore-server --bin deepcore-server -- --port 24288

# client
cargo run -p deepcore-client -- --connect 127.0.0.1:24288 --name YourName

# balance report (raw ore per Era, nuclear chain cost, tech tree shape)
cargo run -p deepcore-server --bin balance_report
```

Headless (no GPU) smoke test of the client:

```bash
Xvfb :99 -screen 0 1600x900x24 &
DISPLAY=:99 WGPU_BACKEND=vulkan XDG_RUNTIME_DIR=/tmp \
  cargo run -p deepcore-client -- --connect 127.0.0.1:24288
```

## Pinned stack (verified on crates.io 2026-11-12)

bevy 0.20.1 · bevy_replicon 0.45.5 · bevy_replicon_renet 0.21.0 (renet 2.0) · bevy_egui 0.43.1
(egui 0.36) · serde 1 / ron 0.12 / postcard 1.1 / zstd 0.13 · noise 0.9 / rand 0.9 /
rand_chacha 0.9 · rusqlite 0.37 (bundled) · clap 4.5 · anyhow 1 · thiserror 2 · proptest 1.9.
Rust 1.99.1 stable, edition 2024.

Bevy 0.20 API notes that cost time and are worth remembering:

* `ui::Interaction` and `ui::widgets::Button` are deprecated → `bevy_ui_widgets::Button` +
  `Hovered`/`Pressed`. This project uses plain `Node`/`Text` for the HUD and egui for menus.
* `AmbientLight` is a **component** now, not a resource.
* `DirectionalLight::shadows_enabled` → `shadow_maps_enabled`; `SpotLight` has no shadows field.
* `WindowResolution::new(w, h)` takes integers.
* egui 0.36 `CentralPanel`/`Panel::show` take a `&mut Ui`, not a `&mut Context`; windows
  (`egui::Window::new(..).show(ctx, ..)`) still take the context. This project uses windows.
* `Context::wants_pointer_input` → `Context::is_pointer_over_egui`.
* `Mesh3d`, `MeshMaterial3d`, `AudioPlayer::<T>` generic tuple struct for custom sources.
* `bevy_audio` custom sources implement `Decodable` with a rodio `Source` decoder whose
  `channels()`/`sample_rate()` return `NonZero*`.
* Messages: `app.add_message::<T>()`, systems read `MessageReader<T>` / write `MessageWriter<T>`.
  A system whose message type was never registered panics at first run — register before adding
  systems.
* `Query` conflicts (two `&mut Transform` queries) panic at runtime; add `Without<..>` filters.

---

## Milestone status

| # | Milestone | State |
|---|---|---|
| M0 | Workspace, four crates, pinned versions, `scripts/check.sh`, both binaries start | **done** |
| M1 | Voxels, palette chunks, deterministic worldgen (6 layers, caves, ores), procedural atlas, greedy meshing, FPS controller, mining/placing, hotbar, depth meter | **done** |
| M2 | Authoritative server + client, handshake (protocol + content hash), chunk streaming, replicated players, prediction/reconciliation, validated intents, latency flags | **mostly done** — see gaps below |
| M3 | Content, inventory, crafting (Era 0) | partially: content + validator + inventory + hand crafting exist; UI is basic |
| M4 | Machines, research, Era 1 | server side exists (machine tick, fuel, tech tree); no placement UI flow yet |
| M5–M8 | Eras 2–5 | data + shared rules exist (steam/electric networks, nuclear chain recipes, reactor physics); machine placement/interaction UI incomplete |
| M9 | Persistence | region files + SQLite autosave written; load-on-start not implemented |
| M10 | Lobby | not started |

### What works right now (verified by running)

* `cargo test --workspace`: **all green** (shared 57, client 9, server 2).
* `cargo clippy --workspace --all-targets -- -D warnings`: clean.
* Server boots, binds UDP, generates chunks on worker threads, streams them to clients.
* Client boots, builds the procedural atlas, connects, handshakes, receives chunks, decodes
  them, meshes them on the async pool, renders the terrain, follows the player, shows the HUD
  (health/oxygen/dose/suit, depth + layer + Geiger, hotbar, crosshair, notifications).
* Content: 83 blocks, 148 items, 134 recipes, 36 machines, 30 tech nodes, 6 layers — validated
  at load (unknown refs, dead-end items, cycles, layer gaps all fail fast).
* Chunk generation: **2.4 ms/chunk** (budget 5 ms) after rewriting ore veins as per-cell stamps.

### Known gaps and bugs (in priority order)

1. **Machine placement flow is not wired end to end.** `place_machine` exists on the server but
   `PlaceBlock` does not yet route machine kits to it, and the client has no ghost preview. This
   is the next milestone-sized task (M4).
2. **Inventory UI is read-mostly.** Drag/split/quick-move intents exist server-side
   (`MoveItems`), the egui screen issues them, but there is no slot-to-slot drag affordance.
3. **Chunk unload** is implemented server-side (`unload_queue`) but never populated; clients
   keep every chunk they have received.
4. **Persistence loads nothing on startup** — chunks/players/tech are saved but not restored.
5. **Reactor console, scram panel, multi-block assembly** are data + tick only; no build flow.
6. **Bots** (`--bots N`) flag is parsed but not implemented.
7. **Latency/loss simulation flags** are parsed; renet 2.0 has no built-in simulation hook, so
   the flags are inert. Implementing them needs a wrapping transport.
8. Client `SessionState` fallback matches the local player by display name after 1 s if the
   `LocalOwner` visibility-filter marker has not arrived; the marker path should be preferred
   once it is confirmed on a real network.
9. Audio device absent in headless environments logs a warning; harmless.

## Architecture (where things live)

```
crates/shared/     every rule; no rendering, no sockets
  content/         RON model, loaders, validator, registry (ContentId u16 space)
  voxel/           Chunk (palette + 16-bit packed), BlockPos/ChunkPos, BlockSource trait
  worldgen/        layers, caves, per-cell ore veins, flat camp, spawn, surface height
  movement/        deterministic AABB controller + PlayerInput (shared by both sides)
  inventory/       ItemStack, Inventory, transfer rules, SlotRef/SlotKind
  crafting/        mining rules (break-time formula, tier gate, area tools), machine state
                   machine, hand crafting, TechTree (deposit/complete)
  power/           Network (steam + electric), batteries, brownout, flood fill
  hazards/         Vitals, WornGear, dose maths, environment sampling, fall damage
  protocol/        PROTOCOL_VERSION, replicated components, intents, server messages,
                   LocalOwner visibility filter, channel registration
crates/server/
  net/             renet transport, handshake, chunk streaming, per-client sent-chunk set
  world/           WorldManager (generator, edits, tasks), async generation, eviction
  sim/             player, mining, inventory, machine, power, interaction, chat, respawn
  persistence/     region files (DCR1 + zstd) and SQLite, atomic writes, autosave
  crafting_state   team TechTree resource + change-driven broadcast
crates/client/
  render/          atlas (procedural tiles), mesher (greedy, async), chunk entities
  input/           FPS controls, DDA raycast targeting, mining/placing intents
  prediction/      local body, input buffer, reconciliation, crack animation
  hud/             crosshair, hotbar, vitals, depth/Geiger, notifications, F3 overlay
  ui/              egui: inventory, crafting list, machine screen, tech tree, chat, objectives
  audio/           procedural sounds (dig, place, hum, alarm, Geiger)
tools/
  content_authoring.py   regenerates assets/data/*.ron from tables (not used at runtime)
  atlas_gen/             placeholder crate (atlas is generated at runtime in the client)
assets/data/       blocks, items, recipes, machines, tech_tree, layers, hazards (RON)
```

## Simulation rules worth knowing

* Server `FixedUpdate` at **30 Hz**; movement uses the same 1/60 s fixed step on both sides.
* Break time = `hardness / (tool_speed × power)`, rounded **up** to server ticks.
* Tier gate: a tool below the block's `min_tier` cannot break it at all; the client shows a red
  crosshair and "Too hard — needs Tier N".
* Ore veins are stamped per lattice cell (cell size from frequency), never per block; this is
  what keeps generation inside budget.
* The world has a flat, cave-free 40-block-radius camp at the centre (`SURFACE_Y = 448`).
* Machines are ECS entities with a `BlockPos`; the voxel at that position holds the machine
  block so world and entity never disagree.
* Power: topology rebuilt only on change; each tick only sums supply/demand; brownout scales
  every consumer by `supply / demand`; batteries charge on surplus and cover deficits.

## Tests

| Area | Where |
|---|---|
| Palette/chunk round-trips, bounds | `shared/src/voxel/chunk.rs` |
| Worldgen determinism, ores per layer, generation speed, spawn on the camp | `shared/src/worldgen/mod.rs` |
| Break-time formula, tier gate, area tools | `shared/src/crafting/mining.rs` |
| Inventory stacking/moves + proptest conservation | `shared/src/inventory/mod.rs` |
| Machine tick, fuel, output-full, locked recipes | `shared/src/crafting/machine.rs` |
| Tech tree deposits/ordering | `shared/src/crafting/mod.rs` |
| Power solve, brownout, batteries | `shared/src/power/mod.rs` |
| Radiation dose, gear, oxygen, heat | `shared/src/hazards/mod.rs` |
| Protocol round-trips, chunk compression | `shared/src/protocol/mod.rs` |
| Atlas generation, tile UVs, determinism | `client/src/render/atlas.rs` |
| Greedy meshing, face culling | `client/src/render/mesher.rs` |
| Procedural audio sanity | `client/src/audio/mod.rs` |
| Region file round-trip, atomic write | `server/src/persistence/mod.rs` |

Missing (planned): headless Bevy app tests driving intents, per-Era progression tests, the M8
nuclear integration test, network convergence tests with simulated latency.

## Next session: start here

1. Wire machine kits through `PlaceBlock` → `place_machine` (server) and add a ghost preview +
   rotation on the client. Then the Era 1 loop is playable.
2. Add headless app tests: build `App` with `sim::SimPlugin`, insert a player, send intents,
   step `FixedUpdate`, assert the block was mined and the item appeared.
3. Implement chunk unload and the loading screen progress gate.
4. Implement save loading on startup.
