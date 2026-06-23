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
use pixels::wgpu::StorageTextureAccess::Atomic;
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
fn main() {}
