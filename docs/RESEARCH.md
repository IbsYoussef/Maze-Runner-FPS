# Maze Wars — Technical Research

A compressed reference covering the core concepts underpinning this project.

---

## 1. Client-Server Architecture

One machine (the server) owns the authoritative game state. All player machines (clients) connect to it — never directly to each other.

**Data flow:**

- Clients send → **inputs** (keypresses, movement direction)
- Server sends back → **world state** (positions of all players)

**Why this matters:**

- No client ever trusts another client — cheating is prevented at the source
- Scales cleanly: adding a 10th player means one more connection to the server, not 9 new peer connections
- The server is the single source of truth for collision, scoring, and game logic

**Core problems to design around:**

| Problem         | Cause                           | Solution                                                                            |
| --------------- | ------------------------------- | ----------------------------------------------------------------------------------- |
| Latency         | Packets take 50–150ms to travel | Client-side prediction — move locally, reconcile with server                        |
| Dropped packets | UDP has no built-in retry       | Design state updates to be self-contained; a missed frame is just stale, not broken |
| Player desync   | Clients see different states    | Server broadcasts at fixed intervals; clients interpolate between updates           |

---

## 2. UDP vs TCP

The brief mandates UDP. Here's why it's the right call for a game.

**TCP** guarantees delivery and ordering. If a packet is lost, it blocks everything behind it until a retransmit succeeds. For a browser or file transfer this is correct. For a game running at 60 updates/sec, a 200ms stall to retransmit a position update is fatal.

**UDP** is fire-and-forget. Packets may be lost, arrive out of order, or duplicate. The sender never waits.

**Why UDP wins for games:** A dropped position packet doesn't matter — the next one arrives 16ms later with fresher data anyway. Stale data beats a frozen game every time.

**What you handle manually with UDP:**

- Sessions — track clients by their `SocketAddr` since UDP has no connection concept
- Reliability for critical events — player death, level transitions need your own lightweight ACK layer
- Ordering — add a sequence number to packets; discard any that arrive older than the last processed one

---

## 3. Raycasting (the rendering technique)

Maze Wars uses a raycaster — not a 3D engine. The map is a flat 2D grid. For every vertical column of pixels on screen, one ray is fired from the player's position into the map. Where it hits a wall, the distance determines the height of that column's wall slice.

```
Close wall  →  long ray distance  →  tall column slice
Far wall    →  short ray distance →  short column slice
```

**The algorithm (DDA — Digital Differential Analysis):**

1. Cast a ray for each screen column at an angle offset from the player's viewing direction
2. Step the ray through the grid, checking each cell for a wall
3. Record the distance to the first wall hit
4. Draw a vertical slice at that column: `slice_height = screen_height / distance`
5. Apply a shade based on distance (farther = darker) for depth perception

**Fisheye correction:** Rays at the edges of the screen are angled further from centre, making them artificially longer. Without correction, walls curve outward. Fix: multiply raw distance by `cos(ray_angle - player_angle)` before computing slice height.

**Reference:** Lode's Raycasting Tutorial — `lodev.org/cgptutorial/raycasting.html` — is the canonical walkthrough of this algorithm.

---

## 4. Rust Crate Decisions

### Workspace structure

Three crates in a Cargo workspace:

```
maze_wars/
├── shared/    # Packet types, map grid, constants — used by both binaries
├── server/    # UDP listener, game loop, state broadcast
└── client/    # Raycaster, window, input, HUD
```

### Crate selection

| Crate                 | Used in | Purpose                                                               |
| --------------------- | ------- | --------------------------------------------------------------------- |
| `serde` + `bincode`   | both    | Serialise/deserialise UDP packets into compact binary                 |
| `winit`               | client  | OS window creation and event loop (keyboard, mouse, resize)           |
| `pixels`              | client  | Raw pixel buffer — write colour values directly, then flush to screen |
| `glam`                | client  | Vector and matrix maths for ray direction calculations                |
| `clap`                | both    | Parse CLI args (IP address, username prompt at startup)               |
| `tracing`             | server  | Structured logging with level filtering                               |
| `std::net::UdpSocket` | both    | Standard library UDP — no external crate needed                       |

**Why `winit` + `pixels`:** A raycaster writes every pixel manually anyway. `pixels` gives a direct framebuffer to write into and handles the GPU blit. `winit` wraps the OS window and event loop. No game engine overhead, no shader knowledge required.

**Why `bincode`:** Produces the smallest possible binary output from a `serde`-derived struct. Smaller packets = lower latency and less risk of fragmentation on the network.

---

## 5. Concurrency — Threads vs Async

Both approaches answer the same question: how do you handle multiple things at once? They answer it completely differently.

---

### Threading

Threads are true parallelism. Each thread is an independent worker with its own stack, running simultaneously on a separate CPU core. The OS schedules them.

**The problem:** when multiple threads need to touch the same data (e.g. the game state), you need a `Mutex` to prevent simultaneous writes. This introduces lock contention — threads queuing up waiting for the lock — and the risk of deadlock if two threads each hold a lock the other needs.

```
Core 1 → Thread A → running
Core 2 → Thread B → blocked (waiting for Mutex on game state)
Core 3 → Thread C → running
Core 4 → Thread D → blocked (same Mutex)
```

**In Rust:** `std::thread::spawn` + `Arc<Mutex<T>>` to share state safely across threads.

---

### Async

Async is cooperative concurrency on a single thread (or small thread pool). Tasks voluntarily yield control when they have nothing to do — typically when waiting on I/O. The async runtime immediately switches to the next ready task, so the thread is never idle.

```
Single thread timeline:
[Task A runs] → yields (waiting for UDP packet) →
[Task B runs] → yields (waiting to send) →
[Task C runs] → yields →
[Task A resumes — packet arrived]
```

Because only one task runs at a time, **no locks are needed** on shared state — there's no possibility of two tasks writing simultaneously. The failure mode is the opposite of threading: if a task never yields (e.g. a tight CPU loop), it starves everything else.

**In Rust:** `async`/`await` syntax + `tokio` runtime. Tasks are spawned with `tokio::spawn`.

---

### Side-by-side comparison

|                 | Threads               | Async                                  |
| --------------- | --------------------- | -------------------------------------- |
| Parallelism     | True — multiple cores | Concurrent — interleaved on one thread |
| Best for        | CPU-heavy compute     | I/O-heavy waiting (network, disk)      |
| Shared state    | Needs `Mutex` / `Arc` | No locks needed                        |
| Memory per unit | ~8MB stack per thread | Tiny — tasks are state machines        |
| Failure mode    | Deadlock              | Starvation if a task never yields      |
| Rust primitives | `std::thread`         | `tokio`, `async`/`await`               |

---

### Decision for this project

**Server → `tokio` async.**
The server spends almost all its time waiting — waiting for a UDP packet to arrive, waiting to send state back. That is I/O work, which is exactly where async excels. With `tokio` you handle 10+ player connections without a thread per player, and game state needs no locking since the async executor runs one task at a time.

Recommended server task layout:

```
tokio::spawn → UDP listener task     (receives packets, queues inputs)
tokio::spawn → Game tick task        (fixed interval: update state, check collisions)
tokio::spawn → Broadcast task        (sends world state to all clients)
```

**Client → blocking main loop + one background thread.**
The render loop (raycasting 60 times/sec) is pure CPU — it never waits on anything. Async would add complexity for zero benefit here. The clean split is:

```
main thread     → raycaster loop (blocking, runs flat out)
std::thread     → UDP send/receive (spawned once, communicates via channel)
```

The two sides communicate via `std::sync::mpsc` channels — the network thread pushes received state into a channel, the render loop reads from it each frame.

---

_Last updated: April 2026_
