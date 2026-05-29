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

## Phase 3 — Server 🔄

- [x] UDP socket binding with configurable port via `clap`
- [x] Tokio async runtime with three tasks (listener, tick, broadcast)
- [x] Player registry by `SocketAddr`
- [x] Session token validation
- [x] Rate limiting per client
- [x] Player timeout and cleanup
- [x] Sequence number ordering — discard out-of-order packets
- [ ] Refactor movement out of listener task into game tick task
- [ ] Collision detection against map using `shared::map::is_wall()`
- [ ] Support minimum 10 simultaneous connections (verify and test)

---

## Phase 4 — Client 🔄

- [ ] CLI prompt for server IP address and username on startup
- [ ] UDP network thread (send `InputPacket`, receive `StatePacket`)
- [ ] `mpsc` channel between network thread and render loop
- [ ] Window setup with `winit`
- [ ] Pixel buffer with `pixels`
- [ ] Raycaster render loop (DDA algorithm, fisheye correction)
- [ ] Wall shading based on distance
- [ ] Other player sprites rendered in world
- [ ] Mini-map overlay showing player position and map layout
- [ ] FPS counter displayed on screen

---

## Phase 5 — Integration

- [ ] Client and server running on same machine communicating over UDP
- [ ] Client and server running on separate machines over LAN
- [ ] Multiple clients connecting simultaneously (test with 3+ players)
- [ ] Player positions syncing correctly across all clients
- [ ] Level selection working across all 3 levels

---

## Phase 6 — Polish & Performance

- [ ] Consistent 50+ FPS in release mode (`cargo run --release`)
- [ ] Clean disconnect handling (player removed on timeout)
- [ ] Level transitions working correctly
- [ ] Critical event ACK layer (player death, level change)
- [ ] Basic shooting mechanic

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

_Last updated: May 2026_
