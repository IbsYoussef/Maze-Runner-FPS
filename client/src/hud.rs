// hud.rs
// Everything drawn as a flat overlay on top of the 3D view: the minimap,
// the scoreboard, and the win screen. All of this is drawn after
// switching back to the default 2D camera, once the 3D scene is finished.

use macroquad::prelude::*;

use shared::protocol::{PlayerState, StatePacket};

use crate::player::LocalPlayer;

// Draws a small top down view of the maze in the bottom right corner,
// showing every player's position and our own facing.
pub fn draw_minimap(map: &shared::map::Map, player: &LocalPlayer, state: &Option<StatePacket>) {
    const SCALE: f32 = 6.0;
    const MARGIN: f32 = 12.0;

    let size = map.width as f32 * SCALE;
    let ox = screen_width() - size - MARGIN;
    let oy = screen_height() - size - MARGIN;

    // the map itself, walls drawn lighter than open floor
    for gy in 0..map.height {
        for gx in 0..map.width {
            let color = if map.is_wall(gx, gy) {
                LIGHTGRAY
            } else {
                Color::new(0.1, 0.1, 0.1, 0.8)
            };
            draw_rectangle(
                ox + gx as f32 * SCALE,
                oy + gy as f32 * SCALE,
                SCALE,
                SCALE,
                color,
            );
        }
    }

    // every other player, shown as a blue dot
    if let Some(state) = state {
        for p in &state.players {
            if p.id == state.your_id {
                continue;
            }
            draw_circle(ox + p.x * SCALE, oy + p.y * SCALE, 2.5, SKYBLUE);
        }
    }

    // our own position, shown as a yellow dot, uses the local player's
    // position directly so it matches exactly what the camera is showing
    draw_circle(ox + player.x * SCALE, oy + player.z * SCALE, 2.5, YELLOW);
}

// Shortens a player's name if it is unusually long, so the scoreboard
// panel can never grow wide enough to break the layout.
fn display_name(p: &PlayerState) -> String {
    let name = if p.username.is_empty() {
        format!("P{}", p.id)
    } else {
        p.username.clone()
    };
    if name.chars().count() > 12 {
        format!("{}...", name.chars().take(12).collect::<String>())
    } else {
        name
    }
}

// Draws the kill count for every connected player, in the top left corner.
// The panel width is measured against the longest row of text, so it
// always fits its content whether names are short or long.
pub fn draw_scoreboard(state: &StatePacket) {
    let rows: f32 = state.players.len() as f32;

    let mut panel_w = 100.0f32;
    for p in &state.players {
        let text = format!("> {}  kills: {}", display_name(p), p.kills);
        let w = measure_text(&text, None, 20, 1.0).width;
        panel_w = panel_w.max(w + 24.0);
    }

    draw_rectangle(
        8.0,
        60.0,
        panel_w,
        10.0 + rows * 22.0,
        Color::new(0.0, 0.0, 0.0, 0.6),
    );

    for (i, p) in state.players.iter().enumerate() {
        let y = 78.0 + i as f32 * 22.0;
        let is_me = p.id == state.your_id;
        let color = if is_me { YELLOW } else { SKYBLUE };
        let marker = if is_me { ">" } else { " " };
        draw_text(
            &format!("{} {}  kills: {}", marker, display_name(p), p.kills),
            14.0,
            y,
            20.0,
            color,
        );
    }
}

// Dims the whole screen and shows either YOU WIN or GAME OVER, once the
// server reports the match has ended.
pub fn draw_win_overlay(state: &StatePacket) {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.55),
    );

    let (msg, color) = if state.winner_id == state.your_id {
        ("YOU WIN!", GREEN)
    } else {
        ("GAME OVER", RED)
    };
    let w = measure_text(msg, None, 72, 1.0).width;
    draw_text(
        msg,
        (screen_width() - w) / 2.0,
        screen_height() / 2.0,
        72.0,
        color,
    );

    let sub = format!("Player {} wins the match", state.winner_id);
    let sw = measure_text(&sub, None, 28, 1.0).width;
    draw_text(
        &sub,
        (screen_width() - sw) / 2.0,
        screen_height() / 2.0 + 40.0,
        28.0,
        WHITE,
    );
}
