# Task List — 2026-04-21

## PR Reviews (waiting on teammate)
- [ ] Teammate reviews `feat/shared-packet-protocol` → approve or request changes
- [ ] Merge `feat/shared-packet-protocol` into `dev` once approved
- [ ] Teammate reviews `docs/security-and-game-spec` → approve or merge (independent)
- [ ] Merge `feat/server-udp-listener` into `dev` **after** shared PR is merged

## Client Crate — `feat/client-udp-network`
- [ ] Create branch `feat/client-udp-network` from `dev`
- [ ] Implement network thread in `client/src/main.rs`
  - UDP send/receive loop
  - mpsc channel to communicate with render thread
  - Send `InputPacket`, receive `StatePacket`
  - Reuse types from `shared` crate

## Client Crate — `feat/client-renderer`
- [ ] Create branch `feat/client-renderer` from `dev`
- [ ] Implement render loop in `client/src/main.rs`
  - Raycaster (walls, map)
  - Mini-map overlay
  - FPS counter

## Housekeeping
- [ ] Verify `dev` branch protection rules match `main`
