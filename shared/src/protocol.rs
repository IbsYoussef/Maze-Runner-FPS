// protocol.rs
// Defines all UDP packet types sent between client and server.
// InputPacket  — client -> server (player keypresses)
// StatePacket  — server -> client (world state for all players)
// PlayerState  — individual player position and angle inside StatePacket

use serde::{Deserialize, Serialize};

pub const MAX_PACKET_BYTES: usize = 256;
pub const KILL_LIMIT: u32 = 10;

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
    pub shoot: bool,
    pub angle: f32,
    pub x: f32,           // client-authoritative position
    pub y: f32,           // (server "y" == client world z)
    pub username: String, // sent once on first packet, empty afterward
}

// Server → Client
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatePacket {
    pub sequence: u32,
    pub your_id: u32,
    pub players: Vec<PlayerState>,
    pub match_over: bool,
    pub winner_id: u32,
    pub shot_events: Vec<ShotEvent>, // shots resolved this tick, for cosmetic FX
    pub level: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerState {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub fuel: f32,
    pub kills: u32,
    pub respawning: bool, // true while dead & waiting to respawn
    pub username: String,
}

// A shot that was resolved this tick — used by clients to trigger
// cosmetic projectile + splatter effects. Cleared every broadcast,
// so this is "events this tick" not a persistent history.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShotEvent {
    pub shooter_id: u32,
    pub shooter_x: f32,
    pub shooter_y: f32,
    pub shooter_angle: f32,
    pub hit_x: f32,
    pub hit_y: f32,
    pub hit: bool, // true if it landed on a player, false if it just missed/expired
}
