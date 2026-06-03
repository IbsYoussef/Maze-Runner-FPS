# Maze Runner FPS

A multiplayer first-person maze game written in Rust — inspired by the classic *Maze Wars* (1973).  
Client-server architecture over UDP, raycaster renderer, up to 10+ simultaneous players.

---

## Requirements

| Dependency | Version |
|---|---|
| Rust + Cargo | 1.80+ (edition 2024) |
| OS | macOS, Linux, or Windows |

Install Rust via [rustup.rs](https://rustup.rs) if not already present.

---

## Build

```bash
# Clone the repository
git clone https://github.com/IbsYoussef/Maze-Runner-FPS.git
cd Maze-Runner-FPS

# Build all crates (server + client + shared)
cargo build --release
```

Build output goes to `target/release/`.

---

## Running the game

### Step 1 — Start the server

```bash
cargo run -p server --bin server --release
```

Optional flags:

| Flag | Default | Description |
|---|---|---|
| `--port` / `-p` | `34254` | UDP port to listen on |
| `--level` / `-l` | `1` | Level to load (1, 2, or 3) |

Example — level 2 on a custom port:
```bash
cargo run -p server --bin server --release -- --port 34254 --level 2
```

### Step 2 — Connect a client

```bash
cargo run -p client --release
```

The client will prompt you for the server address and username:

```
--server   Server IP:port  (default: 127.0.0.1:34254)
--username Player name     (default: player)
```

Example — connecting to a server on the same machine:
```bash
cargo run -p client --release -- --server 127.0.0.1:34254 --username Alice
```

Example — connecting to a server on the local network:
```bash
cargo run -p client --release -- --server 192.168.1.42:34254 --username Bob
```

### Multiple clients

Open a new terminal for each additional player and run the client command with a different username. Each client connects independently.

---

## Controls

| Key | Action |
|---|---|
| `W` / `↑` | Move forward |
| `S` / `↓` | Move backward |
| `A` / `←` | Turn left |
| `D` / `→` | Turn right |
| `Escape` | Quit |

---

## Gameplay

You are a rescue operative with a limited fuel supply. Navigate the maze and reach the **trapped miner** before your fuel runs out.

- The **miner** appears as a pixel-art figure at the far end of the maze — gold dot on the mini-map
- Get within range to rescue them — a gold border flash confirms the rescue
- Your **fuel bar** depletes over ~90 seconds (green → yellow → red)
- Running out of fuel freezes you in place — you respawn at the start after 3 seconds
- Other players appear as **cyan sprites** in the world and on the mini-map

---

## HUD

| Element | Location | Description |
|---|---|---|
| FPS counter | Top-left | Live frame rate, neon yellow bitmap digits |
| Mini-map | Top-right | Full maze layout, player dots with direction arrow, miner position |
| Fuel bar | Bottom | Colour-coded energy remaining (green / yellow / red) |

---

## Levels

| Level | Flag | Description |
|---|---|---|
| 1 | `--level 1` | Open corridors — easiest navigation |
| 2 | `--level 2` | More dead ends and tighter turns |
| 3 | `--level 3` | Dense maze — longest paths to the miner |

All levels are 16×16 tiles. The miner is placed at the far end of the top corridor on every level.

---

## Architecture

```
┌─────────────────────────────┐
│           shared            │  protocol types, map definitions
│  InputPacket / StatePacket  │
│  Map (16×16) / PlayerState  │
└────────────┬────────────────┘
             │
   ┌─────────┴────────┐
   │                  │
┌──▼───┐          ┌───▼──────┐
│server│          │  client  │
│      │  UDP     │          │
│ 3    │◄────────►│ raycaster│
│tasks │          │ network  │
│      │          │ thread   │
└──────┘          └──────────┘
```

**Server** — three concurrent Tokio tasks:
- `udp_listener` — receives `InputPacket`s, registers players, enforces rate limiting and session tokens
- `game_tick` — runs at 62.5 hz (16ms), applies movement, collision detection, fuel drain, miner proximity
- `broadcast` — sends a per-client `StatePacket` with `your_id`, `fuel`, `miner_rescued` every tick

**Client** — two threads:
- `net_thread` — sends `InputPacket` at 60 hz, receives `StatePacket`, communicates via `mpsc::sync_channel`
- `main thread` — winit event loop, DDA raycaster render, client-side movement prediction

**Shared** — `postcard` + `serde` serialisation over UDP:
- `InputPacket` — directional flags + sequence number + session token
- `StatePacket` — all player positions, fuel levels, miner state
- `Map` — 16×16 grid with `is_wall()` used by both server (collision) and client (raycasting)

---

## Performance

| Mode | Typical FPS |
|---|---|
| `cargo run` (debug) | 50–60 fps |
| `cargo run --release` | 200+ fps |

The renderer writes directly into the `pixels` frame buffer — no heap allocation in the render loop. Target of 50+ fps is met in both debug and release builds.

---

## Testing

### Stress test — 12 simultaneous connections

```bash
cargo run -p server --bin stress_test
```

Starts the server in-process, connects 12 concurrent UDP clients, verifies all 12 receive `StatePacket`s listing 10+ simultaneous players.

### Bot — simulated player

```bash
cargo run -p client --bin bot -- --server 127.0.0.1:34254
```

Connects as an autonomous player that walks forward and turns when stuck. Useful for testing multiplayer rendering and miner rescue without a second human.

---

## Project structure

```
Maze-Runner-FPS/
├── shared/         — protocol types and map definitions (used by both binaries)
│   └── src/
│       ├── protocol.rs
│       └── map.rs
├── server/         — game server
│   └── src/
│       ├── main.rs
│       └── bin/
│           ├── server.rs    (default binary)
│           └── stress_test.rs
├── client/         — game client
│   └── src/
│       ├── main.rs
│       └── bin/
│           └── bot.rs
└── docs/           — architecture, research, schedule, git workflow
```

---

## Implemented features

### Core requirements
- [x] First-person raycaster renderer (DDA algorithm, fisheye correction)
- [x] Mini-map with player positions and facing direction
- [x] FPS counter on screen
- [x] Client-server architecture over UDP
- [x] Server accepts 10+ simultaneous connections (stress-tested with 12)
- [x] Client prompts for server IP and username on startup
- [x] 3 levels of increasing difficulty
- [x] 50+ FPS in both debug and release mode

### Additional features
- [x] H.E.R.O.-inspired gameplay — trapped miner rescue objective
- [x] Fuel/energy system with respawn
- [x] Pixel-art miner sprite (hard hat, suit, legs)
- [x] Synthwave visual style (starfield, neon walls, perspective grid floor)
- [x] Wall column-stripe texture and cinematic vignette
- [x] Client-side movement prediction (zero input lag)
- [x] Session token validation and rate limiting per client
- [x] Sequence number ordering (out-of-order packet discard)
- [x] Player timeout and automatic cleanup

---

_Written in Rust. Built June 2026._
