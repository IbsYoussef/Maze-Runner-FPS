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
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use shared::map::Map;
use shared::protocol::{InputPacket, StatePacket, MAX_PACKET_BYTES};

const WIDTH: u32  = 640;
const HEIGHT: u32 = 480;
const NET_HZ: u64 = 60;
const FOV_PLANE: f32 = 0.66;

// Must match server constants exactly so client-side prediction stays in sync
const PLAYER_SPEED: f32      = 0.05;
const PLAYER_TURN_SPEED: f32 = 0.04;
const MOVE_TICK: Duration    = Duration::from_millis(16);

const MINI_CELL: usize = 4;               // px per map tile on the mini-map
const MINI_X: usize = WIDTH as usize - 16 * MINI_CELL - 8;  // top-right, 8px margin
const MINI_Y: usize = 8;

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "maze-runner")]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1:34254")]
    server: String,
    #[arg(short, long, default_value = "player")]
    username: String,
}

// ── Input flags ───────────────────────────────────────────────────────────────

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

struct Camera { x: f32, y: f32, dir_x: f32, dir_y: f32, plane_x: f32, plane_y: f32 }

impl Default for Camera {
    fn default() -> Self {
        Self { x: 1.5, y: 1.5, dir_x: 1.0, dir_y: 0.0, plane_x: 0.0, plane_y: FOV_PLANE }
    }
}
impl Camera {
    fn set_angle(&mut self, a: f32) {
        self.dir_x = a.cos(); self.dir_y = a.sin();
        self.plane_x = -a.sin() * FOV_PLANE; self.plane_y = a.cos() * FOV_PLANE;
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

struct App {
    window:        Option<Arc<Window>>,
    pixels:        Option<Pixels<'static>>,
    state_rx:      mpsc::Receiver<StatePacket>,
    input:         Arc<InputFlags>,
    last_state:    Option<StatePacket>,
    args:          Args,
    cam:           Camera,
    map:           Map,
    z_buf:         Vec<f32>,
    local_id:      Option<u32>,
    fps_timer:     Instant,
    fps_count:     u32,
    fps:           f32,
    rescue_flash:  Option<Instant>,
    miner_rescued: bool,
    move_timer:    Instant, // drives client-side prediction at server tick rate
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Maze Runner FPS")
                    .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
                    .with_resizable(false),
            ).expect("window failed"),
        );
        let surface = SurfaceTexture::new(WIDTH, HEIGHT, Arc::clone(&window));
        let pixels  = Pixels::new(WIDTH, HEIGHT, surface).expect("pixels failed");
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
                let p = state == ElementState::Pressed;
                match code {
                    KeyCode::KeyW | KeyCode::ArrowUp    => self.input.forward.store(p, Ordering::Relaxed),
                    KeyCode::KeyS | KeyCode::ArrowDown  => self.input.backward.store(p, Ordering::Relaxed),
                    KeyCode::KeyA | KeyCode::ArrowLeft  => self.input.turn_left.store(p, Ordering::Relaxed),
                    KeyCode::KeyD | KeyCode::ArrowRight => self.input.turn_right.store(p, Ordering::Relaxed),
                    KeyCode::Escape => event_loop.exit(),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                while let Ok(s) = self.state_rx.try_recv() { self.last_state = Some(s); }

                // client-side prediction: apply movement at server tick rate
                // using += keeps us in sync even when frames take longer than 16ms
                while self.move_timer.elapsed() >= MOVE_TICK {
                    self.apply_movement();
                    self.move_timer += MOVE_TICK;
                }

                self.fps_count += 1;
                let e = self.fps_timer.elapsed();
                if e.as_secs_f32() >= 0.5 {
                    self.fps = self.fps_count as f32 / e.as_secs_f32();
                    self.fps_count = 0;
                    self.fps_timer = Instant::now();
                }
                self.render();
                if let Some(w) = &self.window { w.request_redraw(); }
            }
            _ => {}
        }
    }

    // poll mode: process events and redraw as fast as possible — eliminates key lag
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Some(w) = &self.window { w.request_redraw(); }
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
            px[0] = 0xff; px[1] = 0x10; px[2] = 0xc8;
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

// Pass 2 — DDA raycaster
fn cast_walls(frame: &mut [u8], cam: &Camera, map: &Map, z_buf: &mut [f32]) {
    let mid = HEIGHT as i32 / 2;

    for x in 0..WIDTH as usize {
        let cam_x  = 2.0 * x as f32 / WIDTH as f32 - 1.0;
        let ray_dx = cam.dir_x + cam.plane_x * cam_x;
        let ray_dy = cam.dir_y + cam.plane_y * cam_x;

        let mut map_x = cam.x as i32;
        let mut map_y = cam.y as i32;

        let ddx = if ray_dx == 0.0 { f32::INFINITY } else { (1.0 / ray_dx).abs() };
        let ddy = if ray_dy == 0.0 { f32::INFINITY } else { (1.0 / ray_dy).abs() };

        let (step_x, mut sdx) = if ray_dx < 0.0 { (-1i32, (cam.x - map_x as f32) * ddx) }
                                else             { (1i32,  (map_x as f32 + 1.0 - cam.x) * ddx) };
        let (step_y, mut sdy) = if ray_dy < 0.0 { (-1i32, (cam.y - map_y as f32) * ddy) }
                                else             { (1i32,  (map_y as f32 + 1.0 - cam.y) * ddy) };

        let mut side = 0u8;
        for _ in 0..32 {
            if sdx < sdy { sdx += ddx; map_x += step_x; side = 0; }
            else         { sdy += ddy; map_y += step_y; side = 1; }
            if map_x < 0 || map_y < 0 { break; }
            if map.is_wall(map_x as usize, map_y as usize) { break; }
        }

        let perp = (if side == 0 { sdx - ddx } else { sdy - ddy }).max(0.001);
        z_buf[x] = perp;

        // wall texture coordinate — used for column-stripe pattern
        let wall_x = {
            let wx = if side == 0 { cam.y + perp * ray_dy } else { cam.x + perp * ray_dx };
            wx - wx.floor()
        };
        let tex_col = (wall_x * 32.0) as usize % 32;

        let line_h   = (HEIGHT as f32 / perp) as i32;
        let draw_top = (mid - line_h / 2).max(0) as usize;
        let draw_bot = (mid + line_h / 2).min(HEIGHT as i32 - 1) as usize;
        let bright   = (1.5 / perp).clamp(0.15, 1.0);

        for row in draw_top..=draw_bot {
            // column-stripe texture: dark vertical lines every 8 units = concrete block look
            let is_mortar = tex_col % 8 < 1;
            let b = if is_mortar { bright * 0.35 } else { bright };

            let idx = (row * WIDTH as usize + x) * 4;
            if side == 0 {
                frame[idx] = 0x00; frame[idx+1] = (0xff as f32 * b) as u8; frame[idx+2] = (0xdd as f32 * b) as u8;
            } else {
                let b2 = b * 0.65;
                frame[idx] = (0xcc as f32 * b2) as u8; frame[idx+1] = 0x00; frame[idx+2] = (0xff as f32 * b2) as u8;
            }
            frame[idx+3] = 0xff;
        }
    }
}

// ── Miner pixel-art sprite (8 wide × 16 tall) ────────────────────────────────
// 0=transparent  1=yellow hard hat  2=skin  3=orange suit  4=dark belt
const MINER_PIX: [[u8; 8]; 16] = [
    [0,0,1,1,1,1,0,0], // hard hat crown
    [0,1,1,1,1,1,1,0], // hard hat brim
    [0,0,2,2,2,2,0,0], // face
    [0,0,2,2,2,2,0,0], // face
    [0,3,3,3,3,3,3,0], // shoulders
    [0,3,3,3,3,3,3,0], // chest
    [0,3,3,3,3,3,3,0], // chest
    [4,4,4,4,4,4,4,4], // belt
    [0,3,3,3,3,3,3,0], // hips
    [0,3,3,0,0,3,3,0], // upper legs
    [0,3,3,0,0,3,3,0],
    [0,3,3,0,0,3,3,0], // lower legs
    [0,3,3,0,0,3,3,0],
    [0,3,3,0,0,3,3,0],
    [3,3,0,0,0,0,3,3], // feet
    [3,3,0,0,0,0,3,3],
];
const MINER_COL: [[u8; 3]; 5] = [
    [0,0,0],            // 0 transparent
    [0xff,0xff,0x00],   // 1 yellow hat
    [0xff,0xcc,0x88],   // 2 skin
    [0xff,0x88,0x00],   // 3 orange suit
    [0x88,0x44,0x00],   // 4 dark belt
];

// Pass 3 — billboard sprite rendering
// sprites: (world_x, world_y, [r,g,b], is_miner)
fn draw_sprites(frame: &mut [u8], cam: &Camera, sprites: &[(f32, f32, [u8; 3], bool)], z_buf: &[f32]) {
    let mid     = HEIGHT as i32 / 2;
    let inv_det = 1.0 / (cam.plane_x * cam.dir_y - cam.dir_x * cam.plane_y);

    let mut order: Vec<usize> = (0..sprites.len()).collect();
    order.sort_by(|&a, &b| {
        let da = (sprites[a].0 - cam.x).powi(2) + (sprites[a].1 - cam.y).powi(2);
        let db = (sprites[b].0 - cam.x).powi(2) + (sprites[b].1 - cam.y).powi(2);
        db.partial_cmp(&da).unwrap()
    });

    for i in order {
        let (sx, sy, color, is_miner) = sprites[i];
        let dx = sx - cam.x;
        let dy = sy - cam.y;

        let tx = inv_det * (cam.dir_y * dx - cam.dir_x * dy);
        let ty = inv_det * (-cam.plane_y * dx + cam.plane_x * dy);
        if ty <= 0.05 { continue; }

        let screen_cx = ((WIDTH as f32 / 2.0) * (1.0 + tx / ty)) as i32;
        let h = ((HEIGHT as f32 / ty) as i32).max(1);
        let w = h;

        let top_i   = (mid - h / 2).max(0);
        let bot_i   = (mid + h / 2).min(HEIGHT as i32 - 1);
        let left_i  = (screen_cx - w / 2).max(0);
        let right_i = (screen_cx + w / 2).min(WIDTH as i32 - 1);

        if right_i < 0 || left_i >= WIDTH as i32 || left_i > right_i { continue; }

        let top   = top_i  as usize;
        let bot   = bot_i  as usize;
        let left  = left_i  as usize;
        let right = right_i as usize;
        let bright = (1.5 / ty).clamp(0.1, 1.0);

        // unclamped edges needed for correct texture coordinate mapping
        let unclamped_left = screen_cx - w / 2;
        let unclamped_top  = mid - h / 2;

        for col in left..=right {
            if z_buf[col] <= ty { continue; }

            let tex_x = ((col as i32 - unclamped_left) * 8 / w.max(1)).clamp(0, 7) as usize;

            for row in top..=bot {
                let idx = (row * WIDTH as usize + col) * 4;

                if is_miner {
                    let tex_y = ((row as i32 - unclamped_top) * 16 / h.max(1)).clamp(0, 15) as usize;
                    let texel = MINER_PIX[tex_y][tex_x] as usize;
                    if texel == 0 { continue; }
                    let c = MINER_COL[texel];
                    frame[idx]   = (c[0] as f32 * bright) as u8;
                    frame[idx+1] = (c[1] as f32 * bright) as u8;
                    frame[idx+2] = (c[2] as f32 * bright) as u8;
                } else {
                    // player: solid with bright edge outline
                    let is_edge = col == left || col == right
                               || row == top  || row == bot;
                    let b = if is_edge { (bright * 1.4).min(1.0) } else { bright * 0.8 };
                    frame[idx]   = (color[0] as f32 * b) as u8;
                    frame[idx+1] = (color[1] as f32 * b) as u8;
                    frame[idx+2] = (color[2] as f32 * b) as u8;
                }
                frame[idx+3] = 0xff;
            }
        }
    }
}

// Pass 4 — mini-map (top-right corner)
fn draw_minimap(frame: &mut [u8], map: &Map, cam: &Camera, state: &StatePacket) {
    // tiles
    for my in 0..map.height {
        for mx in 0..map.width {
            let (r, g, b) = if map.is_wall(mx, my) { (0x44, 0x00, 0x55) } else { (0x0a, 0x00, 0x12) };
            let px = MINI_X + mx * MINI_CELL;
            let py = MINI_Y + my * MINI_CELL;
            for dy in 0..MINI_CELL { for dx in 0..MINI_CELL {
                let idx = ((py + dy) * WIDTH as usize + px + dx) * 4;
                frame[idx] = r; frame[idx+1] = g; frame[idx+2] = b; frame[idx+3] = 0xff;
            }}
        }
    }

    // miner dot — 4×4 gold with bright centre
    if !state.miner_rescued {
        let (mx, my) = map.miner_pos;
        let px = MINI_X + mx as usize * MINI_CELL;
        let py = MINI_Y + my as usize * MINI_CELL;
        for dy in 0..4 { for dx in 0..4 {
            if px + dx < WIDTH as usize && py + dy < HEIGHT as usize {
                let centre = dx >= 1 && dx <= 2 && dy >= 1 && dy <= 2;
                let (r,g,b) = if centre { (0xff,0xff,0x44) } else { (0xff,0xcc,0x00) };
                let idx = ((py + dy) * WIDTH as usize + px + dx) * 4;
                frame[idx] = r; frame[idx+1] = g; frame[idx+2] = b; frame[idx+3] = 0xff;
            }
        }}
    }

    // player dots — 4×4 with bright centre + 5-pixel facing arrow for local player
    for p in &state.players {
        let px = MINI_X + p.x as usize * MINI_CELL;
        let py = MINI_Y + p.y as usize * MINI_CELL;
        let is_local = p.id == state.your_id;
        let (r, g, b) = if is_local { (0x00u8, 0xffu8, 0xffu8) } else { (0xffu8, 0x00u8, 0xffu8) };

        for dy in 0..4 { for dx in 0..4 {
            if px + dx < WIDTH as usize && py + dy < HEIGHT as usize {
                let centre = dx >= 1 && dx <= 2 && dy >= 1 && dy <= 2;
                let (pr,pg,pb) = if centre { (0xff, 0xff, 0xff) } else { (r, g, b) };
                let idx = ((py + dy) * WIDTH as usize + px + dx) * 4;
                frame[idx] = pr; frame[idx+1] = pg; frame[idx+2] = pb; frame[idx+3] = 0xff;
            }
        }}

        // facing arrow — 5-pixel white line for local player
        if is_local {
            for t in 1..=5i32 {
                let fx = px as f32 + cam.dir_x * t as f32;
                let fy = py as f32 + cam.dir_y * t as f32;
                if fx >= 0.0 && fy >= 0.0 {
                    let fx = fx as usize; let fy = fy as usize;
                    if fx < WIDTH as usize && fy < HEIGHT as usize {
                        let idx = (fy * WIDTH as usize + fx) * 4;
                        frame[idx] = 0xff; frame[idx+1] = 0xff; frame[idx+2] = 0xff; frame[idx+3] = 0xff;
                    }
                }
            }
        }
    }
}

// Pass 5 — fuel bar (bottom of screen, 18px tall with 2px neon border)
fn draw_fuel_bar(frame: &mut [u8], fuel: f32) {
    let bar_x  = 10usize;
    let bar_y  = HEIGHT as usize - 26;
    let bar_w  = WIDTH as usize - 20;
    let bar_h  = 16usize;
    let fill   = (bar_w as f32 * (fuel / 100.0).clamp(0.0, 1.0)) as usize;

    let (fr, fg, fb) = if fuel > 50.0      { (0x00u8, 0xffu8, 0x44u8) }
                       else if fuel > 25.0 { (0xffu8, 0xddu8, 0x00u8) }
                       else                { (0xffu8, 0x00u8, 0x00u8) };

    for row in bar_y - 2..bar_y + bar_h + 2 {
        for col in bar_x - 2..bar_x + bar_w + 2 {
            let idx = (row * WIDTH as usize + col) * 4;
            let on_border = row < bar_y || row >= bar_y + bar_h
                         || col < bar_x || col >= bar_x + bar_w;
            if on_border {
                // neon border matches fill colour
                frame[idx] = fr / 2; frame[idx+1] = fg / 2; frame[idx+2] = fb / 2;
            } else if col < bar_x + fill {
                frame[idx] = fr; frame[idx+1] = fg; frame[idx+2] = fb;
            } else {
                frame[idx] = 0x0a; frame[idx+1] = 0x00; frame[idx+2] = 0x0a;
            }
            frame[idx+3] = 0xff;
        }
    }
}

// Pass 6 — rescue flash: gold border that fades over 3 seconds
fn draw_rescue_flash(frame: &mut [u8], t: f32) {
    let intensity = 1.0 - t; // 1.0 at rescue, 0.0 at 3 seconds
    let r = (0xff as f32 * intensity) as u8;
    let g = (0xcc as f32 * intensity) as u8;
    let border = 10usize;
    let w = WIDTH as usize;
    let h = HEIGHT as usize;

    for row in 0..h {
        for col in 0..w {
            if row < border || row >= h - border || col < border || col >= w - border {
                let idx = (row * w + col) * 4;
                frame[idx]   = r;
                frame[idx+1] = g;
                frame[idx+2] = 0x00;
                frame[idx+3] = 0xff;
            }
        }
    }
}

// Pass 7 — cinematic vignette (darkens corners, adds depth)
fn apply_vignette(frame: &mut [u8]) {
    let cx = WIDTH  as f32 / 2.0;
    let cy = HEIGHT as f32 / 2.0;
    let max_d = (cx * cx + cy * cy).sqrt();
    for (i, px) in frame.chunks_exact_mut(4).enumerate() {
        let x  = (i % WIDTH as usize) as f32;
        let y  = (i / WIDTH as usize) as f32;
        let d  = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        let t  = (d / max_d).powi(2);
        let k  = 1.0 - t * 0.65;
        px[0] = (px[0] as f32 * k) as u8;
        px[1] = (px[1] as f32 * k) as u8;
        px[2] = (px[2] as f32 * k) as u8;
    }
}

// Pass 8 — FPS counter (top-left, 3×5 bitmap font 2× scaled, neon yellow)
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
    [0b111, 0b101, 0b111, 0x001, 0b111],
];
const SC: usize = 2; // scale factor
const DW: usize = 3 * SC;
const DH: usize = 5 * SC;
const GAP: usize = 2;
const PAD: usize = 4;

fn draw_fps(frame: &mut [u8], fps: f32) {
    let n = fps as u32;
    let digits: &[usize] = if n >= 100 { &[(n/100%10) as usize, (n/10%10) as usize, (n%10) as usize] }
                           else if n >= 10 { &[(n/10%10) as usize, (n%10) as usize] }
                           else { &[(n%10) as usize] };

    let bg_w = digits.len() * DW + (digits.len() - 1) * GAP + PAD * 2;
    let bg_h = DH + PAD * 2;
    for row in 0..bg_h { for col in 0..bg_w {
        let idx = (row * WIDTH as usize + col) * 4;
        frame[idx] = 0; frame[idx+1] = 0; frame[idx+2] = 0; frame[idx+3] = 0xff;
    }}
    for (di, &d) in digits.iter().enumerate() {
        let ox = PAD + di * (DW + GAP);
        let oy = PAD;
        for (r, &bits) in DIGITS_3X5[d].iter().enumerate() {
            for c in 0..3usize {
                if bits & (1 << (2 - c)) != 0 {
                    for sy in 0..SC { for sx in 0..SC {
                        let idx = ((oy + r*SC + sy) * WIDTH as usize + ox + c*SC + sx) * 4;
                        frame[idx] = 0xff; frame[idx+1] = 0xff; frame[idx+2] = 0x00; frame[idx+3] = 0xff;
                    }}
                }
            }
        }
    }
}

// ── Movement prediction (mirrors server game_tick physics exactly) ────────────

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
        let ix = nx as i32; let iy = ny as i32;
        if ix >= 0 && iy >= 0 && !self.map.is_wall(ix as usize, iy as usize) {
            self.cam.x = nx; self.cam.y = ny;
        }
    }

// ── Main render ───────────────────────────────────────────────────────────────

    fn render(&mut self) {
        let pixels = match &mut self.pixels { Some(p) => p, None => return };

        // local camera driven by apply_movement — only read server state for HUD/others
        if let Some(state) = &self.last_state {
            if self.local_id.is_none() { self.local_id = Some(state.your_id); }
            if state.miner_rescued && !self.miner_rescued {
                self.miner_rescued = true;
                self.rescue_flash  = Some(Instant::now());
            }
        }

        let frame = pixels.frame_mut();

        draw_background(frame);
        cast_walls(frame, &self.cam, &self.map, &mut self.z_buf);

        // collect sprites: other players (cyan) + miner (gold, if not rescued)
        if let Some(state) = &self.last_state {
            let mut sprites: Vec<(f32, f32, [u8; 3], bool)> = state.players.iter()
                .filter(|p| Some(p.id) != self.local_id)
                .map(|p| (p.x, p.y, [0x00u8, 0xffu8, 0xccu8], false))
                .collect();

            if !state.miner_rescued {
                let (mx, my) = self.map.miner_pos;
                sprites.push((mx, my, [0xff, 0xcc, 0x00], true));
            }

            draw_sprites(frame, &self.cam, &sprites, &self.z_buf);
            apply_vignette(frame);
            draw_minimap(frame, &self.map, &self.cam, state);

            let fuel = state.players.iter()
                .find(|p| Some(p.id) == self.local_id)
                .map(|p| p.fuel)
                .unwrap_or(100.0);
            draw_fuel_bar(frame, fuel);
        } else {
            apply_vignette(frame);
        }

        if let Some(start) = self.rescue_flash {
            let t = start.elapsed().as_secs_f32() / 3.0;
            if t < 1.0 { draw_rescue_flash(frame, t); }
            else { self.rescue_flash = None; }
        }

        draw_fps(frame, self.fps);

        if let Err(e) = pixels.render() { eprintln!("render error: {e}"); }
    }
}

// ── Network thread ────────────────────────────────────────────────────────────

fn net_thread(server_addr: String, input: Arc<InputFlags>, state_tx: mpsc::SyncSender<StatePacket>) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind failed");
    socket.connect(&server_addr).expect("connect failed");
    socket.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
    println!("network thread connected to {}", server_addr);

    let interval = Duration::from_millis(1000 / NET_HZ);
    let mut seq  = 0u32;
    let mut buf  = vec![0u8; MAX_PACKET_BYTES];

    loop {
        let t0 = Instant::now();
        seq += 1;
        let pkt = InputPacket {
            sequence: seq, player_id: 0, session_token: 0,
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
        window: None, pixels: None, state_rx, input,
        last_state: None, args,
        cam:           Camera::default(),
        map:           shared::map::get_level(1),
        z_buf:         vec![0.0f32; WIDTH as usize],
        local_id:      None,
        fps_timer:     Instant::now(),
        fps_count:     0, fps: 0.0,
        rescue_flash:  None,
        miner_rescued: false,
        move_timer:    Instant::now(),
    };
    event_loop.run_app(&mut app).expect("event loop error");
}
