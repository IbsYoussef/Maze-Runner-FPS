// client — entry point
// Responsibilities:
//   Main thread    — render loop (raycaster, mini-map, FPS counter)
//   Network thread — UDP send/receive, communicates with main via mpsc channel

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
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use shared::map::Map;
use shared::protocol::{InputPacket, MAX_PACKET_BYTES, StatePacket};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const NET_HZ: u64 = 60;
const FOV_PLANE: f32 = 0.66;

// must match server constants exactly for client-side prediction
const PLAYER_SPEED: f32 = 0.05;
const PLAYER_TURN_SPEED: f32 = 0.04;
const MOVE_TICK: Duration = Duration::from_millis(16);

// mini-map config
const MINI_CELL: usize = 4;
const MINI_X: usize = WIDTH as usize - 16 * MINI_CELL - 8;
const MINI_Y: usize = 8;

#[derive(Parser, Debug)]
#[command(name = "maze-runner")]
struct Args {
    /// Server address e.g. 127.0.0.1:34254
    #[arg(short, long, default_value = "127.0.0.1:34254")]
    server: String,

    /// Player username
    #[arg(short, long, default_value = "player")]
    username: String,
}

// atomic booleans so both the main thread and network thread can read input safely
struct InputFlags {
    forward: AtomicBool,
    backward: AtomicBool,
    turn_left: AtomicBool,
    turn_right: AtomicBool,
}

impl InputFlags {
    fn new() -> Self {
        Self {
            forward: AtomicBool::new(false),
            backward: AtomicBool::new(false),
            turn_left: AtomicBool::new(false),
            turn_right: AtomicBool::new(false),
        }
    }
}

struct Camera {
    x: f32,
    y: f32,
    dir_x: f32,
    dir_y: f32,
    plane_x: f32,
    plane_y: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            x: 1.5,
            y: 1.5,
            dir_x: 1.0,
            dir_y: 0.0,
            plane_x: 0.0,
            plane_y: FOV_PLANE,
        }
    }
}

impl Camera {
    fn set_angle(&mut self, a: f32) {
        self.dir_x = a.cos();
        self.dir_y = a.sin();
        self.plane_x = -a.sin() * FOV_PLANE;
        self.plane_y = a.cos() * FOV_PLANE;
    }
}

struct App {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    state_rx: mpsc::Receiver<StatePacket>,
    input: Arc<InputFlags>,
    last_state: Option<StatePacket>,
    args: Args,
    cam: Camera,
    map: Map,
    z_buf: Vec<f32>,
    local_id: Option<u32>,
    fps_timer: Instant,
    fps_count: u32,
    fps: f32,
    move_timer: Instant,
}

impl App {
    fn apply_movement(&mut self) {
        if self.input.turn_left.load(Ordering::Relaxed) {
            let a = self.cam.dir_y.atan2(self.cam.dir_x) - PLAYER_TURN_SPEED;
            self.cam.set_angle(a);
        }
        if self.input.turn_right.load(Ordering::Relaxed) {
            let a = self.cam.dir_y.atan2(self.cam.dir_x) + PLAYER_TURN_SPEED;
            self.cam.set_angle(a);
        }
        let mut nx = self.cam.x;
        let mut ny = self.cam.y;
        if self.input.forward.load(Ordering::Relaxed) {
            nx += self.cam.dir_x * PLAYER_SPEED;
            ny += self.cam.dir_y * PLAYER_SPEED;
        }
        if self.input.backward.load(Ordering::Relaxed) {
            nx -= self.cam.dir_x * PLAYER_SPEED;
            ny -= self.cam.dir_y * PLAYER_SPEED;
        }
        let ix = nx as i32;
        let iy = ny as i32;
        if ix >= 0 && iy >= 0 && !self.map.is_wall(ix as usize, iy as usize) {
            self.cam.x = nx;
            self.cam.y = ny;
        }
    }

    fn render(&mut self) {
        let pixels = match &mut self.pixels {
            Some(p) => p,
            None => return,
        };

        if let Some(state) = &self.last_state {
            if self.local_id.is_none() {
                self.local_id = Some(state.your_id);
            }
        }

        let frame = pixels.frame_mut();
        draw_background(frame);
        cast_walls(frame, &self.cam, &self.map, &mut self.z_buf);

        if let Some(state) = &self.last_state {
            let sprites: Vec<(f32, f32, [u8; 3])> = state
                .players
                .iter()
                .filter(|p| Some(p.id) != self.local_id)
                .map(|p| (p.x, p.y, [0x00u8, 0xffu8, 0xccu8]))
                .collect();
            draw_sprites(frame, &self.cam, &sprites, &self.z_buf);
            draw_minimap(frame, &self.map, &self.cam, state);
        }

        draw_fps(frame, self.fps);

        if let Err(e) = pixels.render() {
            eprintln!("render error: {e}");
        }
    }
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
                .expect("window failed"),
        );
        let surface = SurfaceTexture::new(WIDTH, HEIGHT, Arc::clone(&window));
        let pixels = Pixels::new(WIDTH, HEIGHT, surface).expect("pixels failed");
        window.request_redraw();
        self.window = Some(window);
        self.pixels = Some(pixels);
        println!(
            "Connected — user: '{}' | server: {}",
            self.args.username, self.args.server
        );
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                match code {
                    KeyCode::KeyW | KeyCode::ArrowUp => {
                        self.input.forward.store(pressed, Ordering::Relaxed)
                    }
                    KeyCode::KeyS | KeyCode::ArrowDown => {
                        self.input.backward.store(pressed, Ordering::Relaxed)
                    }
                    KeyCode::KeyA | KeyCode::ArrowLeft => {
                        self.input.turn_left.store(pressed, Ordering::Relaxed)
                    }
                    KeyCode::KeyD | KeyCode::ArrowRight => {
                        self.input.turn_right.store(pressed, Ordering::Relaxed)
                    }
                    KeyCode::Escape => event_loop.exit(),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                // drain incoming state packets
                while let Ok(s) = self.state_rx.try_recv() {
                    self.last_state = Some(s);
                }

                // client-side prediction at server tick rate
                while self.move_timer.elapsed() >= MOVE_TICK {
                    self.apply_movement();
                    self.move_timer += MOVE_TICK;
                }

                // FPS counter
                self.fps_count += 1;
                let elapsed = self.fps_timer.elapsed();
                if elapsed.as_secs_f32() >= 0.5 {
                    self.fps = self.fps_count as f32 / elapsed.as_secs_f32();
                    self.fps_count = 0;
                    self.fps_timer = Instant::now();
                }

                self.render();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn draw_background(frame: &mut [u8]) {
    let mid = (HEIGHT / 2) as usize;
    for (i, px) in frame.chunks_exact_mut(4).enumerate() {
        let row = i / WIDTH as usize;
        if row < mid {
            // dark ceiling
            px[0] = 0x1a;
            px[1] = 0x1a;
            px[2] = 0x2e;
        } else {
            // dark floor
            px[0] = 0x10;
            px[1] = 0x10;
            px[2] = 0x10;
        }
        px[3] = 0xff;
    }
}

fn cast_walls(frame: &mut [u8], cam: &Camera, map: &Map, z_buf: &mut [f32]) {
    let mid = HEIGHT as i32 / 2;

    for x in 0..WIDTH as usize {
        let cam_x = 2.0 * x as f32 / WIDTH as f32 - 1.0;
        let ray_dx = cam.dir_x + cam.plane_x * cam_x;
        let ray_dy = cam.dir_y + cam.plane_y * cam_x;

        let mut map_x = cam.x as i32;
        let mut map_y = cam.y as i32;

        let ddx = if ray_dx == 0.0 {
            f32::INFINITY
        } else {
            (1.0 / ray_dx).abs()
        };
        let ddy = if ray_dy == 0.0 {
            f32::INFINITY
        } else {
            (1.0 / ray_dy).abs()
        };

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

        let mut side = 0u8;
        for _ in 0..64 {
            if sdx < sdy {
                sdx += ddx;
                map_x += step_x;
                side = 0;
            } else {
                sdy += ddy;
                map_y += step_y;
                side = 1;
            }
            if map_x < 0 || map_y < 0 {
                break;
            }
            if map.is_wall(map_x as usize, map_y as usize) {
                break;
            }
        }

        let perp = (if side == 0 { sdx - ddx } else { sdy - ddy }).max(0.001);
        z_buf[x] = perp;

        let line_h = (HEIGHT as f32 / perp) as i32;
        let draw_top = (mid - line_h / 2).max(0) as usize;
        let draw_bot = (mid + line_h / 2).min(HEIGHT as i32 - 1) as usize;
        let bright = (1.5 / perp).clamp(0.15, 1.0);

        for row in draw_top..=draw_bot {
            let idx = (row * WIDTH as usize + x) * 4;
            if side == 0 {
                let b = (0xcc as f32 * bright) as u8;
                frame[idx] = b;
                frame[idx + 1] = b;
                frame[idx + 2] = b;
            } else {
                let b = (0x88 as f32 * bright) as u8;
                frame[idx] = b;
                frame[idx + 1] = b;
                frame[idx + 2] = b;
            }
            frame[idx + 3] = 0xff;
        }
    }
}

fn draw_sprites(frame: &mut [u8], cam: &Camera, sprites: &[(f32, f32, [u8; 3])], z_buf: &[f32]) {
    let mid = HEIGHT as i32 / 2;
    let inv_det = 1.0 / (cam.plane_x * cam.dir_y - cam.dir_x * cam.plane_y);

    let mut order: Vec<usize> = (0..sprites.len()).collect();
    order.sort_by(|&a, &b| {
        let da = (sprites[a].0 - cam.x).powi(2) + (sprites[a].1 - cam.y).powi(2);
        let db = (sprites[b].0 - cam.x).powi(2) + (sprites[b].1 - cam.y).powi(2);
        db.partial_cmp(&da).unwrap()
    });

    for i in order {
        let (sx, sy, color) = sprites[i];
        let dx = sx - cam.x;
        let dy = sy - cam.y;
        let tx = inv_det * (cam.dir_y * dx - cam.dir_x * dy);
        let ty = inv_det * (-cam.plane_y * dx + cam.plane_x * dy);
        if ty <= 0.05 {
            continue;
        }

        let screen_cx = ((WIDTH as f32 / 2.0) * (1.0 + tx / ty)) as i32;
        let h = ((HEIGHT as f32 / ty) as i32).max(1);

        let top = (mid - h / 2).max(0) as usize;
        let bot = (mid / 2).min(HEIGHT as i32 - 1) as usize;
        let left = (screen_cx - h / 2).max(0) as usize;
        let right = (screen_cx + h / 2).min(WIDTH as i32 - 1) as usize;

        let bright = (1.5 / ty).clamp(0.1, 1.0);

        for col in left..=right {
            if z_buf[col] <= ty {
                continue;
            }
            for row in top..=bot {
                let idx = (row * WIDTH as usize + col) * 4;
                frame[idx] = (color[0] as f32 * bright) as u8;
                frame[idx + 1] = (color[1] as f32 * bright) as u8;
                frame[idx + 2] = (color[2] as f32 * bright) as u8;
                frame[idx + 3] = 0xff;
            }
        }
    }
}

fn draw_minimap(frame: &mut [u8], map: &Map, cam: &Camera, state: &StatePacket) {
    // draw map tiles
    for my in 0..map.height {
        for mx in 0..map.width {
            let (r, g, b) = if map.is_wall(mx, my) {
                (0x55u8, 0x55u8, 0x55u8)
            } else {
                (0x1au8, 0x1au8, 0x1au8)
            };
            let px = MINI_X + mx * MINI_CELL;
            let py = MINI_Y + my * MINI_CELL;
            for dy in 0..MINI_CELL {
                for dx in 0..MINI_CELL {
                    let idx = ((py + dy) * WIDTH as usize + px + dx) * 4;
                    if idx + 3 < frame.len() {
                        frame[idx] = r;
                        frame[idx + 1] = g;
                        frame[idx + 2] = b;
                        frame[idx + 3] = 0xff;
                    }
                }
            }
        }
    }

    // draw players
    for p in &state.players {
        let px = MINI_X + p.x as usize * MINI_CELL;
        let py = MINI_Y + p.y as usize * MINI_CELL;
        let is_local = p.id == state.your_id;
        let (r, g, b) = if is_local {
            (0x00u8, 0xffu8, 0xffu8)
        } else {
            (0xffu8, 0x44u8, 0x44u8)
        };
        for dy in 0..MINI_CELL {
            for dx in 0..MINI_CELL {
                let idx = ((py + dy) * WIDTH as usize + px + dx) * 4;
                if idx + 3 < frame.len() {
                    frame[idx] = r;
                    frame[idx + 1] = g;
                    frame[idx + 2] = b;
                    frame[idx + 3] = 0xff;
                }
            }
        }
    }

    // draw facing arrow for local player
    for t in 1..=5i32 {
        let fx = (MINI_X as f32 + cam.x * MINI_CELL as f32 + cam.dir_x * t as f32) as usize;
        let fy = (MINI_Y as f32 + cam.y * MINI_CELL as f32 + cam.dir_y * t as f32) as usize;
        if fx < WIDTH as usize && fy < HEIGHT as usize {
            let idx = (fy * WIDTH as usize + fx) * 4;
            frame[idx] = 0xff;
            frame[idx + 1] = 0xff;
            frame[idx + 2] = 0xff;
            frame[idx + 3] = 0xff;
        }
    }
}

const DIGITS_3X5: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

const SC: usize = 2;
const DW: usize = 3 * SC;
const DH: usize = 5 * SC;
const GAP: usize = 2;
const PAD: usize = 4;

fn draw_fps(frame: &mut [u8], fps: f32) {
    let n = fps as u32;
    let digits: Vec<usize> = if n >= 100 {
        vec![
            (n / 100 % 10) as usize,
            (n / 10 % 10) as usize,
            (n % 10) as usize,
        ]
    } else if n >= 10 {
        vec![(n / 10 % 10) as usize, (n % 10) as usize]
    } else {
        vec![(n % 10) as usize]
    };

    let bg_w = digits.len() * DW + (digits.len() - 1) * GAP + PAD * 2;
    let bg_h = DH + PAD * 2;

    // black background
    for row in 0..bg_h {
        for col in 0..bg_w {
            let idx = (row * WIDTH as usize + col) * 4;
            if idx + 3 < frame.len() {
                frame[idx] = 0;
                frame[idx + 1] = 0;
                frame[idx + 2] = 0;
                frame[idx + 3] = 0xff;
            }
        }
    }

    // yellow digits
    for (di, &d) in digits.iter().enumerate() {
        let ox = PAD + di * (DW + GAP);
        let oy = PAD;
        for (r, &bits) in DIGITS_3X5[d].iter().enumerate() {
            for c in 0..3usize {
                if bits & (1 << (2 - c)) != 0 {
                    for sy in 0..SC {
                        for sx in 0..SC {
                            let idx = ((oy + r * SC + sy) * WIDTH as usize + ox + c * SC + sx) * 4;
                            if idx + 3 < frame.len() {
                                frame[idx] = 0xff;
                                frame[idx + 1] = 0xff;
                                frame[idx + 2] = 0x00;
                                frame[idx + 3] = 0xff;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn net_thread(
    server_addr: String,
    input: Arc<InputFlags>,
    state_tx: mpsc::SyncSender<StatePacket>,
) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind failed");
    socket.connect(&server_addr).expect("connect failed");
    socket
        .set_read_timeout(Some(Duration::from_millis(1)))
        .unwrap();
    println!("Network thread connected to {}", server_addr);

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
            turn_left: input.turn_left.load(Ordering::Relaxed),
            turn_right: input.turn_right.load(Ordering::Relaxed),
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

fn main() {
    let args = Args::parse();
    let input = Arc::new(InputFlags::new());
    let (state_tx, state_rx) = mpsc::sync_channel::<StatePacket>(1);

    // spawn network thread
    {
        let server = args.server.clone();
        let input = Arc::clone(&input);
        thread::spawn(move || net_thread(server, input, state_tx));
    }

    let event_loop = EventLoop::new().expect("event loop failed");
    let mut app = App {
        window: None,
        pixels: None,
        state_rx,
        input,
        last_state: None,
        args,
        cam: Camera::default(),
        map: shared::map::get_level(1),
        z_buf: vec![0.0f32; WIDTH as usize],
        local_id: None,
        fps_timer: Instant::now(),
        fps_count: 0,
        fps: 0.0,
        move_timer: Instant::now(),
    };
    event_loop.run_app(&mut app).expect("event loop error");
}
