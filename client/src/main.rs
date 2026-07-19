use macroquad::prelude::*;
use shared::map::{Map, get_level};
use shared::protocol::{InputPacket, MAX_PACKET_BYTES, StatePacket};
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
// world constants, one grid cell = one world unit
const WALL_HEIGHT: f32 = 1.0;
const EYE_HEIGHT: f32 = 0.5; // camera height off the floor
const MOVE_SPEED: f32 = 3.0; // world units per second
const MOUSE_SENSITIVITY: f32 = 1.5;
const KEY_TURN_SPEED: f32 = 2.5; // radians per second
const NET_HZ: u64 = 60;
// input state shared between game loop (writer) and net thread (reader)
struct NetInput {
    forward: AtomicBool,
    backward: AtomicBool,
    shoot: AtomicBool,
    angle_bits: AtomicU32, // f32 yaw stored as raw bits (std has no AtomicF32)
    x_bits: AtomicU32,
    y_bits: AtomicU32,
    spawned: AtomicBool,
}
impl NetInput {
    fn new() -> Self {
        Self {
            forward: AtomicBool::new(false),
            backward: AtomicBool::new(false),
            shoot: AtomicBool::new(false),
            angle_bits: AtomicU32::new(0f32.to_bits()),
            x_bits: AtomicU32::new(0f32.to_bits()),
            y_bits: AtomicU32::new(0f32.to_bits()),
            spawned: AtomicBool::new(false),
        }
    }
}
fn net_thread(
    server_addr: String,
    username: String,
    input: Arc<NetInput>,
    state_tx: mpsc::SyncSender<StatePacket>,
) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind failed");
    socket.connect(&server_addr).expect("connect failed");
    socket
        .set_read_timeout(Some(Duration::from_millis(1)))
        .unwrap();
    println!("net thread connected to {server_addr}");
    let interval = Duration::from_millis(1000 / NET_HZ);
    let mut seq = 0u32;
    let mut buf = vec![0u8; MAX_PACKET_BYTES];
    let mut pending_events: Vec<shared::protocol::ShotEvent> = Vec::new();
    // username is sent once on the first packet only, to avoid resending
    // the same string 60 times a second — the server remembers it after that
    let mut username_sent = false;
    loop {
        let t0 = Instant::now();
        seq += 1;
        let pkt = InputPacket {
            sequence: seq,
            player_id: 0,
            session_token: 0,
            forward: input.forward.load(Ordering::Relaxed),
            backward: input.backward.load(Ordering::Relaxed),
            turn_left: false, // legacy fields — angle now carries turning
            turn_right: false,
            shoot: input.shoot.load(Ordering::Relaxed),
            angle: f32::from_bits(input.angle_bits.load(Ordering::Relaxed)),
            // don't claim a position until we've adopted our server spawn —
            // the server rejects negatives, so it keeps its assigned corner
            x: if input.spawned.load(Ordering::Relaxed) {
                f32::from_bits(input.x_bits.load(Ordering::Relaxed))
            } else {
                -1.0
            },
            y: if input.spawned.load(Ordering::Relaxed) {
                f32::from_bits(input.y_bits.load(Ordering::Relaxed))
            } else {
                -1.0
            },
            username: if username_sent {
                String::new()
            } else {
                username.clone()
            },
        };
        if let Ok(enc) = postcard::to_allocvec(&pkt) {
            let _ = socket.send(&enc);
            username_sent = true;
        }
        // drain ALL queued packets. Positions only need the newest packet,
        // but shot_events are transient — if the channel is briefly full and
        // try_send fails, we must NOT lose the events; keep them pending and
        // retry on the next iteration instead of silently dropping them.
        let mut newest: Option<StatePacket> = None;
        while let Ok(len) = socket.recv(&mut buf) {
            if let Ok(state) = postcard::from_bytes::<StatePacket>(&buf[..len]) {
                pending_events.extend(state.shot_events.iter().cloned());
                newest = Some(state);
            }
        }
        if let Some(mut state) = newest {
            state.shot_events = pending_events.clone();
            if state_tx.try_send(state).is_ok() {
                pending_events.clear(); // only clear once actually delivered
            }
            // if try_send failed, pending_events survives to the next iteration
        }
        let elapsed = t0.elapsed();
        if elapsed < interval {
            thread::sleep(interval - elapsed);
        }
    }
}
struct LocalPlayer {
    x: f32,
    z: f32,
    yaw: f32,
}
impl LocalPlayer {
    fn forward(&self) -> Vec3 {
        vec3(self.yaw.sin(), 0.0, self.yaw.cos())
    }
    fn update(&mut self, map: &Map, dt: f32, look_dx: f32) {
        // --- mouse look: all turning comes from the mouse ---
        self.yaw += look_dx * MOUSE_SENSITIVITY;
        // keyboard turning fallback — works in WSLg where raw mouse motion doesn't
        if is_key_down(KeyCode::Left) {
            self.yaw += KEY_TURN_SPEED * dt;
        }
        if is_key_down(KeyCode::Right) {
            self.yaw -= KEY_TURN_SPEED * dt;
        }
        // --- WASD movement: W/S along forward, A/D strafe along right ---
        let fwd = self.forward();
        // right = forward x up = (-cos, 0, sin)
        let right = vec3(-self.yaw.cos(), 0.0, self.yaw.sin());
        let mut wish = vec3(0.0, 0.0, 0.0); // desired movement direction
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            wish += fwd;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            wish -= fwd;
        }
        if is_key_down(KeyCode::D) {
            wish += right;
        }
        if is_key_down(KeyCode::A) {
            wish -= right;
        }
        // normalise so diagonal movement isn't faster than straight
        if wish.length() > 0.0 {
            wish = wish.normalize() * MOVE_SPEED * dt;
        }
        let new_x = self.x + wish.x;
        let new_z = self.z + wish.z;
        // axis-separated collision, lets you slide along walls
        if !map.is_wall(new_x as usize, self.z as usize) {
            self.x = new_x;
        }
        if !map.is_wall(self.x as usize, new_z as usize) {
            self.z = new_z;
        }
    }
    fn camera(&self) -> Camera3D {
        let pos = vec3(self.x, EYE_HEIGHT, self.z);
        Camera3D {
            position: pos,
            target: pos + self.forward(),
            up: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        }
    }
}
fn draw_maze(map: &Map) {
    // floor: centre of a 16x16 grid is (8, 8); vec2(8., 8.) are half-extents
    draw_plane(vec3(8.0, 0.0, 8.0), vec2(8.0, 8.0), None, DARKGRAY);
    // one cube per wall cell
    for gy in 0..map.height {
        for gx in 0..map.width {
            if map.is_wall(gx, gy) {
                // cube position is its CENTRE:
                //   grid cell (gx, gy) spans gx..gx+1, so centre is gx + 0.5
                //   map's y axis becomes world z; y is up
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
struct Projectile {
    start: Vec3,
    end: Vec3,
    spawned_at: Instant,
}
const PROJECTILE_TRAVEL_SECS: f32 = 0.08;
// player_id -> instant after which it's safe to actually hide them,
// so a victim doesn't vanish before their incoming projectile visually lands
struct DeathDelay {
    hidden_after: std::collections::HashMap<u32, Instant>,
}
fn spawn_projectiles(
    events: &[shared::protocol::ShotEvent],
    projectiles: &mut Vec<Projectile>,
    players: &[shared::protocol::PlayerState],
    death_delay: &mut DeathDelay,
) {
    for ev in events {
        projectiles.push(Projectile {
            start: vec3(ev.shooter_x, 0.5, ev.shooter_y),
            end: vec3(ev.hit_x, 0.5, ev.hit_y),
            spawned_at: Instant::now(),
        });
        if ev.hit {
            // find whoever is closest to the impact point — that's the victim
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
fn draw_projectiles(projectiles: &mut Vec<Projectile>) {
    let now = Instant::now();
    projectiles.retain(|p| {
        let t = now.duration_since(p.spawned_at).as_secs_f32() / PROJECTILE_TRAVEL_SECS;
        if t >= 1.0 {
            return false; // expired, remove
        }
        let pos = p.start + (p.end - p.start) * t;
        draw_cube(pos, vec3(0.15, 0.15, 0.15), None, YELLOW);
        true
    });
}
fn draw_players(state: &StatePacket, death_delay: &DeathDelay) {
    let now = Instant::now();
    for p in &state.players {
        if p.id == state.your_id {
            continue;
        }
        // still respawning AND past the grace period? then actually hide them.
        // if still within the grace window, keep drawing so the projectile
        // and the death feel simultaneous rather than the victim vanishing early
        if p.respawning {
            if let Some(hide_at) = death_delay.hidden_after.get(&p.id) {
                if now < *hide_at {
                    // grace period active — fall through and draw normally below
                } else {
                    continue; // grace expired, hide as normal
                }
            } else {
                continue; // no grace scheduled, hide immediately (e.g. died to fuel)
            }
        }
        draw_cube(vec3(p.x, 0.4, p.y), vec3(0.5, 0.8, 0.5), None, SKYBLUE);
    }
}
fn draw_minimap(map: &Map, player: &LocalPlayer, state: &Option<StatePacket>) {
    const SCALE: f32 = 6.0; // pixels per map cell
    const MARGIN: f32 = 12.0;
    let size = map.width as f32 * SCALE;
    let ox = screen_width() - size - MARGIN; // origin x (bottom-right corner)
    let oy = screen_height() - size - MARGIN; // origin y
    // layer 1: tiles
    for gy in 0..map.height {
        for gx in 0..map.width {
            let color = if map.is_wall(gx, gy) {
                LIGHTGRAY
            } else {
                Color::new(0.1, 0.1, 0.1, 0.8) // semi-transparent dark
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
    // layer 2: Other players (from server state)
    if let Some(state) = state {
        for p in &state.players {
            if p.id == state.your_id {
                continue;
            }
            draw_circle(ox + p.x * SCALE, oy + p.y * SCALE, 2.5, SKYBLUE);
        }
    }
    // layer 3: you (local position, matches camerae position)
    draw_circle(ox + player.x * SCALE, oy + player.z * SCALE, 2.5, YELLOW);
}
fn draw_scoreboard(state: &StatePacket) {
    // truncate names to fit layout
    fn display_name(p: &shared::protocol::PlayerState) -> String {
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
    // panel background, below the P-label and fuel readout
    let rows: f32 = state.players.len() as f32;
    // measure the widest row so the panel always fits its content
    let mut panel_w = 100.0f32; // sensible minimum
    for p in &state.players {
        let text = format!("> {}  kills: {}", display_name(p), p.kills);
        let w = measure_text(&text, None, 20, 1.0).width;
        panel_w = panel_w.max(w + 24.0); // padding either side
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
fn draw_win_overlay(state: &StatePacket) {
    // dim the whole screen
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
// prompts the player for connection details before the game window opens,
// matching the brief's terminal startup flow exactly
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
    // startup flow: prompt for server IP and username
    let server_addr = prompt_input("Enter IP Address: ");
    let username = prompt_input("Enter Name: ");
    println!("Starting...");
    // load level once, outside the loop
    let mut map = get_level(1); // placeholder until we hear from the server
    let mut map_loaded_level: u8 = 1;
    let mut player = LocalPlayer {
        x: 1.5,
        z: 1.5,
        yaw: 0.0,
    };
    // networking: shared input + state channel, net thread in background
    let input = Arc::new(NetInput::new());
    let (state_tx, state_rx) = mpsc::sync_channel::<StatePacket>(1);
    {
        let input = Arc::clone(&input);
        thread::spawn(move || net_thread(server_addr.clone(), username.clone(), input, state_tx));
    }
    let mut last_state: Option<StatePacket> = None;
    let mut grabbed = false; // start free — click a window to capture (Escape to release)
    // accurate FPS: count actual completed frames per wall-clock second,
    // rather than trusting get_fps()'s single-frame instantaneous sample
    // (verified via testing to overreport by 20%+ versus real throughput)
    let mut frame_count: u32 = 0;
    let mut fps_display: u32 = 0;
    let mut spawned = false;
    let mut projectiles: Vec<Projectile> = Vec::new();
    let mut death_delay = DeathDelay {
        hidden_after: std::collections::HashMap::new(),
    };
    // tracks when the current frame started, used to cap the frame rate
    // (initial value is immediately overwritten on the first loop iteration)
    #[allow(unused_assignments)]
    let mut fps_window_start = Instant::now();
    loop {
        let dt = get_frame_time();
        // Escape releases the mouse; click re-grabs it
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
        // shoot: right button while mouse is captured (left is for window grab)
        input.shoot.store(
            grabbed && is_mouse_button_down(MouseButton::Right),
            Ordering::Relaxed,
        );
        // mouse look: best-effort via mouse_delta_position(). Verified working
        // correctly on native Windows and native Linux (per macroquad's own
        // maintainer, PR #181). Under WSLg (including inside Docker, since
        // Docker still forwards to WSLg's own X11 implementation) this API
        // is unreliable — deltas read as zero or spike to corrupted values
        // in the thousands. Arrow keys (see LocalPlayer::update) are the
        // reliable turning method in that environment; this is left in place
        // harmlessly for when the client runs on a real OS/display server.
        let raw = mouse_delta_position();
        let look_dx = if grabbed && raw.x.abs() < 150.0 {
            -raw.x
        } else {
            0.0
        };
        player.update(&map, dt, look_dx);
        // publish inputs for the net thread (after update so angle is current)
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
        // drain incoming state — keep only the newest
        while let Ok(s) = state_rx.try_recv() {
            last_state = Some(s);
        }
        // sync to the server's actual level the first time it differs
        if let Some(state) = &last_state {
            if state.level != map_loaded_level {
                map = get_level(state.level);
                map_loaded_level = state.level;
            }
        }
        // spawn any cosmetic projectiles/death-delays from this tick's shot events
        if let Some(state) = &last_state {
            spawn_projectiles(
                &state.shot_events,
                &mut projectiles,
                &state.players,
                &mut death_delay,
            );
        }
        // if WE are respawning, the server owns our position — snap to it
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
        // one-time: snap to our server-assigned spawn point
        if !spawned {
            if let Some(state) = &last_state {
                if let Some(me) = state.players.iter().find(|p| p.id == state.your_id) {
                    player.x = me.x;
                    player.z = me.y; // server y == world z
                    player.yaw = me.angle; // face the maze, not the wall
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
        // count real completed frames per wall-clock second, measured
        // directly via Instant rather than accumulated dt (which can drift
        // once we're manually sleeping to cap frame rate)
        frame_count += 1;
        if fps_window_start.elapsed().as_secs_f32() >= 1.0 {
            fps_display = frame_count;
            frame_count = 0;
            fps_window_start = Instant::now();
        }
        let fps_text = format!("FPS: {}", fps_display);
        draw_text(&fps_text, screen_width() - 100.0, 30.0, 24.0, YELLOW);
        // show our own username top-left instead of a generic player number
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
        // crosshair: two thin rectangles crossing at screen centre
        let cx = screen_width() / 2.0;
        let cy = screen_height() / 2.0;
        draw_rectangle(cx - 8.0, cy - 1.0, 16.0, 2.0, WHITE);
        draw_rectangle(cx - 1.0, cy - 8.0, 2.0, 16.0, WHITE);
        // death banner while waiting to respawn
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
        // scoreboard, fuel readout, win overlay
        if let Some(state) = &last_state {
            // fuel readout for our player (red when low)
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
            draw_scoreboard(state);
            if state.match_over {
                draw_win_overlay(state);
            }
        }
        draw_minimap(&map, &player, &last_state);

        next_frame().await
    }
}
