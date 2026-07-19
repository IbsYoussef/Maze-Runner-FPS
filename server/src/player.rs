// player.rs
// Defines a connected player and how they spawn or respawn in the maze.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use crate::config::FUEL_MAX;

// every connected player, keyed by the network address they are sending from
pub type Players = Arc<Mutex<HashMap<SocketAddr, Player>>>;

#[derive(Debug)]
pub struct Player {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub fuel: f32,
    pub kills: u32,
    pub respawn_at: Option<Instant>,
    pub session_token: u64,
    pub last_sequence: u32,
    pub last_seen: Instant,
    pub packet_count: u32,
    pub rate_window_start: Instant,

    // input flags, set by the listener task and read by the game tick task
    pub input_forward: bool,
    pub input_backward: bool,
    pub input_turn_left: bool,
    pub input_turn_right: bool,
    pub input_shoot: bool,
    pub just_shot: bool,
    pub last_shot_at: Option<Instant>,

    pub username: String,
}

// Collects every open (non-wall) floor cell in the map.
// This is computed once when the server starts and then reused for every
// spawn and respawn, instead of scanning the whole map every single time.
pub fn open_cells(map: &shared::map::Map) -> Vec<(usize, usize)> {
    let mut cells = Vec::new();
    for gy in 0..map.height {
        for gx in 0..map.width {
            if !map.is_wall(gx, gy) {
                cells.push((gx, gy));
            }
        }
    }
    cells
}

// Picks a spawn position and facing direction for a player.
//
// The position is a random open floor cell, but we sample a handful of
// candidates and keep whichever one is furthest from any currently live
// player. This spreads players out and stops someone from camping a
// known spawn point, which becomes more important as the lobby grows
// toward the brief's 10 player target.
//
// The facing direction is picked from the open cardinal directions
// (up, down, left, right) so a player never spawns staring straight
// into a wall. Among the open directions we prefer whichever one points
// most toward the centre of the maze.
pub fn spawn_pos(
    map: &shared::map::Map,
    open: &[(usize, usize)],
    occupied: &[(f32, f32)],
) -> (f32, f32, f32) {
    use rand::RngExt;
    use std::f32::consts::{FRAC_PI_2, PI};

    let mut rng = rand::rng();

    let mut best_cell = open[rng.random_range(0..open.len())];
    let mut best_dist = f32::MIN;
    for _ in 0..8 {
        let candidate = open[rng.random_range(0..open.len())];
        let (cx, cy) = (candidate.0 as f32 + 0.5, candidate.1 as f32 + 0.5);
        let min_dist = occupied
            .iter()
            .map(|(ox, oy)| (cx - ox).powi(2) + (cy - oy).powi(2))
            .fold(f32::MAX, f32::min);
        if min_dist > best_dist {
            best_dist = min_dist;
            best_cell = candidate;
        }
    }

    let x = best_cell.0 as f32 + 0.5;
    let y = best_cell.1 as f32 + 0.5;

    // each candidate facing is (yaw, forward dx, forward dy)
    // forward direction for a given yaw is (sin yaw, cos yaw)
    let candidates: [(f32, f32, f32); 4] = [
        (0.0, 0.0, 1.0),
        (FRAC_PI_2, 1.0, 0.0),
        (PI, 0.0, -1.0),
        (-FRAC_PI_2, -1.0, 0.0),
    ];

    let (cx, cy) = (8.0 - x, 8.0 - y);
    let mut best_yaw = 0.0f32;
    let mut best_score = f32::MIN;
    for (yaw, dx, dy) in candidates {
        let nx = (x + dx) as i32;
        let ny = (y + dy) as i32;
        if nx < 0 || ny < 0 || map.is_wall(nx as usize, ny as usize) {
            continue;
        }
        let score = dx * cx + dy * cy;
        if score > best_score {
            best_score = score;
            best_yaw = yaw;
        }
    }

    (x, y, best_yaw)
}

impl Player {
    pub fn spawn(
        id: u32,
        token: u64,
        map: &shared::map::Map,
        open: &[(usize, usize)],
        occupied: &[(f32, f32)],
    ) -> Self {
        let (x, y, angle) = spawn_pos(map, open, occupied);
        Self {
            id,
            x,
            y,
            angle,
            fuel: FUEL_MAX,
            kills: 0,
            respawn_at: None,
            session_token: token,
            last_sequence: 0,
            last_seen: Instant::now(),
            packet_count: 0,
            rate_window_start: Instant::now(),
            input_forward: false,
            input_backward: false,
            input_turn_left: false,
            input_turn_right: false,
            input_shoot: false,
            just_shot: false,
            last_shot_at: None,
            username: String::new(),
        }
    }

    pub fn respawn(
        &mut self,
        map: &shared::map::Map,
        open: &[(usize, usize)],
        occupied: &[(f32, f32)],
    ) {
        let (x, y, angle) = spawn_pos(map, open, occupied);
        self.x = x;
        self.y = y;
        self.angle = angle;
        self.fuel = FUEL_MAX;
        self.respawn_at = None;
    }
}
