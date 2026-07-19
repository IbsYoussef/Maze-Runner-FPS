// config.rs
// All tunable constants and shared type aliases for the server.
// Keeping these in one file means every timing, distance, or limit value
// can be found and changed without digging through the game logic files.

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use shared::protocol::ShotEvent;

// how often the game tick and broadcast tasks run, in milliseconds
pub const TICK_MS: u64 = 16;

// how far a player moves per tick when a movement key is held
pub const PLAYER_SPEED: f32 = 0.05;

// a player is dropped from the game if no packet arrives for this many seconds
pub const TIMEOUT_SECS: u64 = 5;

// maximum input packets accepted per player per second, guards against spam
pub const RATE_LIMIT_PER_SEC: u32 = 128;

// starting and maximum fuel value
pub const FUEL_MAX: f32 = 100.0;

// fuel depletes fully over about 90 seconds, at roughly 62.5 ticks per second
pub const FUEL_DRAIN: f32 = FUEL_MAX / (90.0 * (1000.0 / TICK_MS as f32));

// how many seconds a player is frozen and hidden after being shot or running out of fuel
pub const RESPAWN_SECS: u64 = 3;

// how far a shot can travel before it is treated as a miss
pub const SHOOT_RANGE: f32 = 10.0;

// how close a shot's path must pass to a player to count as a hit
pub const SHOOT_WIDTH: f32 = 0.3;

// how long the win screen stays up before kills reset and a new match begins
pub const WIN_DISPLAY_SECS: u64 = 4;

// minimum time between shots for one player, lets you click as fast as you
// want without ever firing faster than this rate
pub const SHOOT_COOLDOWN_MS: u64 = 250;

// Shared state passed between the three async tasks.
// Wrapping each one in Arc<Mutex<...>> lets multiple tasks read and write
// safely, since they all run concurrently on the same data.

// records who won and when, so the win screen can be held for a few
// seconds before kills are reset for the next match
pub type MatchState = Arc<Mutex<Option<(u32, Instant)>>>;

// holds this tick's shot events until the broadcast task picks them up
// and sends them out, so every client can show the same visual effects
pub type ShotEvents = Arc<Mutex<Vec<ShotEvent>>>;

// every open (non-wall) floor cell in the loaded map, computed once at
// startup and reused every time a player needs a new spawn point
pub type OpenCells = Arc<Vec<(usize, usize)>>;
