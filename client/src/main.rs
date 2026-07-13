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
}

impl NetInput {
    fn new() -> Self {
        Self {
            forward: AtomicBool::new(false),
            backward: AtomicBool::new(false),
            shoot: AtomicBool::new(false),
            angle_bits: AtomicU32::new(0f32.to_bits()),
        }
    }
}

fn net_thread(server_addr: String, input: Arc<NetInput>, state_tx: mpsc::SyncSender<StatePacket>) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind failed");
    socket.connect(&server_addr).expect("connect failed");
    socket
        .set_read_timeout(Some(Duration::from_millis(1)))
        .unwrap();
    println!("net thread connected to {server_addr}");

    let interval = Duration::from_millis(1000 / NET_HZ);
    let mut seq = 0u32;
    let mut buf = vec![0u8; MAX_PACKET_BYTES];

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
        };
        if let Ok(enc) = postcard::to_allocvec(&pkt) {
            let _ = socket.send(&enc);
        }

        if let Ok(len) = socket.recv(&mut buf) {
            if let Ok(state) = postcard::from_bytes::<StatePacket>(&buf[..len]) {
                let _ = state_tx.try_send(state);
            }
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

fn draw_players(state: &StatePacket) {
    for p in &state.players {
        if p.id == state.your_id {
            continue;
        }
        // server (x, y) is our world (x, z); server angle unused for now
        draw_cube(
            vec3(p.x, 0.4, p.y), // slightly lower than walls
            vec3(0.5, 0.8, 0.5), // slimmer than a wall cube, reads as a figure
            None,
            SKYBLUE,
        );
    }
}

#[macroquad::main("Maze Runner FPS")]
async fn main() {
    // load level once, outside the loop
    let map = get_level(1);
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
        thread::spawn(move || net_thread("127.0.0.1:34254".into(), input, state_tx));
    }
    let mut last_state: Option<StatePacket> = None;

    let mut grabbed = true;
    set_cursor_grab(true); // lock the mouse to the window
    show_mouse(false);

    let mut fps_display = 0;
    let mut fps_timer = 0.0f32;

    let mut spawned = false;

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

        // mouse look delta — mouse_delta_position() handles cursor-grab warping;
        // the spike filter discards implausible jumps (e.g. the initial grab warp)
        let raw = mouse_delta_position();
        let look_dx = if grabbed && raw.x.abs() < 0.2 {
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

        // drain incoming state — keep only the newest
        while let Ok(s) = state_rx.try_recv() {
            last_state = Some(s);
        }

        // one-time: snap to our server-assigned spawn point
        if !spawned {
            if let Some(state) = &last_state {
                if let Some(me) = state.players.iter().find(|p| p.id == state.your_id) {
                    player.x = me.x;
                    player.z = me.y; // server y == world z
                    spawned = true;
                }
            }
        }

        clear_background(BLACK);

        set_camera(&player.camera());
        draw_maze(&map);

        if let Some(state) = &last_state {
            draw_players(state);
        }

        set_default_camera();
        // FPS display: sample twice a second so the number doesn't flicker
        fps_timer += dt;
        if fps_timer >= 0.5 {
            fps_display = get_fps();
            fps_timer = 0.0;
        }
        let fps_text = format!("FPS: {}", fps_display);
        draw_text(&fps_text, screen_width() - 100.0, 30.0, 24.0, YELLOW);

        next_frame().await
    }
}
