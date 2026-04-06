# Architecture

A reference for the system design, internal structure, and work split for this project.

---

## 1. System overview

Three Cargo crates in a workspace:

```
maze_wars/
├── shared/    # Packet types, map grid, constants — imported by both binaries
├── server/    # UDP listener, game loop, state broadcast
└── client/    # Raycaster, window, input handling, HUD
```

`shared` sits at the top of the dependency tree. Neither `server` nor `client` depends on each other — only on `shared`. This is the decision that lets two developers work in parallel without merge conflicts.

**Data flow over the network:**

- Client → Server: input packets (keypresses, movement direction)
- Server → Client: world state packets (positions of all players)

---

## 2. Server internals

The server runs entirely inside a `tokio` async runtime with three tasks:

```
tokio runtime
├── UDP listener task   — receives input packets, queues them per client SocketAddr
├── Game tick task      — runs on a fixed 16ms interval, processes inputs,
│                         updates positions, checks collisions, updates scores
└── Broadcast task      — reads current game state, serialises and sends
                          a state packet to every connected client
```

Because all three tasks run inside the same async executor, game state can be shared between them without a `Mutex` — only one task runs at any moment, so there is no concurrent write risk.

**Player session tracking:** UDP has no connection concept. The server identifies players by their `SocketAddr` (IP + port). When a packet arrives from an unknown address, the server registers it as a new player. If no packet arrives from an address within a timeout window, that player is dropped.

**Critical events** (player death, level change) get a lightweight acknowledgement layer on top of UDP — the server retries sending them until the client echoes back a receipt sequence number. Position updates are fire-and-forget.

---

## 3. Client internals

The client uses two threads communicating via `std::sync::mpsc` channels:

```
main thread (render loop)            network thread (std::thread)
─────────────────────────            ───────────────────────────
1. Read input (WASD, mouse)    ←──── Receives world state from server
2. Send input via channel      ────→ Sends input packet to server
3. Read state from channel
4. Raycast frame (DDA)
5. Draw HUD (mini-map, FPS)
6. Flush pixel buffer
↻ repeat every ~16ms
```

The main thread never blocks on network I/O. It reads whatever state the network thread has already placed in the channel and renders immediately. If the channel is empty (no new state arrived), it renders using the last known state — producing a smooth frame regardless of network timing.

**Raycaster:** One ray is cast per screen column. Each ray steps through the 2D map grid using the DDA algorithm until it hits a wall. The distance to that hit determines the height of the vertical slice drawn for that column. Fisheye correction is applied: `corrected_distance = raw_distance × cos(ray_angle − player_angle)`.

**Pixel buffer:** `winit` provides the window and event loop. `pixels` provides the raw framebuffer. The raycaster writes directly into the framebuffer byte-by-byte; `pixels` handles the GPU blit to screen.

---

## 4. Packet protocol (`shared/protocol.rs`)

Defined once in `shared`, consumed by both binaries. Serialised with `serde` + `bincode` for minimal byte size.

```rust
// Client → Server
pub struct InputPacket {
    pub sequence: u32,
    pub player_id: u32,
    pub forward: bool,
    pub backward: bool,
    pub turn_left: bool,
    pub turn_right: bool,
}

// Server → Client
pub struct StatePacket {
    pub sequence: u32,
    pub players: Vec<PlayerState>,
}

pub struct PlayerState {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
}
```

`sequence` fields are used to discard out-of-order packets — if an arriving packet has a lower sequence number than the last processed one, it is dropped.

> **Agree and commit this before splitting work.** Any change to protocol types touches both binaries.

---

## 5. Map representation (`shared/map.rs`)

```rust
pub struct Map {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<u8>,  // 0 = empty, 1 = wall
}

impl Map {
    pub fn is_wall(&self, x: usize, y: usize) -> bool {
        self.cells[y * self.width + x] == 1
    }
}
```

Three levels ship with the game, defined as constants in `shared`. Level difficulty is expressed as maze complexity — more dead ends, longer corridors, fewer open areas. The raycaster and the server both read from the same `Map` type.

---

## 6. Crate dependencies

| Crate                 | Used in | Purpose                                        |
| --------------------- | ------- | ---------------------------------------------- |
| `serde` + `bincode`   | both    | Packet serialisation into compact binary       |
| `tokio`               | server  | Async runtime for the three server tasks       |
| `winit`               | client  | OS window creation and event loop              |
| `pixels`              | client  | Raw pixel framebuffer and GPU blit             |
| `glam`                | client  | Vector maths for ray direction and rotation    |
| `clap`                | both    | CLI argument parsing (IP address, username)    |
| `tracing`             | server  | Structured server-side logging                 |
| `std::net::UdpSocket` | both    | UDP send/receive — no external crate needed    |
| `std::sync::mpsc`     | client  | Channel between render loop and network thread |

---

## 7. Work split

|                  | iyoussef                                               | teammate                                                           |
| ---------------- | ------------------------------------------------------ | ------------------------------------------------------------------ |
| Primary crate    | `server/`                                              | `client/`                                                          |
| Shared ownership | `shared/` — agreed together first                      | `shared/` — agreed together first                                  |
| Owns             | UDP listener · game tick · player registry · broadcast | Raycaster · pixel buffer · window · input · mini-map · FPS counter |
| Network layer    | Server-side (receive inputs, send state)               | Client-side (send inputs, receive state)                           |
| Level design     | 3 maze layouts in `shared/map.rs`                      | Level loading and rendering                                        |

### Build order

```
Week 1 — both together
  Agree and commit shared/protocol.rs and shared/map.rs
  Tag: shared-v1 — neither binary is touched until this is done

Week 2 — parallel
  iyoussef:  server skeleton (UDP socket open, echo packets back to sender)
  teammate:  client skeleton (window opens, black screen renders at 60fps)

Week 3 — parallel
  iyoussef:  full game tick (movement, collision detection, player registry)
  teammate:  raycaster working against a local hardcoded test map

Week 4 — connect and polish
  Wire server <-> client over real UDP on same machine first, then LAN
  Mini-map, FPS counter, all 3 levels
  Performance pass — must hit 50+ FPS in release mode
```

### The one rule

> Never change `shared/protocol.rs` or `shared/map.rs` unilaterally. Any change to the shared types must be discussed, agreed, and PR'd together — it breaks both binaries simultaneously.

---

_Last updated: April 2026_
