// protocol.rs
// Defines all UDP packet types sent between client and server.
// InputPacket  — client -> server (player keypresses)
// StatePacket  — server -> client (world state for all players)
// PlayerState  — individual player position and angle inside StatePacket

use serde::{Deserialize, Serialize};

pub const MAX_PACKET_BYTES: usize = 256;

// Client → Server
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InputPacket {
    pub sequence: u32,
    pub player_id: u32,
    pub session_token: u64,
    pub forward: bool,
    pub backward: bool,
    pub turn_left: bool,
    pub turn_right: bool,
}

// Server → Client
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatePacket {
    pub sequence: u32,
    pub players: Vec<PlayerState>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerState {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
}
