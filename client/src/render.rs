// render.rs
// Draws the 3D world: the maze walls, the floor, and other players.
// Everything here is drawn inside the 3D camera, before switching back
// to 2D screen space for the HUD.

use macroquad::prelude::*;
use std::time::Instant;

use shared::protocol::StatePacket;

use crate::projectile::DeathDelay;

pub const WALL_HEIGHT: f32 = 1.0;

// Draws the floor and every wall cell of the maze.
// One cube is drawn per wall cell in the map's grid. Grid cell (gx, gy)
// covers the world space from gx to gx+1 and gy to gy+1, so its centre
// sits at gx + 0.5 and gy + 0.5. The map's y axis becomes the world's z
// axis, since in 3D space y is used for height instead.
pub fn draw_maze(map: &shared::map::Map) {
    draw_plane(vec3(8.0, 0.0, 8.0), vec2(8.0, 8.0), None, DARKGRAY);

    for gy in 0..map.height {
        for gx in 0..map.width {
            if map.is_wall(gx, gy) {
                draw_cube(
                    vec3(gx as f32 + 0.5, WALL_HEIGHT / 2.0, gy as f32 + 0.5),
                    vec3(1.0, WALL_HEIGHT, 1.0),
                    None,
                    GRAY,
                );
            }
        }
    }
}

// Draws every other connected player as a simple cube.
// Our own player is never drawn here, since we are looking through their
// eyes via the camera, there is nothing to draw for ourselves in 3D space.
//
// A player who was just shot stays visible for a short grace period after
// their death is reported by the server, controlled by DeathDelay, so
// that the moment they disappear lines up with the projectile visually
// reaching them instead of them vanishing a beat early.
pub fn draw_players(state: &StatePacket, death_delay: &DeathDelay) {
    let now = Instant::now();
    for p in &state.players {
        if p.id == state.your_id {
            continue;
        }

        if p.respawning {
            if let Some(hide_at) = death_delay.hidden_after.get(&p.id) {
                if now < *hide_at {
                    // still within the grace period, fall through and draw normally
                } else {
                    continue;
                }
            } else {
                continue; // no grace period scheduled, hide immediately
            }
        }

        draw_cube(vec3(p.x, 0.4, p.y), vec3(0.5, 0.8, 0.5), None, SKYBLUE);
    }
}
