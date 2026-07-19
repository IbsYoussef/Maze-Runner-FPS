// projectile.rs
// The cosmetic flying projectile shown for every shot fired, hit or miss.
//
// The server resolves hits instantly (this is called hitscan), there is
// no real travelling bullet as far as the game logic is concerned. This
// file is purely visual: when the server reports a shot in its state
// packet, we spawn a small cube here that flies from the shooter to the
// target point over a fraction of a second, so the shot feels like it
// has some presence rather than just appearing and disappearing instantly.

use macroquad::prelude::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use shared::protocol::{PlayerState, ShotEvent};

pub const PROJECTILE_TRAVEL_SECS: f32 = 0.08;

pub struct Projectile {
    pub start: Vec3,
    pub end: Vec3,
    pub spawned_at: Instant,
}

// Maps a player's id to the moment it becomes safe to actually hide them
// from view. Without this, a victim's cube would vanish from the world
// the instant the server reports their death, which happens before the
// projectile has visually finished travelling to them. Holding them
// visible for PROJECTILE_TRAVEL_SECS longer makes the hit and the
// disappearance feel simultaneous instead of the victim vanishing early.
pub struct DeathDelay {
    pub hidden_after: HashMap<u32, Instant>,
}

// Turns this tick's shot events into visible projectiles, and schedules
// a death delay for whichever player was closest to a hit's impact point.
pub fn spawn_projectiles(
    events: &[ShotEvent],
    projectiles: &mut Vec<Projectile>,
    players: &[PlayerState],
    death_delay: &mut DeathDelay,
) {
    for ev in events {
        projectiles.push(Projectile {
            start: vec3(ev.shooter_x, 0.5, ev.shooter_y),
            end: vec3(ev.hit_x, 0.5, ev.hit_y),
            spawned_at: Instant::now(),
        });

        if ev.hit {
            // the shot event only carries the impact point, not which
            // player id was hit, so we find whoever is standing closest
            // to that point and treat them as the victim
            if let Some(victim) = players.iter().min_by(|a, b| {
                let da = (a.x - ev.hit_x).powi(2) + (a.y - ev.hit_y).powi(2);
                let db = (b.x - ev.hit_x).powi(2) + (b.y - ev.hit_y).powi(2);
                da.partial_cmp(&db).unwrap()
            }) {
                death_delay.hidden_after.insert(
                    victim.id,
                    Instant::now() + Duration::from_secs_f32(PROJECTILE_TRAVEL_SECS),
                );
            }
        }
    }
}

// Draws every active projectile at its current position along its flight
// path, and removes any that have finished travelling.
pub fn draw_projectiles(projectiles: &mut Vec<Projectile>) {
    let now = Instant::now();
    projectiles.retain(|p| {
        let t = now.duration_since(p.spawned_at).as_secs_f32() / PROJECTILE_TRAVEL_SECS;
        if t >= 1.0 {
            return false;
        }
        let pos = p.start + (p.end - p.start) * t;
        draw_cube(pos, vec3(0.15, 0.15, 0.15), None, YELLOW);
        true
    });
}
