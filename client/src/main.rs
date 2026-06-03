// client — entry point
// Main thread    — winit event loop + raycaster render
// Network thread — UDP send/receive via mpsc channel

use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use shared::map::Map;
use shared::protocol::{InputPacket, StatePacket, MAX_PACKET_BYTES};

const WIDTH: u32  = 640;
const HEIGHT: u32 = 480;
const NET_HZ: u64 = 60;
const FOV_PLANE: f32 = 0.66; // half-width of camera plane → ~66° horizontal FOV

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "maze-runner")]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1:34254")]
    server: String,

    #[arg(short, long, default_value = "player")]
    username: String,
}

// ── Input flags shared between event loop and network thread ──────────────────

struct InputFlags {
    forward:    AtomicBool,
    backward:   AtomicBool,
    turn_left:  AtomicBool,
    turn_right: AtomicBool,
}

impl InputFlags {
    fn new() -> Self {
        Self {
            forward:    AtomicBool::new(false),
            backward:   AtomicBool::new(false),
            turn_left:  AtomicBool::new(false),
            turn_right: AtomicBool::new(false),
        }
    }
}

// ── Camera ────────────────────────────────────────────────────────────────────

struct Camera {
    x: f32, y: f32,
    dir_x: f32, dir_y: f32,
    plane_x: f32, plane_y: f32,
}

impl Default for Camera {
    fn default() -> Self {
        // spawn matches server default: (1.5, 1.5), angle 0 = facing +x
        Self { x: 1.5, y: 1.5, dir_x: 1.0, dir_y: 0.0, plane_x: 0.0, plane_y: FOV_PLANE }
    }
}

impl Camera {
    fn set_angle(&mut self, angle: f32) {
        self.dir_x   =  angle.cos();
        self.dir_y   =  angle.sin();
        self.plane_x = -angle.sin() * FOV_PLANE;
        self.plane_y =  angle.cos() * FOV_PLANE;
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

struct App {
    window:     Option<Arc<Window>>,
    pixels:     Option<Pixels<'static>>,
    state_rx:   mpsc::Receiver<StatePacket>,
    input:      Arc<InputFlags>,
    last_state: Option<StatePacket>,
    args:       Args,
    cam:        Camera,
    map:        Map,
    z_buf:      Vec<f32>, // per-column wall distance — used by sprite pass on Day 3
    local_id:   Option<u32>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Maze Runner FPS")
                        .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
                        .with_resizable(false),
                )
                .expect("failed to create window"),
        );
        let surface = SurfaceTexture::new(WIDTH, HEIGHT, Arc::clone(&window));
        let pixels  = Pixels::new(WIDTH, HEIGHT, surface).expect("pixels init failed");
        window.request_redraw();
        self.window = Some(window);
        self.pixels = Some(pixels);
        println!("Maze Runner FPS — user: '{}' | server: {}", self.args.username, self.args.server);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput {
                event: KeyEvent { physical_key: PhysicalKey::Code(code), state, .. }, ..
            } => {
                let pressed = state == ElementState::Pressed;
                match code {
                    KeyCode::KeyW | KeyCode::ArrowUp    => self.input.forward.store(pressed, Ordering::Relaxed),
                    KeyCode::KeyS | KeyCode::ArrowDown  => self.input.backward.store(pressed, Ordering::Relaxed),
                    KeyCode::KeyA | KeyCode::ArrowLeft  => self.input.turn_left.store(pressed, Ordering::Relaxed),
                    KeyCode::KeyD | KeyCode::ArrowRight => self.input.turn_right.store(pressed, Ordering::Relaxed),
                    KeyCode::Escape => event_loop.exit(),
                    _ => {}
                }
            }

            WindowEvent::RedrawRequested => {
                while let Ok(state) = self.state_rx.try_recv() {
                    self.last_state = Some(state);
                }
                self.render();
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            _ => {}
        }
    }
}

// ── Render helpers ────────────────────────────────────────────────────────────

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).clamp(0.0, 255.0) as u8
}

fn star(row: usize, col: usize) -> bool {
    (row.wrapping_mul(2654435761) ^ col.wrapping_mul(2246822519)) % 300 == 0
}

// Pass 1 — synthwave sky + perspective-grid floor
fn draw_background(frame: &mut [u8]) {
    let mid    = (HEIGHT / 2) as usize;
    let half_h = HEIGHT as f32 / 2.0;

    for (i, px) in frame.chunks_exact_mut(4).enumerate() {
        let row = i / WIDTH as usize;
        let col = i % WIDTH as usize;

        if row < mid {
            let t = row as f32 / mid as f32;
            if t < 0.7 && star(row, col) {
                let b = lerp(0xaa, 0xff, t);
                px[0] = b; px[1] = b; px[2] = b;
            } else {
                px[0] = lerp(0x02, 0x2a, t);
                px[1] = 0x00;
                px[2] = lerp(0x10, 0x40, t);
            }
        } else if row == mid {
            px[0] = 0xff; px[1] = 0x10; px[2] = 0xc8; // hot-pink horizon
        } else {
            let t          = (row - mid) as f32 / mid as f32;
            let floor_dist = half_h / (row as f32 - half_h).max(0.5);
            let world_x    = (col as f32 / WIDTH as f32 - 0.5) * 2.0 * floor_dist;
            let world_y    = floor_dist;
            let pw         = 2.0 * floor_dist / WIDTH as f32;
            let lw         = pw * 1.5;
            let on_grid    = world_x.fract().abs() < lw
                || (1.0 - world_x.fract().abs()) < lw
                || world_y.fract().abs() < lw
                || (1.0 - world_y.fract().abs()) < lw;

            if on_grid {
                let bright = (1.0 - t * 0.85).max(0.05);
                px[0] = 0x00;
                px[1] = (0xff as f32 * bright) as u8;
                px[2] = (0xff as f32 * bright) as u8;
            } else {
                px[0] = lerp(0x0d, 0x04, t);
                px[1] = 0x00;
                px[2] = lerp(0x1a, 0x06, t);
            }
        }
        px[3] = 0xff;
    }
}

// Pass 2 — DDA raycaster, one ray per column
fn cast_walls(frame: &mut [u8], cam: &Camera, map: &Map, z_buf: &mut [f32]) {
    let mid = HEIGHT as i32 / 2;

    for x in 0..WIDTH as usize {
        // ray direction for this column
        let cam_x   = 2.0 * x as f32 / WIDTH as f32 - 1.0; // -1 (left) to 1 (right)
        let ray_dx  = cam.dir_x + cam.plane_x * cam_x;
        let ray_dy  = cam.dir_y + cam.plane_y * cam_x;

        let mut map_x = cam.x as i32;
        let mut map_y = cam.y as i32;

        // how far to travel along the ray to cross one cell boundary
        let ddx = if ray_dx == 0.0 { f32::INFINITY } else { (1.0 / ray_dx).abs() };
        let ddy = if ray_dy == 0.0 { f32::INFINITY } else { (1.0 / ray_dy).abs() };

        // step direction and initial fractional distance to first boundary
        let (step_x, mut sdx) = if ray_dx < 0.0 {
            (-1i32, (cam.x - map_x as f32) * ddx)
        } else {
            (1i32, (map_x as f32 + 1.0 - cam.x) * ddx)
        };
        let (step_y, mut sdy) = if ray_dy < 0.0 {
            (-1i32, (cam.y - map_y as f32) * ddy)
        } else {
            (1i32, (map_y as f32 + 1.0 - cam.y) * ddy)
        };

        // DDA — step until wall hit (max 32 iterations for 16×16 map)
        let mut side = 0u8; // 0 = hit vertical cell boundary (N/S wall face)
                            // 1 = hit horizontal cell boundary (E/W wall face)
        for _ in 0..32 {
            if sdx < sdy { sdx += ddx; map_x += step_x; side = 0; }
            else         { sdy += ddy; map_y += step_y; side = 1; }
            if map_x < 0 || map_y < 0 { break; }
            if map.is_wall(map_x as usize, map_y as usize) { break; }
        }

        // perpendicular wall distance — avoids fisheye distortion
        let perp = (if side == 0 { sdx - ddx } else { sdy - ddy }).max(0.001);
        z_buf[x] = perp;

        // wall strip height on screen
        let line_h    = (HEIGHT as f32 / perp) as i32;
        let draw_top  = (mid - line_h / 2).max(0) as usize;
        let draw_bot  = (mid + line_h / 2).min(HEIGHT as i32 - 1) as usize;

        // brightness falls off with distance — keeps close walls punchy
        let bright = (1.5 / perp).clamp(0.15, 1.0);

        for row in draw_top..=draw_bot {
            let idx = (row * WIDTH as usize + x) * 4;
            if side == 0 {
                // N/S face — neon cyan
                frame[idx]     = 0x00;
                frame[idx + 1] = (0xff as f32 * bright) as u8;
                frame[idx + 2] = (0xdd as f32 * bright) as u8;
            } else {
                // E/W face — neon magenta, slightly dimmer for depth
                let b = bright * 0.65;
                frame[idx]     = (0xcc as f32 * b) as u8;
                frame[idx + 1] = 0x00;
                frame[idx + 2] = (0xff as f32 * b) as u8;
            }
            frame[idx + 3] = 0xff;
        }
    }
}

impl App {
    fn render(&mut self) {
        let pixels = match &mut self.pixels {
            Some(p) => p,
            None => return,
        };

        // sync camera to authoritative server position
        if let Some(state) = &self.last_state {
            if self.local_id.is_none() {
                self.local_id = Some(state.your_id);
            }
            if let Some(id) = self.local_id {
                if let Some(p) = state.players.iter().find(|p| p.id == id) {
                    self.cam.x = p.x;
                    self.cam.y = p.y;
                    self.cam.set_angle(p.angle);
                }
            }
        }

        let frame = pixels.frame_mut();
        draw_background(frame);
        cast_walls(frame, &self.cam, &self.map, &mut self.z_buf);

        if let Err(e) = pixels.render() {
            eprintln!("render error: {e}");
        }
    }
}

// ── Network thread ────────────────────────────────────────────────────────────

fn net_thread(server_addr: String, input: Arc<InputFlags>, state_tx: mpsc::SyncSender<StatePacket>) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("client: bind failed");
    socket.connect(&server_addr).expect("client: connect failed");
    socket.set_read_timeout(Some(Duration::from_millis(1))).expect("set_read_timeout failed");
    println!("network thread connected to {}", server_addr);

    let interval  = Duration::from_millis(1000 / NET_HZ);
    let mut seq   = 0u32;
    let mut buf   = vec![0u8; MAX_PACKET_BYTES];

    loop {
        let t0 = Instant::now();
        seq += 1;

        let pkt = InputPacket {
            sequence:    seq,
            player_id:   0,
            session_token: 0,
            forward:    input.forward.load(Ordering::Relaxed),
            backward:   input.backward.load(Ordering::Relaxed),
            turn_left:  input.turn_left.load(Ordering::Relaxed),
            turn_right: input.turn_right.load(Ordering::Relaxed),
        };
        if let Ok(enc) = postcard::to_allocvec(&pkt) { let _ = socket.send(&enc); }

        if let Ok(len) = socket.recv(&mut buf) {
            if let Ok(state) = postcard::from_bytes::<StatePacket>(&buf[..len]) {
                let _ = state_tx.try_send(state);
            }
        }

        let elapsed = t0.elapsed();
        if elapsed < interval { thread::sleep(interval - elapsed); }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    let input = Arc::new(InputFlags::new());
    let (state_tx, state_rx) = mpsc::sync_channel::<StatePacket>(1);

    {
        let server = args.server.clone();
        let input  = Arc::clone(&input);
        thread::spawn(move || net_thread(server, input, state_tx));
    }

    let event_loop = EventLoop::new().expect("event loop failed");
    let mut app = App {
        window:     None,
        pixels:     None,
        state_rx,
        input,
        last_state: None,
        args,
        cam:      Camera::default(),
        map:      shared::map::get_level(1),
        z_buf:    vec![0.0f32; WIDTH as usize],
        local_id: None,
    };

    event_loop.run_app(&mut app).expect("event loop error");
}
