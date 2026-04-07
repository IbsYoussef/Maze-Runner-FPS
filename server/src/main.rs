// server — entry point
// Responsibilities:
//   UDP listener task  — receives InputPackets from connected clients
//   Game tick task     — runs on a fixed 16ms interval, updates game state
//   Broadcast task     — sends StatePackets to all connected clients
