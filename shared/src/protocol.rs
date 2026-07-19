// protocol.rs
// Every packet type sent between the client and the server over UDP.
//
// There are two directions of travel. InputPacket goes from client to
// server, sent about 60 times a second, and carries what the player is
// currently doing: which keys are held, whether they are shooting, and
// where the client believes it is standing. StatePacket goes from
// server to client, sent about the same rate, and carries the current
// state of the whole match: every player's position, score, and
// anything else the client needs to draw the game correctly.
//
// Both sides serialise these structs into raw bytes before sending them
// over the network, and deserialise them back into these same structs
// on the receiving end. As long as both sides agree on the shape of
// these structs, which is the entire purpose of this shared crate, the
// bytes sent by one side can always be understood by the other.

use serde::{Deserialize, Serialize};

/// The largest number of bytes a single UDP packet is allowed to be.
/// Anything larger than this is rejected as suspicious rather than
/// processed, since a legitimate packet should never come close to
/// this size.
pub const MAX_PACKET_BYTES: usize = 256;

/// How many kills are needed to win a match. Once a player reaches this
/// many kills, the server declares them the winner and the match ends.
pub const KILL_LIMIT: u32 = 10;

/// Sent from the client to the server, roughly 60 times a second.
/// Describes what the player is currently doing: which movement keys
/// are held, whether they are shooting, which way they are facing, and
/// where they currently believe they are standing.
///
/// Position and facing angle are reported by the client rather than
/// simulated by the server. The server still checks that a reported
/// position is not inside a wall before accepting it, but trusting the
/// client's own movement removes the small stutter that would come
/// from waiting for the server to simulate and echo back every step.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InputPacket {
    /// increases by one with every packet sent, lets the server tell
    /// packets apart and discard any that arrive out of order
    pub sequence: u32,
    /// not currently used, reserved for a future use where the client
    /// might need to identify itself before the server has assigned it
    /// a proper id
    pub player_id: u32,
    /// a value the server hands the client once it registers, used to
    /// confirm later packets really are coming from that same player
    pub session_token: u64,
    pub forward: bool,
    pub backward: bool,
    /// no longer used for turning, the client turns using `angle`
    /// below instead, kept here so older packets still deserialise
    /// correctly rather than for any active purpose
    pub turn_left: bool,
    pub turn_right: bool,
    pub shoot: bool,
    /// the direction the player is currently facing, in radians
    pub angle: f32,
    /// the player's current x position in the maze, as the client
    /// currently understands it
    pub x: f32,
    /// the player's current y position in the maze. note that the
    /// client's own 3D world uses this same value as its z axis, since
    /// in 3D rendering y is normally reserved for height instead
    pub y: f32,
    /// the player's chosen name. only sent with a real value on the
    /// very first packet after connecting, every packet after that
    /// leaves this as an empty string, since the server only needs to
    /// learn it once
    pub username: String,
    /// true only on the single final packet a client sends right before
    /// it quits cleanly, using the dedicated quit key. the server uses
    /// this to remove the player immediately instead of waiting for the
    /// usual timeout, which still exists separately to catch a client
    /// that crashes or loses its connection without ever sending this
    pub disconnecting: bool,
}

/// Sent from the server to every client, roughly 60 times a second.
/// Describes the entire current state of the match: where every
/// player is, whether the match has ended, and any shots fired since
/// the last packet.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatePacket {
    /// increases by one with every packet sent, mirrors the same idea
    /// as InputPacket's sequence number
    pub sequence: u32,
    /// tells the receiving client which of the players in the list
    /// below is themself, since every client receives the same list of
    /// every connected player
    pub your_id: u32,
    /// every currently connected player, including the receiving
    /// client's own player
    pub players: Vec<PlayerState>,
    /// true once a player has reached the kill limit and the match has
    /// ended
    pub match_over: bool,
    /// the id of whichever player won, only meaningful once
    /// `match_over` is true
    pub winner_id: u32,
    /// every shot fired since the last packet was sent. this exists so
    /// clients can show a flying projectile and know whether it was a
    /// hit or a miss, even though the server itself resolves whether a
    /// shot landed instantly rather than simulating a travelling bullet
    pub shot_events: Vec<ShotEvent>,
    /// which of the three levels the server currently has loaded. the
    /// client uses this to make sure it is drawing the exact same maze
    /// layout the server is using for collision checks
    pub level: u8,
}

/// One player's current state, as seen from the server. This is what
/// every client uses to draw every other connected player, and to
/// check their own scoreboard entry.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerState {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    /// remaining fuel, from 0 up to 100. running out causes the player
    /// to respawn, so this acts as a soft time pressure on staying
    /// still or hiding
    pub fuel: f32,
    pub kills: u32,
    /// true for the few seconds right after this player was shot or ran
    /// out of fuel, during which they are frozen and cannot be shot
    /// again, clients use this to hide them from view during that
    /// window rather than showing them standing still
    pub respawning: bool,
    pub username: String,
}

/// One shot that was fired and resolved during a single game tick.
///
/// The server checks whether a shot hits a player instantly, there is
/// no simulated bullet travelling through the air as far as the game
/// logic is concerned. This struct exists purely so clients can show a
/// small cosmetic projectile flying from the shooter toward wherever
/// the shot actually ended up, whether that is a player it hit or just
/// the point in space (or the wall) where the shot's range ran out.
///
/// This list is cleared and rebuilt every single tick on the server, so
/// receiving an empty list simply means nobody fired that tick, not
/// that something went wrong.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShotEvent {
    pub shooter_id: u32,
    /// where the shooter was standing at the moment they fired
    pub shooter_x: f32,
    pub shooter_y: f32,
    pub shooter_angle: f32,
    /// where the shot ended up. if it hit a player this is that
    /// player's position at the moment of the hit, if it missed this is
    /// wherever the shot's travel distance or a wall stopped it
    pub hit_x: f32,
    pub hit_y: f32,
    /// true if this shot actually landed on a player, false if it
    /// missed entirely
    pub hit: bool,
}
