# Project Roadmap

Tracks all work from setup to submission. Updated as tasks are completed.

---

## Phase 1 — Foundation ✅

- [x] Cargo workspace with `shared`, `server`, `client` crates
- [x] Git dual-push remote setup (GitHub + Gitea)
- [x] GitHub Action to mirror all branches and deletions to Gitea
- [x] Git remote setup script (`setup-git.sh`)
- [x] Branch strategy documented (`GIT-WORKFLOW.md`)
- [x] Architecture documented (`ARCHITECTURE.md`)
- [x] Research documented (`RESEARCH.md`)
- [x] `.gitignore` configured

---

## Phase 2 — Shared Types ✅

- [x] `shared/src/protocol.rs` — `InputPacket`, `StatePacket`, `PlayerState`
- [x] `shared/src/map.rs` — `Map` struct, `is_wall()`, 3 level definitions
- [x] Serialisation — `postcard` + `serde` across all crates
- [x] All crate dependencies declared

---

## Phase 3 — Server ✅

- [x] UDP socket binding with configurable port via `clap`
- [x] Tokio async runtime with three tasks (listener, tick, broadcast)
- [x] Player registry by `SocketAddr`
- [x] Session token validation
- [x] Rate limiting per client
- [x] Player timeout and cleanup
- [x] Sequence number ordering — discard out-of-order packets
- [x] Refactor movement out of listener task into game tick task
- [x] Collision detection against map using `shared::map::is_wall()`
- [x] Support minimum 10 simultaneous connections (verify and test)
- [x] Fuel drain per tick (~90s supply) with respawn after 3s on empty
- [x] Miner rescue detection — proximity check within 0.8 world units
- [x] Per-player `StatePacket` broadcast with `your_id` and `miner_rescued`

---

## Phase 4 — Client ✅

- [x] CLI prompt for server IP address and username on startup (`--server`, `--username`)
- [x] UDP network thread (send `InputPacket` at 60 hz, receive `StatePacket`)
- [x] `mpsc` channel between network thread and render loop
- [x] Window setup with `winit` 0.30 (`ApplicationHandler` trait, `Arc<Window>`)
- [x] Pixel buffer with `pixels` 0.17 (`Pixels<'static>` via `Arc<Window>`)
- [x] Raycaster render loop (DDA algorithm, fisheye correction, per-column z-buffer)
- [x] Wall shading based on distance + column-stripe texture pattern
- [x] Other player sprites rendered in world (billboard, z-buffer clipped, edge outline)
- [x] Miner sprite rendered as pixel-art figure (hard hat, body, legs, transparent bg)
- [x] Mini-map overlay — 4×4 dots, bright centre, direction arrow for local player
- [x] FPS counter displayed on screen (3×5 bitmap font, neon yellow, top-left)
- [x] Synthwave aesthetic — starfield sky, neon horizon, perspective grid floor, vignette
- [x] Fuel bar — bottom of screen, colour shifts green → yellow → red
- [x] Gold rescue flash — border effect fades over 3s when miner is rescued
- [x] Client-side prediction — movement applied locally, no server round-trip lag
- [x] `ControlFlow::Poll` — continuous event loop, no key-event queuing
- [x] Bot binary (`client --bin bot`) — simulated player for multiplayer testing

---

## Phase 5 — Integration 🔄

- [x] Client and server running on same machine communicating over UDP
- [ ] Client and server running on separate machines over LAN
- [x] Multiple clients connecting simultaneously (bot + manual client verified)
- [x] Player positions syncing correctly across all clients
- [ ] Level selection working across all 3 levels

---

## Phase 6 — Polish & Performance

- [ ] Consistent 50+ FPS in release mode (`cargo run --release`)
- [x] Clean disconnect handling (server timeout removes player after 10s)
- [ ] Level transitions working correctly
- [ ] Critical event ACK layer (player death, level change)
- [ ] Basic shooting mechanic
- [ ] Onscreen objective text ("FIND THE MINER", distance indicator)
- [ ] Win/rescue screen on miner reached

---

## Phase 7 — Submission

- [ ] All features from brief verified working
- [ ] `README.md` updated with build and run instructions
- [ ] Final merge to `main` on GitHub
- [ ] Gitea `main` verified in sync for course submission
- [ ] `cargo build --release` produces clean build with no warnings

---

## Bonus (if time permits)

- [ ] Maze auto-generation algorithm
- [ ] Level editor
- [ ] AI players
- [ ] GUI-based server connection and history

---

_Last updated: June 3, 2026_
