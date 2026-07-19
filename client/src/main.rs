// main.rs
// Client entry point.
//
// On startup, prompts for a server address and username in the terminal,
// matching the brief's required flow. From there the main loop runs three
// things every frame: reads input and updates the local player, receives
// the latest state from the server (via a background thread, see net.rs),
// and draws the 3D world followed by the 2D HUD.

mod hud;
mod net;
mod player;
mod projectile;
mod render;

use macroquad::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use shared::map::get_level;
use shared::protocol::StatePacket;

use net::{NetInput, net_thread};
use player::LocalPlayer;
use projectile::{DeathDelay, Projectile, draw_projectiles, spawn_projectiles};
use render::{draw_maze, draw_players};

// Prompts the player for connection details before the game window opens.
// This matches the brief's required terminal startup flow exactly.
fn prompt_input(label: &str) -> String {
    use std::io::Write;
    print!("{label}");
    std::io::stdout().flush().unwrap();
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .expect("failed to read input");
    input.trim().to_string()
}

#[macroquad::main("Maze Runner FPS")]
async fn main() {
    let server_addr = prompt_input("Enter IP Address: ");
    let username = prompt_input("Enter Name: ");
    println!("Starting...");

    // the level is a placeholder until the server tells us which one it
    // actually loaded, see the level sync check further down
    let mut map = get_level(1);
    let mut map_loaded_level: u8 = 1;
    let mut player = LocalPlayer {
        x: 1.5,
        z: 1.5,
        yaw: 0.0,
    };

    let input = Arc::new(NetInput::new());
    let (state_tx, state_rx) = mpsc::sync_channel::<StatePacket>(1);
    {
        let input = Arc::clone(&input);
        thread::spawn(move || net_thread(server_addr.clone(), username.clone(), input, state_tx));
    }
    let mut last_state: Option<StatePacket> = None;

    let mut grabbed = false; // start free, click a window to capture the mouse

    // accurate frame rate: count actual completed frames per wall clock
    // second, rather than trusting get_fps(), which measures a single
    // frame's instantaneous duration and can overreport significantly
    let mut frame_count: u32 = 0;
    let mut fps_display: u32 = 0;
    let mut fps_window_start = Instant::now();

    let mut spawned = false;
    let mut projectiles: Vec<Projectile> = Vec::new();
    let mut death_delay = DeathDelay {
        hidden_after: HashMap::new(),
    };

    loop {
        let dt = get_frame_time();

        if is_key_pressed(KeyCode::Escape) {
            grabbed = false;
            set_cursor_grab(false);
            show_mouse(true);
        }

        if is_mouse_button_pressed(MouseButton::Left) && !grabbed {
            grabbed = true;
            set_cursor_grab(true);
            show_mouse(false);
        }

        input.shoot.store(
            grabbed && is_mouse_button_down(MouseButton::Right),
            Ordering::Relaxed,
        );

        // Mouse look via mouse_delta_position(). This is confirmed working
        // correctly on native Windows and native Linux (macroquad's own
        // maintainer verified it in PR #181). Under WSLg, including inside
        // Docker (since Docker still connects through WSLg's own X11
        // implementation), this reads as zero or occasionally spikes to
        // corrupted values in the thousands. Arrow key turning in
        // LocalPlayer::update is the reliable fallback in that
        // environment, this line is left in place so it works correctly
        // the moment the client runs on a real display server.
        //
        // The raw delta is captured here but not yet applied. We wait
        // until we know whether the player is respawning (checked just
        // below) before deciding whether to use it, since the server owns
        // our facing direction during that window and a mouse spike
        // arriving at the same moment could otherwise fight that value
        // and cause a visible flicker.
        let raw = mouse_delta_position();

        // drain incoming state and check respawn status before applying
        // any movement or look input this frame
        while let Ok(s) = state_rx.try_recv() {
            last_state = Some(s);
        }

        // if we are currently respawning, the server owns our position,
        // snap to wherever it says we are every frame while dead
        let mut respawning = false;
        if let Some(state) = &last_state {
            if let Some(me) = state.players.iter().find(|p| p.id == state.your_id) {
                if me.respawning {
                    respawning = true;
                    player.x = me.x;
                    player.z = me.y;
                    player.yaw = me.angle;
                }
            }
        }

        // mouse look is disabled entirely for now while working on WSL,
        // where it is unreliable, arrow keys are the only way to turn
        // until this is revisited on native Windows or native Linux
        let look_dx = 0.0;
        let _ = raw; // keep the variable so re-enabling this later is a one-line change

        player.update(&map, dt, look_dx);

        input.forward.store(
            is_key_down(KeyCode::W) || is_key_down(KeyCode::Up),
            Ordering::Relaxed,
        );
        input.backward.store(
            is_key_down(KeyCode::S) || is_key_down(KeyCode::Down),
            Ordering::Relaxed,
        );
        input
            .angle_bits
            .store(player.yaw.to_bits(), Ordering::Relaxed);
        input.x_bits.store(player.x.to_bits(), Ordering::Relaxed);
        input.y_bits.store(player.z.to_bits(), Ordering::Relaxed);

        // switch to whichever level the server is actually running the
        // first time it differs from what we have loaded locally
        if let Some(state) = &last_state {
            if state.level != map_loaded_level {
                map = get_level(state.level);
                map_loaded_level = state.level;
            }
        }

        if let Some(state) = &last_state {
            spawn_projectiles(
                &state.shot_events,
                &mut projectiles,
                &state.players,
                &mut death_delay,
            );
        }

        // one time only: adopt the spawn point and facing the server
        // assigned us, instead of the placeholder position we started at
        if !spawned {
            if let Some(state) = &last_state {
                if let Some(me) = state.players.iter().find(|p| p.id == state.your_id) {
                    player.x = me.x;
                    player.z = me.y;
                    player.yaw = me.angle;
                    spawned = true;
                    input.spawned.store(true, Ordering::Relaxed);
                }
            }
        }

        clear_background(BLACK);

        set_camera(&player.camera());
        draw_maze(&map);
        if let Some(state) = &last_state {
            draw_players(state, &death_delay);
            draw_projectiles(&mut projectiles);
        }

        set_default_camera();

        frame_count += 1;
        if fps_window_start.elapsed().as_secs_f32() >= 1.0 {
            fps_display = frame_count;
            frame_count = 0;
            fps_window_start = Instant::now();
        }
        draw_text(
            &format!("FPS: {}", fps_display),
            screen_width() - 100.0,
            30.0,
            24.0,
            YELLOW,
        );

        if let Some(state) = &last_state {
            if let Some(me) = state.players.iter().find(|p| p.id == state.your_id) {
                let name = if me.username.is_empty() {
                    format!("P{}", me.id)
                } else {
                    me.username.clone()
                };
                draw_text(&name, 12.0, 30.0, 24.0, YELLOW);
            }
        }

        let cx = screen_width() / 2.0;
        let cy = screen_height() / 2.0;
        draw_rectangle(cx - 8.0, cy - 1.0, 16.0, 2.0, WHITE);
        draw_rectangle(cx - 1.0, cy - 8.0, 2.0, 16.0, WHITE);

        if respawning {
            let msg = "RESPAWNING...";
            let w = measure_text(msg, None, 48, 1.0).width;
            draw_text(
                msg,
                (screen_width() - w) / 2.0,
                screen_height() / 2.0 - 60.0,
                48.0,
                RED,
            );
        }

        if let Some(state) = &last_state {
            if let Some(me) = state.players.iter().find(|p| p.id == state.your_id) {
                let fuel_color = if me.fuel < 25.0 { RED } else { WHITE };
                draw_text(
                    &format!("FUEL: {:.0}", me.fuel),
                    14.0,
                    48.0,
                    20.0,
                    fuel_color,
                );
            }
            hud::draw_scoreboard(state);
            if state.match_over {
                hud::draw_win_overlay(state);
            }
        }

        hud::draw_minimap(&map, &player, &last_state);

        next_frame().await
    }
}
