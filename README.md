# Deepcore

A co-op 3D voxel mining game in Rust, built on Bevy. Up to 16 players share one persistent
1024 × 1024 × 512-block mine. Everyone starts with a steel pickaxe at a surface camp. Progression
is depth: dig, haul ore home, build a machine, the machine unlocks a better tool or material, dig
deeper. Six Eras take the team from a hand pickaxe to a nuclear reactor and a **Nuclear Drill**
that bores 3 × 3 tunnels.

![The camp at the surface](docs/screenshot-surface.png)

## Build and run

```bash
# Linux system libraries for Bevy
sudo apt-get install -y libasound2-dev libudev-dev libx11-dev libxkbcommon-dev \
    libwayland-dev libvulkan-dev pkg-config

cargo build --release

# dedicated server (headless, 30 Hz authoritative)
cargo run --release -p deepcore-server --bin deepcore-server -- --port 24288

# client, joining that server
cargo run --release -p deepcore-client -- --connect 127.0.0.1:24288 --name YourName
```

The server also takes `--seed`, `--content`, `--save-dir`, `--simulate-latency` and
`--simulate-loss`. The client takes `--connect`, `--name`, `--content` and `--render-distance`.

## Quality gate

```bash
./scripts/check.sh      # cargo fmt --check, clippy -D warnings, cargo test
```

## Controls

| Key | Action |
|---|---|
| WASD | move |
| Space | jump / climb |
| Shift | sprint |
| Ctrl | crouch |
| Left mouse | mine |
| Right mouse | place the held block |
| Middle mouse | interact with a machine or crate |
| 1–9 | select hotbar slot |
| E | inventory and hand crafting |
| T | tech tree |
| Q | drop the held stack |
| Esc / Tab | release the mouse |
| F3 | debug overlay (FPS, chunks, corrections, networks) |

## Content is data

Blocks, items, recipes, machines, the tech tree, layers and hazard tuning live in
`assets/data/*.ron` and are validated at load: unknown references, dead-end items, tech cycles,
missing parents and layer gaps all fail fast with a readable message. Adding an ore or a recipe
never touches Rust. `cargo run -p deepcore-server --bin balance_report` prints the raw materials
each Era needs and the cost of the nuclear chain.

## Architecture

`shared` holds every rule (voxels, worldgen, movement, inventory, crafting, machines, power,
hazards, protocol) with no rendering and no sockets. `server` decides: it owns the world,
inventories, machines, power networks and hazards, and validates every client intent. `client`
shows: it predicts its own movement with the same shared code and sends intents for everything
else. See `docs/PLAN.md` for the full design and `PROGRESS.md` for the current state.

## What is in the box

| Area | State |
|---|---|
| Voxel world | 1024 × 1024 × 512 blocks, palette-compressed 32³ chunks, deterministic worldgen with six layers, caves and ore veins, greedy meshing on the async pool |
| Mining | hardness/tier rules, area drills, server-validated intents, predicted crack animation |
| Multiplayer | authoritative 30 Hz server, handshake with content hash, chunk streaming, movement prediction and reconciliation |
| Content | 83 blocks, 148 items, 134 recipes, 36 machines, 30 tech nodes — all RON, validated at load |
| Progression | all six Eras, the full nuclear chain (crush → leach → precipitate → convert → enrich → fabricate), reactor physics with scram, waste handling, the Nuclear Drill |
| Base building | machine kit placement with ghost preview, machine screens, conveyors, power (steam and electric with brownouts), research desk and tech tree |
| Hazards | darkness, flooding, gas, heat, cave-ins, radiation with a Geiger counter and gear |
| Persistence | region files + SQLite, autosave, atomic writes, load on startup, returning players restored |
| Lobby | axum + sqlx accounts (argon2), hashed tokens, server browser, heartbeats, Dockerfiles |
| Polish | procedural atlas and audio (zero downloaded assets), HUD, egui screens, guided objectives |

## Tests

```bash
cargo test --workspace   # 96 tests
```

Coverage: chunk palette round-trips, worldgen determinism/ore placement/generation speed, the
break-time formula and tier gate, inventory stacking (property tests), machine ticks, tech tree
deposits, power solving and brownouts, radiation maths, the wire protocol, procedural atlas and
audio, greedy meshing, region-file round-trips — plus **end-to-end intent tests** (mine, refuse
too-hard, refuse out-of-reach, place, broadcast), nine server integration tests, four nuclear
chain tests including the whole enrichment chain, and four lobby HTTP tests.

`PROGRESS.md` lists every known gap, and the bugs the end-to-end tests caught.
