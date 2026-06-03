# Delivery Schedule — Week of June 3–6, 2026

End-of-week target: project complete, `cargo build --release` clean, submitted to Gitea `main`.

---

## Status Snapshot (June 3)

| Phase | Description | Status |
|---|---|---|
| 1 | Foundation (workspace, CI, git) | ✅ Done |
| 2 | Shared types (protocol, map) | ✅ Done |
| 3 | Server (UDP, tick, broadcast, collisions) | ✅ ~95% — 10-connection test remaining |
| 4 | Client (raycaster, network, window) | ❌ Empty — entire client is remaining work |
| 5 | Integration | ❌ Not started |
| 6 | Polish & performance | ❌ Not started |
| 7 | Submission | ❌ Not started |

The client is the critical path. All Phase 5–7 work is blocked on it.

---

## Day 1 — Tuesday June 3: Client skeleton + networking

**Goal:** Client connects to server, sends input, receives state.

| Task | Est. | Notes |
|---|---|---|
| Server 10-connection stress test | 30 min | Spawn 10 local UDP sockets, confirm no panics — closes out Phase 3 |
| Client CLI prompt (`--server`, `--username`) | 30 min | `clap` args, print assigned session token on connect |
| UDP network thread | 1 h | Background thread: send `InputPacket` at ~60 hz, receive `StatePacket` |
| `mpsc` channel between network thread and render loop | 30 min | `std::sync::mpsc`, sender on network thread, receiver on main |
| `winit` + `pixels` window stub | 1 h | Open 640×480 window, fill solid colour — **must compile and run before end of day** |

**Exit criteria:** Client window opens, connects to server, server logs the new player, client disconnects cleanly on close.

> **Note on `winit` 0.30:** The event loop moved from a closure callback to the `ApplicationHandler` trait. Use `pixels`' own bundled examples (`~/.cargo/registry/src/.../pixels-0.17.1/examples/`) as the template — not random tutorials. Getting this boilerplate compiling today prevents API surprises on Day 2 when raycasting work begins.

---

## Day 2 — Wednesday June 4: Raycaster (critical path)

**Goal:** First-person view rendering correctly in the maze.

| Task | Est. | Notes |
|---|---|---|
| Keyboard input → `InputPacket` flags | 30 min | WASD / arrow keys, update flags on `KeyboardInput` event |
| DDA raycaster — basic wall rendering | 3 h | Ray per column, DDA step, hit detection via `shared::map::is_wall()`, fisheye correction, wall height from projected distance |
| Wall shading by distance | 30 min | Multiply colour by `1.0 / distance`; N/S faces slightly darker than E/W |
| FPS counter | 30 min | Frame delta in window title or top-left corner |

**Exit criteria:** Single-player, smooth first-person view navigating a maze. 60+ FPS in debug mode.

> **DDA note:** Implement the per-column Z-buffer (store hit distance per column) alongside the raycaster — not after. It is needed for sprite depth-sorting on Day 3 and is painful to retrofit.

---

## Day 3 — Thursday June 5: Multiplayer visuals + integration

**Goal:** Other players visible in world, integration tests passing.

| Task | Est. | Notes |
|---|---|---|
| Other player sprites | 2 h | Sort by distance from camera, project to screen column range using Z-buffer, draw as solid-coloured quad scaled by distance |
| Mini-map overlay | 1 h | 1 px per tile in a corner, filled square for walls, dot for each player, line for local player facing direction |
| Localhost integration test | 30 min | Server + 1 client on same machine, confirm position sync |
| LAN integration test | 30 min | Server and client on separate machines |
| 3-player simultaneous test | 1 h | 3 clients connected, all player positions update on all screens |

**Exit criteria:** 3 clients in the same maze, all players visible to each other, mini-map updates in real time.

---

## Day 4 — Friday June 6: Polish + submission

**Goal:** Release-quality build, all checklist items signed off, submitted.

| Task | Est. | Notes |
|---|---|---|
| 50+ FPS in release mode | 1 h | `cargo run --release`; if below target, profile the pixel write loop first — avoid allocations inside render |
| Clean disconnect handling | 30 min | Server timeout already works; client should send a disconnect signal on window close |
| Level transitions | 30 min | Client reads current level from `StatePacket` (add field if needed); server `--level` flag already exists |
| Critical event ACK layer | 1 h | Reliable send with retry for player death and level change events |
| Basic shooting mechanic | 1 h | Add `fire: bool` to `InputPacket`; server resolves hit by casting a ray against other player AABBs |
| `README.md` build & run instructions | 30 min | How to start server, how to connect a client, tested commands only |
| `cargo build --release` — zero warnings | 15 min | Fix any remaining `#[allow(dead_code)]` or unused import warnings |
| Final merge to `main` | 15 min | `dev` → `main` on GitHub; confirm Gitea mirror in sync |

**Exit criteria:** All Phase 7 submission checklist items checked. Clean release build. Gitea `main` matches GitHub `main`.

---

## Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| `winit` 0.30 API mismatch (closure → trait) | Lose Day 2 to compiler errors | Write and run the window stub on Day 1; use `pixels` bundled examples as reference |
| DDA raycaster fisheye taking longer than estimated | Day 2 slips into Day 3 | Use a reference DDA walkthrough as a blueprint; defer sprite rendering if needed — a working single-player view is more valuable than a broken multiplayer one |
| Player sprites bleeding through walls | Visual corruption | Z-buffer built alongside raycaster (Day 2), not added after |
| Pixels crate allocation in render loop killing FPS | Miss 50+ FPS target | Write directly into the `frame: &mut [u8]` slice; no intermediate `Vec` allocations per frame |
| Scope creep from bonus features | Miss submission deadline | Bonus items (maze gen, AI players) are explicitly deferred until after Phase 7 is signed off |

---

## Deferred (post-submission)

The following are captured in `TASKLIST.md` under Bonus but will not be attempted this week:

- Maze auto-generation algorithm
- Level editor
- AI players
- GUI-based server connection and history

---

_Created: June 3, 2026_
