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

Since this phase, `protocol.rs` has grown considerably: `ShotEvent`, `username`, `disconnecting`, `level`, `fuel`, `kills`, and `respawning` were all added as the game grew. `map.rs` is unchanged in structure, only documentation was added later.

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

Movement was later changed again: the server no longer simulates player movement at all, position and facing are client-authoritative and the server only validates them against the map. See Phase 8 for why.

---

## Phase 4 — Client ✅

- [x] CLI prompt for server IP address and username on startup
- [x] UDP network thread (send `InputPacket`, receive `StatePacket`)
- [x] `mpsc` channel between network thread and render loop
- [ ] ~~Window setup with `winit`~~ superseded, see note below
- [ ] ~~Pixel buffer with `pixels`~~ superseded, see note below
- [ ] ~~Raycaster render loop (DDA algorithm, fisheye correction)~~ superseded, see note below
- [ ] ~~Wall shading based on distance~~ superseded, see note below
- [x] Other player sprites rendered in world
- [x] Mini-map overlay showing player position and map layout
- [x] FPS counter displayed on screen

The original client was built on `winit` and `pixels` with a hand rolled DDA raycaster. This client worked, but repeatedly crashed under specific conditions (players standing very close together, certain screen resolutions) that were difficult to fully resolve. The entire client was rebuilt from scratch on `macroquad`, a proper 3D framework, which removed the crash class entirely and left far less rendering code to maintain. See Phase 8.

---

## Phase 5 — Integration ✅

- [x] Client and server running on same machine communicating over UDP
- [x] Client and server running on separate machines over LAN
- [x] Multiple clients connecting simultaneously (test with 3+ players)
- [x] Player positions syncing correctly across all clients
- [x] Level selection working across all 3 levels

---

## Phase 6 — Polish & Performance ✅

- [x] Consistent 50+ FPS in release mode (`cargo run --release`)
- [x] Clean disconnect handling (player removed on timeout)
- [x] Level transitions working correctly
- [ ] ~~Critical event ACK layer (player death, level change)~~ not needed, see note below
- [x] Basic shooting mechanic

An acknowledgement layer for critical events was planned but turned out unnecessary. The state packet already carries the full current state of the match every tick, so a client that missed a packet simply catches up on the next one rather than needing an explicit acknowledgement and retry system.

---

## Phase 7 — Submission

- [ ] All features from brief verified working, final audit pending
- [ ] `README.md` updated with build and run instructions
- [ ] Final merge to `main` on GitHub
- [x] Gitea `main` verified in sync for course submission
- [x] `cargo build --release` produces clean build with no warnings

---

## Phase 8 — Client Rebuild ✅

The original `winit` and `pixels` based client suffered from a class of crashes that were eventually root caused to unsafe integer casts and missing bounds checks in the hand rolled rendering code, made worse by WSLg specific display quirks that made debugging unusually difficult. Rather than continuing to patch individual crash sites, the client was rebuilt entirely on `macroquad`.

- [x] New client architecture on `macroquad` (3D camera, `draw_cube` walls, built in text rendering)
- [x] WASD movement, mouse look, arrow key turning fallback
- [x] First person camera with proper collision against the map
- [x] Client-authoritative position and facing angle, with server-side validation against walls, removing simulation drift entirely
- [x] Cosmetic flying projectile shown for every shot, hit or miss
- [x] Death delay so a shot victim's disappearance visually lines up with the projectile landing, instead of vanishing a beat early
- [x] Server-side fix for a bug where a respawning player's position was reassigned on every tick instead of once, causing a visible rapid teleport effect during the respawn freeze
- [x] Random open floor cell spawning with anti-camping distance sampling, replacing the earlier fixed four corner spawn table
- [x] Map-aware spawn facing, so a player never spawns staring directly into a wall
- [x] Username system, with a live scoreboard showing real names instead of player numbers
- [x] Accurate frame rate counter, replacing `get_fps()` after testing showed it could overreport by 20% or more against real measured throughput
- [x] Dedicated quit key (Q) plus a goodbye packet, so a clean quit is detected by the server instantly instead of waiting out the five second timeout

---

## Phase 9 — Infrastructure and Documentation ✅

- [x] Docker setup for both server and client, with X11 forwarding investigated for Linux mouse support
- [x] Full module split for both `server` and `client`, each file now handling one clear responsibility (`config`, `player`, `listener`, `tick`, `broadcast` on the server; `net`, `player`, `render`, `projectile`, `hud` on the client)
- [x] Documentation pass across the entire `shared` crate
- [ ] Visual polish (wall colours, ceiling and floor appearance, player character look) — deferred, see Future Work
- [ ] Audio (shoot and impact sounds) — deferred, see Future Work
- [ ] Impact splatter effect on a landed hit — deferred, see Future Work

---

## Known Limitations

- Mouse look is confirmed working correctly on native Windows and native Linux, but is unreliable under WSLg, including when run inside Docker, since Docker still connects through WSLg's own display server. Arrow key turning is the reliable fallback in that specific environment.
- The match win screen currently resets the whole session automatically after a short display window. There is no per player option to leave or continue individually, every connected client is reset together. See Future Work.

---

## Future Work (not required for submission)

- [ ] Visual polish: wall colours, ceiling and floor appearance, distinct player character look
- [ ] Audio: shoot sound, impact sound
- [ ] Impact splatter visual effect on a landed hit
- [ ] Per player choice at match end (play again or leave individually), with the server shutting down once every player has chosen to leave
- [ ] Re-test mouse look once permanently migrated to a native Linux install, since WSLg's known limitations should not apply there

---

## Bonus planned features

- [ ] Maze auto-generation algorithm
- [ ] Level editor
- [ ] AI players
- [ ] GUI-based server connection and history

---

_Last updated: July 19th 2026_
