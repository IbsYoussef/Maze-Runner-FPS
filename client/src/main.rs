// client — entry point
// Responsibilities:
//   Main thread    — winit event loop, render (raycaster placeholder for Day 1)
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
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use shared::protocol::{InputPacket, StatePacket, MAX_PACKET_BYTES};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const NET_HZ: u64 = 60;

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "maze-runner")]
struct Args {
    /// Server address, e.g. 127.0.0.1:34254
    #[arg(short, long, default_value = "127.0.0.1:34254")]
    server: String,

    /// Player username shown in server logs
    #[arg(short, long, default_value = "player")]
    username: String,
}

// ── Input flags shared between the event loop and the network thread ──────────

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

// ── App (winit ApplicationHandler) ───────────────────────────────────────────

struct App {
    window:     Option<Arc<Window>>,
    pixels:     Option<Pixels<'static>>,
    state_rx:   mpsc::Receiver<StatePacket>,
    input:      Arc<InputFlags>,
    last_state: Option<StatePacket>,
    args:       Args,
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
        let pixels = Pixels::new(WIDTH, HEIGHT, surface).expect("failed to create pixel buffer");

        window.request_redraw();
        self.window = Some(window);
        self.pixels = Some(pixels);

        println!(
            "Maze Runner FPS — user: '{}' | server: {}",
            self.args.username, self.args.server
        );
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    physical_key: PhysicalKey::Code(code),
                    state,
                    ..
                },
                ..
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
                // drain channel — keep only the latest state
                while let Ok(state) = self.state_rx.try_recv() {
                    self.last_state = Some(state);
                }
                self.render();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            _ => {}
        }
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).clamp(0.0, 255.0) as u8
}

fn star(row: usize, col: usize) -> bool {
    let h = row.wrapping_mul(2654435761) ^ col.wrapping_mul(2246822519);
    h % 300 == 0
}

impl App {
    fn render(&mut self) {
        let pixels = match &mut self.pixels {
            Some(p) => p,
            None => return,
        };

        let frame = pixels.frame_mut();

        // synthwave placeholder — sky, neon horizon, perspective grid floor
        // entire render replaced by raycaster on Day 2
        let mid = (HEIGHT / 2) as usize;
        let half_h = HEIGHT as f32 / 2.0;

        for (i, pixel) in frame.chunks_exact_mut(4).enumerate() {
            let row = i / WIDTH as usize;
            let col = i % WIDTH as usize;

            if row < mid {
                // sky: near-black at top → deep purple at horizon
                let t = row as f32 / mid as f32;
                let r = lerp(0x02, 0x2a, t);
                let g = lerp(0x00, 0x00, t);
                let b = lerp(0x10, 0x40, t);

                // sparse stars in upper 70% of sky
                if t < 0.7 && star(row, col) {
                    let bright = lerp(0xaa, 0xff, t);
                    pixel[0] = bright;
                    pixel[1] = bright;
                    pixel[2] = bright;
                } else {
                    pixel[0] = r;
                    pixel[1] = g;
                    pixel[2] = b;
                }
            } else if row == mid {
                // neon horizon line — hot pink
                pixel[0] = 0xff;
                pixel[1] = 0x10;
                pixel[2] = 0xc8;
            } else {
                // floor: perspective grid in neon cyan on deep purple
                let t = (row - mid) as f32 / mid as f32;
                let floor_dist = half_h / (row as f32 - half_h).max(0.5);

                // world coords at this pixel (90° horizontal FOV)
                let world_x = (col as f32 / WIDTH as f32 - 0.5) * 2.0 * floor_dist;
                let world_y = floor_dist;

                // grid line threshold: ~1.5px wide in world space
                let pw = 2.0 * floor_dist / WIDTH as f32;
                let lw = pw * 1.5;
                let on_grid = world_x.fract().abs() < lw
                    || (1.0 - world_x.fract().abs()) < lw
                    || world_y.fract().abs() < lw
                    || (1.0 - world_y.fract().abs()) < lw;

                if on_grid {
                    // neon cyan fading to dark with distance
                    let bright = (1.0 - t * 0.85).max(0.05);
                    pixel[0] = 0x00;
                    pixel[1] = (0xff as f32 * bright) as u8;
                    pixel[2] = (0xff as f32 * bright) as u8;
                } else {
                    pixel[0] = lerp(0x0d, 0x04, t);
                    pixel[1] = 0x00;
                    pixel[2] = lerp(0x1a, 0x06, t);
                }
            }
            pixel[3] = 0xff;
        }

        // show live player count from latest state
        if let Some(state) = &self.last_state {
            let _ = state; // raycaster will consume this on Day 2
        }

        if let Err(e) = pixels.render() {
            eprintln!("render error: {e}");
        }
    }
}

// ── Network thread ────────────────────────────────────────────────────────────

fn net_thread(
    server_addr: String,
    input: Arc<InputFlags>,
    state_tx: mpsc::SyncSender<StatePacket>,
) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("client: failed to bind socket");
    socket.connect(&server_addr).expect("client: failed to connect to server");
    socket
        .set_read_timeout(Some(Duration::from_millis(1)))
        .expect("client: set_read_timeout failed");

    println!("network thread connected to {}", server_addr);

    let interval = Duration::from_millis(1000 / NET_HZ);
    let mut sequence: u32 = 0;
    let mut recv_buf = vec![0u8; MAX_PACKET_BYTES];

    loop {
        let frame_start = Instant::now();

        // send input packet
        sequence += 1;
        let pkt = InputPacket {
            sequence,
            player_id: 0,
            session_token: 0,
            forward:    input.forward.load(Ordering::Relaxed),
            backward:   input.backward.load(Ordering::Relaxed),
            turn_left:  input.turn_left.load(Ordering::Relaxed),
            turn_right: input.turn_right.load(Ordering::Relaxed),
        };
        if let Ok(encoded) = postcard::to_allocvec(&pkt) {
            let _ = socket.send(&encoded);
        }

        // receive — 1ms timeout so we never block the send loop
        match socket.recv(&mut recv_buf) {
            Ok(len) => {
                if let Ok(state) = postcard::from_bytes::<StatePacket>(&recv_buf[..len]) {
                    // try_send: drop stale states if render loop is behind
                    let _ = state_tx.try_send(state);
                }
            }
            Err(_) => {} // timeout or nothing available
        }

        // pace to NET_HZ
        let elapsed = frame_start.elapsed();
        if elapsed < interval {
            thread::sleep(interval - elapsed);
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    let input = Arc::new(InputFlags::new());
    // capacity 1: render loop always gets the freshest StatePacket
    let (state_tx, state_rx) = mpsc::sync_channel::<StatePacket>(1);

    {
        let server = args.server.clone();
        let input = Arc::clone(&input);
        thread::spawn(move || net_thread(server, input, state_tx));
    }

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = App {
        window: None,
        pixels: None,
        state_rx,
        input,
        last_state: None,
        args,
    };

    event_loop.run_app(&mut app).expect("event loop error");
}
