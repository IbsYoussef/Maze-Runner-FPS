// server — entry point
// Responsibilities:
//   UDP listener task  — receives InputPackets from connected clients
//   Game tick task     — runs on a fixed 16ms interval, updates game state
//   Broadcast task     — sends StatePackets to all connected clients

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time;

use shared::map::get_level;
use shared::protocol::{InputPacket, MAX_PACKET_BYTES, PlayerState, StatePacket};

use clap::Parser;

const TICK_MS: u64 = 16;
const PLAYER_SPEED: f32 = 0.05;
const PLAYER_TURN_SPEED: f32 = 0.04;
const TIMEOUT_SECS: u64 = 10;
const RATE_LIMIT_PER_SEC: u32 = 128;

const FUEL_MAX: f32 = 100.0;
// depletes over ~90 seconds at 62.5 ticks/sec
const FUEL_DRAIN: f32 = FUEL_MAX / (90.0 * (1000.0 / TICK_MS as f32));
const RESCUE_DIST: f32 = 0.8;  // world units — player must be this close to rescue
const RESPAWN_SECS: u64 = 3;

#[derive(Parser, Debug)]
#[command(name = "maze-wars-server")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 34254)]
    port: u16,
    /// Level to load (1, 2, or 3)
    #[arg(short, long, default_value_t = 1)]
    level: u8,
}

#[derive(Debug)]
struct Player {
    id: u32,
    x: f32,
    y: f32,
    angle: f32,
    fuel: f32,
    respawn_at: Option<Instant>,
    session_token: u64,
    last_sequence: u32,
    last_seen: Instant,
    packet_count: u32,
    rate_window_start: Instant,
    input_forward: bool,
    input_backward: bool,
    input_turn_left: bool,
    input_turn_right: bool,
}

impl Player {
    fn spawn(id: u32, token: u64) -> Self {
        Self {
            id, x: 1.5, y: 1.5, angle: 0.0,
            fuel: FUEL_MAX, respawn_at: None,
            session_token: token,
            last_sequence: 0, last_seen: Instant::now(),
            packet_count: 0, rate_window_start: Instant::now(),
            input_forward: false, input_backward: false,
            input_turn_left: false, input_turn_right: false,
        }
    }

    fn respawn(&mut self) {
        self.x = 1.5; self.y = 1.5; self.angle = 0.0;
        self.fuel = FUEL_MAX; self.respawn_at = None;
    }
}

type Players     = Arc<Mutex<HashMap<SocketAddr, Player>>>;
type MinerSaved  = Arc<AtomicBool>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let addr = format!("0.0.0.0:{}", args.port);
    let socket = Arc::new(UdpSocket::bind(&addr).await.expect("bind failed"));
    tracing::info!("server listening on {}", addr);

    let map = Arc::new(get_level(args.level));
    tracing::info!("loaded level {} — miner at ({:.1}, {:.1})", args.level, map.miner_pos.0, map.miner_pos.1);

    let players: Players   = Arc::new(Mutex::new(HashMap::new()));
    let miner_saved: MinerSaved = Arc::new(AtomicBool::new(false));

    let lh = tokio::spawn(udp_listener(Arc::clone(&socket), Arc::clone(&players)));
    let th = tokio::spawn(game_tick(Arc::clone(&players), Arc::clone(&map), Arc::clone(&miner_saved)));
    let bh = tokio::spawn(broadcast(Arc::clone(&socket), Arc::clone(&players), Arc::clone(&miner_saved)));

    let _ = tokio::try_join!(lh, th, bh);
}

// ── UDP listener ──────────────────────────────────────────────────────────────

async fn udp_listener(socket: Arc<UdpSocket>, players: Players) {
    let mut buf = vec![0u8; MAX_PACKET_BYTES];
    let mut next_id: u32 = 1;

    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => { tracing::warn!("recv error: {e}"); continue; }
        };

        if len > MAX_PACKET_BYTES {
            tracing::warn!("oversized packet from {src}");
            continue;
        }

        let packet: InputPacket = match postcard::from_bytes(&buf[..len]) {
            Ok(p) => p,
            Err(_) => { tracing::warn!("malformed packet from {src}"); continue; }
        };

        let mut players = players.lock().await;

        if !players.contains_key(&src) {
            let token = src.port() as u64 ^ next_id as u64 ^ 0xdeadbeefcafe;
            tracing::info!("new player {} from {}", next_id, src);
            players.insert(src, Player::spawn(next_id, token));
            next_id += 1;
        }

        let player = players.get_mut(&src).unwrap();

        let now = Instant::now();
        if now.duration_since(player.rate_window_start) >= Duration::from_secs(1) {
            player.packet_count = 0;
            player.rate_window_start = now;
        }
        player.packet_count += 1;
        if player.packet_count > RATE_LIMIT_PER_SEC { continue; }

        if packet.session_token != 0 && packet.session_token != player.session_token {
            tracing::warn!("bad token from {src}"); continue;
        }
        if packet.sequence <= player.last_sequence { continue; }

        player.last_sequence = packet.sequence;
        player.last_seen = now;
        player.input_forward   = packet.forward;
        player.input_backward  = packet.backward;
        player.input_turn_left  = packet.turn_left;
        player.input_turn_right = packet.turn_right;
    }
}

// ── Game tick ─────────────────────────────────────────────────────────────────

async fn game_tick(players: Players, map: Arc<shared::map::Map>, miner_saved: MinerSaved) {
    let mut interval = time::interval(Duration::from_millis(TICK_MS));
    let (mx, my) = map.miner_pos;

    loop {
        interval.tick().await;
        let mut players = players.lock().await;

        // drop timed-out players
        players.retain(|addr, p| {
            let alive = p.last_seen.elapsed().as_secs() < TIMEOUT_SECS;
            if !alive { tracing::info!("player {} ({}) timed out", p.id, addr); }
            alive
        });

        for player in players.values_mut() {
            // handle pending respawn
            if let Some(at) = player.respawn_at {
                if Instant::now() >= at {
                    tracing::info!("player {} respawning", player.id);
                    player.respawn();
                }
                continue; // frozen until respawn
            }

            // drain fuel
            player.fuel -= FUEL_DRAIN;
            if player.fuel <= 0.0 {
                player.fuel = 0.0;
                player.respawn_at = Some(Instant::now() + Duration::from_secs(RESPAWN_SECS));
                tracing::info!("player {} out of fuel — respawn in {}s", player.id, RESPAWN_SECS);
                continue;
            }

            // movement + collision
            let mut nx = player.x;
            let mut ny = player.y;
            if player.input_forward  { nx += player.angle.cos() * PLAYER_SPEED; ny += player.angle.sin() * PLAYER_SPEED; }
            if player.input_backward { nx -= player.angle.cos() * PLAYER_SPEED; ny -= player.angle.sin() * PLAYER_SPEED; }
            if player.input_turn_left  { player.angle -= PLAYER_TURN_SPEED; }
            if player.input_turn_right { player.angle += PLAYER_TURN_SPEED; }

            let ix = nx as i32; let iy = ny as i32;
            if ix >= 0 && iy >= 0 && !map.is_wall(ix as usize, iy as usize) {
                player.x = nx; player.y = ny;
            }

            // miner rescue proximity check
            if !miner_saved.load(Ordering::Relaxed) {
                let dx = player.x - mx;
                let dy = player.y - my;
                if (dx * dx + dy * dy).sqrt() < RESCUE_DIST {
                    miner_saved.store(true, Ordering::Relaxed);
                    tracing::info!("player {} rescued the miner!", player.id);
                }
            }
        }
    }
}

// ── Broadcast ─────────────────────────────────────────────────────────────────

async fn broadcast(socket: Arc<UdpSocket>, players: Players, miner_saved: MinerSaved) {
    let mut interval = time::interval(Duration::from_millis(TICK_MS));
    let mut sequence: u32 = 0;

    loop {
        interval.tick().await;
        sequence = sequence.wrapping_add(1);

        let players = players.lock().await;
        if players.is_empty() { continue; }

        let rescued = miner_saved.load(Ordering::Relaxed);
        let player_list: Vec<PlayerState> = players
            .values()
            .map(|p| PlayerState { id: p.id, x: p.x, y: p.y, angle: p.angle, fuel: p.fuel })
            .collect();

        for (addr, player) in players.iter() {
            let state = StatePacket {
                sequence,
                your_id: player.id,
                miner_rescued: rescued,
                players: player_list.clone(),
            };
            let encoded = match postcard::to_allocvec(&state) {
                Ok(b) => b,
                Err(e) => { tracing::error!("serialize error: {e}"); continue; }
            };
            if let Err(e) = socket.send_to(&encoded, addr).await {
                tracing::warn!("send error to {addr}: {e}");
            }
        }
    }
}
