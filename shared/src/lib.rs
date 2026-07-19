// lib.rs
// The shared crate. Contains every type and function that both the
// server and the client need to agree on, so they always speak the
// exact same language over the network.
//
// This crate is deliberately small. It has no game logic of its own,
// it only defines the shapes of things: the maze grid, and the packets
// sent back and forth over UDP. Both server and client depend on this
// crate directly, so a change here affects both sides at once, which is
// exactly the point, it stops the two ends of the network connection
// from ever silently drifting out of sync with each other.

pub mod map;
pub mod protocol;
